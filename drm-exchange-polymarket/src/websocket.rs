use async_trait::async_trait;
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::time::{interval, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use drm_core::{Orderbook, OrderBookWebSocket, OrderbookStream, PriceLevel, WebSocketError, WebSocketState};

const WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";
const PING_INTERVAL_SECS: u64 = 20;
const RECONNECT_BASE_DELAY_MS: u64 = 3000;
const RECONNECT_MAX_DELAY_MS: u64 = 60000;
const MAX_RECONNECT_ATTEMPTS: u32 = 10;

#[derive(Debug, Clone, serde::Serialize)]
struct SubscribeMessage {
    auth: HashMap<String, String>,
    markets: Vec<String>,
    assets_ids: Vec<String>,
    #[serde(rename = "type")]
    msg_type: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct WsMessage {
    event_type: Option<String>,
    asset_id: Option<String>,
    market: Option<String>,
    bids: Option<Vec<WsPriceLevel>>,
    asks: Option<Vec<WsPriceLevel>>,
    price_changes: Option<Vec<WsPriceChange>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct WsPriceLevel {
    price: String,
    size: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct WsPriceChange {
    asset_id: String,
    best_bid: Option<String>,
    best_ask: Option<String>,
}

type OrderbookSender = broadcast::Sender<Result<Orderbook, WebSocketError>>;

pub struct PolymarketWebSocket {
    state: Arc<RwLock<WebSocketState>>,
    subscriptions: Arc<RwLock<HashMap<String, Vec<String>>>>,
    orderbook_senders: Arc<RwLock<HashMap<String, OrderbookSender>>>,
    orderbooks: Arc<RwLock<HashMap<String, Orderbook>>>,
    write_tx: Arc<Mutex<Option<futures::channel::mpsc::UnboundedSender<Message>>>>,
    shutdown_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    auto_reconnect: bool,
    reconnect_attempts: Arc<Mutex<u32>>,
}

impl PolymarketWebSocket {
    pub fn new() -> Self {
        Self::with_config(true)
    }

    pub fn with_config(auto_reconnect: bool) -> Self {
        Self {
            state: Arc::new(RwLock::new(WebSocketState::Disconnected)),
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            orderbook_senders: Arc::new(RwLock::new(HashMap::new())),
            orderbooks: Arc::new(RwLock::new(HashMap::new())),
            write_tx: Arc::new(Mutex::new(None)),
            shutdown_tx: Arc::new(Mutex::new(None)),
            auto_reconnect,
            reconnect_attempts: Arc::new(Mutex::new(0)),
        }
    }

    async fn reset_reconnect_attempts(&self) {
        let mut attempts = self.reconnect_attempts.lock().await;
        *attempts = 0;
    }

    #[allow(dead_code)]
    async fn increment_reconnect_attempts(&self) -> u32 {
        let mut attempts = self.reconnect_attempts.lock().await;
        *attempts += 1;
        *attempts
    }

    #[allow(dead_code)]
    pub async fn get_reconnect_attempts(&self) -> u32 {
        *self.reconnect_attempts.lock().await
    }

    async fn set_state(&self, new_state: WebSocketState) {
        let mut state = self.state.write().await;
        *state = new_state;
    }

    async fn send_message(&self, msg: &str) -> Result<(), WebSocketError> {
        let tx = self.write_tx.lock().await;
        if let Some(ref sender) = *tx {
            sender
                .unbounded_send(Message::Text(msg.into()))
                .map_err(|e| WebSocketError::Connection(format!("send failed: {e}")))?;
        }
        Ok(())
    }

    async fn handle_message(&self, text: &str) {
        let msg: WsMessage = match serde_json::from_str(text) {
            Ok(m) => m,
            Err(_) => return,
        };

        match msg.event_type.as_deref() {
            Some("book") => self.handle_book_message(&msg).await,
            Some("price_change") => self.handle_price_change(&msg).await,
            _ => {}
        }
    }

    async fn handle_book_message(&self, msg: &WsMessage) {
        let asset_id = match &msg.asset_id {
            Some(id) => id.clone(),
            None => return,
        };

        let market_id = msg.market.clone().unwrap_or_default();

        let bids: Vec<PriceLevel> = msg
            .bids
            .as_ref()
            .map(|b| {
                b.iter()
                    .filter_map(|l| {
                        let price = l.price.parse::<f64>().ok()?;
                        let size = l.size.parse::<f64>().ok()?;
                        if price > 0.0 && size > 0.0 {
                            Some(PriceLevel::new(price, size))
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let asks: Vec<PriceLevel> = msg
            .asks
            .as_ref()
            .map(|a| {
                a.iter()
                    .filter_map(|l| {
                        let price = l.price.parse::<f64>().ok()?;
                        let size = l.size.parse::<f64>().ok()?;
                        if price > 0.0 && size > 0.0 {
                            Some(PriceLevel::new(price, size))
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let orderbook = Orderbook {
            market_id: market_id.clone(),
            asset_id: asset_id.clone(),
            bids,
            asks,
            last_update_id: None,
            timestamp: Some(chrono::Utc::now()),
        };

        {
            let mut obs = self.orderbooks.write().await;
            obs.insert(asset_id.clone(), orderbook.clone());
        }

        self.broadcast_orderbook(&asset_id, orderbook).await;
    }

    async fn handle_price_change(&self, msg: &WsMessage) {
        let changes = match &msg.price_changes {
            Some(c) => c,
            None => return,
        };

        for change in changes {
            let asset_id = &change.asset_id;

            let mut obs = self.orderbooks.write().await;
            if let Some(ob) = obs.get_mut(asset_id) {
                if let Some(ref bid_str) = change.best_bid {
                    if let Ok(price) = bid_str.parse::<f64>() {
                        if price > 0.0 {
                            if ob.bids.is_empty() {
                                ob.bids.push(PriceLevel::new(price, 1.0));
                            } else {
                                ob.bids[0].price = price;
                            }
                        }
                    }
                }
                if let Some(ref ask_str) = change.best_ask {
                    if let Ok(price) = ask_str.parse::<f64>() {
                        if price > 0.0 {
                            if ob.asks.is_empty() {
                                ob.asks.push(PriceLevel::new(price, 1.0));
                            } else {
                                ob.asks[0].price = price;
                            }
                        }
                    }
                }
                ob.timestamp = Some(chrono::Utc::now());

                let orderbook = ob.clone();
                drop(obs);
                self.broadcast_orderbook(asset_id, orderbook).await;
            }
        }
    }

    async fn broadcast_orderbook(&self, asset_id: &str, orderbook: Orderbook) {
        let senders = self.orderbook_senders.read().await;
        if let Some(sender) = senders.get(asset_id) {
            let _ = sender.send(Ok(orderbook));
        }
    }

    async fn resubscribe_all(&self) -> Result<(), WebSocketError> {
        let subs = self.subscriptions.read().await;
        for (market_id, asset_ids) in subs.iter() {
            let msg = SubscribeMessage {
                auth: HashMap::new(),
                markets: vec![market_id.clone()],
                assets_ids: asset_ids.clone(),
                msg_type: "market".into(),
            };
            let json = serde_json::to_string(&msg)
                .map_err(|e| WebSocketError::Protocol(e.to_string()))?;
            self.send_message(&json).await?;
        }
        Ok(())
    }

    fn calculate_reconnect_delay(attempt: u32) -> Duration {
        let delay = RECONNECT_BASE_DELAY_MS as f64 * 1.5_f64.powi(attempt as i32);
        let delay = delay.min(RECONNECT_MAX_DELAY_MS as f64) as u64;
        Duration::from_millis(delay)
    }
}

impl Default for PolymarketWebSocket {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OrderBookWebSocket for PolymarketWebSocket {
    async fn connect(&mut self) -> Result<(), WebSocketError> {
        self.set_state(WebSocketState::Connecting).await;

        let (ws_stream, _) = connect_async(WS_URL)
            .await
            .map_err(|e| WebSocketError::Connection(e.to_string()))?;

        let (write, read) = ws_stream.split();
        let (tx, rx) = futures::channel::mpsc::unbounded::<Message>();

        {
            let mut write_tx = self.write_tx.lock().await;
            *write_tx = Some(tx);
        }

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        {
            let mut stx = self.shutdown_tx.lock().await;
            *stx = Some(shutdown_tx);
        }

        let state = self.state.clone();
        let subscriptions = self.subscriptions.clone();
        let orderbook_senders = self.orderbook_senders.clone();
        let orderbooks = self.orderbooks.clone();
        let write_tx_clone = self.write_tx.clone();

        let ws_self = PolymarketWebSocket {
            state: state.clone(),
            subscriptions: subscriptions.clone(),
            orderbook_senders: orderbook_senders.clone(),
            orderbooks: orderbooks.clone(),
            write_tx: write_tx_clone.clone(),
            shutdown_tx: Arc::new(Mutex::new(None)),
            auto_reconnect: self.auto_reconnect,
            reconnect_attempts: self.reconnect_attempts.clone(),
        };

        let auto_reconnect = self.auto_reconnect;
        let reconnect_attempts_clone = self.reconnect_attempts.clone();

        tokio::spawn(async move {
            let write_future = rx.map(Ok).forward(write);
            let read_future = async {
                let mut read = read;
                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            ws_self.handle_message(&text).await;
                        }
                        Ok(Message::Ping(data)) => {
                            if let Some(ref tx) = *ws_self.write_tx.lock().await {
                                let _ = tx.unbounded_send(Message::Pong(data));
                            }
                        }
                        Ok(Message::Close(_)) => break,
                        Err(_) => break,
                        _ => {}
                    }
                }
            };

            let ping_future = async {
                let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_SECS));
                loop {
                    ping_interval.tick().await;
                    if let Some(ref tx) = *ws_self.write_tx.lock().await {
                        let _ = tx.unbounded_send(Message::Ping(vec![]));
                    }
                }
            };

            tokio::select! {
                _ = write_future => {},
                _ = read_future => {},
                _ = ping_future => {},
                _ = shutdown_rx => {},
            }

            {
                let mut s = state.write().await;
                if *s == WebSocketState::Closed {
                    return;
                }
                *s = WebSocketState::Disconnected;
            }

            if auto_reconnect {
                let mut attempt = {
                    let mut a = reconnect_attempts_clone.lock().await;
                    *a += 1;
                    *a
                };

                while attempt <= MAX_RECONNECT_ATTEMPTS {
                    {
                        let mut s = state.write().await;
                        *s = WebSocketState::Reconnecting;
                    }

                    let delay = Self::calculate_reconnect_delay(attempt);
                    tokio::time::sleep(delay).await;

                    match connect_async(WS_URL).await {
                        Ok((new_ws, _)) => {
                            let (new_write, new_read) = new_ws.split();
                            let (new_tx, new_rx) = futures::channel::mpsc::unbounded::<Message>();

                            {
                                let mut wtx = write_tx_clone.lock().await;
                                *wtx = Some(new_tx);
                            }

                            {
                                let mut s = state.write().await;
                                *s = WebSocketState::Connected;
                            }

                            {
                                let mut a = reconnect_attempts_clone.lock().await;
                                *a = 0;
                            }

                            let _ = ws_self.resubscribe_all().await;

                            let write_future = new_rx.map(Ok).forward(new_write);
                            let read_future = async {
                                let mut read = new_read;
                                while let Some(msg) = read.next().await {
                                    match msg {
                                        Ok(Message::Text(text)) => {
                                            ws_self.handle_message(&text).await;
                                        }
                                        Ok(Message::Ping(data)) => {
                                            if let Some(ref tx) = *ws_self.write_tx.lock().await {
                                                let _ = tx.unbounded_send(Message::Pong(data));
                                            }
                                        }
                                        Ok(Message::Close(_)) => break,
                                        Err(_) => break,
                                        _ => {}
                                    }
                                }
                            };

                            tokio::select! {
                                _ = write_future => {},
                                _ = read_future => {},
                            }

                            {
                                let s = state.read().await;
                                if *s == WebSocketState::Closed {
                                    return;
                                }
                            }

                            attempt = {
                                let mut a = reconnect_attempts_clone.lock().await;
                                *a += 1;
                                *a
                            };
                        }
                        Err(_) => {
                            attempt = {
                                let mut a = reconnect_attempts_clone.lock().await;
                                *a += 1;
                                *a
                            };
                        }
                    }
                }

                let mut s = state.write().await;
                *s = WebSocketState::Disconnected;
            }
        });

        self.set_state(WebSocketState::Connected).await;
        self.reset_reconnect_attempts().await;
        self.resubscribe_all().await?;

        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), WebSocketError> {
        self.set_state(WebSocketState::Closed).await;
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(());
        }
        Ok(())
    }

    async fn subscribe(&mut self, market_id: &str) -> Result<(), WebSocketError> {
        let asset_ids = vec![market_id.to_string()];

        {
            let mut subs = self.subscriptions.write().await;
            subs.insert(market_id.to_string(), asset_ids.clone());
        }

        {
            let mut senders = self.orderbook_senders.write().await;
            if !senders.contains_key(market_id) {
                let (tx, _) = broadcast::channel(100);
                senders.insert(market_id.to_string(), tx);
            }
        }

        if *self.state.read().await == WebSocketState::Connected {
            let msg = SubscribeMessage {
                auth: HashMap::new(),
                markets: vec![],
                assets_ids: asset_ids,
                msg_type: "market".into(),
            };
            let json = serde_json::to_string(&msg)
                .map_err(|e| WebSocketError::Protocol(e.to_string()))?;
            self.send_message(&json).await?;
        }

        Ok(())
    }

    async fn unsubscribe(&mut self, market_id: &str) -> Result<(), WebSocketError> {
        {
            let mut subs = self.subscriptions.write().await;
            subs.remove(market_id);
        }
        {
            let mut senders = self.orderbook_senders.write().await;
            senders.remove(market_id);
        }
        {
            let mut obs = self.orderbooks.write().await;
            obs.remove(market_id);
        }
        Ok(())
    }

    fn state(&self) -> WebSocketState {
        futures::executor::block_on(async { *self.state.read().await })
    }

    async fn orderbook_stream(
        &mut self,
        market_id: &str,
    ) -> Result<OrderbookStream, WebSocketError> {
        let senders = self.orderbook_senders.read().await;
        let sender = senders
            .get(market_id)
            .ok_or_else(|| WebSocketError::Subscription(format!("not subscribed to {market_id}")))?;

        let rx = sender.subscribe();

        Ok(Box::pin(tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(
            |result| async move {
                result.ok()
            },
        )))
    }
}

pub fn get_orderbook_snapshot(ws: &PolymarketWebSocket, asset_id: &str) -> Option<Orderbook> {
    futures::executor::block_on(async {
        let obs = ws.orderbooks.read().await;
        obs.get(asset_id).cloned()
    })
}
