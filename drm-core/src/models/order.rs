use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Pending,
    Open,
    Filled,
    PartiallyFilled,
    Cancelled,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub market_id: String,
    pub outcome: String,
    pub side: OrderSide,
    pub price: f64,
    pub size: f64,
    pub filled: f64,
    pub status: OrderStatus,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

impl Order {
    pub fn remaining(&self) -> f64 {
        self.size - self.filled
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status, OrderStatus::Open | OrderStatus::PartiallyFilled)
    }

    pub fn is_filled(&self) -> bool {
        self.status == OrderStatus::Filled || self.filled >= self.size
    }

    pub fn fill_percentage(&self) -> f64 {
        if self.size == 0.0 {
            return 0.0;
        }
        self.filled / self.size
    }
}
