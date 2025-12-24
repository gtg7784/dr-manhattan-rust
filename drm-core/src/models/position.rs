use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub market_id: String,
    pub outcome: String,
    pub size: f64,
    pub average_price: f64,
    pub current_price: f64,
}

impl Position {
    pub fn cost_basis(&self) -> f64 {
        self.size * self.average_price
    }

    pub fn current_value(&self) -> f64 {
        self.size * self.current_price
    }

    pub fn unrealized_pnl(&self) -> f64 {
        self.current_value() - self.cost_basis()
    }

    pub fn unrealized_pnl_percent(&self) -> f64 {
        let cost = self.cost_basis();
        if cost == 0.0 {
            return 0.0;
        }
        (self.unrealized_pnl() / cost) * 100.0
    }
}
