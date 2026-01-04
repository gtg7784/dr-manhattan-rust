use thiserror::Error;

#[derive(Debug, Error)]
pub enum PredictFunError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("api error: {0}")]
    Api(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("rate limited")]
    RateLimited,

    #[error("authentication required")]
    AuthRequired,

    #[error("authentication error: {0}")]
    Auth(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("market not found: {0}")]
    MarketNotFound(String),

    #[error("invalid order: {0}")]
    InvalidOrder(String),

    #[error("signing error: {0}")]
    Signing(String),
}

impl From<PredictFunError> for drm_core::ExchangeError {
    fn from(err: PredictFunError) -> Self {
        match err {
            PredictFunError::MarketNotFound(id) => drm_core::ExchangeError::MarketNotFound(id),
            PredictFunError::AuthRequired | PredictFunError::Auth(_) => {
                drm_core::ExchangeError::Authentication(err.to_string())
            }
            PredictFunError::InvalidOrder(msg) => drm_core::ExchangeError::InvalidOrder(msg),
            PredictFunError::Api(msg) => drm_core::ExchangeError::Api(msg),
            other => drm_core::ExchangeError::Api(other.to_string()),
        }
    }
}
