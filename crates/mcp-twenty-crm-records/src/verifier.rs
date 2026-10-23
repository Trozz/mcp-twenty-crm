//! Checks a Twenty API key when a person signs in or sends it as a bearer token.

use crate::api::TwentyClient;
use crate::error::TwentyError;
use mcp_server_core::credential::{CheckFuture, Credential, CredentialCheck, CredentialVerifier};
use std::sync::Arc;

/// Longest key accepted. Twenty API keys are JWTs of a few hundred bytes, above the core's
/// default limit of 256.
pub const MAX_KEY_LEN: usize = 4096;

pub struct TwentyKeyVerifier {
    client: Arc<TwentyClient>,
    /// The API URL: signing in to another Twenty server signs everyone out.
    generation: Vec<u8>,
}

impl TwentyKeyVerifier {
    pub fn new(client: Arc<TwentyClient>) -> Self {
        let generation = client.base().as_str().as_bytes().to_vec();
        TwentyKeyVerifier { client, generation }
    }
}

/// The verdict for the outcome of one cheap authenticated request made with the key.
pub fn verdict(result: Result<(), TwentyError>) -> CredentialCheck {
    match result {
        // The key authenticated; a role that cannot read workspace members is still a valid key.
        Ok(())
        | Err(TwentyError::NotFound(_))
        | Err(TwentyError::Validation(_))
        | Err(TwentyError::Permission(_)) => CredentialCheck::Valid,
        Err(TwentyError::Auth) => CredentialCheck::Invalid(None),
        Err(TwentyError::RateLimited { retry_after_secs }) => {
            CredentialCheck::Unavailable(match retry_after_secs {
                Some(s) => format!("Twenty's rate limit is used up; try again in {s} seconds"),
                None => "Twenty's rate limit is used up; try again in a minute".into(),
            })
        }
        Err(e) => CredentialCheck::Unavailable(format!(
            "Twenty did not answer as expected ({})",
            e.kind()
        )),
    }
}

impl CredentialVerifier for TwentyKeyVerifier {
    fn label(&self) -> &str {
        "Twenty API key"
    }

    /// Twenty API keys are JWTs: three base64url parts joined by dots. Anything else, such as
    /// another service's token, is refused without being sent to Twenty.
    fn plausible(&self, credential: &str) -> bool {
        let parts: Vec<&str> = credential.split('.').collect();
        (20..=MAX_KEY_LEN).contains(&credential.len())
            && parts.len() == 3
            && parts.iter().all(|p| {
                !p.is_empty()
                    && p.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            })
    }

    fn help(&self) -> &str {
        "An API key from your Twenty workspace: Settings, APIs & Webhooks, then Create API key. Copy it when shown; it is not shown again. The key's role decides what you can read and change."
    }

    fn generation(&self) -> &[u8] {
        &self.generation
    }

    fn check<'a>(&'a self, credential: &'a str) -> CheckFuture<'a> {
        Box::pin(async move {
            let key = Credential::new(credential);
            let result = self
                .client
                .get(
                    &key,
                    "key_check",
                    &["workspaceMembers"],
                    &[("limit", "1".to_string())],
                )
                .await
                .map(|_| ());
            verdict(result)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verifier() -> TwentyKeyVerifier {
        let client = Arc::new(
            TwentyClient::new(crate::api::ApiSettings::new(
                "https://api.twenty.com".parse().unwrap(),
            ))
            .unwrap(),
        );
        TwentyKeyVerifier::new(client)
    }

    #[test]
    fn only_jwt_shaped_values_are_sent_upstream() {
        let v = verifier();
        let jwt = format!(
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.{}.Sfl_KxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
            "e".repeat(400)
        );
        assert!(jwt.len() > 256);
        assert!(v.plausible(&jwt));
        for bad in [
            "short",
            "not-a-jwt-but-long-enough-to-pass",
            "a.b",
            "aaaaaaaaaa..bbbbbbbbbbb",
            "has space.in it.here-and-more",
            "aaaaaaaaaa.bbbbbbbbbbb.cccc.dddd",
            &format!("a.b.{}", "c".repeat(MAX_KEY_LEN)),
        ] {
            assert!(!v.plausible(bad), "{bad:?}");
        }
    }

    #[test]
    fn verdicts_follow_the_upstream_answer() {
        assert_eq!(verdict(Ok(())), CredentialCheck::Valid);
        assert_eq!(
            verdict(Err(TwentyError::Permission("x".into()))),
            CredentialCheck::Valid
        );
        assert_eq!(
            verdict(Err(TwentyError::Auth)),
            CredentialCheck::Invalid(None)
        );
        assert!(matches!(
            verdict(Err(TwentyError::RateLimited { retry_after_secs: Some(42) })),
            CredentialCheck::Unavailable(m) if m.contains("42 seconds")
        ));
        assert!(matches!(
            verdict(Err(TwentyError::Upstream("GET /rest/x returned HTTP 503".into()))),
            CredentialCheck::Unavailable(m) if !m.contains("/rest/x")
        ));
    }

    #[test]
    fn the_api_url_is_part_of_the_sign_in_generation() {
        assert_eq!(verifier().generation(), b"https://api.twenty.com/");
    }
}
