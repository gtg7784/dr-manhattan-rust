use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolymarketError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("api error: {0}")]
    Api(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("rate limited, retry after {retry_after}s")]
    RateLimited { retry_after: u64 },

    #[error("authentication required")]
    AuthRequired,

    #[error("authentication error: {0}")]
    Auth(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("market not found: {0}")]
    MarketNotFound(String),

    #[error("signing error: {0}")]
    Signing(String),
}

impl From<PolymarketError> for drm_core::ExchangeError {
    fn from(err: PolymarketError) -> Self {
        match err {
            PolymarketError::MarketNotFound(id) => drm_core::ExchangeError::MarketNotFound(id),
            PolymarketError::AuthRequired | PolymarketError::Auth(_) => {
                drm_core::ExchangeError::Authentication(err.to_string())
            }
            PolymarketError::Api(msg) => drm_core::ExchangeError::Api(msg),
            other => drm_core::ExchangeError::Api(other.to_string()),
        }
    }
}
