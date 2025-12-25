use drm_core::ExchangeConfig;

pub const BASE_URL: &str = "https://api.elections.kalshi.com/trade-api/v2";
pub const DEMO_URL: &str = "https://demo-api.kalshi.co/trade-api/v2";

#[derive(Debug, Clone)]
pub struct KalshiConfig {
    pub base: ExchangeConfig,
    pub api_url: String,
    /// API key ID (the public key identifier)
    pub api_key_id: Option<String>,
    /// Path to the RSA private key PEM file
    pub private_key_path: Option<String>,
    /// RSA private key PEM content (alternative to path)
    pub private_key_pem: Option<String>,
    /// Use demo environment
    pub demo: bool,
}

impl Default for KalshiConfig {
    fn default() -> Self {
        Self {
            base: ExchangeConfig::default(),
            api_url: BASE_URL.into(),
            api_key_id: None,
            private_key_path: None,
            private_key_pem: None,
            demo: false,
        }
    }
}

impl KalshiConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn demo() -> Self {
        Self {
            api_url: DEMO_URL.into(),
            demo: true,
            ..Default::default()
        }
    }

    pub fn with_api_url(mut self, url: impl Into<String>) -> Self {
        self.api_url = url.into();
        self
    }

    pub fn with_api_key_id(mut self, key_id: impl Into<String>) -> Self {
        self.api_key_id = Some(key_id.into());
        self
    }

    pub fn with_private_key_path(mut self, path: impl Into<String>) -> Self {
        self.private_key_path = Some(path.into());
        self
    }

    pub fn with_private_key_pem(mut self, pem: impl Into<String>) -> Self {
        self.private_key_pem = Some(pem.into());
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.base = self.base.with_verbose(verbose);
        self
    }

    pub fn is_authenticated(&self) -> bool {
        self.api_key_id.is_some()
            && (self.private_key_path.is_some() || self.private_key_pem.is_some())
    }
}
