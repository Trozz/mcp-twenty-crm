#[derive(Debug, thiserror::Error)]
pub enum TwentyError {
    #[error(
        "Twenty rejected your API key. Sign in again with a key from Settings, APIs & Webhooks in this Twenty workspace; keys can expire or be revoked"
    )]
    Auth,
    /// The key is valid but its role may not do this.
    #[error("{0}")]
    Permission(String),
    #[error("this request carried no Twenty API key; sign in again")]
    MissingKey,
    /// A complete message: tool errors are already prefixed with the kind, `not_found: `.
    #[error("{0}")]
    NotFound(String),
    #[error("{}", rate_limited_message(*.retry_after_secs))]
    RateLimited {
        /// Seconds Twenty asked us to wait, when it said.
        retry_after_secs: Option<u64>,
    },
    #[error("{0}")]
    Validation(String),
    #[error("Twenty error: {0}")]
    Upstream(String),
    #[error("{0}")]
    Internal(String),
}

fn rate_limited_message(retry_after_secs: Option<u64>) -> String {
    let base =
        "the Twenty API rate limit (100 requests per minute per workspace by default) is used up";
    match retry_after_secs {
        Some(s) => format!("{base}; try again in {s} seconds"),
        None => format!("{base}; try again in a minute"),
    }
}

impl TwentyError {
    pub fn kind(&self) -> &'static str {
        match self {
            TwentyError::Auth | TwentyError::MissingKey => "auth",
            TwentyError::Permission(_) => "permission",
            TwentyError::NotFound(_) => "not_found",
            TwentyError::RateLimited { .. } => "rate_limited",
            TwentyError::Validation(_) => "validation",
            TwentyError::Upstream(_) => "upstream",
            TwentyError::Internal(_) => "internal",
        }
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        TwentyError::Validation(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, TwentyError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_match_the_spec() {
        let all = [
            (TwentyError::Auth, "auth"),
            (TwentyError::MissingKey, "auth"),
            (TwentyError::Permission("no".into()), "permission"),
            (TwentyError::NotFound("x".into()), "not_found"),
            (
                TwentyError::RateLimited {
                    retry_after_secs: None,
                },
                "rate_limited",
            ),
            (TwentyError::invalid("bad"), "validation"),
            (TwentyError::Upstream("500".into()), "upstream"),
            (TwentyError::Internal("bug".into()), "internal"),
        ];
        for (e, kind) in all {
            assert_eq!(e.kind(), kind);
        }
    }

    #[test]
    fn rate_limit_message_says_when_to_retry() {
        let msg = TwentyError::RateLimited {
            retry_after_secs: Some(42),
        }
        .to_string();
        assert!(msg.contains("try again in 42 seconds"), "{msg}");
        let msg = TwentyError::RateLimited {
            retry_after_secs: None,
        }
        .to_string();
        assert!(msg.contains("try again in a minute"), "{msg}");
    }

    #[test]
    fn auth_message_tells_the_person_what_to_do() {
        let msg = TwentyError::Auth.to_string();
        assert!(msg.contains("Sign in again"), "{msg}");
    }
}
