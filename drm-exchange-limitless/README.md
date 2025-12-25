# drm-exchange-limitless

[![Crates.io](https://img.shields.io/crates/v/drm-exchange-limitless.svg)](https://crates.io/crates/drm-exchange-limitless)
[![Documentation](https://docs.rs/drm-exchange-limitless/badge.svg)](https://docs.rs/drm-exchange-limitless)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

[Limitless](https://limitless.exchange) exchange implementation for dr-manhattan.

## Overview

This crate provides a complete Limitless integration including:

- **REST API**: Fetch markets, create/cancel orders, manage positions
- **WebSocket**: Real-time orderbook streaming via Socket.IO
- **CLOB Client**: Direct access to Limitless's Central Limit Order Book
- **Authentication**: Ethereum wallet signing for trading

## Installation

```toml
[dependencies]
drm-exchange-limitless = "0.1"
```

## Quick Start

```rust
use drm_core::Exchange;
use drm_exchange_limitless::{Limitless, LimitlessConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Public API (no auth required)
    let exchange = Limitless::with_default_config()?;
    
    // Fetch markets
    let markets = exchange.fetch_markets(None).await?;
    for market in markets.iter().take(5) {
        println!("{}: {:?}", market.question, market.prices);
    }
    
    Ok(())
}
```

## Authentication

For trading operations, you need to provide your Ethereum private key:

```rust
use drm_exchange_limitless::{Limitless, LimitlessConfig};

let config = LimitlessConfig::new()
    .with_private_key("0x...");

let exchange = Limitless::new(config)?;
exchange.authenticate().await?;

// Now you can create orders, cancel orders, etc.
```

## WebSocket Streaming

```rust
use drm_exchange_limitless::LimitlessWebSocket;
use drm_core::WebSocketClient;

let ws = LimitlessWebSocket::new();
let mut stream = ws.subscribe_orderbook("market_id").await?;

while let Some(orderbook) = stream.next().await {
    println!("Bids: {:?}, Asks: {:?}", orderbook.bids, orderbook.asks);
}
```

## Features

| Feature | Status |
|---------|--------|
| Fetch markets | ✅ |
| Fetch orderbook | ✅ |
| Create orders | ✅ |
| Cancel orders | ✅ |
| Fetch positions | ✅ |
| Fetch balance | ✅ |
| WebSocket orderbook | ✅ |

## Part of dr-manhattan-rust

This crate is part of the [dr-manhattan-rust](https://github.com/gtg7784/dr-manhattan-rust) project, a Rust port of [guzus/dr-manhattan](https://github.com/guzus/dr-manhattan).

## License

MIT
