use crate::api::{ApiSettings, TwentyClient};
use crate::config::RecordsConfig;
use crate::tools::CrmServer;
use crate::verifier::TwentyKeyVerifier;
use mcp_server_core::combined::DynHandler;
use mcp_server_core::credential::CredentialVerifier;
use mcp_server_core::service::{Service, ServiceContext, ServiceEnv, mcp_service};
use std::sync::Arc;

/// The CRM records service, mounted by the server at `/crm`.
pub struct RecordsService {
    client: Arc<TwentyClient>,
    default_currency: String,
}

impl RecordsService {
    pub fn new(client: Arc<TwentyClient>, default_currency: impl Into<String>) -> Self {
        Self {
            client,
            default_currency: default_currency.into(),
        }
    }

    fn server(&self) -> CrmServer {
        CrmServer::new(self.client.clone(), self.default_currency.clone())
    }
}

impl Service for RecordsService {
    fn name(&self) -> &'static str {
        "crm"
    }

    fn tool_names(&self) -> Vec<String> {
        self.server().tool_names()
    }

    fn router(self: Arc<Self>, ctx: &ServiceContext) -> axum::Router {
        tracing::info!(
            api = %self.client.base().origin().ascii_serialization(),
            currency = %self.default_currency,
            "twenty settings"
        );
        mcp_service(ctx, move || Ok(self.server()))
    }

    fn credential_verifier(&self) -> Option<Arc<dyn CredentialVerifier>> {
        Some(Arc::new(TwentyKeyVerifier::new(self.client.clone())))
    }

    fn handler(&self) -> DynHandler {
        DynHandler::new(self.server())
    }
}

/// Reads the CRM configuration and builds the service. Errors go to `env.errors`.
pub fn from_env(env: &mut ServiceEnv) -> Option<Arc<dyn Service>> {
    let cfg = RecordsConfig::from_env(env)?;
    let client = match TwentyClient::new(ApiSettings::new(cfg.api_url)) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            env.errors.push(format!("Twenty client setup failed: {e}"));
            return None;
        }
    };
    Some(Arc::new(RecordsService::new(client, cfg.default_currency)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> ServiceEnv {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ServiceEnv::new(Box::new(move |k: &str| map.get(k).cloned()))
    }

    #[test]
    fn builds_from_env() {
        let mut e = env(&[]);
        let svc = from_env(&mut e).expect("service");
        assert!(e.errors.is_empty(), "{:?}", e.errors);
        assert_eq!(svc.name(), "crm");
        let verifier = svc.credential_verifier().expect("a key verifier");
        assert_eq!(verifier.label(), "Twenty API key");
        assert_eq!(
            svc.tool_names(),
            vec![
                "create_company",
                "create_note",
                "create_opportunity",
                "create_person",
                "create_record",
                "create_records",
                "create_task",
                "delete_record",
                "describe_object",
                "get_record",
                "list_objects",
                "list_record_activity",
                "list_records",
                "list_workspace_members",
                "restore_record",
                "search_records",
                "update_record",
            ]
        );
    }

    #[test]
    fn bad_config_is_reported_and_no_service_is_built() {
        let mut e = env(&[("TWENTY_API_URL", "ftp://x")]);
        assert!(from_env(&mut e).is_none());
        assert!(e.errors.join("\n").contains("TWENTY_API_URL"));
    }
}
