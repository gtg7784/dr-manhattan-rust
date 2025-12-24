use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolymarketError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("api error: {status} - {message}")]
    Api { status: u16, message: String },

    #[error("rate limited, retry after {retry_after}s")]
    RateLimited { retry_after: u64 },

    #[error("authentication required")]
    AuthRequired,

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
            PolymarketError::AuthRequired => {
                drm_core::ExchangeError::Authentication("authentication required".into())
            }
            PolymarketError::Api { status, message } => {
                drm_core::ExchangeError::Api(format!("{}: {}", status, message))
            }
            other => drm_core::ExchangeError::Api(other.to_string()),
        }
    }
}
