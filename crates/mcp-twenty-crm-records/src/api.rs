//! Thin client for the Twenty REST API: bearer auth with the caller's API key, no redirects,
//! timeouts, one retry of reads after a transport error or a 5xx, status mapping and metrics.
//! Writes are never retried, so a slow create cannot run twice.

use crate::error::{Result, TwentyError};
use mcp_server_core::credential::Credential;
use mcp_server_core::observability::content_logging_enabled;
use reqwest::header::RETRY_AFTER;
use reqwest::{Method, StatusCode};
use serde_json::Value;
use std::time::{Duration, Instant};
use url::Url;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const RETRY_DELAY: Duration = Duration::from_millis(250);
/// Largest response body read: a page of 200 records with every field fits easily.
pub const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const MAX_ERROR_DETAIL: usize = 300;
/// Path below `TWENTY_API_URL` that serves the REST API.
pub const REST_PREFIX: &str = "rest";

pub struct ApiSettings {
    /// `TWENTY_API_URL`: every request goes to this origin, below `<path>/rest`.
    pub base: Url,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub retry_delay: Duration,
    /// Always log response bodies at debug level. Without it, bodies are logged only while
    /// `DEBUG_UNSAFE_LOG_CONTENT` is on.
    pub log_bodies: bool,
}

impl ApiSettings {
    pub fn new(base: Url) -> Self {
        ApiSettings {
            base,
            connect_timeout: CONNECT_TIMEOUT,
            request_timeout: REQUEST_TIMEOUT,
            retry_delay: RETRY_DELAY,
            log_bodies: false,
        }
    }
}

pub struct TwentyClient {
    http: reqwest::Client,
    settings: ApiSettings,
}

fn status_class(status: Option<StatusCode>) -> &'static str {
    match status.map(|s| s.as_u16()) {
        Some(200..=299) => "2xx",
        Some(300..=399) => "3xx",
        Some(400..=499) => "4xx",
        Some(500..=599) => "5xx",
        _ => "error",
    }
}

/// A short, single-line message from a Twenty error body, if it has one. Bodies come as
/// `{"statusCode":400,"error":"BadRequestException","messages":["..."]}`, `{"message":"..."}`
/// or `{"errors":[{"message":"..."}]}`.
pub fn error_detail(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let mut parts: Vec<String> = Vec::new();
    for key in ["messages", "errors"] {
        for m in v.get(key).and_then(Value::as_array).into_iter().flatten() {
            match m {
                Value::String(s) => parts.push(s.clone()),
                other => {
                    if let Some(s) = other.get("message").and_then(Value::as_str) {
                        parts.push(s.to_string());
                    }
                }
            }
        }
    }
    if parts.is_empty() {
        match v.get("message") {
            Some(Value::String(s)) => parts.push(s.clone()),
            Some(Value::Array(items)) => {
                parts.extend(items.iter().filter_map(Value::as_str).map(str::to_string))
            }
            _ => {}
        }
    }
    if parts.is_empty()
        && let Some(s) = v.get("error").and_then(Value::as_str)
    {
        parts.push(s.to_string());
    }
    let joined = parts.join("; ");
    let joined = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.is_empty() {
        return None;
    }
    if joined.chars().count() > MAX_ERROR_DETAIL {
        let cut: String = joined.chars().take(MAX_ERROR_DETAIL).collect();
        return Some(format!("{cut}..."));
    }
    Some(joined)
}

/// Whether a 403 body says the token itself was refused rather than the role's permissions:
/// Twenty answers a JWT it did not issue with 403 `INVALID_JWT_TOKEN_TYPE`, and a missing token
/// with 403 "Missing authentication token".
fn is_token_error(body: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    let code = v.get("error").and_then(Value::as_str).unwrap_or_default();
    if code.contains("TOKEN") || code.contains("UNAUTHENTICATED") {
        return true;
    }
    error_detail(body).is_some_and(|d| d.to_ascii_lowercase().contains("token"))
}

fn map_status(
    method: &Method,
    path: &str,
    status: StatusCode,
    body: &str,
    retry_after_secs: Option<u64>,
) -> TwentyError {
    let detail = error_detail(body);
    let with_detail = |prefix: String| match &detail {
        Some(d) => format!("{prefix}: {d}"),
        None => prefix,
    };
    match status.as_u16() {
        401 => TwentyError::Auth,
        403 if is_token_error(body) => TwentyError::Auth,
        403 => TwentyError::Permission(with_detail(
            "Twenty refused this with your API key (HTTP 403); its role may not read or change this object".into(),
        )),
        404 => TwentyError::NotFound(with_detail(format!("nothing found at {path}"))),
        429 => TwentyError::RateLimited { retry_after_secs },
        400 | 409 | 422 => TwentyError::Validation(with_detail(format!(
            "Twenty rejected the request (HTTP {})",
            status.as_u16()
        ))),
        300..=399 => TwentyError::Upstream(format!(
            "{method} {path} returned an unexpected redirect (HTTP {}); check TWENTY_API_URL",
            status.as_u16()
        )),
        code => TwentyError::Upstream(with_detail(format!("{method} {path} returned HTTP {code}"))),
    }
}

