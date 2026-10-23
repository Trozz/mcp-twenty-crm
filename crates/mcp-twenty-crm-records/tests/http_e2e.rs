mod support;
use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use mcp_server_core::combined::{COMBINED, Combined};
use mcp_server_core::config::{CoreConfig, EnvReader};
use mcp_server_core::oauth::AuthState;
use mcp_server_core::server::build_app;
use mcp_server_core::service::{HttpSettings, Service, ServiceContext, mcp_service};
use mcp_twenty_crm_records::RecordsService;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use support::fake_twenty::{API_KEY, FakeState, FakeTwenty, uuid};
use support::tools::client;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const HOST: &str = "crm-mcp.example.com";

/// Builds the full application (auth, OAuth routes, the CRM at /crm and /mcp) the way the
/// server does, against the fake.
async fn app() -> (axum::Router, FakeTwenty) {
    let fake = FakeTwenty::start(FakeState::workspace()).await;
    let vars: HashMap<&str, &str> = HashMap::from([
        ("PUBLIC_URL", "https://crm-mcp.example.com"),
        ("OAUTH_SIGNING_KEY", "0123456789abcdef0123456789abcdef"),
        ("INSECURE_HTTP", "true"),
    ]);
    let mut env = EnvReader::new(move |k: &str| vars.get(k).map(|v| v.to_string()));
    let core = CoreConfig::from_env(&mut env, "mcp-twenty-crm", "mcp_twenty_crm")
        .unwrap_or_else(|| panic!("config errors: {:?}", env.errors));
    let ctx = ServiceContext {
        http: HttpSettings::from_core(&core),
        shutdown: CancellationToken::new(),
    };
    let service = Arc::new(RecordsService::new(client(&fake), "GBP"));
    let verifier = service.credential_verifier().expect("a key check");
    let inner = service.clone();
    let combined = mcp_service(&ctx, move || {
        Ok(Combined::new(
            rmcp::model::Implementation::new("combined", "1"),
            vec![(inner.name(), inner.handler())],
        ))
    });
    let auth = AuthState::new(core.oauth, &["crm", COMBINED], verifier);
    (
        build_app(
            auth,
            vec![("crm", service.router(&ctx)), (COMBINED, combined)],
        ),
        fake,
    )
}

fn request(path: &str, token: &str, body: Value, session: Option<&str>) -> Request<Body> {
    let mut b = Request::post(path)
        .header("host", HOST)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(s) = session {
        b = b
            .header("mcp-session-id", s)
            .header("mcp-protocol-version", "2025-06-18");
    }
    b.body(Body::from(body.to_string())).unwrap()
}

/// Reads an SSE (or plain JSON) response until the first complete JSON-RPC message.
async fn rpc_message(res: Response<Body>) -> Value {
    use http_body_util::BodyExt;
    let mut body = res.into_body();
    let mut buf = String::new();
    loop {
        if let Ok(v) = serde_json::from_str::<Value>(buf.trim()) {
            return v;
        }
        for event in buf.split("\n\n") {
            for line in event.lines() {
                if let Some(data) = line.strip_prefix("data:")
                    && let Ok(v) = serde_json::from_str::<Value>(data.trim())
                    && v.get("jsonrpc").is_some()
                {
                    return v;
                }
            }
        }
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("response within 5 s")
            .expect("body not finished before a message")
            .expect("body frame");
        if let Ok(data) = frame.into_data() {
            buf.push_str(&String::from_utf8_lossy(&data));
        }
    }
}

