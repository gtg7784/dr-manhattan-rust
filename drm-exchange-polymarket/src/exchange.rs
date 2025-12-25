use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use drm_core::{
    CryptoHourlyMarket, CryptoMarketType, DrmError, Exchange, ExchangeInfo, FetchMarketsParams,
    FetchOrdersParams, Market, MarketDirection, Nav, Order, Orderbook, OrderSide, OrderStatus,
    Position, PriceLevel, PriceHistoryInterval, PricePoint, PublicTrade, RateLimiter,
    normalize_token_symbol,
};
use regex::Regex;

use crate::client::HttpClient;
use crate::clob::{ApiCredentials, ClobClient, ClobOrderData, ClobOrderSide, ClobOrderType, OrderArgs};
use crate::config::PolymarketConfig;
use crate::error::PolymarketError;
use crate::websocket::PolymarketWebSocket;

pub struct Polymarket {
    config: PolymarketConfig,
    client: HttpClient,
    rate_limiter: Arc<Mutex<RateLimiter>>,
    clob_client: Option<Arc<Mutex<ClobClient>>>,
}

impl Polymarket {
    pub fn new(config: PolymarketConfig) -> Result<Self, PolymarketError> {
        let client = HttpClient::new(&config)?;
        let rate_limiter = Arc::new(Mutex::new(RateLimiter::new(
            config.base.rate_limit_per_second,
        )));

        let clob_client = if let Some(ref private_key) = config.private_key {
            let clob = ClobClient::new(private_key, config.funder.as_deref())?;
            Some(Arc::new(Mutex::new(clob)))
        } else {
            None
        };

        Ok(Self {
            config,
            client,
            rate_limiter,
            clob_client,
        })
    }

    pub fn with_default_config() -> Result<Self, PolymarketError> {
        Self::new(PolymarketConfig::default())
    }

    /// Derives API credentials. Call before trading.
    pub async fn init_trading(&self) -> Result<ApiCredentials, PolymarketError> {
        let clob = self
            .clob_client
            .as_ref()
            .ok_or_else(|| PolymarketError::Auth("private key required for trading".into()))?;

        let mut clob = clob.lock().await;
        clob.derive_api_credentials().await
    }

    pub async fn set_api_credentials(&self, creds: ApiCredentials) -> Result<(), PolymarketError> {
        let clob = self
            .clob_client
            .as_ref()
            .ok_or_else(|| PolymarketError::Auth("private key required for trading".into()))?;

        clob.lock().await.set_api_credentials(creds);
        Ok(())
    }

    async fn rate_limit(&self) {
        self.rate_limiter.lock().await.wait().await;
    }

