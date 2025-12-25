use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use drm_core::{
    DrmError, Exchange, ExchangeInfo, FetchMarketsParams, FetchOrdersParams, Market, Order,
    OrderSide, OrderStatus, Orderbook, Position, PriceLevel, RateLimiter,
};

use crate::auth::KalshiAuth;
use crate::config::KalshiConfig;
use crate::error::KalshiError;

pub struct Kalshi {
    config: KalshiConfig,
    client: reqwest::Client,
    rate_limiter: Arc<Mutex<RateLimiter>>,
    auth: Option<KalshiAuth>,
}

impl Kalshi {
    pub fn new(config: KalshiConfig) -> Result<Self, KalshiError> {
        let client = reqwest::Client::builder()
            .timeout(config.base.timeout)
            .build()?;

        let rate_limiter = Arc::new(Mutex::new(RateLimiter::new(
            config.base.rate_limit_per_second,
        )));

        // Initialize auth if credentials are provided
        let auth = if config.is_authenticated() {
            let auth = if let Some(ref path) = config.private_key_path {
                KalshiAuth::from_file(path)?
            } else if let Some(ref pem) = config.private_key_pem {
                KalshiAuth::from_pem(pem)?
            } else {
                return Err(KalshiError::AuthRequired);
            };
            Some(auth)
        } else {
            None
        };

        Ok(Self {
            config,
            client,
            rate_limiter,
            auth,
        })
    }

    pub fn with_default_config() -> Result<Self, KalshiError> {
        Self::new(KalshiConfig::default())
    }

    async fn rate_limit(&self) {
        self.rate_limiter.lock().await.wait().await;
    }