impl TwentyClient {
    pub fn new(settings: ApiSettings) -> Result<Self> {
        mcp_server_core::install_crypto_provider();
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(settings.connect_timeout)
            .timeout(settings.request_timeout)
            .user_agent(format!("mcp-twenty-crm/{}", crate::VERSION))
            .build()
            .map_err(|e| TwentyError::Internal(format!("building the HTTP client failed: {e}")))?;
        Ok(TwentyClient { http, settings })
    }

    pub fn base(&self) -> &Url {
        &self.settings.base
    }

    fn log_bodies(&self) -> bool {
        self.settings.log_bodies || content_logging_enabled()
    }

    /// `base/rest` with `segments` appended (each percent-encoded) and `query` added.
    pub fn url(&self, segments: &[&str], query: &[(&str, String)]) -> Url {
        let mut url = self.settings.base.clone();
        url.path_segments_mut()
            .expect("an http(s) base URL has path segments")
            .pop_if_empty()
            .push(REST_PREFIX)
            .extend(segments);
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        url
    }

    /// GET a JSON resource. `endpoint` is a fixed label for metrics and logs.
    pub async fn get(
        &self,
        key: &Credential,
        endpoint: &'static str,
        segments: &[&str],
        query: &[(&str, String)],
    ) -> Result<Value> {
        self.send(key, Method::GET, endpoint, segments, query, None)
            .await
    }

    /// Sends one request with an optional JSON body and returns the JSON answer. A 2xx with an
    /// empty body is `Value::Null`.
    pub async fn send(
        &self,
        key: &Credential,
        method: Method,
        endpoint: &'static str,
        segments: &[&str],
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Value> {
        let url = self.url(segments, query);
        let path = url.path().to_string();
        let retryable = method == Method::GET;
        let mut retried = false;
        loop {
            let started = Instant::now();
            let mut request = self
                .http
                .request(method.clone(), url.clone())
                .bearer_auth(key.expose())
                .header(reqwest::header::ACCEPT, "application/json");
            if let Some(body) = body {
                request = request
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(body.to_string());
            }
            let result = request.send().await;
            let status = result.as_ref().ok().map(|r| r.status());
            metrics::counter!("twenty_requests_total", "endpoint" => endpoint, "status_class" => status_class(status))
                .increment(1);
            metrics::histogram!("twenty_request_duration_seconds", "endpoint" => endpoint)
                .record(started.elapsed().as_secs_f64());
            tracing::debug!(
                endpoint,
                method = %method,
                path = %path,
                status = status.map(|s| s.as_u16()),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "twenty request"
            );
            let response = match result {
                Ok(r) => r,
                Err(e) if retryable && !retried => {
                    tracing::debug!(endpoint, error = %describe(e), "retrying");
                    retried = true;
                    tokio::time::sleep(self.settings.retry_delay).await;
                    continue;
                }
                Err(e) => {
                    let what = if e.is_timeout() {
                        "timed out"
                    } else {
                        "failed"
                    };
                    return Err(TwentyError::Upstream(format!(
                        "{method} {path} {what}: {}",
                        describe(e)
                    )));
                }
            };
            let status = response.status();
            if status.is_server_error() && retryable && !retried {
                tracing::debug!(endpoint, status = status.as_u16(), "retrying");
                retried = true;
                tokio::time::sleep(self.settings.retry_delay).await;
                continue;
            }
            let retry_after_secs = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());
            let text = read_body(response, &path).await?;
            if self.log_bodies() {
                tracing::debug!(endpoint, path = %path, status = status.as_u16(), body = %text, "twenty response");
            }
            if !status.is_success() {
                if status == StatusCode::TOO_MANY_REQUESTS {
                    metrics::counter!("twenty_rate_limited_total").increment(1);
                }
                return Err(map_status(&method, &path, status, &text, retry_after_secs));
            }
            if text.trim().is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_str(&text).map_err(|e| {
                TwentyError::Upstream(format!(
                    "{method} {path} returned invalid JSON ({e}); check TWENTY_API_URL points at the Twenty server"
                ))
            });
        }
    }
}

/// A reqwest error with its causes, without the URL: the URL carries the caller's filters.
fn describe(e: reqwest::Error) -> String {
    let e = e.without_url();
    let mut msg = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(s) = source {
        msg = format!("{msg}: {s}");
        source = s.source();
    }
    msg
}

