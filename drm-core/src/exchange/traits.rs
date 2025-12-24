use async_trait::async_trait;
use std::collections::HashMap;

use crate::error::DrmError;
use crate::models::{Market, Order, OrderSide, Position};

use super::config::{FetchMarketsParams, FetchOrdersParams};

#[async_trait]
pub trait Exchange: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;

    async fn fetch_markets(
        &self,
        params: Option<FetchMarketsParams>,
    ) -> Result<Vec<Market>, DrmError>;

    async fn fetch_market(&self, market_id: &str) -> Result<Market, DrmError>;

    async fn fetch_markets_by_slug(&self, slug: &str) -> Result<Vec<Market>, DrmError> {
        let _ = slug;
        Err(DrmError::Exchange(crate::error::ExchangeError::NotSupported(
            "fetch_markets_by_slug".into(),
        )))
    }

    async fn create_order(
        &self,
        market_id: &str,
        outcome: &str,
        side: OrderSide,
        price: f64,
        size: f64,
        params: HashMap<String, String>,
    ) -> Result<Order, DrmError>;

    async fn cancel_order(
        &self,
        order_id: &str,
        market_id: Option<&str>,
    ) -> Result<Order, DrmError>;

    async fn fetch_order(
        &self,
        order_id: &str,
        market_id: Option<&str>,
    ) -> Result<Order, DrmError>;

    async fn fetch_open_orders(
        &self,
        params: Option<FetchOrdersParams>,
    ) -> Result<Vec<Order>, DrmError>;

    async fn fetch_positions(
        &self,
        market_id: Option<&str>,
    ) -> Result<Vec<Position>, DrmError>;

    async fn fetch_balance(&self) -> Result<HashMap<String, f64>, DrmError>;

    fn describe(&self) -> ExchangeInfo {
        ExchangeInfo {
            id: self.id(),
            name: self.name(),
            has_fetch_markets: true,
            has_create_order: true,
            has_websocket: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExchangeInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub has_fetch_markets: bool,
    pub has_create_order: bool,
    pub has_websocket: bool,
}
