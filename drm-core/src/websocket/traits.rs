use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::error::WebSocketError;
use crate::models::Orderbook;

pub type OrderbookStream =
    Pin<Box<dyn Stream<Item = Result<Orderbook, WebSocketError>> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSocketState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Closed,
}

#[async_trait]
pub trait OrderBookWebSocket: Send + Sync {
    async fn connect(&mut self) -> Result<(), WebSocketError>;

    async fn disconnect(&mut self) -> Result<(), WebSocketError>;

    async fn subscribe(&mut self, market_id: &str) -> Result<(), WebSocketError>;

    async fn unsubscribe(&mut self, market_id: &str) -> Result<(), WebSocketError>;

    fn state(&self) -> WebSocketState;

    async fn orderbook_stream(
        &mut self,
        market_id: &str,
    ) -> Result<OrderbookStream, WebSocketError>;
}