    pub async fn get_orderbook(
        &self,
        token_id: &str,
    ) -> Result<Orderbook, PolymarketError> {
        self.rate_limit().await;

        let url = format!("{}/book?token_id={}", crate::clob::CLOB_URL, token_id);
        let response = reqwest::get(&url)
            .await
            .map_err(|e| PolymarketError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Ok(Orderbook {
                market_id: String::new(),
                asset_id: token_id.to_string(),
                bids: vec![],
                asks: vec![],
                last_update_id: None,
                timestamp: Some(chrono::Utc::now()),
            });
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| PolymarketError::Api(e.to_string()))?;

        let parse_levels = |key: &str| -> Vec<PriceLevel> {
            data.get(key)
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| {
                            let price = item
                                .get("price")
                                .and_then(|p| p.as_str().and_then(|s| s.parse().ok()).or_else(|| p.as_f64()))
                                .unwrap_or(0.0);
                            let size = item
                                .get("size")
                                .and_then(|s| s.as_str().and_then(|s| s.parse().ok()).or_else(|| s.as_f64()))
                                .unwrap_or(0.0);
                            if price > 0.0 && size > 0.0 {
                                Some(PriceLevel { price, size })
                            } else {
                                None
                            }
                        })
                        .collect()
                })
                .unwrap_or_default()
        };

        Ok(Orderbook {
            market_id: String::new(),
            asset_id: token_id.to_string(),
            bids: parse_levels("bids"),
            asks: parse_levels("asks"),
            last_update_id: None,
            timestamp: Some(chrono::Utc::now()),
        })
    }

    fn parse_clob_order(&self, data: &ClobOrderData) -> Order {
        let id = data.id.clone().unwrap_or_default();
        let market_id = data.market.clone().unwrap_or_default();
        let outcome = data.outcome.clone().unwrap_or_default();

        let side = match data.side.as_deref() {
            Some("BUY") | Some("buy") => OrderSide::Buy,
            _ => OrderSide::Sell,
        };

        let price = data
            .price
            .as_ref()
            .and_then(|p| p.parse().ok())
            .unwrap_or(0.0);

        let size = data
            .original_size
            .as_ref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);

        let filled = data
            .size_matched
            .as_ref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);

        let status = match data.status.as_deref() {
            Some("LIVE") | Some("live") => OrderStatus::Open,
            Some("FILLED") | Some("filled") | Some("MATCHED") | Some("matched") => {
                OrderStatus::Filled
            }
            Some("CANCELLED") | Some("cancelled") | Some("CANCELED") | Some("canceled") => {
                OrderStatus::Cancelled
            }
            Some("PARTIALLY_FILLED") | Some("partially_filled") => OrderStatus::PartiallyFilled,
            _ => OrderStatus::Open,
        };

        let created_at = data
            .created_at
            .as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);

        let updated_at = data
            .updated_at
            .as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        Order {
            id,
            market_id,
            outcome,
            side,
            price,
            size,
            filled,
            status,
            created_at,
            updated_at,
        }
    }

    pub async fn fetch_token_ids(&self, condition_id: &str) -> Result<Vec<String>, PolymarketError> {
        self.rate_limit().await;

        let url = format!("{}/simplified-markets", crate::clob::CLOB_URL);
        let response = reqwest::get(&url)
            .await
            .map_err(|e| PolymarketError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(PolymarketError::Api("failed to fetch markets".into()));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| PolymarketError::Api(e.to_string()))?;

        let markets_list = data
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_else(|| data.as_array().cloned().unwrap_or_default());

        for market in markets_list {
            let market_id = market
                .get("condition_id")
                .or_else(|| market.get("id"))
                .and_then(|v| v.as_str());

            if market_id == Some(condition_id) {
                if let Some(tokens) = market.get("tokens").and_then(|v| v.as_array()) {
                    let token_ids: Vec<String> = tokens
                        .iter()
                        .filter_map(|t| {
                            if let Some(obj) = t.as_object() {
                                obj.get("token_id").and_then(|v| v.as_str()).map(String::from)
                            } else if let Some(s) = t.as_str() {
                                Some(s.to_string())
                            } else {
                                None
                            }
                        })
                        .collect();

                    if !token_ids.is_empty() {
                        return Ok(token_ids);
                    }
                }

                if let Some(clob_tokens) = market.get("clobTokenIds").and_then(|v| v.as_array()) {
                    let token_ids: Vec<String> = clob_tokens
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                    if !token_ids.is_empty() {
                        return Ok(token_ids);
                    }
                }
            }
        }

        Err(PolymarketError::Api(format!("token IDs not found for market {}", condition_id)))
    }

    pub async fn fetch_price_history(
        &self,
        market_id: &str,
        outcome: Option<usize>,
        interval: PriceHistoryInterval,
        fidelity: Option<u32>,
    ) -> Result<Vec<PricePoint>, PolymarketError> {
        self.rate_limit().await;

        let token_ids = self.fetch_token_ids(market_id).await?;

        if token_ids.is_empty() {
            return Err(PolymarketError::Api("no token IDs found for market".into()));
        }

        let outcome_idx = outcome.unwrap_or(0);
        if outcome_idx >= token_ids.len() {
            return Err(PolymarketError::Api(format!(
                "outcome index {} out of range for market {}",
                outcome_idx, market_id
            )));
        }

        let token_id = &token_ids[outcome_idx];
        let fidelity = fidelity.unwrap_or(10);

        let url = format!(
            "{}/prices-history?market={}&interval={}&fidelity={}",
            crate::clob::CLOB_URL,
            token_id,
            interval.as_str(),
            fidelity
        );

        let response = reqwest::get(&url)
            .await
            .map_err(|e| PolymarketError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(PolymarketError::Api(format!(
                "fetch price history failed: {} - {}",
                status, text
            )));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| PolymarketError::Api(e.to_string()))?;

        let history = data
            .get("history")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut points = Vec::with_capacity(history.len());
        for item in history {
            let t = item.get("t").and_then(|v| v.as_i64());
            let p = item.get("p").and_then(|v| {
                v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            });

            if let (Some(timestamp), Some(price)) = (t, p) {
                if let Some(dt) = chrono::DateTime::from_timestamp(timestamp, 0) {
                    points.push(PricePoint {
                        timestamp: dt,
                        price,
                        raw: item.clone(),
                    });
                }
            }
        }

        points.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        Ok(points)
    }

    pub async fn search_markets(
        &self,
        query: Option<&str>,
        min_liquidity: Option<f64>,
        binary_only: Option<bool>,
        limit: Option<usize>,
    ) -> Result<Vec<Market>, PolymarketError> {
        let params = drm_core::FetchMarketsParams {
            active_only: true,
            limit: Some(limit.unwrap_or(100)),
            ..Default::default()
        };

        let markets = self.fetch_markets(Some(params)).await
            .map_err(|e| PolymarketError::Api(format!("{}", e)))?;

        let query_lower = query.map(|q| q.to_lowercase());
        let min_liq = min_liquidity.unwrap_or(0.0);
        let binary = binary_only.unwrap_or(false);

        let filtered: Vec<Market> = markets
            .into_iter()
            .filter(|m| {
                if binary && !m.is_binary() {
                    return false;
                }

                if m.liquidity < min_liq {
                    return false;
                }

                if let Some(ref q) = query_lower {
                    let text = format!(
                        "{} {}",
                        m.question.to_lowercase(),
                        m.description.to_lowercase()
                    );
                    if !text.contains(q) {
                        return false;
                    }
                }

                true
            })
            .take(limit.unwrap_or(100))
            .collect();

        Ok(filtered)
    }

    pub async fn fetch_public_trades(
        &self,
        market: Option<&str>,
        limit: Option<usize>,
        offset: Option<usize>,
        user: Option<&str>,
        side: Option<&str>,
        taker_only: Option<bool>,
    ) -> Result<Vec<PublicTrade>, PolymarketError> {
        self.rate_limit().await;

        const DATA_API_URL: &str = "https://data-api.polymarket.com";
        const PAGE_SIZE: usize = 500;

        let total_limit = limit.unwrap_or(100);
        let initial_offset = offset.unwrap_or(0);
        let taker = taker_only.unwrap_or(true);

        let mut all_trades: Vec<PublicTrade> = Vec::new();
        let mut current_offset = initial_offset;

        loop {
            let page_limit = PAGE_SIZE.min(total_limit - all_trades.len());
            if page_limit == 0 {
                break;
            }

            let mut url = format!(
                "{}/trades?limit={}&offset={}&takerOnly={}",
                DATA_API_URL,
                page_limit,
                current_offset,
                taker
            );

            if let Some(m) = market {
                url.push_str(&format!("&market={}", m));
            }
            if let Some(u) = user {
                url.push_str(&format!("&user={}", u));
            }
            if let Some(s) = side {
                url.push_str(&format!("&side={}", s));
            }

            let response = reqwest::get(&url)
                .await
                .map_err(|e| PolymarketError::Network(e.to_string()))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(PolymarketError::Api(format!(
                    "fetch public trades failed: {} - {}",
                    status, text
                )));
            }

            let data: Vec<serde_json::Value> = response
                .json()
                .await
                .map_err(|e| PolymarketError::Api(e.to_string()))?;

            if data.is_empty() {
                break;
            }

            for item in &data {
                if let Some(trade) = self.parse_public_trade(item) {
                    all_trades.push(trade);
                }
            }

            if data.len() < page_limit {
                break;
            }

            current_offset += data.len();

            if all_trades.len() >= total_limit {
                break;
            }
        }

        all_trades.truncate(total_limit);
        Ok(all_trades)
    }

    fn parse_public_trade(&self, data: &serde_json::Value) -> Option<PublicTrade> {
        let obj = data.as_object()?;

        let proxy_wallet = obj.get("proxyWallet")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let side = obj.get("side")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let asset = obj.get("asset")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let condition_id = obj.get("conditionId")
            .or_else(|| obj.get("market"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let size = obj.get("size")
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let price = obj.get("price")
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let timestamp = obj.get("timestamp")
            .or_else(|| obj.get("matchTime"))
            .and_then(|v| {
                v.as_i64()
                    .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                    .or_else(|| {
                        v.as_str().and_then(|s| {
                            chrono::DateTime::parse_from_rfc3339(s)
                                .ok()
                                .map(|dt| dt.with_timezone(&chrono::Utc))
                                .or_else(|| {
                                    s.parse::<i64>()
                                        .ok()
                                        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                                })
                        })
                    })
            })
            .unwrap_or_else(chrono::Utc::now);

        Some(PublicTrade {
            proxy_wallet,
            side,
            asset,
            condition_id,
            size,
            price,
            timestamp,
            title: obj.get("title").and_then(|v| v.as_str()).map(String::from),
            slug: obj.get("slug").and_then(|v| v.as_str()).map(String::from),
            icon: obj.get("icon").and_then(|v| v.as_str()).map(String::from),
            event_slug: obj.get("eventSlug").and_then(|v| v.as_str()).map(String::from),
            outcome: obj.get("outcome").and_then(|v| v.as_str()).map(String::from),
            outcome_index: obj.get("outcomeIndex").and_then(|v| v.as_u64()).map(|n| n as u32),
            name: obj.get("name").and_then(|v| v.as_str()).map(String::from),
            pseudonym: obj.get("pseudonym").and_then(|v| v.as_str()).map(String::from),
            bio: obj.get("bio").and_then(|v| v.as_str()).map(String::from),
            profile_image: obj.get("profileImage").and_then(|v| v.as_str()).map(String::from),
            profile_image_optimized: obj.get("profileImageOptimized").and_then(|v| v.as_str()).map(String::from),
            transaction_hash: obj.get("transactionHash").and_then(|v| v.as_str()).map(String::from),
        })
    }

    pub async fn fetch_positions_for_market(&self, market: &Market) -> Result<Vec<Position>, PolymarketError> {
        self.fetch_positions(Some(&market.id))
            .await
            .map_err(|e| PolymarketError::Api(format!("{}", e)))
    }

    pub async fn calculate_nav(&self, market: &Market) -> Result<Nav, PolymarketError> {
        let balances = self.fetch_balance()
            .await
            .map_err(|e| PolymarketError::Api(format!("{}", e)))?;

        let cash = balances.get("USDC").copied().unwrap_or(0.0);

        let positions = self.fetch_positions_for_market(market).await?;

        Ok(Nav::calculate(cash, &positions))
    }

    pub async fn find_crypto_hourly_market(
        &self,
        token_symbol: Option<&str>,
        min_liquidity: f64,
        limit: usize,
        is_active: bool,
        is_expired: bool,
        tag_id: Option<&str>,
    ) -> Result<Option<(Market, CryptoHourlyMarket)>, PolymarketError> {
        const TAG_1H: &str = "102175";

        let tag = tag_id.unwrap_or(TAG_1H);

        let mut all_markets: Vec<Market> = Vec::new();
        let mut offset = 0usize;
        const PAGE_SIZE: usize = 100;

        while all_markets.len() < limit {
            self.rate_limit().await;

            let fetch_limit = PAGE_SIZE.min(limit - all_markets.len());
            let url = format!(
                "{}/markets?active=true&closed=false&limit={}&offset={}&order=volume&ascending=false&tag_id={}",
                self.config.gamma_url, fetch_limit, offset, tag
            );

            let response = reqwest::get(&url)
                .await
                .map_err(|e| PolymarketError::Network(e.to_string()))?;

            if !response.status().is_success() {
                break;
            }

            let data: Vec<serde_json::Value> = response
                .json()
                .await
                .map_err(|e| PolymarketError::Api(e.to_string()))?;

            if data.is_empty() {
                break;
            }

            for item in &data {
                if let Some(market) = self.parse_market(item.clone()) {
                    all_markets.push(market);
                }
            }

            offset += data.len();

            if data.len() < PAGE_SIZE {
                break;
            }
        }

        let up_down_pattern = Regex::new(
            r"(?i)(?P<token>Bitcoin|Ethereum|Solana|BTC|ETH|SOL|XRP)\s+Up or Down"
        ).unwrap();

        let strike_pattern = Regex::new(
            r"(?i)(?:(?P<token1>BTC|ETH|SOL|BITCOIN|ETHEREUM|SOLANA)\s+.*?(?P<direction>above|below|over|under|reach)\s+[\$]?(?P<price1>[\d,]+(?:\.\d+)?))|(?:[\$]?(?P<price2>[\d,]+(?:\.\d+)?)\s+.*?(?P<token2>BTC|ETH|SOL|BITCOIN|ETHEREUM|SOLANA))"
        ).unwrap();

        let now = chrono::Utc::now();

        for market in all_markets {
            if !market.is_binary() {
                continue;
            }

            if market.liquidity < min_liquidity {
                continue;
            }

            if let Some(close_time) = market.close_time {
                let time_until_expiry = (close_time - now).num_seconds();

                if is_expired {
                    if time_until_expiry > 0 {
                        continue;
                    }
                } else if time_until_expiry <= 0 {
                    continue;
                }

                if is_active && !is_expired && time_until_expiry > 3600 {
                    continue;
                }
            }

            if let Some(caps) = up_down_pattern.captures(&market.question) {
                let parsed_token = normalize_token_symbol(caps.name("token").unwrap().as_str());

                if let Some(filter) = token_symbol {
                    if parsed_token != normalize_token_symbol(filter) {
                        continue;
                    }
                }

                let expiry = market.close_time.unwrap_or_else(|| now + chrono::Duration::hours(1));

                let crypto_market = CryptoHourlyMarket {
                    token_symbol: parsed_token,
                    expiry_time: expiry,
                    strike_price: None,
                    direction: None,
                    market_type: Some(CryptoMarketType::UpDown),
                };

                return Ok(Some((market, crypto_market)));
            }

            if let Some(caps) = strike_pattern.captures(&market.question) {
                let token_str = caps.name("token1")
                    .or_else(|| caps.name("token2"))
                    .map(|m| m.as_str())
                    .unwrap_or("");

                let parsed_token = normalize_token_symbol(token_str);

                let price_str = caps.name("price1")
                    .or_else(|| caps.name("price2"))
                    .map(|m| m.as_str())
                    .unwrap_or("0");

                let parsed_price: f64 = price_str.replace(',', "").parse().unwrap_or(0.0);

                if let Some(filter) = token_symbol {
                    if parsed_token != normalize_token_symbol(filter) {
                        continue;
                    }
                }

                let expiry = market.close_time.unwrap_or_else(|| now + chrono::Duration::hours(1));

                let direction = caps.name("direction").map(|m| {
                    match m.as_str().to_lowercase().as_str() {
                        "above" | "over" | "reach" => MarketDirection::Up,
                        _ => MarketDirection::Down,
                    }
                });

                let crypto_market = CryptoHourlyMarket {
                    token_symbol: parsed_token,
                    expiry_time: expiry,
                    strike_price: Some(parsed_price),
                    direction,
                    market_type: Some(CryptoMarketType::StrikePrice),
                };

                return Ok(Some((market, crypto_market)));
            }
        }

        Ok(None)
    }

    pub fn get_websocket(&self) -> PolymarketWebSocket {
        PolymarketWebSocket::new()
    }

    pub fn get_websocket_with_config(&self, auto_reconnect: bool) -> PolymarketWebSocket {
        PolymarketWebSocket::with_config(auto_reconnect)
    }

    pub fn parse_market_identifier(identifier: &str) -> String {
        if identifier.is_empty() {
            return String::new();
        }

        if identifier.starts_with("http") {
            let without_query = identifier.split('?').next().unwrap_or(identifier);
            let parts: Vec<&str> = without_query.trim_end_matches('/').split('/').collect();

            if let Some(idx) = parts.iter().position(|&p| p == "event") {
                if idx + 1 < parts.len() {
                    return parts[idx + 1].to_string();
                }
            }

            return parts.last().unwrap_or(&"").to_string();
        }

        identifier.to_string()
    }

    pub async fn get_tag_by_slug(&self, slug: &str) -> Result<serde_json::Value, PolymarketError> {
        if slug.is_empty() {
            return Err(PolymarketError::Api("slug must be non-empty".into()));
        }

        self.rate_limit().await;

        let url = format!("{}/tags/slug/{}", self.config.gamma_url, slug);
        let response = reqwest::get(&url)
            .await
            .map_err(|e| PolymarketError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(PolymarketError::Api(format!("get_tag_by_slug failed: {} - {}", status, text)));
        }

        response.json().await.map_err(|e| PolymarketError::Api(e.to_string()))
    }

    fn parse_market(&self, data: serde_json::Value) -> Option<Market> {
        let obj = data.as_object()?;

        let id = obj.get("id")?.as_str()?.to_string();
        let question = obj.get("question")?.as_str().unwrap_or("").to_string();

        let outcomes: Vec<String> = obj
            .get("outcomes")
            .and_then(|v| {
                if let Some(arr) = v.as_array() {
                    Some(arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                } else if let Some(s) = v.as_str() {
                    serde_json::from_str(s).ok()
                } else {
                    None
                }
            })
            .unwrap_or_else(|| vec!["Yes".into(), "No".into()]);

        let prices_raw = obj.get("outcomePrices");
        let mut prices = HashMap::new();

        if let Some(prices_val) = prices_raw {
            let price_list: Vec<f64> = if let Some(arr) = prices_val.as_array() {
                arr.iter()
                    .filter_map(|v| v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64()))
                    .collect()
            } else if let Some(s) = prices_val.as_str() {
                // API returns JSON-encoded string array: "[\"0.0045\", \"0.9955\"]"
                serde_json::from_str::<Vec<String>>(s)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|p| p.parse::<f64>().ok())
                    .collect()
            } else {
                vec![]
            };

            for (outcome, price) in outcomes.iter().zip(price_list.iter()) {
                if *price > 0.0 {
                    prices.insert(outcome.clone(), *price);
                }
            }
        }

        let volume = obj
            .get("volumeNum")
            .or_else(|| obj.get("volume"))
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let liquidity = obj
            .get("liquidityNum")
            .or_else(|| obj.get("liquidity"))
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let tick_size = obj
            .get("minimum_tick_size")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.01);

        let description = obj
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Some(Market {
            id,
            question,
            outcomes,
            close_time: None,
            volume,
            liquidity,
            prices,
            metadata: data,
            tick_size,
            description,
        })
    }
}

