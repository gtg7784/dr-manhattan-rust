# drm-exchange-predictfun

[![Crates.io](https://img.shields.io/crates/v/drm-exchange-predictfun.svg)](https://crates.io/crates/drm-exchange-predictfun)
[![Documentation](https://docs.rs/drm-exchange-predictfun/badge.svg)](https://docs.rs/drm-exchange-predictfun)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

[Predict.fun](https://predict.fun) exchange implementation for dr-manhattan.

## Overview

This crate provides a complete Predict.fun integration including:

- **REST API**: Fetch markets, create/cancel orders, manage positions
- **EIP-191 Authentication**: JWT token acquisition via signed messages
- **EIP-712 Order Signing**: Typed data signing for order creation
- **Dual Network Support**: Both mainnet (BNB Chain) and testnet

## Installation

```toml
[dependencies]
drm-exchange-predictfun = "0.1"
```

## Quick Start

```rust
use drm_core::Exchange;
use drm_exchange_predictfun::{PredictFun, PredictFunConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Public API (no auth required)
    let exchange = PredictFun::with_default_config()?;
    
    // Fetch markets
    let markets = exchange.fetch_markets(None).await?;
    for market in markets.iter().take(5) {
        println!("{}: {:?}", market.question, market.prices);
    }
    
    Ok(())
}
```

## Authentication

For trading operations, you need to provide your API key and Ethereum private key:

```rust
use drm_exchange_predictfun::{PredictFun, PredictFunConfig};

let config = PredictFunConfig::new()
    .with_api_key("your-api-key")
    .with_private_key("0x...");

let exchange = PredictFun::new(config)?;
exchange.authenticate().await?;

// Now you can create orders, cancel orders, etc.
```

## Testnet

```rust
use drm_exchange_predictfun::{PredictFun, PredictFunConfig};

// Use testnet (BNB Testnet, chain ID 97)
let config = PredictFunConfig::testnet()
    .with_api_key("your-api-key")
    .with_private_key("0x...");

let exchange = PredictFun::new(config)?;
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
| EIP-712 signing | ✅ |
| Testnet support | ✅ |

## Contract Addresses

### Mainnet (BNB Chain, ID: 56)
- Yield-bearing CTF Exchange: `0x6bEb5a40C032AFc305961162d8204CDA16DECFa5`
- Yield-bearing NegRisk CTF Exchange: `0x8A289d458f5a134bA40015085A8F50Ffb681B41d`
- CTF Exchange: `0x8BC070BEdAB741406F4B1Eb65A72bee27894B689`
- NegRisk CTF Exchange: `0x365fb81bd4A24D6303cd2F19c349dE6894D8d58A`

### Testnet (BNB Testnet, ID: 97)
- Yield-bearing CTF Exchange: `0x8a6B4Fa700A1e310b106E7a48bAFa29111f66e89`
- Yield-bearing NegRisk CTF Exchange: `0x95D5113bc50eD201e319101bbca3e0E250662fCC`
- CTF Exchange: `0x2A6413639BD3d73a20ed8C95F634Ce198ABbd2d7`
- NegRisk CTF Exchange: `0xd690b2bd441bE36431F6F6639D7Ad351e7B29680`

## API Reference

- [Predict.fun API Documentation](https://dev.predict.fun/)

## Part of dr-manhattan-rust

This crate is part of the [dr-manhattan-rust](https://github.com/gtg7784/dr-manhattan-rust) project, a Rust port of [guzus/dr-manhattan](https://github.com/guzus/dr-manhattan).

## License

MIT
