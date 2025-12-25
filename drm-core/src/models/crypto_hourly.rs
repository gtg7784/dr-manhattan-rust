use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarketDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CryptoMarketType {
    UpDown,
    StrikePrice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoHourlyMarket {
    pub token_symbol: String,
    pub expiry_time: DateTime<Utc>,
    pub strike_price: Option<f64>,
    pub direction: Option<MarketDirection>,
    pub market_type: Option<CryptoMarketType>,
}

pub fn normalize_token_symbol(token: &str) -> String {
    match token.to_uppercase().as_str() {
        "BITCOIN" => "BTC".to_string(),
        "ETHEREUM" => "ETH".to_string(),
        "SOLANA" => "SOL".to_string(),
        other => other.to_string(),
    }
}
