use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use drm_core::{
    DrmError, Exchange, ExchangeInfo, FetchMarketsParams, FetchOrdersParams,
    Market, Nav, Order, Orderbook, OrderSide, OrderStatus, Position, PriceLevel, PriceHistoryInterval,
    PricePoint, RateLimiter,
};

use crate::config::OpinionConfig;
use crate::error::OpinionError;

#[derive(Debug, serde::Deserialize)]
struct ApiResponse<T> {
    errno: i32,
    errmsg: Option<String>,
    result: Option<ApiResult<T>>,
}

#[derive(Debug, serde::Deserialize)]
struct ApiResult<T> {
    data: Option<T>,
    list: Option<Vec<T>>,
}

pub struct Opinion {
    config: OpinionConfig,
    client: reqwest::Client,
    rate_limiter: Arc<Mutex<RateLimiter>>,
}

impl Opinion {
    pub fn new(config: OpinionConfig) -> Result<Self, OpinionError> {
        let client = reqwest::Client::builder()
            .timeout(config.base.timeout)
            .build()?;

        let rate_limiter = Arc::new(Mutex::new(RateLimiter::new(
            config.base.rate_limit_per_second,
        )));

        Ok(Self {
            config,
            client,
            rate_limiter,
        })
    }

    pub fn with_default_config() -> Result<Self, OpinionError> {
        Self::new(OpinionConfig::default())
    }

    async fn rate_limit(&self) {
        self.rate_limiter.lock().await.wait().await;
    }

