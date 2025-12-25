# drm-core

[![Crates.io](https://img.shields.io/crates/v/drm-core.svg)](https://crates.io/crates/drm-core)
[![Documentation](https://docs.rs/drm-core/badge.svg)](https://docs.rs/drm-core)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Core traits, models, and errors for the dr-manhattan prediction market SDK.

## Overview

`drm-core` provides the foundational components for building prediction market integrations:

- **Exchange trait**: Unified async interface for all prediction market operations
- **Models**: `Market`, `Order`, `Position`, `Orderbook`, and more
- **WebSocket trait**: Real-time orderbook and trade streaming
- **Strategy trait**: Framework for building trading strategies
- **Error types**: Comprehensive error hierarchy (`DrmError`)

## Installation

```toml
[dependencies]
drm-core = "0.1"
```

## Usage

This crate is typically used as a dependency by exchange implementations (`drm-exchange-polymarket`, `drm-exchange-limitless`, `drm-exchange-opinion`).

```rust
use drm_core::{Exchange, Market, Order, OrderSide, DrmError};

// The Exchange trait defines the unified API
#[async_trait]
pub trait Exchange: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    
    async fn fetch_markets(&self, params: Option<FetchMarketsParams>) -> Result<Vec<Market>, DrmError>;
    async fn fetch_market(&self, market_id: &str) -> Result<Market, DrmError>;
    async fn create_order(&self, ...) -> Result<Order, DrmError>;
    // ... more methods
}
```

## Models

| Model | Description |
|-------|-------------|
| `Market` | Prediction market with question, outcomes, prices, volume |
| `Order` | Order with price, size, status, timestamps |
| `Position` | Position with size, average price, current price |
| `Orderbook` | Orderbook with bids and asks |
| `Trade` | Executed trade information |

## Features

- **Async-first**: Built on `tokio` for high-performance async operations
- **Type-safe**: Leverage Rust's type system for compile-time safety
- **Serde support**: All models are serializable/deserializable

## Part of dr-manhattan-rust

This crate is part of the [dr-manhattan-rust](https://github.com/gtg7784/dr-manhattan-rust) project, a Rust port of [guzus/dr-manhattan](https://github.com/guzus/dr-manhattan).

## License

MIT
