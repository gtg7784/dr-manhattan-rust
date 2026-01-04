use drm_core::ExchangeConfig;

pub const BASE_URL: &str = "https://api.predict.fun";
pub const TESTNET_URL: &str = "https://api-testnet.predict.fun";

pub const CHAIN_ID: u64 = 56; // BNB Mainnet
pub const TESTNET_CHAIN_ID: u64 = 97; // BNB Testnet

// Yield-bearing CTFExchange contract addresses (default for most markets)
pub const YIELD_BEARING_CTF_EXCHANGE_MAINNET: &str = "0x6bEb5a40C032AFc305961162d8204CDA16DECFa5";
pub const YIELD_BEARING_CTF_EXCHANGE_TESTNET: &str = "0x8a6B4Fa700A1e310b106E7a48bAFa29111f66e89";
pub const YIELD_BEARING_NEG_RISK_CTF_EXCHANGE_MAINNET: &str =
    "0x8A289d458f5a134bA40015085A8F50Ffb681B41d";
pub const YIELD_BEARING_NEG_RISK_CTF_EXCHANGE_TESTNET: &str =
    "0x95D5113bc50eD201e319101bbca3e0E250662fCC";

// Non-yield-bearing CTFExchange contract addresses
pub const CTF_EXCHANGE_MAINNET: &str = "0x8BC070BEdAB741406F4B1Eb65A72bee27894B689";
pub const CTF_EXCHANGE_TESTNET: &str = "0x2A6413639BD3d73a20ed8C95F634Ce198ABbd2d7";
pub const NEG_RISK_CTF_EXCHANGE_MAINNET: &str = "0x365fb81bd4A24D6303cd2F19c349dE6894D8d58A";
pub const NEG_RISK_CTF_EXCHANGE_TESTNET: &str = "0xd690b2bd441bE36431F6F6639D7Ad351e7B29680";

// EIP-712 domain name (must match official SDK)
pub const PROTOCOL_NAME: &str = "predict.fun CTF Exchange";
pub const PROTOCOL_VERSION: &str = "1";

#[derive(Debug, Clone)]
pub struct PredictFunConfig {
    pub base: ExchangeConfig,
    pub api_url: String,
    pub api_key: Option<String>,
    pub private_key: Option<String>,
    pub testnet: bool,
    pub chain_id: u64,
}

impl Default for PredictFunConfig {
    fn default() -> Self {
        Self {
            base: ExchangeConfig::default(),
            api_url: BASE_URL.into(),
            api_key: None,
            private_key: None,
            testnet: false,
            chain_id: CHAIN_ID,
        }
    }
}

impl PredictFunConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn testnet() -> Self {
        Self {
            base: ExchangeConfig::default(),
            api_url: TESTNET_URL.into(),
            api_key: None,
            private_key: None,
            testnet: true,
            chain_id: TESTNET_CHAIN_ID,
        }
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

    pub fn with_testnet(mut self, testnet: bool) -> Self {
        self.testnet = testnet;
        if testnet {
            self.api_url = TESTNET_URL.into();
            self.chain_id = TESTNET_CHAIN_ID;
        } else {
            self.api_url = BASE_URL.into();
            self.chain_id = CHAIN_ID;
        }
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.base = self.base.with_verbose(verbose);
        self
    }

    pub fn is_authenticated(&self) -> bool {
        self.api_key.is_some() && self.private_key.is_some()
    }

    pub fn get_yield_bearing_ctf_exchange(&self) -> &'static str {
        if self.testnet {
            YIELD_BEARING_CTF_EXCHANGE_TESTNET
        } else {
            YIELD_BEARING_CTF_EXCHANGE_MAINNET
        }
    }

    pub fn get_yield_bearing_neg_risk_ctf_exchange(&self) -> &'static str {
        if self.testnet {
            YIELD_BEARING_NEG_RISK_CTF_EXCHANGE_TESTNET
        } else {
            YIELD_BEARING_NEG_RISK_CTF_EXCHANGE_MAINNET
        }
    }

    pub fn get_ctf_exchange(&self) -> &'static str {
        if self.testnet {
            CTF_EXCHANGE_TESTNET
        } else {
            CTF_EXCHANGE_MAINNET
        }
    }

    pub fn get_neg_risk_ctf_exchange(&self) -> &'static str {
        if self.testnet {
            NEG_RISK_CTF_EXCHANGE_TESTNET
        } else {
            NEG_RISK_CTF_EXCHANGE_MAINNET
        }
    }
}
