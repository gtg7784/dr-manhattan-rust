# dr-manhattan-rust 🦀

CCXT-style unified API for prediction markets, rewritten in Rust.

## Architecture

```
dr-manhattan-rust/
├── drm-core/                  # Core traits, models, and errors
│   ├── models/                # Market, Order, Position, Orderbook
│   ├── exchange/              # Exchange trait, config, rate limiting
│   ├── websocket/             # WebSocket trait for orderbook streaming
│   └── error.rs               # DrmError hierarchy
├── drm-exchange-polymarket/   # Polymarket implementation
├── drm-examples/              # Example binaries
└── Cargo.toml                 # Workspace configuration
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
| Polymarket | 🟡 Partial | ✅ fetch_markets | 🚧 WIP |
| Opinion | 🚧 Planned | - | - |
| Limitless | 🚧 Planned | - | - |

## Running Examples

```bash
cargo run --bin list-markets
```

## Development

```bash
cargo build
cargo test
cargo clippy
```

## License

MIT
