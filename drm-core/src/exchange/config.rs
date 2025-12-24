use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ExchangeConfig {
    pub timeout: Duration,
    pub rate_limit_per_second: u32,
    pub max_retries: u32,
    pub retry_delay: Duration,
    pub verbose: bool,
}

impl Default for ExchangeConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            rate_limit_per_second: 10,
            max_retries: 3,
            retry_delay: Duration::from_secs(1),
            verbose: false,
        }
    }
}

impl ExchangeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_rate_limit(mut self, requests_per_second: u32) -> Self {
        self.rate_limit_per_second = requests_per_second;
        self
    }

    pub fn with_retries(mut self, max_retries: u32, delay: Duration) -> Self {
        self.max_retries = max_retries;
        self.retry_delay = delay;
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }
}

#[derive(Debug, Clone)]
pub struct FetchMarketsParams {
    pub limit: Option<usize>,
    pub active_only: bool,
}

impl Default for FetchMarketsParams {
    fn default() -> Self {
        Self {
            limit: None,
            active_only: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FetchOrdersParams {
    pub market_id: Option<String>,
}