#[async_trait]
impl Exchange for Polymarket {
    fn id(&self) -> &'static str {
        "polymarket"
    }

    fn name(&self) -> &'static str {
        "Polymarket"
    }

    async fn fetch_markets(
        &self,
        params: Option<FetchMarketsParams>,
    ) -> Result<Vec<Market>, DrmError> {
        self.rate_limit().await;

        let params = params.unwrap_or_default();
        let mut query = String::new();

        if params.active_only {
            query.push_str("?active=true&closed=false");
        }

        if let Some(limit) = params.limit {
            if query.is_empty() {
                query.push('?');
            } else {
                query.push('&');
            }
            query.push_str(&format!("limit={}", limit));
        }

        let endpoint = format!("/markets{}", query);
        let data: Vec<serde_json::Value> = self
            .client
            .get_gamma(&endpoint)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let markets: Vec<Market> = data
            .into_iter()
            .filter_map(|v| self.parse_market(v))
            .collect();

        Ok(markets)
    }

    async fn fetch_market(&self, market_id: &str) -> Result<Market, DrmError> {
        self.rate_limit().await;

        let endpoint = format!("/markets/{}", market_id);
        let data: serde_json::Value = self
            .client
            .get_gamma(&endpoint)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        self.parse_market(data)
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::MarketNotFound(market_id.into())))
    }

    async fn fetch_markets_by_slug(&self, slug: &str) -> Result<Vec<Market>, DrmError> {
        self.rate_limit().await;

        let slug = if slug.starts_with("http") {
            slug.split('/')
                .find(|s| !s.is_empty() && *s != "event")
                .unwrap_or(slug)
                .split('?')
                .next()
                .unwrap_or(slug)
        } else {
            slug
        };

        let endpoint = format!("/events?slug={}", slug);
        let events: Vec<serde_json::Value> = self
            .client
            .get_gamma(&endpoint)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let event = events
            .into_iter()
            .next()
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::MarketNotFound(slug.into())))?;

        let markets_data = event
            .get("markets")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let markets: Vec<Market> = markets_data
            .into_iter()
            .filter_map(|v| self.parse_market(v))
            .collect();

        if markets.is_empty() {
            return Err(DrmError::Exchange(drm_core::ExchangeError::MarketNotFound(slug.into())));
        }

        Ok(markets)
    }

    async fn create_order(
        &self,
        market_id: &str,
        outcome: &str,
        side: OrderSide,
        price: f64,
        size: f64,
        params: HashMap<String, String>,
    ) -> Result<Order, DrmError> {
        let clob = self
            .clob_client
            .as_ref()
            .ok_or_else(|| {
                DrmError::Exchange(drm_core::ExchangeError::Authentication(
                    "private key required for trading".into(),
                ))
            })?;

        let token_id = params.get("token_id").cloned().unwrap_or_else(|| {
            format!("{}:{}", market_id, outcome)
        });

        let order_type_str = params.get("order_type").map(|s| s.as_str()).unwrap_or("GTC");
        let order_type = match order_type_str.to_uppercase().as_str() {
            "FOK" => ClobOrderType::Fok,
            "IOC" => ClobOrderType::Ioc,
            _ => ClobOrderType::Gtc,
        };

        let clob_side = match side {
            OrderSide::Buy => ClobOrderSide::Buy,
            OrderSide::Sell => ClobOrderSide::Sell,
        };

        let args = OrderArgs {
            token_id: token_id.clone(),
            price,
            size,
            side: clob_side,
        };

        let clob = clob.lock().await;
        let signed_order = clob
            .create_order(args)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let response = clob
            .post_order(signed_order, order_type)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let order_id = response
            .order_id
            .ok_or_else(|| {
                let msg = response.error_msg.unwrap_or_else(|| "unknown error".into());
                DrmError::Exchange(drm_core::ExchangeError::OrderRejected(msg))
            })?;

        Ok(Order {
            id: order_id,
            market_id: market_id.to_string(),
            outcome: outcome.to_string(),
            side,
            price,
            size,
            filled: 0.0,
            status: OrderStatus::Open,
            created_at: chrono::Utc::now(),
            updated_at: None,
        })
    }

    async fn cancel_order(
        &self,
        order_id: &str,
        market_id: Option<&str>,
    ) -> Result<Order, DrmError> {
        let clob = self
            .clob_client
            .as_ref()
            .ok_or_else(|| {
                DrmError::Exchange(drm_core::ExchangeError::Authentication(
                    "private key required for trading".into(),
                ))
            })?;

        clob.lock()
            .await
            .cancel_order(order_id)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        Ok(Order {
            id: order_id.to_string(),
            market_id: market_id.unwrap_or("").to_string(),
            outcome: String::new(),
            side: OrderSide::Buy,
            price: 0.0,
            size: 0.0,
            filled: 0.0,
            status: OrderStatus::Cancelled,
            created_at: chrono::Utc::now(),
            updated_at: Some(chrono::Utc::now()),
        })
    }

    async fn fetch_order(
        &self,
        order_id: &str,
        _market_id: Option<&str>,
    ) -> Result<Order, DrmError> {
        let clob = self
            .clob_client
            .as_ref()
            .ok_or_else(|| {
                DrmError::Exchange(drm_core::ExchangeError::Authentication(
                    "private key required".into(),
                ))
            })?;

        let data = clob
            .lock()
            .await
            .get_order(order_id)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        Ok(self.parse_clob_order(&data))
    }

    async fn fetch_open_orders(
        &self,
        _params: Option<FetchOrdersParams>,
    ) -> Result<Vec<Order>, DrmError> {
        let clob = self
            .clob_client
            .as_ref()
            .ok_or_else(|| {
                DrmError::Exchange(drm_core::ExchangeError::Authentication(
                    "private key required".into(),
                ))
            })?;

        let orders = clob
            .lock()
            .await
            .get_open_orders()
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        Ok(orders.iter().map(|o| self.parse_clob_order(o)).collect())
    }

    async fn fetch_positions(
        &self,
        market_id: Option<&str>,
    ) -> Result<Vec<Position>, DrmError> {
        let clob = self
            .clob_client
            .as_ref()
            .ok_or_else(|| {
                DrmError::Exchange(drm_core::ExchangeError::Authentication(
                    "private key required".into(),
                ))
            })?;

        let market_id = match market_id {
            Some(id) => id,
            None => return Ok(vec![]),
        };

        let market = self.fetch_market(market_id).await?;

        let token_ids: Vec<String> = market
            .metadata
            .get("clobTokenIds")
            .and_then(|v| {
                if let Some(arr) = v.as_array() {
                    Some(arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                } else if let Some(s) = v.as_str() {
                    serde_json::from_str(s).ok()
                } else {
                    None
                }
            })
            .unwrap_or_default();

        if token_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut positions = Vec::new();
        let clob = clob.lock().await;

        for (i, token_id) in token_ids.iter().enumerate() {
            let balance = clob.get_token_balance(token_id).await.unwrap_or(0.0);

            if balance > 0.0 {
                let outcome = market
                    .outcomes
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| if i == 0 { "Yes".into() } else { "No".into() });

                let current_price = market.prices.get(&outcome).copied().unwrap_or(0.0);

                positions.push(Position {
                    market_id: market_id.to_string(),
                    outcome,
                    size: balance,
                    average_price: 0.0,
                    current_price,
                });
            }
        }

        Ok(positions)
    }

    async fn fetch_balance(&self) -> Result<HashMap<String, f64>, DrmError> {
        let clob = self
            .clob_client
            .as_ref()
            .ok_or_else(|| {
                DrmError::Exchange(drm_core::ExchangeError::Authentication(
                    "private key required".into(),
                ))
            })?;

        let data = clob
            .lock()
            .await
            .get_balance_allowance()
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let balance = data
            .balance
            .and_then(|b| b.parse::<f64>().ok())
            .map(|b| b / 1_000_000.0)
            .unwrap_or(0.0);

        let mut result = HashMap::new();
        result.insert("USDC".to_string(), balance);
        Ok(result)
    }

    fn describe(&self) -> ExchangeInfo {
        ExchangeInfo {
            id: self.id(),
            name: self.name(),
            has_fetch_markets: true,
            has_create_order: self.config.is_authenticated(),
            has_websocket: true,
        }
    }
}
