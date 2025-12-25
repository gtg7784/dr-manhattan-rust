# drm-exchange-opinion

[![Crates.io](https://img.shields.io/crates/v/drm-exchange-opinion.svg)](https://crates.io/crates/drm-exchange-opinion)
[![Documentation](https://docs.rs/drm-exchange-opinion/badge.svg)](https://docs.rs/drm-exchange-opinion)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

[Opinion](https://opinion.xyz) exchange implementation for dr-manhattan.

## Overview

This crate provides a complete Opinion integration including:

- **REST API**: Fetch markets, create/cancel orders, manage positions
- **Authentication**: API key and multi-sig wallet support

## Installation

```toml
[dependencies]
drm-exchange-opinion = "0.1"
```

## Quick Start

```rust
use drm_core::Exchange;
use drm_exchange_opinion::{Opinion, OpinionConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Public API (no auth required)
    let exchange = Opinion::with_default_config()?;
    
    // Fetch markets
    let markets = exchange.fetch_markets(None).await?;
    for market in markets.iter().take(5) {
        println!("{}: {:?}", market.question, market.prices);
    }
    
    Ok(())
}
```

## Authentication

For trading operations, you need to provide your API key and wallet credentials:

```rust
use drm_exchange_opinion::{Opinion, OpinionConfig};

let config = OpinionConfig::new()
    .with_api_key("your-api-key")
    .with_private_key("0x...")
    .with_multi_sig_addr("0x...");

let exchange = Opinion::new(config)?;

// Now you can create orders, cancel orders, etc.
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
| WebSocket orderbook | - |

## Part of dr-manhattan-rust

This crate is part of the [dr-manhattan-rust](https://github.com/gtg7784/dr-manhattan-rust) project, a Rust port of [guzus/dr-manhattan](https://github.com/guzus/dr-manhattan).

## License

MIT