    fn auth_headers(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut b = builder;
        if let Some(ref api_key) = self.config.api_key {
            b = b.header("Authorization", format!("Bearer {api_key}"));
            b = b.header("X-API-Key", api_key);
        }
        b
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, endpoint: &str) -> Result<ApiResponse<T>, OpinionError> {
        self.rate_limit().await;

        let url = format!("{}{}", self.config.api_url, endpoint);
        let req = self.auth_headers(self.client.get(&url));
        let response = req.send().await?;

        if response.status() == 429 {
            return Err(OpinionError::RateLimited);
        }

        if response.status() == 401 || response.status() == 403 {
            return Err(OpinionError::AuthRequired);
        }

        if !response.status().is_success() {
            let msg = response.text().await.unwrap_or_default();
            return Err(OpinionError::Api(msg));
        }

        response.json().await.map_err(|e| OpinionError::Api(e.to_string()))
    }

    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        body: &impl serde::Serialize,
    ) -> Result<ApiResponse<T>, OpinionError> {
        self.rate_limit().await;

        let url = format!("{}{}", self.config.api_url, endpoint);
        let req = self.auth_headers(self.client.post(&url)).json(body);
        let response = req.send().await?;

        if response.status() == 429 {
            return Err(OpinionError::RateLimited);
        }

        if response.status() == 401 || response.status() == 403 {
            return Err(OpinionError::AuthRequired);
        }

        if !response.status().is_success() {
            let msg = response.text().await.unwrap_or_default();
            return Err(OpinionError::Api(msg));
        }

        response.json().await.map_err(|e| OpinionError::Api(e.to_string()))
    }

    fn ensure_auth(&self) -> Result<(), OpinionError> {
        if !self.config.is_authenticated() {
            return Err(OpinionError::AuthRequired);
        }
        Ok(())
    }

    fn parse_market(&self, data: serde_json::Value) -> Option<Market> {
        let obj = data.as_object()?;

        let id = obj
            .get("market_id")
            .or_else(|| obj.get("topic_id"))
            .or_else(|| obj.get("id"))
            .and_then(|v| {
                v.as_str()
                    .map(String::from)
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            })?;

        let question = obj
            .get("market_title")
            .or_else(|| obj.get("title"))
            .or_else(|| obj.get("question"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let yes_token = obj.get("yes_token_id").and_then(|v| v.as_str()).map(String::from);
        let no_token = obj.get("no_token_id").and_then(|v| v.as_str()).map(String::from);
        let yes_label = obj.get("yes_label").and_then(|v| v.as_str()).unwrap_or("Yes");
        let no_label = obj.get("no_label").and_then(|v| v.as_str()).unwrap_or("No");

        let mut outcomes = Vec::new();
        let mut token_ids = Vec::new();

        if let (Some(yt), Some(nt)) = (yes_token, no_token) {
            outcomes.push(yes_label.to_string());
            outcomes.push(no_label.to_string());
            token_ids.push(yt);
            token_ids.push(nt);
        } else if let Some(children) = obj.get("child_markets").and_then(|v| v.as_array()) {
            for child in children {
                if let Some(title) = child.get("market_title").and_then(|t| t.as_str()) {
                    outcomes.push(title.to_string());
                    if let Some(token) = child.get("yes_token_id").and_then(|t| t.as_str()) {
                        token_ids.push(token.to_string());
                    }
                }
            }
        }

        if outcomes.is_empty() {
            outcomes = vec!["Yes".into(), "No".into()];
        }

        let volume = obj
            .get("volume")
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let liquidity = obj.get("liquidity").and_then(|v| v.as_f64()).unwrap_or(0.0);

        let close_time = obj
            .get("cutoff_at")
            .or_else(|| obj.get("cutoff_time"))
            .and_then(|v| v.as_i64())
            .filter(|&t| t > 0)
            .and_then(|t| chrono::DateTime::from_timestamp(t, 0));

        let description = obj
            .get("description")
            .or_else(|| obj.get("rules"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut metadata = data.clone();
        if let Some(m) = metadata.as_object_mut() {
            m.insert("clobTokenIds".into(), serde_json::json!(token_ids));
            m.insert("chain_id".into(), serde_json::json!(self.config.chain_id));
        }

        Some(Market {
            id,
            question,
            outcomes,
            close_time,
            volume,
            liquidity,
            prices: HashMap::new(),
            metadata,
            tick_size: 0.001,
            description,
        })
    }

    fn parse_order(&self, data: &serde_json::Value) -> Order {
        let obj = data.as_object();

        let id = obj
            .and_then(|o| o.get("order_id").or(o.get("id")).or(o.get("orderID")))
            .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|n| n.to_string())))
            .unwrap_or_default();

        let market_id = obj
            .and_then(|o| o.get("topic_id").or(o.get("market_id")))
            .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|n| n.to_string())))
            .unwrap_or_default();

        let side = obj
            .and_then(|o| o.get("side_enum").or(o.get("side")))
            .map(|v| {
                if let Some(s) = v.as_str() {
                    if s.to_lowercase() == "buy" { OrderSide::Buy } else { OrderSide::Sell }
                } else if let Some(n) = v.as_i64() {
                    if n == 1 { OrderSide::Buy } else { OrderSide::Sell }
                } else {
                    OrderSide::Buy
                }
            })
            .unwrap_or(OrderSide::Buy);

        let status = obj
            .and_then(|o| o.get("status"))
            .map(|v| {
                if let Some(n) = v.as_i64() {
                    match n {
                        0 => OrderStatus::Pending,
                        1 => OrderStatus::Open,
                        2 => OrderStatus::Filled,
                        3 => OrderStatus::PartiallyFilled,
                        4 => OrderStatus::Cancelled,
                        _ => OrderStatus::Open,
                    }
                } else if let Some(s) = v.as_str() {
                    match s.to_lowercase().as_str() {
                        "filled" | "matched" => OrderStatus::Filled,
                        "cancelled" | "canceled" => OrderStatus::Cancelled,
                        "partially_filled" => OrderStatus::PartiallyFilled,
                        "pending" => OrderStatus::Pending,
                        _ => OrderStatus::Open,
                    }
                } else {
                    OrderStatus::Open
                }
            })
            .unwrap_or(OrderStatus::Open);

        let price = obj
            .and_then(|o| o.get("price"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let size = obj
            .and_then(|o| o.get("order_shares").or(o.get("maker_amount")).or(o.get("size")))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let filled = obj
            .and_then(|o| o.get("filled_shares").or(o.get("matched_amount")).or(o.get("filled")))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let outcome = obj
            .and_then(|o| o.get("outcome"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let created_at = obj
            .and_then(|o| o.get("created_at"))
            .and_then(|v| {
                v.as_i64()
                    .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                    .or_else(|| {
                        v.as_str()
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|dt| dt.with_timezone(&chrono::Utc))
                    })
            })
            .unwrap_or_else(chrono::Utc::now);

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
            updated_at: None,
        }
    }

    fn parse_position(&self, data: &serde_json::Value) -> Position {
        let obj = data.as_object();

        let market_id = obj
            .and_then(|o| o.get("topic_id").or(o.get("market_id")))
            .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|n| n.to_string())))
            .unwrap_or_default();

        let outcome = obj
            .and_then(|o| o.get("outcome").or(o.get("token_name")))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let size = obj
            .and_then(|o| o.get("shares_owned").or(o.get("size")).or(o.get("balance")))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let average_price = obj
            .and_then(|o| o.get("avg_entry_price").or(o.get("average_price")))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let current_price = obj
            .and_then(|o| o.get("current_price").or(o.get("price")))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        Position {
            market_id,
            outcome,
            size,
            average_price,
            current_price,
        }
    }

    pub async fn get_orderbook(&self, token_id: &str) -> Result<Orderbook, OpinionError> {
        let endpoint = format!("/api/v1/orderbook?token_id={token_id}");
        let resp: ApiResponse<serde_json::Value> = self.get(&endpoint).await?;

        let mut bids = Vec::new();
        let mut asks = Vec::new();

        if resp.errno == 0 {
            if let Some(result) = resp.result {
                if let Some(data) = result.data {
                    if let Some(bid_arr) = data.get("bids").and_then(|v| v.as_array()) {
                        for item in bid_arr {
                            let price = item.get("price").and_then(|p| p.as_f64().or_else(|| p.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0);
                            let size = item.get("size").and_then(|s| s.as_f64().or_else(|| s.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0);
                            if price > 0.0 && size > 0.0 {
                                bids.push(PriceLevel { price, size });
                            }
                        }
                    }
                    if let Some(ask_arr) = data.get("asks").and_then(|v| v.as_array()) {
                        for item in ask_arr {
                            let price = item.get("price").and_then(|p| p.as_f64().or_else(|| p.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0);
                            let size = item.get("size").and_then(|s| s.as_f64().or_else(|| s.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0);
                            if price > 0.0 && size > 0.0 {
                                asks.push(PriceLevel { price, size });
                            }
                        }
                    }
                }
            }
        }

        bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap_or(std::cmp::Ordering::Equal));
        asks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal));

        Ok(Orderbook {
            market_id: String::new(),
            asset_id: token_id.to_string(),
            bids,
            asks,
            last_update_id: None,
            timestamp: Some(chrono::Utc::now()),
        })
    }

    pub async fn cancel_all_orders(
        &self,
        market_id: Option<&str>,
        side: Option<OrderSide>,
    ) -> Result<serde_json::Value, OpinionError> {
        self.ensure_auth()?;

        let mut body = serde_json::Map::new();

        if let Some(mid) = market_id {
            if let Ok(market_int) = mid.parse::<i64>() {
                body.insert("market_id".into(), serde_json::json!(market_int));
            }
        }

        if let Some(s) = side {
            let side_val = match s {
                OrderSide::Buy => 1,
                OrderSide::Sell => 2,
            };
            body.insert("side".into(), serde_json::json!(side_val));
        }

        let resp: ApiResponse<serde_json::Value> = self
            .post("/api/v1/orders/cancel-all", &serde_json::Value::Object(body))
            .await?;

        if resp.errno != 0 {
            return Err(OpinionError::Api(
                resp.errmsg.unwrap_or_else(|| "cancel all orders failed".into()),
            ));
        }

        Ok(resp.result
            .and_then(|r| r.data)
            .unwrap_or(serde_json::json!({"cancelled": true})))
    }

    pub async fn enable_trading(&self) -> Result<bool, OpinionError> {
        self.ensure_auth()?;

        let resp: ApiResponse<serde_json::Value> = self
            .post("/api/v1/trading/enable", &serde_json::json!({}))
            .await?;

        if resp.errno != 0 {
            return Err(OpinionError::Api(
                resp.errmsg.unwrap_or_else(|| "enable trading failed".into()),
            ));
        }

        Ok(true)
    }

    pub async fn fetch_price_history(
        &self,
        token_id: &str,
        interval: PriceHistoryInterval,
        start_at: Option<i64>,
        end_at: Option<i64>,
    ) -> Result<Vec<PricePoint>, OpinionError> {
        self.ensure_auth()?;

        let mut endpoint = format!(
            "/api/v1/price-history?token_id={}&interval={}",
            token_id,
            interval.as_str()
        );

        if let Some(start) = start_at {
            endpoint.push_str(&format!("&start_at={start}"));
        }
        if let Some(end) = end_at {
            endpoint.push_str(&format!("&end_at={end}"));
        }

        let resp: ApiResponse<serde_json::Value> = self.get(&endpoint).await?;

        if resp.errno != 0 {
            return Err(OpinionError::Api(
                resp.errmsg.unwrap_or_else(|| "fetch price history failed".into()),
            ));
        }

        let history = resp.result
            .and_then(|r| r.list.or_else(|| r.data.and_then(|d| d.as_array().cloned())))
            .unwrap_or_default();

        let mut points = Vec::with_capacity(history.len());
        for item in history {
            let t = item.get("timestamp").or_else(|| item.get("t"))
                .and_then(|v| v.as_i64());
            let p = item.get("price").or_else(|| item.get("p"))
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
    ) -> Result<Vec<Market>, OpinionError> {
        let params = drm_core::FetchMarketsParams {
            active_only: true,
            limit: Some(limit.unwrap_or(20).min(20)),
        };

        let markets = self.fetch_markets(Some(params)).await
            .map_err(|e| OpinionError::Api(format!("{e}")))?;

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
            .take(limit.unwrap_or(20))
            .collect();

        Ok(filtered)
    }

    pub async fn split(
        &self,
        market_id: &str,
        amount_wei: u128,
        _check_approval: bool,
    ) -> Result<HashMap<String, String>, OpinionError> {
        let _ = (market_id, amount_wei);
        Err(OpinionError::NotSupported(
            "split requires on-chain transaction; use Python opinion_clob_sdk".into(),
        ))
    }

    pub async fn merge(
        &self,
        market_id: &str,
        amount_wei: u128,
        _check_approval: bool,
    ) -> Result<HashMap<String, String>, OpinionError> {
        let _ = (market_id, amount_wei);
        Err(OpinionError::NotSupported(
            "merge requires on-chain transaction; use Python opinion_clob_sdk".into(),
        ))
    }

    pub async fn redeem(
        &self,
        market_id: &str,
        _check_approval: bool,
    ) -> Result<HashMap<String, String>, OpinionError> {
        let _ = market_id;
        Err(OpinionError::NotSupported(
            "redeem requires on-chain transaction; use Python opinion_clob_sdk".into(),
        ))
    }

    pub async fn fetch_positions_for_market(&self, market: &Market) -> Result<Vec<Position>, OpinionError> {
        self.ensure_auth()?;
        self.fetch_positions(Some(&market.id))
            .await
            .map_err(|e| OpinionError::Api(format!("{e}")))
    }

    pub async fn fetch_token_ids(&self, market_id: &str) -> Result<Vec<String>, OpinionError> {
        let market = self.fetch_market(market_id)
            .await
            .map_err(|e| OpinionError::Api(format!("{e}")))?;

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
            return Err(OpinionError::Api(format!("no token IDs found for market {market_id}")));
        }

        Ok(token_ids)
    }

    pub async fn calculate_nav(&self, market: &Market) -> Result<Nav, OpinionError> {
        let balances = self.fetch_balance()
            .await
            .map_err(|e| OpinionError::Api(format!("{e}")))?;

        let cash = balances.get("USDC").copied().unwrap_or(0.0);

        let positions = self.fetch_positions_for_market(market).await?;

        Ok(Nav::calculate(cash, &positions))
    }

    pub async fn fetch_public_trades(
        &self,
        _market: Option<&Market>,
        _limit: Option<usize>,
        _page: Option<usize>,
        _side: Option<&str>,
    ) -> Result<Vec<serde_json::Value>, OpinionError> {
        Ok(vec![])
    }
}