async fn read_body(mut response: reqwest::Response, path: &str) -> Result<String> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if buf.len() + chunk.len() > MAX_BODY_BYTES {
                    return Err(TwentyError::Upstream(format!(
                        "{path} returned more than {MAX_BODY_BYTES} bytes; ask for fewer records or fields"
                    )));
                }
                buf.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => {
                return Err(TwentyError::Upstream(format!(
                    "reading {path} failed: {}",
                    describe(e)
                )));
            }
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(base: &str) -> TwentyClient {
        TwentyClient::new(ApiSettings::new(Url::parse(base).unwrap())).unwrap()
    }

    #[test]
    fn urls_append_rest_and_encoded_segments() {
        let c = client("https://api.twenty.com");
        assert_eq!(
            c.url(&["people", "abc"], &[]).as_str(),
            "https://api.twenty.com/rest/people/abc"
        );
        assert_eq!(
            c.url(
                &["people"],
                &[
                    ("filter", "name.firstName[ilike]:%ann%".into()),
                    ("limit", "5".into())
                ]
            )
            .as_str(),
            "https://api.twenty.com/rest/people?filter=name.firstName%5Bilike%5D%3A%25ann%25&limit=5"
        );
        assert_eq!(c.url(&["../x?y"], &[]).path(), "/rest/..%2Fx%3Fy");
    }

    #[test]
    fn urls_keep_a_base_path() {
        let c = client("https://example.com/twenty/");
        assert_eq!(
            c.url(&["metadata", "objects"], &[]).as_str(),
            "https://example.com/twenty/rest/metadata/objects"
        );
    }

    #[test]
    fn statuses_map_to_the_error_kinds() {
        let kind = |code: u16| {
            map_status(
                &Method::GET,
                "/rest/people",
                StatusCode::from_u16(code).unwrap(),
                "",
                None,
            )
            .kind()
        };
        assert_eq!(kind(401), "auth");
        assert_eq!(kind(403), "permission");
        assert_eq!(kind(404), "not_found");
        assert_eq!(kind(429), "rate_limited");
        assert_eq!(kind(400), "validation");
        assert_eq!(kind(422), "validation");
        assert_eq!(kind(302), "upstream");
        assert_eq!(kind(500), "upstream");
    }

    #[test]
    fn a_403_about_the_token_is_an_auth_failure() {
        let refused = |body: &str| {
            map_status(&Method::GET, "/rest/x", StatusCode::FORBIDDEN, body, None).kind()
        };
        assert_eq!(
            refused(
                r#"{"statusCode":403,"messages":["Invalid token type"],"error":"INVALID_JWT_TOKEN_TYPE"}"#
            ),
            "auth"
        );
        assert_eq!(
            refused(
                r#"{"statusCode":403,"messages":["Missing authentication token"],"error":"FORBIDDEN_EXCEPTION"}"#
            ),
            "auth"
        );
        assert_eq!(
            refused(
                r#"{"statusCode":403,"messages":["Entity not readable"],"error":"PERMISSION_DENIED"}"#
            ),
            "permission"
        );
        assert_eq!(refused(""), "permission");
    }

    #[test]
    fn validation_errors_carry_the_upstream_detail() {
        let body = r#"{"statusCode":400,"error":"BadRequestException","messages":["object 'person' not found. Did you mean 'people'?"]}"#;
        let e = map_status(
            &Method::GET,
            "/rest/person",
            StatusCode::BAD_REQUEST,
            body,
            None,
        );
        assert_eq!(e.kind(), "validation");
        assert!(e.to_string().contains("Did you mean 'people'?"), "{e}");
    }

    #[test]
    fn error_detail_reads_every_body_shape() {
        assert_eq!(
            error_detail(r#"{"statusCode":401,"message":"Token invalid"}"#).as_deref(),
            Some("Token invalid")
        );
        assert_eq!(
            error_detail(r#"{"messages":["a","b"],"error":"BadRequestException"}"#).as_deref(),
            Some("a; b")
        );
        assert_eq!(
            error_detail(r#"{"errors":[{"message":"x"}]}"#).as_deref(),
            Some("x")
        );
        assert_eq!(
            error_detail(r#"{"message":["one","two"]}"#).as_deref(),
            Some("one; two")
        );
        assert_eq!(
            error_detail(r#"{"error":"Forbidden"}"#).as_deref(),
            Some("Forbidden")
        );
        assert_eq!(error_detail("{}"), None);
        assert_eq!(error_detail("<html>"), None);
        let long = format!(r#"{{"message":"{}"}}"#, "x".repeat(1000));
        assert_eq!(
            error_detail(&long).unwrap().chars().count(),
            MAX_ERROR_DETAIL + 3
        );
    }

    #[test]
    fn status_classes() {
        assert_eq!(status_class(Some(StatusCode::OK)), "2xx");
        assert_eq!(status_class(Some(StatusCode::FOUND)), "3xx");
        assert_eq!(status_class(Some(StatusCode::NOT_FOUND)), "4xx");
        assert_eq!(status_class(Some(StatusCode::BAD_GATEWAY)), "5xx");
        assert_eq!(status_class(None), "error");
    }
}
