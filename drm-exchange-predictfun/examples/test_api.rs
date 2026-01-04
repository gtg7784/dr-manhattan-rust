use drm_core::Exchange;
use drm_exchange_predictfun::{PredictFun, PredictFunConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = if let Ok(api_key) = std::env::var("PREDICTFUN_API_KEY") {
        println!("Using API key: {}...", &api_key[..10.min(api_key.len())]);
        PredictFunConfig::new()
            .with_api_key(&api_key)
            .with_verbose(true)
    } else {
        println!("No API key, using testnet");
        PredictFunConfig::testnet().with_verbose(true)
    };

    println!("API URL: {}", config.api_url);
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
