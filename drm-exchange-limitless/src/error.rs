use thiserror::Error;

#[derive(Debug, Error)]
pub enum LimitlessError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("api error: {0}")]
    Api(String),

    #[error("rate limited")]
    RateLimited,

    #[error("authentication required")]
    AuthRequired,

    #[error("authentication error: {0}")]
    Auth(String),

    #[error("market not found: {0}")]
    MarketNotFound(String),

    #[error("invalid order: {0}")]
    InvalidOrder(String),
}

impl From<LimitlessError> for drm_core::ExchangeError {
    fn from(err: LimitlessError) -> Self {
        match err {
            LimitlessError::MarketNotFound(id) => drm_core::ExchangeError::MarketNotFound(id),
            LimitlessError::AuthRequired => {
                drm_core::ExchangeError::Authentication("authentication required".into())
            }
            LimitlessError::Auth(msg) => drm_core::ExchangeError::Authentication(msg),
            LimitlessError::InvalidOrder(msg) => drm_core::ExchangeError::InvalidOrder(msg),
            other => drm_core::ExchangeError::Api(other.to_string()),
        }
    }
}
