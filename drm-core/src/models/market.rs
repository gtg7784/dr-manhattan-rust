use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    pub id: String,
    pub question: String,
    pub outcomes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_time: Option<DateTime<Utc>>,
    pub volume: f64,
    pub liquidity: f64,
    pub prices: HashMap<String, f64>,
    #[serde(default)]
    pub metadata: serde_json::Value,
    pub tick_size: f64,
    #[serde(default)]
    pub description: String,
}

impl Market {
    pub fn is_binary(&self) -> bool {
        self.outcomes.len() == 2
    }

    pub fn is_open(&self) -> bool {
        if let Some(ref metadata) = self.metadata.as_object() {
            if let Some(closed) = metadata.get("closed").and_then(|v| v.as_bool()) {
                return !closed;
            }
        }

        match self.close_time {
            Some(close_time) => Utc::now() < close_time,
            None => true,
        }
    }

    pub fn spread(&self) -> Option<f64> {
        if !self.is_binary() || self.outcomes.len() != 2 {
            return None;
        }

        let prices: Vec<f64> = self.prices.values().copied().collect();
        if prices.len() != 2 {
            return None;
        }

        Some((1.0 - prices.iter().sum::<f64>()).abs())
    }

    pub fn get_token_ids(&self) -> Vec<String> {
        let token_ids = self.metadata.get("clobTokenIds");

        match token_ids {
            Some(serde_json::Value::String(s)) => {
                serde_json::from_str(s).unwrap_or_default()
            }
            Some(serde_json::Value::Array(arr)) => {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            }
            _ => vec![],
        }
    }

    pub fn get_outcome_tokens(&self) -> Vec<OutcomeToken> {
        let token_ids = self.get_token_ids();
        self.outcomes
            .iter()
            .enumerate()
            .map(|(i, outcome)| OutcomeToken {
                outcome: outcome.clone(),
                token_id: token_ids.get(i).cloned().unwrap_or_default(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeToken {
    pub outcome: String,
    pub token_id: String,
}
