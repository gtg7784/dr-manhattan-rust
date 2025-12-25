use thiserror::Error;

#[derive(Debug, Error)]
pub enum DrmError {
    #[error("network error: {0}")]
    Network(#[from] NetworkError),

    #[error("exchange error: {0}")]
    Exchange(#[from] ExchangeError),

    #[error("websocket error: {0}")]
    WebSocket(#[from] WebSocketError),

    #[error("signing error: {0}")]
    Signing(#[from] SigningError),

    #[error("rate limit exceeded")]
    RateLimitExceeded,

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("http request failed: {0}")]
    Http(String),

    #[error("timeout after {0}ms")]
    Timeout(u64),

    #[error("connection failed: {0}")]
    Connection(String),
}

#[derive(Debug, Error)]
pub enum ExchangeError {
    #[error("market not found: {0}")]
    MarketNotFound(String),

    #[error("invalid order: {0}")]
    InvalidOrder(String),

    #[error("order rejected: {0}")]
    OrderRejected(String),

    #[error("insufficient funds: {0}")]
    InsufficientFunds(String),

    #[error("authentication failed: {0}")]
    Authentication(String),

    #[error("not supported: {0}")]
    NotSupported(String),

    #[error("api error: {0}")]
    Api(String),
}

#[derive(Debug, Clone, Error)]
pub enum WebSocketError {
    #[error("connection error: {0}")]
    Connection(String),

    #[error("connection closed")]
    Closed,

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("subscription failed: {0}")]
    Subscription(String),
}

#[derive(Debug, Error)]
pub enum SigningError {
    #[error("invalid private key")]
    InvalidKey,

    #[error("signing failed: {0}")]
    SigningFailed(String),

    #[error("unsupported operation: {0}")]
    Unsupported(String),
}