    fn auth_headers(
        &self,
        builder: reqwest::RequestBuilder,
        method: &str,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, KalshiError> {
        if let (Some(ref auth), Some(ref api_key_id)) = (&self.auth, &self.config.api_key_id) {
            let timestamp_ms = chrono::Utc::now().timestamp_millis();
            let signature = auth.sign(timestamp_ms, method, path);

            Ok(builder
                .header("KALSHI-ACCESS-KEY", api_key_id)
                .header("KALSHI-ACCESS-SIGNATURE", signature)
                .header("KALSHI-ACCESS-TIMESTAMP", timestamp_ms.to_string()))
        } else {
            Ok(builder)
        }
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, KalshiError> {
        self.rate_limit().await;

        let url = format!("{}{}", self.config.api_url, path);
        let req = self.client.get(&url);
        let req = self.auth_headers(req, "GET", path)?;
        let response = req.send().await?;

        if response.status() == 429 {
            return Err(KalshiError::RateLimited);
        }

        if response.status() == 401 || response.status() == 403 {
            let msg = response.text().await.unwrap_or_default();
            return Err(KalshiError::AuthFailed(msg));
        }

        if !response.status().is_success() {
            let msg = response.text().await.unwrap_or_default();
            return Err(KalshiError::Api(msg));
        }

        response
            .json()
            .await
            .map_err(|e| KalshiError::Api(e.to_string()))
    }

    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<T, KalshiError> {
        self.rate_limit().await;

        let url = format!("{}{}", self.config.api_url, path);
        let req = self.client.post(&url).json(body);
        let req = self.auth_headers(req, "POST", path)?;
        let response = req.send().await?;

        if response.status() == 429 {
            return Err(KalshiError::RateLimited);
        }

        if response.status() == 401 || response.status() == 403 {
            let msg = response.text().await.unwrap_or_default();
            return Err(KalshiError::AuthFailed(msg));
        }

        if !response.status().is_success() {
            let msg = response.text().await.unwrap_or_default();
            return Err(KalshiError::Api(msg));
        }

        response
            .json()
            .await
            .map_err(|e| KalshiError::Api(e.to_string()))
    }

    async fn delete<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, KalshiError> {
        self.rate_limit().await;

        let url = format!("{}{}", self.config.api_url, path);
        let req = self.client.delete(&url);
        let req = self.auth_headers(req, "DELETE", path)?;
        let response = req.send().await?;

        if response.status() == 429 {
            return Err(KalshiError::RateLimited);
        }

        if response.status() == 401 || response.status() == 403 {
            let msg = response.text().await.unwrap_or_default();
            return Err(KalshiError::AuthFailed(msg));
        }

        if !response.status().is_success() {
            let msg = response.text().await.unwrap_or_default();
            return Err(KalshiError::Api(msg));
        }

        response
            .json()
            .await
            .map_err(|e| KalshiError::Api(e.to_string()))
    }

    fn ensure_auth(&self) -> Result<(), KalshiError> {
        if !self.config.is_authenticated() {
            return Err(KalshiError::AuthRequired);
        }
        Ok(())
    }

    fn parse_market(&self, data: &serde_json::Value) -> Option<Market> {
        let obj = data.as_object()?;

        // Kalshi uses 'ticker' as market ID
        let id = obj.get("ticker").and_then(|v| v.as_str())?.to_string();

        let question = obj
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Binary markets: Yes/No outcomes
        let outcomes = vec!["Yes".to_string(), "No".to_string()];

        // Kalshi prices are in cents (1-99), convert to decimal
        let yes_price = obj
            .get("yes_ask")
            .or_else(|| obj.get("last_price"))
            .and_then(|v| v.as_f64())
            .map(|p| p / 100.0)
            .unwrap_or(0.0);

        let no_price = 1.0 - yes_price;

        let mut prices = HashMap::new();
        prices.insert("Yes".to_string(), yes_price);
        prices.insert("No".to_string(), no_price);

        let volume = obj.get("volume").and_then(|v| v.as_f64()).unwrap_or(0.0);

        let liquidity = obj
            .get("open_interest")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        // Parse close time
        let close_time = obj
            .get("close_time")
            .or_else(|| obj.get("expiration_time"))
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        let description = obj
            .get("subtitle")
            .or_else(|| obj.get("rules_primary"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Kalshi uses 0.01 tick size (1 cent)
        let tick_size = 0.01;

        Some(Market {
            id,
            question,
            outcomes,
            close_time,
            volume,
            liquidity,
            prices,
            metadata: data.clone(),
            tick_size,
            description,
        })
    }

    fn parse_order(&self, data: &serde_json::Value) -> Order {
        let obj = data.as_object();

        let id = obj
            .and_then(|o| o.get("order_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let market_id = obj
            .and_then(|o| o.get("ticker"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Kalshi uses 'side' for yes/no and 'action' for buy/sell
        let action = obj
            .and_then(|o| o.get("action"))
            .and_then(|v| v.as_str())
            .unwrap_or("buy");

        let side = if action.to_lowercase() == "buy" {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        let outcome = obj
            .and_then(|o| o.get("side"))
            .and_then(|v| v.as_str())
            .map(|s| if s == "yes" { "Yes" } else { "No" })
            .unwrap_or("Yes")
            .to_string();

        let status = obj
            .and_then(|o| o.get("status"))
            .and_then(|v| v.as_str())
            .map(|s| match s.to_lowercase().as_str() {
                "resting" | "active" | "pending" => OrderStatus::Open,
                "executed" | "filled" => OrderStatus::Filled,
                "canceled" | "cancelled" => OrderStatus::Cancelled,
                "partial" => OrderStatus::PartiallyFilled,
                _ => OrderStatus::Open,
            })
            .unwrap_or(OrderStatus::Open);

        // Price in cents, convert to decimal
        let price = obj
            .and_then(|o| o.get("yes_price").or(o.get("no_price")))
            .and_then(|v| v.as_f64())
            .map(|p| p / 100.0)
            .unwrap_or(0.0);

        let size = obj
            .and_then(|o| o.get("count").or(o.get("remaining_count")))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let filled = obj
            .and_then(|o| o.get("filled_count"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let created_at = obj
            .and_then(|o| o.get("created_time"))
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);

        let updated_at = obj
            .and_then(|o| o.get("updated_time"))
            .and_then(|v| v.as_str())
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

    fn parse_position(&self, data: &serde_json::Value) -> Position {
        let obj = data.as_object();

        let market_id = obj
            .and_then(|o| o.get("ticker"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Kalshi positions have yes/no counts
        let yes_count = obj
            .and_then(|o| o.get("position"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        // Positive = Yes position, Negative = No position
        let (outcome, size) = if yes_count >= 0.0 {
            ("Yes".to_string(), yes_count)
        } else {
            ("No".to_string(), -yes_count)
        };

        let average_price = obj
            .and_then(|o| o.get("average_price"))
            .and_then(|v| v.as_f64())
            .map(|p| p / 100.0)
            .unwrap_or(0.0);

        let current_price = obj
            .and_then(|o| o.get("market_value"))
            .and_then(|v| v.as_f64())
            .map(|p| p / 100.0)
            .unwrap_or(0.0);

        Position {
            market_id,
            outcome,
            size,
            average_price,
            current_price,
        }
    }

    /// Fetch orderbook for a market
    pub async fn fetch_orderbook(&self, ticker: &str) -> Result<Orderbook, KalshiError> {
        self.ensure_auth()?;

        #[derive(serde::Deserialize)]
        struct OrderbookResponse {
            orderbook: OrderbookData,
        }

        #[derive(serde::Deserialize)]
        struct OrderbookData {
            yes: Option<Vec<Vec<f64>>>,
            no: Option<Vec<Vec<f64>>>,
        }

        let path = format!("/markets/{ticker}/orderbook");
        let resp: OrderbookResponse = self.get(&path).await?;

        // Convert yes/no orderbook to bids/asks
        // Bids = buying Yes (or selling No)
        // Asks = selling Yes (or buying No)
        let mut bids = Vec::new();
        let mut asks = Vec::new();

        if let Some(yes_levels) = resp.orderbook.yes {
            for level in yes_levels {
                if level.len() >= 2 {
                    let price = level[0] / 100.0; // Convert cents to decimal
                    let size = level[1];
                    bids.push(PriceLevel { price, size });
                }
            }
        }

        if let Some(no_levels) = resp.orderbook.no {
            for level in no_levels {
                if level.len() >= 2 {
                    let price = level[0] / 100.0;
                    let size = level[1];
                    // No orders are inverse - convert to Yes-equivalent asks
                    asks.push(PriceLevel {
                        price: 1.0 - price,
                        size,
                    });
                }
            }
        }

        // Sort: bids descending, asks ascending
        bids.sort_by(|a, b| {
            b.price
                .partial_cmp(&a.price)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        asks.sort_by(|a, b| {
            a.price
                .partial_cmp(&b.price)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(Orderbook {
            market_id: ticker.to_string(),
            asset_id: ticker.to_string(),
            bids,
            asks,
            last_update_id: None,
            timestamp: Some(chrono::Utc::now()),
        })
    }
}

#[async_trait]
impl Exchange for Kalshi {
    fn id(&self) -> &'static str {
        "kalshi"
    }

    fn name(&self) -> &'static str {
        "Kalshi"
    }

    async fn fetch_markets(
        &self,
        params: Option<FetchMarketsParams>,
    ) -> Result<Vec<Market>, DrmError> {
        let params = params.unwrap_or_default();
        let limit = params.limit.unwrap_or(100).min(200);

        #[derive(serde::Deserialize)]
        struct MarketsResponse {
            markets: Vec<serde_json::Value>,
            #[allow(dead_code)]
            cursor: Option<String>,
        }

        let mut endpoint = format!("/markets?limit={limit}");

        if params.active_only {
            endpoint.push_str("&status=open");
        }

        let resp: MarketsResponse = self
            .get(&endpoint)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let markets: Vec<Market> = resp
            .markets
            .iter()
            .filter_map(|v| self.parse_market(v))
            .collect();

        Ok(markets)
    }

    async fn fetch_market(&self, market_id: &str) -> Result<Market, DrmError> {
        #[derive(serde::Deserialize)]
        struct MarketResponse {
            market: serde_json::Value,
        }

        let path = format!("/markets/{market_id}");
        let resp: MarketResponse = self
            .get(&path)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        self.parse_market(&resp.market).ok_or_else(|| {
            DrmError::Exchange(drm_core::ExchangeError::MarketNotFound(market_id.into()))
        })
    }

    async fn fetch_markets_by_slug(&self, slug: &str) -> Result<Vec<Market>, DrmError> {
        // Kalshi uses event_ticker for grouping markets
        #[derive(serde::Deserialize)]
        struct MarketsResponse {
            markets: Vec<serde_json::Value>,
        }

        let path = format!("/markets?event_ticker={slug}");
        let resp: MarketsResponse = self
            .get(&path)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let markets: Vec<Market> = resp
            .markets
            .iter()
            .filter_map(|v| self.parse_market(v))
            .collect();

        if markets.is_empty() {
            return Err(DrmError::Exchange(drm_core::ExchangeError::MarketNotFound(
                slug.into(),
            )));
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
        _params: HashMap<String, String>,
    ) -> Result<Order, DrmError> {
        self.ensure_auth()
            .map_err(|e| DrmError::Exchange(e.into()))?;

        if price <= 0.0 || price >= 1.0 {
            return Err(DrmError::Exchange(drm_core::ExchangeError::InvalidOrder(
                "Price must be between 0 and 1".into(),
            )));
        }

        // Convert outcome to Kalshi side (yes/no)
        let kalshi_side = outcome.to_lowercase();
        if kalshi_side != "yes" && kalshi_side != "no" {
            return Err(DrmError::Exchange(drm_core::ExchangeError::InvalidOrder(
                "Outcome must be 'Yes' or 'No'".into(),
            )));
        }

        // Convert side to Kalshi action
        let action = match side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        };

        // Price in cents
        let price_cents = (price * 100.0).round() as i64;

        #[derive(serde::Serialize)]
        struct CreateOrderRequest {
            ticker: String,
            action: String,
            side: String,
            #[serde(rename = "type")]
            order_type: String,
            count: i64,
            yes_price: Option<i64>,
            no_price: Option<i64>,
        }

        let (yes_price, no_price) = if kalshi_side == "yes" {
            (Some(price_cents), None)
        } else {
            (None, Some(price_cents))
        };

        let request = CreateOrderRequest {
            ticker: market_id.to_string(),
            action: action.to_string(),
            side: kalshi_side.clone(),
            order_type: "limit".to_string(),
            count: size as i64,
            yes_price,
            no_price,
        };

        #[derive(serde::Deserialize)]
        struct CreateOrderResponse {
            order: serde_json::Value,
        }

        let resp: CreateOrderResponse = self
            .post("/portfolio/orders", &request)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        Ok(self.parse_order(&resp.order))
    }

    async fn cancel_order(
        &self,
        order_id: &str,
        _market_id: Option<&str>,
    ) -> Result<Order, DrmError> {
        self.ensure_auth()
            .map_err(|e| DrmError::Exchange(e.into()))?;

        #[derive(serde::Deserialize)]
        struct CancelResponse {
            order: serde_json::Value,
        }

        let path = format!("/portfolio/orders/{order_id}");
        let resp: CancelResponse = self
            .delete(&path)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        Ok(self.parse_order(&resp.order))
    }

    async fn fetch_order(
        &self,
        order_id: &str,
        _market_id: Option<&str>,
    ) -> Result<Order, DrmError> {
        self.ensure_auth()
            .map_err(|e| DrmError::Exchange(e.into()))?;

        #[derive(serde::Deserialize)]
        struct OrderResponse {
            order: serde_json::Value,
        }

        let path = format!("/portfolio/orders/{order_id}");
        let resp: OrderResponse = self
            .get(&path)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        Ok(self.parse_order(&resp.order))
    }

    async fn fetch_open_orders(
        &self,
        _params: Option<FetchOrdersParams>,
    ) -> Result<Vec<Order>, DrmError> {
        self.ensure_auth()
            .map_err(|e| DrmError::Exchange(e.into()))?;

        #[derive(serde::Deserialize)]
        struct OrdersResponse {
            orders: Vec<serde_json::Value>,
        }

        let path = "/portfolio/orders?status=resting";
        let resp: OrdersResponse = self
            .get(path)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        Ok(resp.orders.iter().map(|o| self.parse_order(o)).collect())
    }

    async fn fetch_positions(&self, _market_id: Option<&str>) -> Result<Vec<Position>, DrmError> {
        self.ensure_auth()
            .map_err(|e| DrmError::Exchange(e.into()))?;

        #[derive(serde::Deserialize)]
        struct PositionsResponse {
            market_positions: Vec<serde_json::Value>,
        }

        let path = "/portfolio/positions";
        let resp: PositionsResponse = self
            .get(path)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        Ok(resp
            .market_positions
            .iter()
            .map(|p| self.parse_position(p))
            .collect())
    }

    async fn fetch_balance(&self) -> Result<HashMap<String, f64>, DrmError> {
        self.ensure_auth()
            .map_err(|e| DrmError::Exchange(e.into()))?;

        #[derive(serde::Deserialize)]
        struct BalanceResponse {
            balance: f64, // In cents
        }

        let path = "/portfolio/balance";
        let resp: BalanceResponse = self
            .get(path)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let mut result = HashMap::new();
        // Convert cents to dollars
        result.insert("USD".to_string(), resp.balance / 100.0);
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
