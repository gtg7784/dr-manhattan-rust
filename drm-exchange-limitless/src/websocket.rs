use async_trait::async_trait;
use futures::{FutureExt, StreamExt};
use rust_socketio::{
    asynchronous::{Client, ClientBuilder},
    Payload,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use drm_core::{
    OrderBookWebSocket, Orderbook, OrderbookStream, PriceLevel, WebSocketError, WebSocketState,
};

const WS_URL: &str = "wss://ws.limitless.exchange";
const NAMESPACE: &str = "/markets";

#[derive(Debug, Clone, Serialize)]
struct SubscribePayload {
    #[serde(rename = "marketSlugs", skip_serializing_if = "Vec::is_empty")]
    market_slugs: Vec<String>,
    #[serde(rename = "marketAddresses", skip_serializing_if = "Vec::is_empty")]
    market_addresses: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OrderbookUpdateData {
    #[serde(rename = "marketSlug", alias = "slug")]
    market_slug: Option<String>,
    orderbook: Option<OrderbookData>,
    bids: Option<Vec<PriceLevelData>>,
    asks: Option<Vec<PriceLevelData>>,
    #[allow(dead_code)]
    timestamp: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct OrderbookData {
    bids: Option<Vec<PriceLevelData>>,
    asks: Option<Vec<PriceLevelData>>,
}

#[derive(Debug, Clone, Deserialize)]
struct PriceLevelData {
    price: serde_json::Value,
    size: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct PriceUpdateData {
    #[serde(rename = "marketAddress")]
    market_address: Option<String>,
    #[serde(rename = "updatedPrices")]
    updated_prices: Option<PriceData>,
    #[serde(rename = "blockNumber")]
    #[allow(dead_code)]
    block_number: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
struct PriceData {
    yes: Option<f64>,
    no: Option<f64>,
}

type OrderbookSender = broadcast::Sender<Result<Orderbook, WebSocketError>>;

struct SharedState {
    ws_state: WebSocketState,
    subscribed_slugs: Vec<String>,
    subscribed_addresses: Vec<String>,
    orderbook_senders: HashMap<String, OrderbookSender>,
    orderbooks: HashMap<String, Orderbook>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            ws_state: WebSocketState::Disconnected,
            subscribed_slugs: Vec::new(),
            subscribed_addresses: Vec::new(),
            orderbook_senders: HashMap::new(),
            orderbooks: HashMap::new(),
        }
    }
}

pub struct LimitlessWebSocket {
    shared: Arc<RwLock<SharedState>>,
    client: Arc<RwLock<Option<Client>>>,
    #[allow(dead_code)]
    auto_reconnect: bool,
}

impl LimitlessWebSocket {
    pub fn new() -> Self {
        Self::with_config(true)
    }

    pub fn with_config(auto_reconnect: bool) -> Self {
        Self {
            shared: Arc::new(RwLock::new(SharedState::new())),
            client: Arc::new(RwLock::new(None)),
            auto_reconnect,
        }
    }

    async fn set_state(&self, new_state: WebSocketState) {
        let mut shared = self.shared.write().await;
        shared.ws_state = new_state;
    }

    async fn send_subscription(&self) -> Result<(), WebSocketError> {
        let client_guard = self.client.read().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| WebSocketError::Connection("not connected".into()))?;

        let shared = self.shared.read().await;
        if shared.subscribed_slugs.is_empty() && shared.subscribed_addresses.is_empty() {
            return Ok(());
        }

        let payload = SubscribePayload {
            market_slugs: shared.subscribed_slugs.clone(),
            market_addresses: shared.subscribed_addresses.clone(),
        };

        let json =
            serde_json::to_value(&payload).map_err(|e| WebSocketError::Protocol(e.to_string()))?;

        client
            .emit("subscribe_market_prices", json)
            .await
            .map_err(|e| WebSocketError::Connection(e.to_string()))?;

        Ok(())
    }

    fn parse_price_level(data: &PriceLevelData) -> Option<PriceLevel> {
        let price = match &data.price {
            serde_json::Value::Number(n) => n.as_f64()?,
            serde_json::Value::String(s) => s.parse::<f64>().ok()?,
            _ => return None,
        };

        let size = match &data.size {
            serde_json::Value::Number(n) => n.as_f64()?,
            serde_json::Value::String(s) => s.parse::<f64>().ok()?,
            _ => return None,
        };

        if price > 0.0 && size > 0.0 {
            Some(PriceLevel::new(price, size))
        } else {
            None
        }
    }

    async fn handle_orderbook_update(shared: Arc<RwLock<SharedState>>, data: OrderbookUpdateData) {
        let market_slug = match data.market_slug {
            Some(s) => s,
            None => return,
        };

        let (raw_bids, raw_asks) = if let Some(ob) = data.orderbook {
            (ob.bids.unwrap_or_default(), ob.asks.unwrap_or_default())
        } else {
            (data.bids.unwrap_or_default(), data.asks.unwrap_or_default())
        };

        let mut bids: Vec<PriceLevel> = raw_bids
            .iter()
            .filter_map(Self::parse_price_level)
            .collect();
        let mut asks: Vec<PriceLevel> = raw_asks
            .iter()
            .filter_map(Self::parse_price_level)
            .collect();

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

        let orderbook = Orderbook {
            market_id: market_slug.clone(),
            asset_id: market_slug.clone(),
            bids,
            asks,
            last_update_id: None,
            timestamp: Some(chrono::Utc::now()),
        };

        let mut shared = shared.write().await;
        shared
            .orderbooks
            .insert(market_slug.clone(), orderbook.clone());

        if let Some(sender) = shared.orderbook_senders.get(&market_slug) {
            let _ = sender.send(Ok(orderbook));
        }
    }

    async fn handle_price_update(shared: Arc<RwLock<SharedState>>, data: PriceUpdateData) {
        let market_address = match data.market_address {
            Some(a) => a,
            None => return,
        };

        let prices = match data.updated_prices {
            Some(p) => p,
            None => return,
        };

        let yes_price = prices.yes.unwrap_or(0.0);
        let no_price = prices.no.unwrap_or(0.0);

        if yes_price <= 0.0 && no_price <= 0.0 {
            return;
        }

        let mut bids = Vec::new();
        let mut asks = Vec::new();

        if yes_price > 0.0 {
            bids.push(PriceLevel::new(yes_price, 1.0));
        }
        if no_price > 0.0 {
            asks.push(PriceLevel::new(1.0 - no_price, 1.0));
        }

        let orderbook = Orderbook {
            market_id: market_address.clone(),
            asset_id: market_address.clone(),
            bids,
            asks,
            last_update_id: None,
            timestamp: Some(chrono::Utc::now()),
        };

        let mut shared = shared.write().await;
        shared
            .orderbooks
            .insert(market_address.clone(), orderbook.clone());

        if let Some(sender) = shared.orderbook_senders.get(&market_address) {
            let _ = sender.send(Ok(orderbook));
        }
    }
}

impl Default for LimitlessWebSocket {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OrderBookWebSocket for LimitlessWebSocket {
    async fn connect(&mut self) -> Result<(), WebSocketError> {
        self.set_state(WebSocketState::Connecting).await;

        let shared = self.shared.clone();
        let shared_orderbook = self.shared.clone();
        let shared_price = self.shared.clone();
        let shared_connect = self.shared.clone();
        let shared_disconnect = self.shared.clone();

        let client = ClientBuilder::new(WS_URL)
            .namespace(NAMESPACE)
            .on("connect", move |_, _| {
                let shared = shared_connect.clone();
                async move {
                    let mut s = shared.write().await;
                    s.ws_state = WebSocketState::Connected;
                    tracing::debug!("Connected to Limitless WebSocket");
                }
                .boxed()
            })
            .on("disconnect", move |_, _| {
                let shared = shared_disconnect.clone();
                async move {
                    let mut s = shared.write().await;
                    s.ws_state = WebSocketState::Disconnected;
                    tracing::debug!("Disconnected from Limitless WebSocket");
                }
                .boxed()
            })
            .on("orderbookUpdate", move |payload, _| {
                let shared = shared_orderbook.clone();
                async move {
                    if let Payload::Text(values) = payload {
                        for value in values {
                            if let Ok(data) = serde_json::from_value::<OrderbookUpdateData>(value) {
                                Self::handle_orderbook_update(shared.clone(), data).await;
                            }
                        }
                    }
                }
                .boxed()
            })
            .on("newPriceData", move |payload, _| {
                let shared = shared_price.clone();
                async move {
                    if let Payload::Text(values) = payload {
                        for value in values {
                            if let Ok(data) = serde_json::from_value::<PriceUpdateData>(value) {
                                Self::handle_price_update(shared.clone(), data).await;
                            }
                        }
                    }
                }
                .boxed()
            })
            .on("exception", |payload, _| {
                async move {
                    tracing::warn!("Limitless WebSocket exception: {:?}", payload);
                }
                .boxed()
            })
            .connect()
            .await
            .map_err(|e| WebSocketError::Connection(e.to_string()))?;

        {
            let mut client_guard = self.client.write().await;
            *client_guard = Some(client);
        }

        {
            let mut s = shared.write().await;
            s.ws_state = WebSocketState::Connected;
        }

        self.send_subscription().await?;

        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), WebSocketError> {
        self.set_state(WebSocketState::Closed).await;

        let mut client_guard = self.client.write().await;
        if let Some(client) = client_guard.take() {
            client
                .disconnect()
                .await
                .map_err(|e| WebSocketError::Connection(e.to_string()))?;
        }

        Ok(())
    }

    async fn subscribe(&mut self, market_id: &str) -> Result<(), WebSocketError> {
        {
            let mut shared = self.shared.write().await;
            if !shared.subscribed_slugs.contains(&market_id.to_string()) {
                shared.subscribed_slugs.push(market_id.to_string());
            }
            if !shared.orderbook_senders.contains_key(market_id) {
                let (tx, _) = broadcast::channel(100);
                shared.orderbook_senders.insert(market_id.to_string(), tx);
            }
        }

        let state = {
            let shared = self.shared.read().await;
            shared.ws_state
        };

        if state == WebSocketState::Connected {
            self.send_subscription().await?;
        }

        Ok(())
    }

    async fn unsubscribe(&mut self, market_id: &str) -> Result<(), WebSocketError> {
        {
            let mut shared = self.shared.write().await;
            shared.subscribed_slugs.retain(|s| s != market_id);
            shared.subscribed_addresses.retain(|s| s != market_id);
            shared.orderbook_senders.remove(market_id);
            shared.orderbooks.remove(market_id);
        }

        let state = {
            let shared = self.shared.read().await;
            shared.ws_state
        };

        if state == WebSocketState::Connected {
            self.send_subscription().await?;
        }

        Ok(())
    }

    fn state(&self) -> WebSocketState {
        futures::executor::block_on(async {
            let shared = self.shared.read().await;
            shared.ws_state
        })
    }

    async fn orderbook_stream(
        &mut self,
        market_id: &str,
    ) -> Result<OrderbookStream, WebSocketError> {
        let shared = self.shared.read().await;
        let sender = shared.orderbook_senders.get(market_id).ok_or_else(|| {
            WebSocketError::Subscription(format!("not subscribed to {market_id}"))
        })?;

        let rx = sender.subscribe();

        Ok(Box::pin(
            tokio_stream::wrappers::BroadcastStream::new(rx)
                .filter_map(|result| async move { result.ok() }),
        ))
    }
}

impl LimitlessWebSocket {
    pub async fn subscribe_market_address(
        &mut self,
        market_address: &str,
    ) -> Result<(), WebSocketError> {
        {
            let mut shared = self.shared.write().await;
            if !shared
                .subscribed_addresses
                .contains(&market_address.to_string())
            {
                shared.subscribed_addresses.push(market_address.to_string());
            }
            if !shared.orderbook_senders.contains_key(market_address) {
                let (tx, _) = broadcast::channel(100);
                shared
                    .orderbook_senders
                    .insert(market_address.to_string(), tx);
            }
        }

        let state = {
            let shared = self.shared.read().await;
            shared.ws_state
        };

        if state == WebSocketState::Connected {
            self.send_subscription().await?;
        }

        Ok(())
    }

    pub async fn get_orderbook(&self, market_id: &str) -> Option<Orderbook> {
        let shared = self.shared.read().await;
        shared.orderbooks.get(market_id).cloned()
    }
}
