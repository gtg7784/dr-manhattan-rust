use drm_core::ExchangeConfig;

pub const BASE_URL: &str = "https://proxy.opinion.trade:8443";
pub const CHAIN_ID: u64 = 56;

#[derive(Debug, Clone)]
pub struct OpinionConfig {
    pub base: ExchangeConfig,
    pub api_url: String,
    pub api_key: Option<String>,
    pub private_key: Option<String>,
    pub multi_sig_addr: Option<String>,
    pub chain_id: u64,
}

impl Default for OpinionConfig {
    fn default() -> Self {
        Self {
            base: ExchangeConfig::default(),
            api_url: BASE_URL.into(),
            api_key: None,
            private_key: None,
            multi_sig_addr: None,
            chain_id: CHAIN_ID,
        }
    }
}

impl OpinionConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_api_url(mut self, url: impl Into<String>) -> Self {
        self.api_url = url.into();
        self
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    pub fn with_private_key(mut self, key: impl Into<String>) -> Self {
        self.private_key = Some(key.into());
        self
    }

    pub fn with_multi_sig(mut self, addr: impl Into<String>) -> Self {
        self.multi_sig_addr = Some(addr.into());
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.base = self.base.with_verbose(verbose);
        self
    }

    pub fn is_authenticated(&self) -> bool {
        self.api_key.is_some() && self.private_key.is_some() && self.multi_sig_addr.is_some()
    }
}
