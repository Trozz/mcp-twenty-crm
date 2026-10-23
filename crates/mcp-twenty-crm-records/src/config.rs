use mcp_server_core::config::EnvReader;
use url::Url;

/// MCP `serverInfo.name` of the CRM records service.
pub const SERVICE_NAME: &str = "mcp-twenty-crm-records";
/// Twenty Cloud. Self-hosted workspaces set `TWENTY_API_URL` to their own server.
pub const DEFAULT_API_URL: &str = "https://api.twenty.com";
pub const DEFAULT_CURRENCY: &str = "USD";

const API_URL_VAR: &str = "TWENTY_API_URL";
const CURRENCY_VAR: &str = "TWENTY_DEFAULT_CURRENCY";

/// CRM settings only: the server binary owns the core configuration (URL, OAuth, TLS, logging).
/// There is no API key: each person signs in with their own.
pub struct RecordsConfig {
    pub api_url: Url,
    /// ISO 4217 code used for amounts given without one, e.g. `GBP`.
    pub default_currency: String,
}

impl RecordsConfig {
    pub fn from_env<F: Fn(&str) -> Option<String>>(env: &mut EnvReader<F>) -> Option<Self> {
        let errors_before = env.errors.len();
        let raw_url = env
            .optional(API_URL_VAR)
            .unwrap_or_else(|| DEFAULT_API_URL.to_string());
        let api_url = parse_url(API_URL_VAR, &raw_url)
            .map_err(|msg| env.errors.push(msg))
            .ok();
        let raw_currency = env
            .optional(CURRENCY_VAR)
            .unwrap_or_else(|| DEFAULT_CURRENCY.to_string());
        let default_currency = currency_code(&raw_currency);
        if default_currency.is_none() {
            env.errors.push(format!(
                "{CURRENCY_VAR} must be a three-letter ISO 4217 code such as USD, GBP or EUR"
            ));
        }
        if env.errors.len() != errors_before {
            return None;
        }
        Some(RecordsConfig {
            api_url: api_url?,
            default_currency: default_currency?,
        })
    }
}

/// A three-letter currency code, upper-cased.
pub fn currency_code(raw: &str) -> Option<String> {
    let code = raw.trim().to_ascii_uppercase();
    (code.len() == 3 && code.chars().all(|c| c.is_ascii_uppercase())).then_some(code)
}

/// The Twenty server from variable `name`: https, or http to a loopback address; no credentials,
/// query or fragment.
pub(crate) fn parse_url(name: &str, raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw).map_err(|e| format!("{name} is not a valid URL: {e}"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(format!("{name} must not contain credentials"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(format!("{name} must not contain a query or fragment"));
    }
    if url.path().trim_end_matches('/').ends_with("/rest") {
        return Err(format!(
            "{name} is the Twenty server's address without /rest, e.g. https://crm.example.com"
        ));
    }
    if url.scheme() == "https" && url.host_str().is_some() {
        return Ok(url);
    }
    if url.scheme() == "http" && is_loopback(&url) {
        return Ok(url);
    }
    Err(format!(
        "{name} must use https (http is only allowed for loopback addresses)"
    ))
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn load(pairs: &[(&str, &str)]) -> (Option<RecordsConfig>, Vec<String>) {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        let mut env = EnvReader::new(move |k: &str| map.get(k).cloned());
        let c = RecordsConfig::from_env(&mut env);
        (c, env.errors)
    }

    #[test]
    fn defaults_to_twenty_cloud_and_usd() {
        let (c, errors) = load(&[]);
        assert!(errors.is_empty(), "{errors:?}");
        let c = c.unwrap();
        assert_eq!(c.api_url.as_str(), "https://api.twenty.com/");
        assert_eq!(c.default_currency, "USD");
    }

    #[test]
    fn self_hosted_and_loopback_urls_are_accepted() {
        for ok in [
            "https://crm.example.com",
            "https://example.com/twenty/",
            "http://127.0.0.1:3000",
            "http://localhost:3000/",
            "http://[::1]:3000",
        ] {
            assert!(parse_url(API_URL_VAR, ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn api_url_must_be_https_unless_loopback() {
        for bad in [
            "http://crm.example.com",
            "ftp://x",
            "not a url",
            "http://10.0.0.1:3000",
            "https://user:pw@crm.example.com",
            "https://crm.example.com/?x=1",
            "https://crm.example.com/rest",
            "https://crm.example.com/twenty/rest/",
        ] {
            assert!(parse_url(API_URL_VAR, bad).is_err(), "{bad}");
        }
        let (c, errors) = load(&[("TWENTY_API_URL", "http://crm.example.com")]);
        assert!(c.is_none());
        assert!(
            errors.iter().any(|e| e.contains("TWENTY_API_URL")),
            "{errors:?}"
        );
    }

    #[test]
    fn currency_is_checked_and_upper_cased() {
        let (c, errors) = load(&[("TWENTY_DEFAULT_CURRENCY", "gbp")]);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(c.unwrap().default_currency, "GBP");
        let (c, errors) = load(&[("TWENTY_DEFAULT_CURRENCY", "pounds")]);
        assert!(c.is_none());
        assert!(
            errors.iter().any(|e| e.contains("TWENTY_DEFAULT_CURRENCY")),
            "{errors:?}"
        );
    }

    #[test]
    fn reads_only_twenty_variables() {
        let read = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = read.clone();
        let mut env = EnvReader::new(move |k: &str| {
            seen.lock().unwrap().push(k.to_string());
            None
        });
        assert!(
            RecordsConfig::from_env(&mut env).is_some(),
            "{:?}",
            env.errors
        );
        for k in read.lock().unwrap().iter() {
            assert!(k.starts_with("TWENTY_"), "read a server variable: {k}");
            assert_ne!(k, "TWENTY_SERVICES");
        }
    }
}
