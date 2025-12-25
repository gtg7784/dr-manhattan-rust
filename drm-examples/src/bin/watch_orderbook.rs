use anyhow::Result;
use drm_core::OrderBookWebSocket;
use drm_exchange_polymarket::PolymarketWebSocket;
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    println!("Polymarket WebSocket Orderbook Example");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let mut ws = PolymarketWebSocket::new();

    let token_id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "21742633143463906290569050155826241533067272736897614950488156847949938836455".into());

    println!("Connecting to WebSocket...");
    ws.connect().await?;
    println!("Connected!\n");

    println!("Subscribing to token: {}...", &token_id[..20]);
    ws.subscribe(&token_id).await?;
    println!("Subscribed!\n");

    println!("Waiting for orderbook updates (Ctrl+C to exit)...\n");

    let mut stream = ws.orderbook_stream(&token_id).await?;

    let mut count = 0;
    while let Some(result) = stream.next().await {
        let orderbook = match result {
            Ok(ob) => ob,
            Err(e) => {
                eprintln!("Error: {e}");
                continue;
            }
        };

        count += 1;
        println!("━━━ Update #{count} ━━━");
        println!("Market: {}", orderbook.market_id);
        println!("Asset:  {}...", &orderbook.asset_id[..20.min(orderbook.asset_id.len())]);

        if let Some(ts) = orderbook.timestamp {
            println!("Time:   {}", ts.format("%H:%M:%S%.3f"));
        }

        if let (Some(bid), Some(ask)) = (orderbook.best_bid(), orderbook.best_ask()) {
            println!("Best Bid: {:.4} | Best Ask: {:.4} | Spread: {:.4}",
                bid, ask, ask - bid);
        }

        println!("Bids: {} levels | Asks: {} levels",
            orderbook.bids.len(), orderbook.asks.len());

        if !orderbook.bids.is_empty() {
            print!("  Top 3 bids: ");
            for level in orderbook.bids.iter().take(3) {
                print!("{:.3}@{:.0}  ", level.price, level.size);
            }
            println!();
        }

        if !orderbook.asks.is_empty() {
            print!("  Top 3 asks: ");
            for level in orderbook.asks.iter().take(3) {
                print!("{:.3}@{:.0}  ", level.price, level.size);
            }
            println!();
        }

        println!();

        if count >= 10 {
            println!("Received 10 updates, exiting...");
            break;
        }
    }

    ws.disconnect().await?;
    println!("Disconnected.");

    Ok(())
}