#[async_trait]
impl Exchange for Opinion {
    fn id(&self) -> &'static str {
        "opinion"
    }

    fn name(&self) -> &'static str {
        "Opinion"
    }

    async fn fetch_markets(
        &self,
        params: Option<FetchMarketsParams>,
    ) -> Result<Vec<Market>, DrmError> {
        let params = params.unwrap_or_default();

        let status = if params.active_only { "ACTIVATED" } else { "ALL" };
        let limit = params.limit.unwrap_or(20).min(20);
        let page = 1;

        let endpoint = format!(
            "/api/v1/markets?topic_type=ALL&status={status}&page={page}&limit={limit}"
        );

        let resp: ApiResponse<serde_json::Value> = self
            .get(&endpoint)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        if resp.errno != 0 {
            return Err(DrmError::Exchange(drm_core::ExchangeError::Api(
                resp.errmsg.unwrap_or_else(|| "unknown error".into()),
            )));
        }

        let markets_list = resp
            .result
            .and_then(|r| r.list)
            .unwrap_or_default();

        let markets: Vec<Market> = markets_list
            .into_iter()
            .filter_map(|v| self.parse_market(v))
            .collect();

        Ok(markets)
    }

    async fn fetch_market(&self, market_id: &str) -> Result<Market, DrmError> {
        let endpoint = format!("/api/v1/markets/{market_id}");
        let resp: ApiResponse<serde_json::Value> = self
            .get(&endpoint)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        if resp.errno != 0 {
            return Err(DrmError::Exchange(drm_core::ExchangeError::MarketNotFound(
                market_id.into(),
            )));
        }

        resp.result
            .and_then(|r| r.data)
            .and_then(|d| self.parse_market(d))
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::MarketNotFound(market_id.into())))
    }

    async fn fetch_markets_by_slug(&self, slug: &str) -> Result<Vec<Market>, DrmError> {
        let endpoint = format!("/api/v1/markets?slug={slug}");
        let resp: ApiResponse<serde_json::Value> = self
            .get(&endpoint)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let markets_list = resp
            .result
            .and_then(|r| r.list)
            .unwrap_or_default();

        let markets: Vec<Market> = markets_list
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
        self.ensure_auth().map_err(|e| DrmError::Exchange(e.into()))?;

        let token_id = params.get("token_id").ok_or_else(|| {
            DrmError::Exchange(drm_core::ExchangeError::InvalidOrder(
                "token_id required in params".into(),
            ))
        })?;

        if price <= 0.0 || price >= 1.0 {
            return Err(DrmError::Exchange(drm_core::ExchangeError::InvalidOrder(
                "Price must be between 0 and 1".into(),
            )));
        }

        let order_data = serde_json::json!({
            "market_id": market_id.parse::<i64>().unwrap_or(0),
            "token_id": token_id,
            "side": if side == OrderSide::Buy { 1 } else { 2 },
            "price": price.to_string(),
            "size": size.to_string(),
            "order_type": "LIMIT",
        });

        let resp: ApiResponse<serde_json::Value> = self
            .post("/api/v1/orders", &order_data)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        if resp.errno != 0 {
            return Err(DrmError::Exchange(drm_core::ExchangeError::OrderRejected(
                resp.errmsg.unwrap_or_else(|| "order failed".into()),
            )));
        }

        let order_id = resp
            .result
            .and_then(|r| r.data)
            .and_then(|d| d.get("order_id").and_then(|v| v.as_str()).map(String::from))
            .unwrap_or_default();

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
        self.ensure_auth().map_err(|e| DrmError::Exchange(e.into()))?;

        let endpoint = format!("/api/v1/orders/{order_id}/cancel");
        let resp: ApiResponse<serde_json::Value> = self
            .post(&endpoint, &serde_json::json!({}))
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        if resp.errno != 0 {
            return Err(DrmError::Exchange(drm_core::ExchangeError::Api(
                resp.errmsg.unwrap_or_else(|| "cancel failed".into()),
            )));
        }

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
        self.ensure_auth().map_err(|e| DrmError::Exchange(e.into()))?;

        let endpoint = format!("/api/v1/orders/{order_id}");
        let resp: ApiResponse<serde_json::Value> = self
            .get(&endpoint)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        if resp.errno != 0 {
            return Err(DrmError::Exchange(drm_core::ExchangeError::Api(
                format!("Order {order_id} not found"),
            )));
        }

        let data = resp
            .result
            .and_then(|r| r.data)
            .ok_or_else(|| DrmError::Exchange(drm_core::ExchangeError::Api("order not found".into())))?;

        Ok(self.parse_order(&data))
    }

    async fn fetch_open_orders(
        &self,
        _params: Option<FetchOrdersParams>,
    ) -> Result<Vec<Order>, DrmError> {
        self.ensure_auth().map_err(|e| DrmError::Exchange(e.into()))?;

        let endpoint = "/api/v1/orders?status=1&page=1&limit=100";
        let resp: ApiResponse<serde_json::Value> = self
            .get(endpoint)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        if resp.errno != 0 {
            return Ok(vec![]);
        }

        let orders_list = resp.result.and_then(|r| r.list).unwrap_or_default();
        Ok(orders_list.iter().map(|o| self.parse_order(o)).collect())
    }

    async fn fetch_positions(
        &self,
        _market_id: Option<&str>,
    ) -> Result<Vec<Position>, DrmError> {
        self.ensure_auth().map_err(|e| DrmError::Exchange(e.into()))?;

        let endpoint = "/api/v1/positions?page=1&limit=100";
        let resp: ApiResponse<serde_json::Value> = self
            .get(endpoint)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        if resp.errno != 0 {
            return Ok(vec![]);
        }

        let positions_list = resp.result.and_then(|r| r.list).unwrap_or_default();
        Ok(positions_list.iter().map(|p| self.parse_position(p)).collect())
    }

    async fn fetch_balance(&self) -> Result<HashMap<String, f64>, DrmError> {
        self.ensure_auth().map_err(|e| DrmError::Exchange(e.into()))?;

        let endpoint = "/api/v1/balances";
        let resp: ApiResponse<serde_json::Value> = self
            .get(endpoint)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        if resp.errno != 0 {
            return Err(DrmError::Exchange(drm_core::ExchangeError::Api(
                resp.errmsg.unwrap_or_else(|| "fetch balance failed".into()),
            )));
        }

        let balance = resp
            .result
            .and_then(|r| r.data)
            .and_then(|d| {
                d.get("balances")
                    .and_then(|b| b.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|b| b.get("available_balance"))
                    .and_then(|v| v.as_f64())
            })
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
            has_websocket: false,
        }
    }
}
