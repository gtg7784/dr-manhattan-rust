use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use drm_core::{
    DrmError, Exchange, ExchangeInfo, FetchMarketsParams, FetchOrdersParams,
    Market, Nav, Order, OrderSide, OrderStatus, Position, PriceHistoryInterval, PricePoint,
    RateLimiter,
};

use crate::clob::{LimitlessClobClient, LimitlessOrderType, LimitlessSide};
use crate::config::LimitlessConfig;
use crate::error::LimitlessError;
use crate::websocket::LimitlessWebSocket;

pub struct Limitless {
    config: LimitlessConfig,
    client: reqwest::Client,
    rate_limiter: Arc<Mutex<RateLimiter>>,
    clob_client: Option<Arc<Mutex<LimitlessClobClient>>>,
    token_to_slug: Arc<Mutex<HashMap<String, String>>>,
    no_tokens: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl Limitless {
    pub fn new(config: LimitlessConfig) -> Result<Self, LimitlessError> {
        let client = reqwest::Client::builder()
            .timeout(config.base.timeout)
            .build()?;

        let rate_limiter = Arc::new(Mutex::new(RateLimiter::new(
            config.base.rate_limit_per_second,
        )));

        let clob_client = if let Some(ref pk) = config.private_key {
            Some(Arc::new(Mutex::new(LimitlessClobClient::new(pk, &config.api_url)?)))
        } else {
            None
        };

        Ok(Self {
            config,
            client,
            rate_limiter,
            clob_client,
            token_to_slug: Arc::new(Mutex::new(HashMap::new())),
            no_tokens: Arc::new(Mutex::new(std::collections::HashSet::new())),
        })
    }

    pub fn with_default_config() -> Result<Self, LimitlessError> {
        Self::new(LimitlessConfig::default())
    }

    pub async fn authenticate(&self) -> Result<(), LimitlessError> {
        let clob = self.clob_client.as_ref().ok_or(LimitlessError::AuthRequired)?;
        clob.lock().await.authenticate().await
    }

    async fn rate_limit(&self) {
        self.rate_limiter.lock().await.wait().await;
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, endpoint: &str) -> Result<T, LimitlessError> {
        self.rate_limit().await;

        let url = format!("{}{}", self.config.api_url, endpoint);

        if self.config.base.verbose {
            tracing::debug!("GET {}", url);
        }

        let response = self.client.get(&url).send().await?;

        if response.status() == 429 {
            return Err(LimitlessError::RateLimited);
        }

        if !response.status().is_success() {
            let msg = response.text().await.unwrap_or_default();
            return Err(LimitlessError::Api(msg));
        }

        response.json().await.map_err(|e| LimitlessError::Api(e.to_string()))
    }

    fn parse_market(&self, data: serde_json::Value) -> Option<Market> {
        let obj = data.as_object()?;

        let id = obj
            .get("slug")
            .or_else(|| obj.get("id"))
            .and_then(|v| v.as_str())
            .map(String::from)?;

        let question = obj
            .get("title")
            .or_else(|| obj.get("question"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let tokens = obj.get("tokens").and_then(|v| v.as_object());
        let outcomes = vec!["Yes".into(), "No".into()];

        let mut prices = HashMap::new();
        if let Some(yes_price) = obj.get("yesPrice").and_then(|v| v.as_f64()) {
            let normalized = if yes_price > 1.0 { yes_price / 100.0 } else { yes_price };
            prices.insert("Yes".into(), normalized);
        }
        if let Some(no_price) = obj.get("noPrice").and_then(|v| v.as_f64()) {
            let normalized = if no_price > 1.0 { no_price / 100.0 } else { no_price };
            prices.insert("No".into(), normalized);
        }

        let volume = obj
            .get("volume")
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let liquidity = obj
            .get("liquidity")
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let description = obj
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut token_ids: Vec<String> = vec![];
        if let Some(tokens_obj) = tokens {
            if let Some(yes_id) = tokens_obj.get("yes").and_then(|v| v.as_str()) {
                token_ids.push(yes_id.to_string());
            }
            if let Some(no_id) = tokens_obj.get("no").and_then(|v| v.as_str()) {
                token_ids.push(no_id.to_string());
            }
        }

        let mut metadata = data.clone();
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert("slug".to_string(), serde_json::json!(id));
            obj.insert("clobTokenIds".to_string(), serde_json::json!(token_ids));
            obj.insert("minimum_tick_size".to_string(), serde_json::json!(0.001));
        }

        Some(Market {
            id,
            question,
            outcomes,
            close_time: None,
            volume,
            liquidity,
            prices,
            metadata,
            tick_size: 0.001,
            description,
        })
    }

    async fn register_market_tokens(&self, market: &Market) {
        let slug = &market.id;
        if let Some(tokens) = market.metadata.get("tokens").and_then(|v| v.as_object()) {
            let mut token_map = self.token_to_slug.lock().await;
            let mut no_set = self.no_tokens.lock().await;

            if let Some(yes_id) = tokens.get("yes").and_then(|v| v.as_str()) {
                token_map.insert(yes_id.to_string(), slug.clone());
            }
            if let Some(no_id) = tokens.get("no").and_then(|v| v.as_str()) {
                token_map.insert(no_id.to_string(), slug.clone());
                no_set.insert(no_id.to_string());
            }
        }
    }

    pub async fn get_orderbook(&self, market_slug_or_token_id: &str) -> Result<drm_core::Orderbook, LimitlessError> {
        let is_no_token = self.no_tokens.lock().await.contains(market_slug_or_token_id);
        let slug = {
            let map = self.token_to_slug.lock().await;
            map.get(market_slug_or_token_id).cloned().unwrap_or_else(|| market_slug_or_token_id.to_string())
        };

        let endpoint = format!("/markets/{slug}/orderbook");
        let data: serde_json::Value = self.get(&endpoint).await?;

        let mut bids: Vec<drm_core::PriceLevel> = vec![];
        let mut asks: Vec<drm_core::PriceLevel> = vec![];

        if let Some(orders) = data.get("orders").or(data.get("data")).and_then(|v| v.as_array()) {
            for order in orders {
                let side = order.get("side").and_then(|v| v.as_str()).unwrap_or("");
                let price = order.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let size = order.get("size").and_then(|v| v.as_f64()).unwrap_or(0.0);

                if price > 0.0 && size > 0.0 {
                    let level = drm_core::PriceLevel { price, size };
                    if side.to_lowercase() == "buy" {
                        bids.push(level);
                    } else {
                        asks.push(level);
                    }
                }
            }
        }

        if let Some(bids_arr) = data.get("bids").and_then(|v| v.as_array()) {
            for bid in bids_arr {
                let price = bid.get("price").and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0);
                let size = bid.get("size").and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0);
                bids.push(drm_core::PriceLevel { price, size });
            }
        }

        if let Some(asks_arr) = data.get("asks").and_then(|v| v.as_array()) {
            for ask in asks_arr {
                let price = ask.get("price").and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0);
                let size = ask.get("size").and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0);
                asks.push(drm_core::PriceLevel { price, size });
            }
        }

        bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap_or(std::cmp::Ordering::Equal));
        asks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal));

        if is_no_token {
            let inverted_bids: Vec<drm_core::PriceLevel> = asks.iter()
                .map(|a| drm_core::PriceLevel { price: (1.0 - a.price * 1000.0).round() / 1000.0, size: a.size })
                .collect();
            let inverted_asks: Vec<drm_core::PriceLevel> = bids.iter()
                .map(|b| drm_core::PriceLevel { price: (1.0 - b.price * 1000.0).round() / 1000.0, size: b.size })
                .collect();

            let mut inverted_bids = inverted_bids;
            let mut inverted_asks = inverted_asks;
            inverted_bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap_or(std::cmp::Ordering::Equal));
            inverted_asks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal));

            return Ok(drm_core::Orderbook {
                market_id: slug.clone(),
                asset_id: market_slug_or_token_id.to_string(),
                bids: inverted_bids,
                asks: inverted_asks,
                last_update_id: None,
                timestamp: Some(chrono::Utc::now()),
            });
        }

        Ok(drm_core::Orderbook {
            market_id: slug,
            asset_id: market_slug_or_token_id.to_string(),
            bids,
            asks,
            last_update_id: None,
            timestamp: Some(chrono::Utc::now()),
        })
    }

    pub async fn cancel_all_orders(&self, market_id: &str) -> Result<(), DrmError> {
        let clob = self.clob_client.as_ref()
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::Authentication("not authenticated".into())))?;
        clob.lock().await.cancel_all_orders(market_id).await
            .map_err(|e| DrmError::Exchange(e.into()))
    }

    pub async fn fetch_price_history(
        &self,
        market_slug: &str,
        interval: PriceHistoryInterval,
        start_from: Option<i64>,
        end_to: Option<i64>,
    ) -> Result<Vec<PricePoint>, LimitlessError> {
        self.rate_limit().await;

        let mut endpoint = format!(
            "/markets/{}/historical-price?interval={}",
            market_slug,
            interval.as_str()
        );

        if let Some(from) = start_from {
            endpoint.push_str(&format!("&from={from}"));
        }
        if let Some(to) = end_to {
            endpoint.push_str(&format!("&to={to}"));
        }

        let data: serde_json::Value = self.get(&endpoint).await?;

        let history = data
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_else(|| {
                if data.is_array() {
                    data.as_array().cloned().unwrap_or_default()
                } else {
                    vec![]
                }
            });

        let mut points = Vec::with_capacity(history.len());
        for item in history {
            let t = item.get("timestamp")
                .or_else(|| item.get("t"))
                .or_else(|| item.get("time"))
                .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));

            let p = item.get("price")
                .or_else(|| item.get("p"))
                .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));

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
    ) -> Result<Vec<Market>, LimitlessError> {
        let params = FetchMarketsParams {
            active_only: true,
            limit: Some(limit.unwrap_or(25).min(25)),
        };

        let markets = self.fetch_markets(Some(params)).await
            .map_err(|e| LimitlessError::Api(format!("{e}")))?;

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
            .take(limit.unwrap_or(25))
            .collect();

        Ok(filtered)
    }

    fn parse_order_status(&self, status: &str) -> OrderStatus {
        match status.to_uppercase().as_str() {
            "LIVE" | "OPEN" | "ACTIVE" => OrderStatus::Open,
            "FILLED" | "MATCHED" => OrderStatus::Filled,
            "CANCELLED" | "CANCELED" => OrderStatus::Cancelled,
            "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
            _ => OrderStatus::Open,
        }
    }

    pub async fn fetch_positions_for_market(&self, market: &Market) -> Result<Vec<Position>, LimitlessError> {
        self.fetch_positions(Some(&market.id))
            .await
            .map_err(|e| LimitlessError::Api(format!("{e}")))
    }

    pub async fn fetch_token_ids(&self, market_id: &str) -> Result<Vec<String>, LimitlessError> {
        let market = self.fetch_market(market_id)
            .await
            .map_err(|e| LimitlessError::Api(format!("{e}")))?;

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
            return Err(LimitlessError::Api(format!("no token IDs found for market {market_id}")));
        }

        Ok(token_ids)
    }

    pub async fn calculate_nav(&self, market: &Market) -> Result<Nav, LimitlessError> {
        let balances = self.fetch_balance()
            .await
            .map_err(|e| LimitlessError::Api(format!("{e}")))?;

        let cash = balances.get("USDC").copied().unwrap_or(0.0);

        let positions = self.fetch_positions_for_market(market).await?;

        Ok(Nav::calculate(cash, &positions))
    }

    pub async fn fetch_feed_events(
        &self,
        market_slug: &str,
        page: Option<usize>,
        limit: Option<usize>,
    ) -> Result<Vec<serde_json::Value>, LimitlessError> {
        let page = page.unwrap_or(1);
        let limit = limit.unwrap_or(20);

        let endpoint = format!(
            "/markets/{market_slug}/get-feed-events?page={page}&limit={limit}"
        );

        let data: serde_json::Value = self.get(&endpoint).await?;

        let events = data
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_else(|| {
                if data.is_array() {
                    data.as_array().cloned().unwrap_or_default()
                } else {
                    vec![]
                }
            });

        Ok(events)
    }

    pub async fn fetch_market_events(
        &self,
        market_slug: &str,
        page: Option<usize>,
        limit: Option<usize>,
    ) -> Result<Vec<serde_json::Value>, LimitlessError> {
        let page = page.unwrap_or(1);
        let limit = limit.unwrap_or(20);

        let endpoint = format!(
            "/markets/{market_slug}/events?page={page}&limit={limit}"
        );

        let data: serde_json::Value = self.get(&endpoint).await?;

        let events = data
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_else(|| {
                if data.is_array() {
                    data.as_array().cloned().unwrap_or_default()
                } else {
                    vec![]
                }
            });

        Ok(events)
    }

    pub fn get_websocket(&self) -> LimitlessWebSocket {
        LimitlessWebSocket::new()
    }

    pub fn get_websocket_with_config(&self, auto_reconnect: bool) -> LimitlessWebSocket {
        LimitlessWebSocket::with_config(auto_reconnect)
    }
}

