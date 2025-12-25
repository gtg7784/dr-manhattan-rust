use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeltaInfo {
    pub delta: f64,
    pub max_outcome: Option<String>,
    pub max_position: f64,
}

pub fn calculate_delta(positions: &HashMap<String, f64>) -> DeltaInfo {
    if positions.is_empty() {
        return DeltaInfo::default();
    }

    let mut max_outcome: Option<String> = None;
    let mut max_position: f64 = 0.0;

    for (outcome, &size) in positions {
        if size > max_position {
            max_position = size;
            max_outcome = Some(outcome.clone());
        }
    }

    let delta = if positions.len() == 2 {
        let values: Vec<f64> = positions.values().copied().collect();
        (values[0] - values[1]).abs()
    } else {
        max_position
    };

    DeltaInfo {
        delta,
        max_outcome,
        max_position,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PositionBreakdown {
    pub outcome: String,
    pub size: f64,
    pub current_price: f64,
    pub value: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Nav {
    pub nav: f64,
    pub cash: f64,
    pub positions_value: f64,
    pub positions: Vec<PositionBreakdown>,
}

impl Nav {
    pub fn calculate(cash: f64, positions: &[Position]) -> Self {
        let mut positions_value = 0.0;
        let mut breakdown = Vec::with_capacity(positions.len());

        for pos in positions {
            let value = pos.current_value();
            positions_value += value;
            breakdown.push(PositionBreakdown {
                outcome: pos.outcome.clone(),
                size: pos.size,
                current_price: pos.current_price,
                value,
            });
        }

        Self {
            nav: cash + positions_value,
            cash,
            positions_value,
            positions: breakdown,
        }
    }
}