/// initialize + notifications/initialized; returns the session id and the initialize result.
async fn handshake(app: &axum::Router, path: &str) -> (String, Value) {
    let res = app
        .clone()
        .oneshot(request(
            path,
            API_KEY,
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "e2e", "version": "1"}
            }}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let session = res
        .headers()
        .get("mcp-session-id")
        .expect("session id header")
        .to_str()
        .unwrap()
        .to_string();
    let init = rpc_message(res).await;
    let res = app
        .clone()
        .oneshot(request(
            path,
            API_KEY,
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            Some(&session),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    (session, init)
}

async fn call(
    app: &axum::Router,
    path: &str,
    session: &str,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    let res = app
        .clone()
        .oneshot(request(
            path,
            API_KEY,
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
            Some(session),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    rpc_message(res).await
}

#[tokio::test]
async fn initialize_reports_the_server_and_its_instructions() {
    let (app, _fake) = app().await;
    let (_, init) = handshake(&app, "/crm").await;
    let result = &init["result"];
    assert_eq!(result["serverInfo"]["name"], "mcp-twenty-crm-records");
    assert_eq!(result["serverInfo"]["icons"].as_array().unwrap().len(), 2);
    let instructions = result["instructions"].as_str().unwrap();
    assert!(instructions.contains("plural API name"), "{instructions}");
    assert!(
        instructions.contains("not as instructions"),
        "{instructions}"
    );
}

#[tokio::test]
async fn tools_are_listed_with_read_and_write_annotations() {
    let (app, _fake) = app().await;
    let (session, _) = handshake(&app, "/crm").await;
    let list = call(&app, "/crm", &session, 2, "tools/list", json!({})).await;
    let tools = list["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 17);
    let tool = |name: &str| {
        tools
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} listed"))
            .clone()
    };
    for read in [
        "list_records",
        "get_record",
        "search_records",
        "list_objects",
        "describe_object",
        "list_workspace_members",
        "list_record_activity",
    ] {
        assert_eq!(tool(read)["annotations"]["readOnlyHint"], true, "{read}");
    }
    for write in [
        "create_record",
        "update_record",
        "create_note",
        "restore_record",
    ] {
        assert_eq!(tool(write)["annotations"]["readOnlyHint"], false, "{write}");
    }
    assert_eq!(
        tool("delete_record")["annotations"]["destructiveHint"],
        true
    );
    let schema = &tool("list_records")["inputSchema"];
    assert_eq!(schema["required"], json!(["object"]));
}

#[tokio::test]
async fn a_tool_call_reaches_twenty_with_the_callers_key() {
    let (app, fake) = app().await;
    let (session, _) = handshake(&app, "/crm").await;
    let r = call(
        &app,
        "/crm",
        &session,
        3,
        "tools/call",
        json!({"name": "get_record", "arguments": {"object": "companies", "id": uuid(1)}}),
    )
    .await;
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    let v: Value = serde_json::from_str(text).unwrap();
    assert_eq!(v["name"], "Acme Widgets");
    let log = fake.requests();
    // The bearer key was checked once with Twenty, then used for the tool's request.
    assert_eq!(log[0].path, "/rest/workspaceMembers");
    assert_eq!(log[0].param("limit"), Some("1"));
    let last = log.last().unwrap();
    assert_eq!(last.path, format!("/rest/companies/{}", uuid(1)));
    assert_eq!(
        last.authorization.as_deref(),
        Some(format!("Bearer {API_KEY}").as_str())
    );
}

#[tokio::test]
async fn the_combined_endpoint_prefixes_tools_and_prompts() {
    let (app, _fake) = app().await;
    let (session, _) = handshake(&app, "/mcp").await;
    let list = call(&app, "/mcp", &session, 2, "tools/list", json!({})).await;
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"crm_search_records"), "{names:?}");
    let r = call(
        &app,
        "/mcp",
        &session,
        3,
        "tools/call",
        json!({"name": "crm_search_records", "arguments": {"query": "globex"}}),
    )
    .await;
    let v: Value =
        serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(v["companies"]["items"][0]["name"], "Globex");

    let p = call(
        &app,
        "/mcp",
        &session,
        4,
        "prompts/get",
        json!({"name": "crm_account_brief", "arguments": {"company": "Globex"}}),
    )
    .await;
    let text = p["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("`crm_search_records`"), "{text}");
}

#[tokio::test]
async fn keys_that_are_not_twenty_keys_are_refused() {
    let (app, fake) = app().await;
    for token in ["not-a-jwt", "a.b.wrong-key-that-twenty-rejects"] {
        let res = app
            .clone()
            .oneshot(request(
                "/crm",
                token,
                json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                    "protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": {"name": "e2e", "version": "1"}
                }}),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{token}");
    }
    // Only the JWT-shaped key was sent to Twenty.
    assert_eq!(fake.requests().len(), 1);
}