#[async_trait]
impl Exchange for Limitless {
    fn id(&self) -> &'static str {
        "limitless"
    }

    fn name(&self) -> &'static str {
        "Limitless"
    }

    async fn fetch_markets(
        &self,
        params: Option<FetchMarketsParams>,
    ) -> Result<Vec<Market>, DrmError> {
        let params = params.unwrap_or_default();
        let mut query = String::new();

        let limit = params.limit.unwrap_or(25).min(25);
        query.push_str(&format!("?limit={limit}"));

        let endpoint = format!("/markets/active{query}");
        let data: serde_json::Value = self
            .get(&endpoint)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let markets_arr = data
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_else(|| {
                if data.is_array() {
                    data.as_array().cloned().unwrap_or_default()
                } else {
                    vec![]
                }
            });

        let markets: Vec<Market> = markets_arr
            .into_iter()
            .filter_map(|v| self.parse_market(v))
            .collect();

        for market in &markets {
            self.register_market_tokens(market).await;
        }

        Ok(markets)
    }

    async fn fetch_market(&self, market_id: &str) -> Result<Market, DrmError> {
        let endpoint = format!("/markets/{market_id}");
        let data: serde_json::Value = self
            .get(&endpoint)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let market = self.parse_market(data)
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::MarketNotFound(market_id.into())))?;

        self.register_market_tokens(&market).await;
        Ok(market)
    }

    async fn fetch_markets_by_slug(&self, slug: &str) -> Result<Vec<Market>, DrmError> {
        let market = self.fetch_market(slug).await?;
        Ok(vec![market])
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
        let clob = self.clob_client.as_ref()
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::Authentication("not authenticated".into())))?;

        let market = self.fetch_market(market_id).await?;

        let tokens = market.metadata.get("tokens")
            .and_then(|v| v.as_object())
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::InvalidOrder("no tokens in market".into())))?;

        let token_id = params.get("token_id").cloned().or_else(|| {
            tokens.get(outcome).and_then(|v| v.as_str()).map(|s| s.to_string())
        }).ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::InvalidOrder(format!("no token_id for outcome {outcome}"))))?;

        if price <= 0.0 || price >= 1.0 {
            return Err(DrmError::Exchange(drm_core::ExchangeError::InvalidOrder(format!("price must be between 0 and 1, got {price}"))));
        }

        let exchange_address = market.metadata
            .get("venue")
            .and_then(|v| v.get("exchange"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::InvalidOrder("no venue.exchange in market".into())))?;

        let order_type = params.get("order_type")
            .map(|s| if s.to_uppercase() == "FOK" { LimitlessOrderType::Fok } else { LimitlessOrderType::Gtc })
            .unwrap_or(LimitlessOrderType::Gtc);

        let clob_side = match side {
            OrderSide::Buy => LimitlessSide::Buy,
            OrderSide::Sell => LimitlessSide::Sell,
        };

        let mut clob_guard = clob.lock().await;

        if !clob_guard.is_authenticated() {
            clob_guard.authenticate().await.map_err(|e| DrmError::Exchange(e.into()))?;
        }

        let signed_order = clob_guard.build_signed_order(
            &token_id,
            price,
            size,
            clob_side,
            order_type,
            exchange_address,
            300,
        ).map_err(|e| DrmError::Exchange(e.into()))?;

        let response = clob_guard.post_order(signed_order, order_type, market_id).await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let order_id = response.id.or(response.order_id).unwrap_or_default();
        let status = response.status.as_deref().unwrap_or("LIVE");

        Ok(Order {
            id: order_id,
            market_id: market_id.to_string(),
            outcome: outcome.to_string(),
            side,
            price,
            size,
            filled: response.filled.unwrap_or(0.0),
            status: self.parse_order_status(status),
            created_at: chrono::Utc::now(),
            updated_at: Some(chrono::Utc::now()),
        })
    }

    async fn cancel_order(
        &self,
        order_id: &str,
        market_id: Option<&str>,
    ) -> Result<Order, DrmError> {
        let clob = self.clob_client.as_ref()
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::Authentication("not authenticated".into())))?;

        clob.lock().await.cancel_order(order_id).await
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
        let clob = self.clob_client.as_ref()
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::Authentication("not authenticated".into())))?;

        let data = clob.lock().await.get_order(order_id).await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let side = match data.side.as_deref() {
            Some("BUY") | Some("buy") => OrderSide::Buy,
            _ => OrderSide::Sell,
        };

        Ok(Order {
            id: data.id.or(data.order_id).unwrap_or_default(),
            market_id: data.market_slug.unwrap_or_default(),
            outcome: String::new(),
            side,
            price: data.price.unwrap_or(0.0),
            size: data.size.or(data.original_size).unwrap_or(0.0),
            filled: data.filled.unwrap_or(0.0),
            status: self.parse_order_status(data.status.as_deref().unwrap_or("OPEN")),
            created_at: chrono::Utc::now(),
            updated_at: Some(chrono::Utc::now()),
        })
    }

    async fn fetch_open_orders(
        &self,
        params: Option<FetchOrdersParams>,
    ) -> Result<Vec<Order>, DrmError> {
        let clob = self.clob_client.as_ref()
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::Authentication("not authenticated".into())))?;

        let market_id = params.and_then(|p| p.market_id);

        let orders = clob.lock().await.get_open_orders(market_id.as_deref()).await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let result: Vec<Order> = orders.into_iter().map(|data| {
            let side = match data.side.as_deref() {
                Some("BUY") | Some("buy") => OrderSide::Buy,
                _ => OrderSide::Sell,
            };

            Order {
                id: data.id.or(data.order_id).unwrap_or_default(),
                market_id: data.market_slug.unwrap_or_default(),
                outcome: String::new(),
                side,
                price: data.price.unwrap_or(0.0),
                size: data.size.or(data.original_size).unwrap_or(0.0),
                filled: data.filled.unwrap_or(0.0),
                status: self.parse_order_status(data.status.as_deref().unwrap_or("OPEN")),
                created_at: chrono::Utc::now(),
                updated_at: Some(chrono::Utc::now()),
            }
        }).collect();

        Ok(result)
    }

    async fn fetch_positions(
        &self,
        market_id: Option<&str>,
    ) -> Result<Vec<Position>, DrmError> {
        let clob = self.clob_client.as_ref()
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::Authentication("not authenticated".into())))?;

        let positions = clob.lock().await.get_positions(market_id).await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let result: Vec<Position> = positions.into_iter().map(|p| {
            Position {
                market_id: p.market_slug.unwrap_or_default(),
                outcome: p.outcome.unwrap_or_default(),
                size: p.size.unwrap_or(0.0),
                average_price: p.average_price.unwrap_or(0.0),
                current_price: p.current_price.unwrap_or(0.0),
            }
        }).collect();

        Ok(result)
    }

    async fn fetch_balance(&self) -> Result<HashMap<String, f64>, DrmError> {
        let clob = self.clob_client.as_ref()
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::Authentication("not authenticated".into())))?;

        let balance = clob.lock().await.get_balance().await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let mut result = HashMap::new();
        if let Some(b) = balance.balance {
            if let Ok(val) = b.parse::<f64>() {
                result.insert("USDC".to_string(), val);
            }
        }
        if let Some(a) = balance.available {
            if let Ok(val) = a.parse::<f64>() {
                result.insert("USDC_AVAILABLE".to_string(), val);
            }
        }

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
