use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tokio::time::Duration;

use crate::error::DrmError;
use crate::exchange::Exchange;
use crate::models::{Market, Order, OrderSide, Position};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyState {
    Stopped,
    Running,
    Paused,
}

#[derive(Debug, Clone)]
pub enum StrategyEvent {
    Started,
    Stopped,
    Paused,
    Resumed,
    Order(Order),
    Error(String),
    Tick,
}

#[derive(Debug, Clone)]
pub struct StrategyConfig {
    pub tick_interval_ms: u64,
    pub max_position_size: f64,
    pub spread_bps: u32,
    pub verbose: bool,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            tick_interval_ms: 1000,
            max_position_size: 100.0,
            spread_bps: 100,
            verbose: false,
        }
    }
}

#[async_trait]
pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;
    fn config(&self) -> &StrategyConfig;
    fn state(&self) -> StrategyState;

    async fn on_tick(&mut self) -> Result<(), DrmError>;

    async fn start(&mut self) -> Result<(), DrmError>;
    async fn stop(&mut self) -> Result<(), DrmError>;
    fn pause(&mut self);
    fn resume(&mut self);
}

pub struct BaseStrategy<E: Exchange + 'static> {
    pub exchange: Arc<E>,
    pub market_id: String,
    pub market: Option<Market>,
    pub state: StrategyState,
    pub config: StrategyConfig,
    pub positions: Vec<Position>,
    pub open_orders: Vec<Order>,
    pub event_tx: broadcast::Sender<StrategyEvent>,
    tick_handle: Option<tokio::task::JoinHandle<()>>,
    stop_signal: Arc<Mutex<bool>>,
}

impl<E: Exchange + 'static> BaseStrategy<E> {
    pub fn new(exchange: Arc<E>, market_id: String, config: StrategyConfig) -> Self {
        let (event_tx, _) = broadcast::channel(100);

        Self {
            exchange,
            market_id,
            market: None,
            state: StrategyState::Stopped,
            config,
            positions: Vec::new(),
            open_orders: Vec::new(),
            event_tx,
            tick_handle: None,
            stop_signal: Arc::new(Mutex::new(false)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StrategyEvent> {
        self.event_tx.subscribe()
    }

    pub async fn refresh_state(&mut self) -> Result<(), DrmError> {
        let (positions, orders) = tokio::try_join!(
            self.exchange.fetch_positions(Some(&self.market_id)),
            self.exchange.fetch_open_orders(None),
        )?;

        self.positions = positions;
        self.open_orders = orders
            .into_iter()
            .filter(|o| o.market_id == self.market_id)
            .collect();

        Ok(())
    }

    pub async fn cancel_all_orders(&mut self) -> Result<(), DrmError> {
        for order in self.open_orders.drain(..) {
            let _ = self
                .exchange
                .cancel_order(&order.id, Some(&self.market_id))
                .await;
        }
        Ok(())
    }

    pub fn get_position(&self, outcome: &str) -> Option<&Position> {
        self.positions.iter().find(|p| p.outcome == outcome)
    }

    pub fn get_net_position(&self) -> f64 {
        let market = match &self.market {
            Some(m) if m.outcomes.len() == 2 => m,
            _ => return 0.0,
        };

        let pos1 = self
            .get_position(&market.outcomes[0])
            .map(|p| p.size)
            .unwrap_or(0.0);

        let pos2 = self
            .get_position(&market.outcomes[1])
            .map(|p| p.size)
            .unwrap_or(0.0);

        pos1 - pos2
    }

    pub async fn place_order(
        &mut self,
        outcome: &str,
        side: OrderSide,
        price: f64,
        size: f64,
        token_id: Option<&str>,
    ) -> Result<Order, DrmError> {
        let mut params = HashMap::new();
        if let Some(tid) = token_id {
            params.insert("token_id".to_string(), tid.to_string());
        }

        let order = self
            .exchange
            .create_order(&self.market_id, outcome, side, price, size, params)
            .await?;

        self.open_orders.push(order.clone());
        let _ = self.event_tx.send(StrategyEvent::Order(order.clone()));

        Ok(order)
    }

    pub fn log(&self, message: &str) {
        if self.config.verbose {
            println!(
                "[{}:{}] {}",
                self.exchange.id(),
                self.market_id,
                message
            );
        }
    }

    pub fn is_running(&self) -> bool {
        self.state == StrategyState::Running
    }

    pub async fn signal_stop(&self) {
        let mut stop = self.stop_signal.lock().await;
        *stop = true;
    }

    pub async fn should_stop(&self) -> bool {
        *self.stop_signal.lock().await
    }

    pub async fn reset_stop_signal(&self) {
        let mut stop = self.stop_signal.lock().await;
        *stop = false;
    }

    pub async fn run_loop<F, Fut>(&mut self, mut on_tick: F) -> Result<(), DrmError>
    where
        F: FnMut(&mut Self) -> Fut + Send,
        Fut: std::future::Future<Output = Result<(), DrmError>> + Send,
    {
        self.reset_stop_signal().await;
        self.state = StrategyState::Running;
        let _ = self.event_tx.send(StrategyEvent::Started);
        self.log("Strategy started");

        self.market = Some(self.exchange.fetch_market(&self.market_id).await?);
        self.log(&format!("Loaded market: {}", self.market_id));

        let tick_interval = Duration::from_millis(self.config.tick_interval_ms);

        loop {
            if self.should_stop().await {
                break;
            }

            if self.state == StrategyState::Paused {
                tokio::time::sleep(tick_interval).await;
                continue;
            }

            if let Err(e) = self.refresh_state().await {
                self.log(&format!("Failed to refresh state: {}", e));
                let _ = self.event_tx.send(StrategyEvent::Error(e.to_string()));
            }

            if let Err(e) = on_tick(self).await {
                self.log(&format!("Tick error: {}", e));
                let _ = self.event_tx.send(StrategyEvent::Error(e.to_string()));
            } else {
                let _ = self.event_tx.send(StrategyEvent::Tick);
            }

            tokio::time::sleep(tick_interval).await;
        }

        self.state = StrategyState::Stopped;
        let _ = self.event_tx.send(StrategyEvent::Stopped);
        self.log("Strategy stopped");

        Ok(())
    }

    pub fn pause(&mut self) {
        if self.state == StrategyState::Running {
            self.state = StrategyState::Paused;
            let _ = self.event_tx.send(StrategyEvent::Paused);
            self.log("Strategy paused");
        }
    }

    pub fn resume(&mut self) {
        if self.state == StrategyState::Paused {
            self.state = StrategyState::Running;
            let _ = self.event_tx.send(StrategyEvent::Resumed);
            self.log("Strategy resumed");
        }
    }
}

impl<E: Exchange + 'static> Drop for BaseStrategy<E> {
    fn drop(&mut self) {
        if let Some(handle) = self.tick_handle.take() {
            handle.abort();
        }
    }
}
