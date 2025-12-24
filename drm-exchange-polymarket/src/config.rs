use drm_core::ExchangeConfig;

pub const GAMMA_API_URL: &str = "https://gamma-api.polymarket.com";
pub const CLOB_API_URL: &str = "https://clob.polymarket.com";

#[derive(Debug, Clone)]
pub struct PolymarketConfig {
    pub base: ExchangeConfig,
    pub gamma_url: String,
    pub clob_url: String,
    pub private_key: Option<String>,
    pub funder: Option<String>,
    pub chain_id: u64,
}

impl Default for PolymarketConfig {
    fn default() -> Self {
        Self {
            base: ExchangeConfig::default(),
            gamma_url: GAMMA_API_URL.into(),
            clob_url: CLOB_API_URL.into(),
            private_key: None,
            funder: None,
            chain_id: 137,
        }
    }
}

impl PolymarketConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_private_key(mut self, key: impl Into<String>) -> Self {
        self.private_key = Some(key.into());
        self
    }

    pub fn with_funder(mut self, funder: impl Into<String>) -> Self {
        self.funder = Some(funder.into());
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
