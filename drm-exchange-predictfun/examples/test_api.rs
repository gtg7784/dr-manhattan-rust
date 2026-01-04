use drm_core::Exchange;
use drm_exchange_predictfun::{PredictFun, PredictFunConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = PredictFunConfig::new().with_verbose(true);
    let exchange = PredictFun::new(config)?;

    println!("Exchange: {} ({})", exchange.name(), exchange.id());
    println!("Fetching markets from Predict.fun...\n");

    let markets = exchange
        .fetch_markets(Some(drm_core::FetchMarketsParams {
            limit: Some(5),
            active_only: true,
        }))
        .await?;

    for market in markets {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("ID: {}", market.id);
        println!("Q:  {}", market.question);
        println!("Outcomes: {:?}", market.outcomes);
        println!(
            "Volume: ${:.0} | Liquidity: ${:.0}",
            market.volume, market.liquidity
        );
    }
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    Ok(())
}
