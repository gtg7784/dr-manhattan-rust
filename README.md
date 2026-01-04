# dr-manhattan-rust 🦀

[![CI](https://github.com/gtg7784/dr-manhattan-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/gtg7784/dr-manhattan-rust/actions/workflows/ci.yml)

CCXT-style unified API for prediction markets, rewritten in Rust.

## Inspiration

This project is a Rust port of [guzus/dr-manhattan](https://github.com/guzus/dr-manhattan), an excellent Python library that provides a unified interface for prediction markets (similar to how CCXT unifies crypto exchange APIs). Full credit to the original authors for the design and architecture.

## Features

- **Unified API**: Same interface across all supported prediction market exchanges
- **Type-safe**: Leverage Rust's type system for compile-time safety
- **Async-first**: Built on tokio for high-performance async operations
- **WebSocket support**: Real-time orderbook and trade streaming
- **Rate limiting**: Built-in rate limiter to respect exchange limits

## Architecture

```
dr-manhattan-rust/
├── drm-core/                    # Core traits, models, and errors
│   ├── models/                  # Market, Order, Position, Orderbook
│   ├── exchange/                # Exchange trait, config, rate limiting
│   ├── websocket/               # WebSocket trait for orderbook streaming
│   ├── strategy/                # Strategy trait and order tracker
│   └── error.rs                 # DrmError hierarchy
├── drm-exchange-polymarket/     # Polymarket implementation
├── drm-exchange-limitless/      # Limitless implementation
├── drm-exchange-opinion/        # Opinion implementation
├── drm-exchange-kalshi/         # Kalshi implementation
├── drm-exchange-predictfun/     # Predict.fun implementation
├── drm-examples/                # Example binaries
└── Cargo.toml                   # Workspace configuration
```

## Quick Start

```rust
use drm_core::Exchange;
use drm_exchange_polymarket::{Polymarket, PolymarketConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let exchange = Polymarket::with_default_config()?;
    
    let markets = exchange.fetch_markets(None).await?;
    
    for market in markets.iter().take(5) {
        println!("{}: {:?}", market.question, market.prices);
    }
    
    Ok(())
}
```

## Supported Exchanges

| Exchange | Status | REST | WebSocket |
|----------|--------|------|-----------|
| Polymarket | ✅ Complete | ✅ All endpoints | ✅ Orderbook |
| Limitless | ✅ Complete | ✅ All endpoints | ✅ Orderbook |
| Opinion | ✅ Complete | ✅ All endpoints | - |
| Kalshi | ✅ Complete | ✅ All endpoints | - |
| Predict.fun | ✅ Complete | ✅ All endpoints | - |

## Running Examples

```bash
# List markets from Polymarket
cargo run --bin list-markets

# Watch orderbook updates
cargo run --bin watch-orderbook
```

## Development

```bash
# Build all crates
cargo build

# Run tests
cargo test

# Run clippy
cargo clippy

# Format code
cargo fmt
```

## Configuration

### Polymarket

```rust
use drm_exchange_polymarket::{Polymarket, PolymarketConfig};

// Public API (no auth required)
let exchange = Polymarket::with_default_config()?;

// Authenticated (for trading)
let config = PolymarketConfig::new()
    .with_private_key("0x...")
    .with_funder("0x...");
let exchange = Polymarket::new(config)?;
exchange.init_trading().await?;
```

### Limitless

```rust
use drm_exchange_limitless::{Limitless, LimitlessConfig};

// Public API
let exchange = Limitless::with_default_config()?;

// Authenticated
let config = LimitlessConfig::new()
    .with_private_key("0x...");
let exchange = Limitless::new(config)?;
exchange.authenticate().await?;
```

### Opinion

```rust
use drm_exchange_opinion::{Opinion, OpinionConfig};

// Public API
let exchange = Opinion::with_default_config()?;

// Authenticated
let config = OpinionConfig::new()
    .with_api_key("your-api-key")
    .with_private_key("0x...")
    .with_multi_sig_addr("0x...");
let exchange = Opinion::new(config)?;
```

### Kalshi

```rust
use drm_exchange_kalshi::{Kalshi, KalshiConfig};

// Production API
let config = KalshiConfig::new("your-api-key-id", "/path/to/private-key.pem");
let exchange = Kalshi::new(config)?;

// Demo environment
let config = KalshiConfig::demo("your-api-key-id", "/path/to/private-key.pem");
let exchange = Kalshi::new(config)?;

// With PEM string directly
let config = KalshiConfig::new("your-api-key-id", "")
    .with_private_key_pem("-----BEGIN PRIVATE KEY-----\n...");
let exchange = Kalshi::new(config)?;
```

### Predict.fun

```rust
use drm_exchange_predictfun::{PredictFun, PredictFunConfig};

// Public API
let exchange = PredictFun::with_default_config()?;

// Authenticated (for trading)
let config = PredictFunConfig::new()
    .with_api_key("your-api-key")
    .with_private_key("0x...");
let exchange = PredictFun::new(config)?;
exchange.authenticate().await?;

// Testnet
let config = PredictFunConfig::testnet()
    .with_api_key("your-api-key")
    .with_private_key("0x...");
let exchange = PredictFun::new(config)?;
```

## API Reference

### Exchange Trait

All exchanges implement the `Exchange` trait:

```rust
#[async_trait]
pub trait Exchange: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    
    async fn fetch_markets(&self, params: Option<FetchMarketsParams>) -> Result<Vec<Market>, DrmError>;
    async fn fetch_market(&self, market_id: &str) -> Result<Market, DrmError>;
    async fn fetch_markets_by_slug(&self, slug: &str) -> Result<Vec<Market>, DrmError>;
    
    async fn create_order(&self, market_id: &str, outcome: &str, side: OrderSide, price: f64, size: f64, params: HashMap<String, String>) -> Result<Order, DrmError>;
    async fn cancel_order(&self, order_id: &str, market_id: Option<&str>) -> Result<Order, DrmError>;
    async fn fetch_order(&self, order_id: &str, market_id: Option<&str>) -> Result<Order, DrmError>;
    async fn fetch_open_orders(&self, params: Option<FetchOrdersParams>) -> Result<Vec<Order>, DrmError>;
    
    async fn fetch_positions(&self, market_id: Option<&str>) -> Result<Vec<Position>, DrmError>;
    async fn fetch_balance(&self) -> Result<HashMap<String, f64>, DrmError>;
}
```

### Models

- `Market`: Prediction market with question, outcomes, prices, volume
- `Order`: Order with price, size, status, timestamps
- `Position`: Position with size, average price, current price
- `Orderbook`: Orderbook with bids and asks

## License

MIT

## Acknowledgments

- [guzus/dr-manhattan](https://github.com/guzus/dr-manhattan) - Original Python implementation
- [CCXT](https://github.com/ccxt/ccxt) - Inspiration for the unified exchange API pattern
