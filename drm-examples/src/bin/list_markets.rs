use anyhow::Result;
use drm_core::Exchange;
use drm_exchange_polymarket::{Polymarket, PolymarketConfig};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config = PolymarketConfig::new().with_verbose(true);
    let exchange = Polymarket::new(config)?;

    println!("Exchange: {} ({})", exchange.name(), exchange.id());
    println!("Fetching markets...\n");

    let markets = exchange
        .fetch_markets(Some(drm_core::FetchMarketsParams {
            limit: Some(10),
            active_only: true,
        }))
        .await?;

    for market in markets {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("ID: {}", market.id);
        println!("Q:  {}", market.question);
        println!("Outcomes: {:?}", market.outcomes);

        if !market.prices.is_empty() {
            print!("Prices: ");
            for (outcome, price) in &market.prices {
                print!("{}: {:.2}¢  ", outcome, price * 100.0);
            }
            println!();
        }

        println!(
            "Volume: ${:.0} | Liquidity: ${:.0}",
            market.volume, market.liquidity
        );
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    Ok(())
}
