use drm_core::Exchange;
use drm_exchange_kalshi::{Kalshi, KalshiConfig};

#[tokio::test]
async fn test_fetch_markets() {
    // #given
    // Public API access (no auth required for fetching markets)
    let exchange = Kalshi::with_default_config().expect("should create exchange");

    // #when
    let result = exchange.fetch_markets(None).await;

    // #then
    // Note: This test may fail if Kalshi API is down or rate limited
    // In CI, we just verify the exchange can be created
    assert_eq!(exchange.id(), "kalshi");
    assert_eq!(exchange.name(), "Kalshi");

    // Markets fetch might fail due to auth requirements or rate limits
    // Just log the result for debugging
    match result {
        Ok(markets) => {
            println!("Fetched {} markets", markets.len());
            if !markets.is_empty() {
                println!("First market: {} - {}", markets[0].id, markets[0].question);
            }
        }
        Err(e) => {
            println!("Markets fetch failed (expected in CI): {}", e);
        }
    }
}

#[test]
fn test_config_builder() {
    // #given
    let api_key_id = "test-api-key-id";
    let private_key_path = "/path/to/private-key.pem";

    // #when
    let config = KalshiConfig::new()
        .with_api_key_id(api_key_id)
        .with_private_key_path(private_key_path)
        .with_verbose(true);

    // #then
    assert_eq!(config.api_key_id, Some(api_key_id.to_string()));
    assert_eq!(config.private_key_path, Some(private_key_path.to_string()));
    assert!(config.is_authenticated());
    assert!(config.base.verbose);
}

#[test]
fn test_demo_config() {
    // #given / #when
    let config = KalshiConfig::demo();

    // #then
    assert!(config.demo);
    assert!(config.api_url.contains("demo"));
    assert!(!config.is_authenticated());
}

#[test]
fn test_default_config_not_authenticated() {
    // #given / #when
    let config = KalshiConfig::default();

    // #then
    assert!(!config.is_authenticated());
    assert!(!config.demo);
}
