use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PriceLevel {
    pub price: f64,
    pub size: f64,
}

impl PriceLevel {
    pub fn new(price: f64, size: f64) -> Self {
        Self { price, size }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Orderbook {
    pub market_id: String,
    pub asset_id: String,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_update_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
}

impl Orderbook {
    pub fn best_bid(&self) -> Option<f64> {
        self.bids.first().map(|l| l.price)
    }

    pub fn best_ask(&self) -> Option<f64> {
        self.asks.first().map(|l| l.price)
    }

    pub fn mid_price(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2.0),
            _ => None,
        }
    }

    pub fn spread(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }

    pub fn has_data(&self) -> bool {
        !self.bids.is_empty() && !self.asks.is_empty()
    }

    pub fn from_rest_response(
        bids: &[RestPriceLevel],
        asks: &[RestPriceLevel],
        asset_id: impl Into<String>,
    ) -> Self {
        let mut parsed_bids: Vec<PriceLevel> = bids
            .iter()
            .filter_map(|b| {
                let price = b.price.parse::<f64>().ok()?;
                let size = b.size.parse::<f64>().ok()?;
                if price > 0.0 && size > 0.0 {
                    Some(PriceLevel::new(price, size))
                } else {
                    None
                }
            })
            .collect();

        let mut parsed_asks: Vec<PriceLevel> = asks
            .iter()
            .filter_map(|a| {
                let price = a.price.parse::<f64>().ok()?;
                let size = a.size.parse::<f64>().ok()?;
                if price > 0.0 && size > 0.0 {
                    Some(PriceLevel::new(price, size))
                } else {
                    None
                }
            })
            .collect();

        parsed_bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap_or(std::cmp::Ordering::Equal));
        parsed_asks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal));

        Self {
            market_id: String::new(),
            asset_id: asset_id.into(),
            bids: parsed_bids,
            asks: parsed_asks,
            last_update_id: None,
            timestamp: Some(Utc::now()),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RestPriceLevel {
    pub price: String,
    pub size: String,
}

#[derive(Debug, Default)]
pub struct OrderbookManager {
    orderbooks: HashMap<String, Orderbook>,
}

impl OrderbookManager {
    pub fn new() -> Self {
        Self {
            orderbooks: HashMap::new(),
        }
    }

    pub fn update(&mut self, token_id: impl Into<String>, orderbook: Orderbook) {
        self.orderbooks.insert(token_id.into(), orderbook);
    }

    pub fn get(&self, token_id: &str) -> Option<&Orderbook> {
        self.orderbooks.get(token_id)
    }

    pub fn get_best_bid_ask(&self, token_id: &str) -> (Option<f64>, Option<f64>) {
        match self.get(token_id) {
            Some(ob) => (ob.best_bid(), ob.best_ask()),
            None => (None, None),
        }
    }

    pub fn has_data(&self, token_id: &str) -> bool {
        self.get(token_id).is_some_and(|ob| ob.has_data())
    }

    pub fn has_all_data(&self, token_ids: &[&str]) -> bool {
        token_ids.iter().all(|id| self.has_data(id))
    }

    pub fn clear(&mut self) {
        self.orderbooks.clear();
    }

    pub fn len(&self) -> usize {
        self.orderbooks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.orderbooks.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Orderbook)> {
        self.orderbooks.iter()
    }
}
