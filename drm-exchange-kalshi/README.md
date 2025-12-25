# drm-exchange-kalshi

[![Crates.io](https://img.shields.io/crates/v/drm-exchange-kalshi.svg)](https://crates.io/crates/drm-exchange-kalshi)
[![Documentation](https://docs.rs/drm-exchange-kalshi/badge.svg)](https://docs.rs/drm-exchange-kalshi)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

[Kalshi](https://kalshi.com) exchange implementation for dr-manhattan.

## Overview

This crate provides a complete Kalshi integration including:

- **REST API**: Fetch markets, create/cancel orders, manage positions
- **Authentication**: RSA-PKCS1v15 signature-based authentication

## Installation

```toml
[dependencies]
drm-exchange-kalshi = "0.1"
```

## Quick Start

```rust
use drm_core::Exchange;
use drm_exchange_kalshi::{Kalshi, KalshiConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Public API (limited endpoints)
    let exchange = Kalshi::with_default_config()?;
    
    // Fetch markets
    let markets = exchange.fetch_markets(None).await?;
    for market in markets.iter().take(5) {
        println!("{}: {:?}", market.question, market.prices);
    }
    
    Ok(())
}
```

## Authentication

Kalshi uses RSA-PKCS1v15 signatures for authentication. You need:
1. An API key ID (from your Kalshi account)
2. An RSA private key (PEM format)

### Generating RSA Keys

```bash
# Generate RSA private key
openssl genpkey -algorithm RSA -out kalshi_private_key.pem -pkeyopt rsa_keygen_bits:4096

# Extract public key (upload this to Kalshi)
openssl rsa -pubout -in kalshi_private_key.pem -out kalshi_public_key.pem
```

### Configuration

```rust
use drm_exchange_kalshi::{Kalshi, KalshiConfig};

// From file path
let config = KalshiConfig::new()
    .with_api_key_id("your-api-key-id")
    .with_private_key_path("/path/to/kalshi_private_key.pem");

// Or from PEM string
let config = KalshiConfig::new()
    .with_api_key_id("your-api-key-id")
    .with_private_key_pem(include_str!("../kalshi_private_key.pem"));

let exchange = Kalshi::new(config)?;

// Now you can create orders, fetch positions, etc.
```

### Demo Environment

```rust
use drm_exchange_kalshi::{Kalshi, KalshiConfig};

let config = KalshiConfig::demo()
    .with_api_key_id("demo-api-key-id")
    .with_private_key_path("/path/to/demo_private_key.pem");

let exchange = Kalshi::new(config)?;
```

## Features

| Feature | Status |
|---------|--------|
| Fetch markets | ✅ |
| Fetch market by ticker | ✅ |
| Fetch orderbook | ✅ (auth required) |
| Create orders | ✅ |
| Cancel orders | ✅ |
| Fetch orders | ✅ |
| Fetch positions | ✅ |
| Fetch balance | ✅ |
| WebSocket orderbook | - |

## API Notes

### Market Identifiers

Kalshi uses `ticker` strings as market identifiers (e.g., `"INXD-24DEC31-B5000"`).

### Prices

- Kalshi prices are in cents (1-99)
- This library converts them to decimals (0.01-0.99) for consistency with other exchanges

### Orders

```rust
use drm_core::{Exchange, OrderSide};
use std::collections::HashMap;

// Create a limit order to buy Yes at $0.55
let order = exchange.create_order(
    "INXD-24DEC31-B5000",  // ticker
    "Yes",                  // outcome (Yes or No)
    OrderSide::Buy,         // action
    0.55,                   // price in decimal (55 cents)
    10.0,                   // size (number of contracts)
    HashMap::new(),
).await?;
```

## Part of dr-manhattan-rust

This crate is part of the [dr-manhattan-rust](https://github.com/gtg7784/dr-manhattan-rust) project, a Rust port of [guzus/dr-manhattan](https://github.com/guzus/dr-manhattan).

## License

MIT
