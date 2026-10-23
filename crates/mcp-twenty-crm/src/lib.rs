//! The mcp-twenty-crm server: Twenty CRM tools on one [`mcp_server_core::app::Server`], each
//! person signing in with their own Twenty API key.

use mcp_server_core::app::{Server, SignIn};
pub use mcp_server_core::combined::COMBINED;
use mcp_server_core::service::ServiceFactory;

pub const VERSION: &str = match option_env!("APP_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

pub const SERVICE_NAME: &str = "mcp-twenty-crm";
/// Tracing target prefix: also matches `mcp_twenty_crm_records` and every other
/// `mcp_twenty_crm_*` crate.
pub const CRATE_TARGET: &str = "mcp_twenty_crm";

/// Every service this binary can run, in mount order. Adding a service: a library crate
/// implementing `mcp_server_core::service::Service`, a dependency in Cargo.toml and one line here.
pub const SERVICES: &[(&str, ServiceFactory)] = &[("crm", mcp_twenty_crm_records::from_env)];

/// The server: `TWENTY_SERVICES`, `TWENTY_COMBINED` and `TWENTY_READ_ONLY` choose what runs, and
/// people sign in with their own Twenty API key, checked by the CRM service.
pub fn server() -> Server {
    Server::new(SERVICE_NAME, VERSION)
        .env_prefix("TWENTY")
        .crate_target(CRATE_TARGET)
        .services(SERVICES)
        .sign_in(SignIn::ServiceCredential)
        .icon(
            include_bytes!("../assets/favicon.png"),
            include_str!("../assets/favicon.svg"),
        )
        .docs_url("https://github.com/Trozz/mcp-twenty-crm#configuration")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_server_core::service::ServiceEnv;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> ServiceEnv {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ServiceEnv::new(Box::new(move |k: &str| map.get(k).cloned()))
    }

    const CORE: &[(&str, &str)] = &[
        ("PUBLIC_URL", "https://crm-mcp.example.com"),
        ("OAUTH_SIGNING_KEY", "0123456789abcdef0123456789abcdef"),
    ];

    #[test]
    fn people_sign_in_with_their_own_twenty_key() {
        let mut e = env(CORE);
        let loaded = server().load(&mut e).expect("loads");
        assert!(e.errors.is_empty(), "{:?}", e.errors);
        assert_eq!(loaded.verifier.label(), "Twenty API key");
        assert!(loaded.verifier.carried());
    }

    #[test]
    fn core_and_service_errors_are_reported_together() {
        let mut e = env(&[
            ("PUBLIC_URL", "ftp://x"),
            ("TWENTY_API_URL", "http://crm.example.com"),
        ]);
        assert!(server().load(&mut e).is_none());
        let joined = e.errors.join("\n");
        assert!(joined.contains("PUBLIC_URL"), "{joined}");
        assert!(joined.contains("OAUTH_SIGNING_KEY is required"), "{joined}");
        assert!(joined.contains("TWENTY_API_URL"), "{joined}");
    }

    #[test]
    fn selection_errors_name_the_known_services() {
        let mut e = env(&[CORE, &[("TWENTY_SERVICES", "mail")]].concat());
        assert!(server().load(&mut e).is_none());
        assert_eq!(
            e.errors,
            vec!["TWENTY_SERVICES contains an unknown service: mail (known: crm)".to_string()]
        );
    }

    #[test]
    fn a_static_token_is_refused_because_everyone_brings_their_own_key() {
        let mut e = env(&[CORE, &[("MCP_AUTH_TOKEN", "a-shared-token")]].concat());
        assert!(server().load(&mut e).is_none());
        assert_eq!(
            e.errors,
            vec![
                "MCP_AUTH_TOKEN cannot be used when people sign in with their own Twenty API key"
                    .to_string()
            ]
        );
    }

    #[test]
    fn read_only_hides_the_write_tools() {
        let mut e = env(&[CORE, &[("TWENTY_READ_ONLY", "crm")]].concat());
        let loaded = server().load(&mut e).expect("loads");
        assert!(loaded.safety.is_read_only("crm"));
    }

    #[test]
    fn combined_tool_names_are_unique_and_fit_clients() {
        let mut e = env(CORE);
        let loaded = server().load(&mut e).expect("loads");
        let names = Server::combined_tool_names(&loaded.services);
        assert_eq!(names.len(), 17);
        assert!(names.contains(&"crm_search_records".to_string()));
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
        for n in &names {
            assert!(
                n.len() <= 64
                    && n.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
                "{n}"
            );
        }
    }

    #[test]
    fn debug_directive_covers_service_crates() {
        use tracing::Level;
        use tracing_subscriber::layer::SubscriberExt;
        let directives = mcp_server_core::observability::build_filter("info", true, CRATE_TARGET);
        let subscriber =
            tracing_subscriber::registry().with(tracing_subscriber::EnvFilter::new(directives));
        tracing::subscriber::with_default(subscriber, || {
            assert!(tracing::enabled!(target: "mcp_twenty_crm_records::api", Level::DEBUG));
            assert!(tracing::enabled!(target: "mcp_twenty_crm", Level::DEBUG));
            assert!(!tracing::enabled!(target: "hyper", Level::DEBUG));
        });
    }
}
