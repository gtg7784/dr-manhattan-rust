use drm_core::ExchangeConfig;

pub const BASE_URL: &str = "https://api.limitless.exchange";
pub const WS_URL: &str = "wss://ws.limitless.exchange";
pub const CHAIN_ID: u64 = 8453;

#[derive(Debug, Clone)]
pub struct LimitlessConfig {
    pub base: ExchangeConfig,
    pub api_url: String,
    pub ws_url: String,
    pub private_key: Option<String>,
    pub chain_id: u64,
}

impl Default for LimitlessConfig {
    fn default() -> Self {
        Self {
            base: ExchangeConfig::default(),
            api_url: BASE_URL.into(),
            ws_url: WS_URL.into(),
            private_key: None,
            chain_id: CHAIN_ID,
        }
    }
}

impl LimitlessConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_api_url(mut self, url: impl Into<String>) -> Self {
        self.api_url = url.into();
        self
    }

    pub fn with_ws_url(mut self, url: impl Into<String>) -> Self {
        self.ws_url = url.into();
        self
    }

    pub fn with_private_key(mut self, key: impl Into<String>) -> Self {
        self.private_key = Some(key.into());
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.base = self.base.with_verbose(verbose);
        self
    }

    pub fn is_authenticated(&self) -> bool {
        self.private_key.is_some()
    }
}
