use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use drm_core::{
    DrmError, Exchange, ExchangeInfo, FetchMarketsParams, FetchOrdersParams,
    Market, Order, OrderSide, Position, RateLimiter,
};

use crate::client::HttpClient;
use crate::config::PolymarketConfig;
use crate::error::PolymarketError;

pub struct Polymarket {
    config: PolymarketConfig,
    client: HttpClient,
    rate_limiter: Arc<Mutex<RateLimiter>>,
}

impl Polymarket {
    pub fn new(config: PolymarketConfig) -> Result<Self, PolymarketError> {
        let client = HttpClient::new(&config)?;
        let rate_limiter = Arc::new(Mutex::new(RateLimiter::new(
            config.base.rate_limit_per_second,
        )));

        Ok(Self {
            config,
            client,
            rate_limiter,
        })
    }

    pub fn with_default_config() -> Result<Self, PolymarketError> {
        Self::new(PolymarketConfig::default())
    }

    async fn rate_limit(&self) {
        self.rate_limiter.lock().await.wait().await;
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
                serde_json::from_str(s).unwrap_or_default()
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
        _market_id: &str,
        _outcome: &str,
        _side: OrderSide,
        _price: f64,
        _size: f64,
        _params: HashMap<String, String>,
    ) -> Result<Order, DrmError> {
        if !self.config.is_authenticated() {
            return Err(DrmError::Exchange(drm_core::ExchangeError::Authentication(
                "private key required for trading".into(),
            )));
        }

        Err(DrmError::Exchange(drm_core::ExchangeError::NotSupported(
            "create_order not yet implemented".into(),
        )))
    }

    async fn cancel_order(
        &self,
        _order_id: &str,
        _market_id: Option<&str>,
    ) -> Result<Order, DrmError> {
        Err(DrmError::Exchange(drm_core::ExchangeError::NotSupported(
            "cancel_order not yet implemented".into(),
        )))
    }

    async fn fetch_order(
        &self,
        _order_id: &str,
        _market_id: Option<&str>,
    ) -> Result<Order, DrmError> {
        Err(DrmError::Exchange(drm_core::ExchangeError::NotSupported(
            "fetch_order not yet implemented".into(),
        )))
    }

    async fn fetch_open_orders(
        &self,
        _params: Option<FetchOrdersParams>,
    ) -> Result<Vec<Order>, DrmError> {
        Err(DrmError::Exchange(drm_core::ExchangeError::NotSupported(
            "fetch_open_orders not yet implemented".into(),
        )))
    }

    async fn fetch_positions(
        &self,
        _market_id: Option<&str>,
    ) -> Result<Vec<Position>, DrmError> {
        Err(DrmError::Exchange(drm_core::ExchangeError::NotSupported(
            "fetch_positions not yet implemented".into(),
        )))
    }

    async fn fetch_balance(&self) -> Result<HashMap<String, f64>, DrmError> {
        Err(DrmError::Exchange(drm_core::ExchangeError::NotSupported(
            "fetch_balance not yet implemented".into(),
        )))
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
