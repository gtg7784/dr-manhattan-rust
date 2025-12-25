use thiserror::Error;

#[derive(Debug, Error)]
pub enum OpinionError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("api error: {0}")]
    Api(String),

    #[error("rate limited")]
    RateLimited,

    #[error("authentication required")]
    AuthRequired,

    #[error("market not found: {0}")]
    MarketNotFound(String),

    #[error("not supported: {0}")]
    NotSupported(String),
}

impl From<OpinionError> for drm_core::ExchangeError {
    fn from(err: OpinionError) -> Self {
        match err {
            OpinionError::MarketNotFound(id) => drm_core::ExchangeError::MarketNotFound(id),
            OpinionError::AuthRequired => {
                drm_core::ExchangeError::Authentication("authentication required".into())
            }
            other => drm_core::ExchangeError::Api(other.to_string()),
        }
    }
}
