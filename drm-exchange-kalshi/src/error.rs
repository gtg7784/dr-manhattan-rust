use thiserror::Error;

#[derive(Debug, Error)]
pub enum KalshiError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("api error: {0}")]
    Api(String),

    #[error("rate limited")]
    RateLimited,

    #[error("authentication required")]
    AuthRequired,

    #[error("authentication failed: {0}")]
    AuthFailed(String),

    #[error("market not found: {0}")]
    MarketNotFound(String),

    #[error("not supported: {0}")]
    NotSupported(String),

    #[error("rsa error: {0}")]
    Rsa(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<KalshiError> for drm_core::ExchangeError {
    fn from(err: KalshiError) -> Self {
        match err {
            KalshiError::MarketNotFound(id) => drm_core::ExchangeError::MarketNotFound(id),
            KalshiError::AuthRequired => {
                drm_core::ExchangeError::Authentication("authentication required".into())
            }
            KalshiError::AuthFailed(msg) => drm_core::ExchangeError::Authentication(msg),
            other => drm_core::ExchangeError::Api(other.to_string()),
        }
    }
}
