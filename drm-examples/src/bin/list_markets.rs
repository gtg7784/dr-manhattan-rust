use std::env;

use anyhow::Result;
use clap::Parser;
use drm_core::{list_exchange_names, Exchange, ExchangeId, FetchMarketsParams};
use drm_exchange_kalshi::{Kalshi, KalshiConfig};
use drm_exchange_limitless::{Limitless, LimitlessConfig};
use drm_exchange_opinion::{Opinion, OpinionConfig};
use drm_exchange_polymarket::{Polymarket, PolymarketConfig};
use drm_exchange_predictfun::{PredictFun, PredictFunConfig};

#[derive(Parser, Debug)]
#[command(name = "list-markets")]
#[command(about = "List markets from prediction market exchanges")]
struct Args {
    /// Exchange to query (polymarket, limitless, opinion, kalshi, predictfun, or "all")
    #[arg(default_value = "all")]
    exchange: String,

    /// Number of markets to fetch per exchange
    #[arg(short, long, default_value = "5")]
    limit: usize,

    /// Only show active markets
    #[arg(short, long, default_value = "true")]
    active_only: bool,
}

fn print_markets(markets: &[drm_core::Market]) {
    for market in markets {
        println!("───────────────────────────────────────");
        println!("ID: {}", market.id);
        println!("Q:  {}", market.question);
        println!("Outcomes: {}", market.outcomes.join(" vs "));

        if !market.prices.is_empty() {
            let prices: Vec<String> = market
                .prices
                .iter()
                .map(|(o, p)| format!("{}: {:.1}%", o, p * 100.0))
                .collect();
            println!("Prices: {}", prices.join(", "));
        }

        println!(
            "Volume: ${:.0} | Liquidity: ${:.0}",
            market.volume, market.liquidity
        );
    }
}

async fn run_polymarket(limit: usize, active_only: bool) -> Result<()> {
    let config = PolymarketConfig::new();
    let exchange = Polymarket::new(config)?;

    println!("\n══════════════════════════════════════════");
    println!("Exchange: {} ({})", exchange.name(), exchange.id());
    println!("Fetching markets...");

    let markets = exchange
        .fetch_markets(Some(FetchMarketsParams {
            limit: Some(limit),
            active_only,
        }))
        .await?;

    print_markets(&markets);
    Ok(())
}

async fn run_limitless(limit: usize, active_only: bool) -> Result<()> {
    let config = LimitlessConfig::new();
    let exchange = Limitless::new(config)?;

    println!("\n══════════════════════════════════════════");
    println!("Exchange: {} ({})", exchange.name(), exchange.id());
    println!("Fetching markets...");

    let markets = exchange
        .fetch_markets(Some(FetchMarketsParams {
            limit: Some(limit),
            active_only,
        }))
        .await?;

    print_markets(&markets);
    Ok(())
}

async fn run_opinion(limit: usize, active_only: bool) -> Result<()> {
    let api_key = match env::var("OPINION_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            println!("\n══════════════════════════════════════════");
            println!("[Opinion] Skipped - OPINION_API_KEY required");
            return Ok(());
        }
    };

    let config = OpinionConfig::new().with_api_key(&api_key);
    let exchange = Opinion::new(config)?;

    println!("\n══════════════════════════════════════════");
    println!("Exchange: {} ({})", exchange.name(), exchange.id());
    println!("Fetching markets...");

    let markets = exchange
        .fetch_markets(Some(FetchMarketsParams {
            limit: Some(limit),
            active_only,
        }))
        .await?;

    print_markets(&markets);
    Ok(())
}

async fn run_kalshi(limit: usize, active_only: bool) -> Result<()> {
    let config = KalshiConfig::demo();
    let exchange = Kalshi::new(config)?;

    println!("\n══════════════════════════════════════════");
    println!("Exchange: {} ({})", exchange.name(), exchange.id());
    println!("Fetching markets...");

    let markets = exchange
        .fetch_markets(Some(FetchMarketsParams {
            limit: Some(limit),
            active_only,
        }))
        .await?;

    print_markets(&markets);
    Ok(())
}

async fn run_predictfun(limit: usize, active_only: bool) -> Result<()> {
    let config = match env::var("PREDICTFUN_API_KEY") {
        Ok(api_key) => PredictFunConfig::new().with_api_key(&api_key),
        Err(_) => {
            println!("\n══════════════════════════════════════════");
            println!("[Predict.fun] No API key - using testnet");
            PredictFunConfig::testnet()
        }
    };

    let exchange = PredictFun::new(config)?;

    println!("\n══════════════════════════════════════════");
    println!("Exchange: {} ({})", exchange.name(), exchange.id());
    println!("Fetching markets...");

    let markets = exchange
        .fetch_markets(Some(FetchMarketsParams {
            limit: Some(limit),
            active_only,
        }))
        .await?;

    print_markets(&markets);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let exchange_names = list_exchange_names();

    println!("Available exchanges: {}", exchange_names.join(", "));

    let exchange = args.exchange.to_lowercase();

    if exchange == "all" {
        if let Err(e) = run_polymarket(args.limit, args.active_only).await {
            eprintln!("[Polymarket] Error: {}", e);
        }

        if let Err(e) = run_limitless(args.limit, args.active_only).await {
            eprintln!("[Limitless] Error: {}", e);
        }

        if let Err(e) = run_opinion(args.limit, args.active_only).await {
            eprintln!("[Opinion] Error: {}", e);
        }

        if let Err(e) = run_kalshi(args.limit, args.active_only).await {
            eprintln!("[Kalshi] Error: {}", e);
        }

        if let Err(e) = run_predictfun(args.limit, args.active_only).await {
            eprintln!("[PredictFun] Error: {}", e);
        }
    } else {
        let exchange_id: ExchangeId = exchange.parse().map_err(|_| {
            anyhow::anyhow!(
                "Unknown exchange: {}. Available: {}",
                args.exchange,
                exchange_names.join(", ")
            )
        })?;

        match exchange_id {
            ExchangeId::Polymarket => run_polymarket(args.limit, args.active_only).await?,
            ExchangeId::Limitless => run_limitless(args.limit, args.active_only).await?,
            ExchangeId::Opinion => run_opinion(args.limit, args.active_only).await?,
            ExchangeId::Kalshi => run_kalshi(args.limit, args.active_only).await?,
            ExchangeId::PredictFun => run_predictfun(args.limit, args.active_only).await?,
        }
    }

    println!("\n══════════════════════════════════════════");
    println!("Done!");

    Ok(())
}
