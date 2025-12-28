use drm_core::{Exchange, FetchMarketsParams};
use drm_exchange_kalshi::{Kalshi, KalshiConfig};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sample_markets_response() -> serde_json::Value {
    serde_json::json!({
        "markets": [
            {
                "ticker": "INXD-24DEC31-B5000",
                "title": "S&P 500 above 5000 on Dec 31?",
                "subtitle": "Market resolves Yes if S&P closes above 5000",
                "yes_ask": 65,
                "volume": 150000.0,
                "open_interest": 25000.0,
                "close_time": "2024-12-31T21:00:00Z",
                "status": "open"
            },
            {
                "ticker": "ELON-TWEET-2024",
                "title": "Will Elon tweet about crypto today?",
                "subtitle": "Any tweet mentioning BTC, ETH, or DOGE",
                "yes_ask": 42,
                "volume": 50000.0,
                "open_interest": 8000.0,
                "close_time": "2024-12-28T23:59:59Z",
                "status": "open"
            }
        ],
        "cursor": null
    })
}

fn sample_single_market_response() -> serde_json::Value {
    serde_json::json!({
        "market": {
            "ticker": "INXD-24DEC31-B5000",
            "title": "S&P 500 above 5000 on Dec 31?",
            "subtitle": "Market resolves Yes if S&P closes above 5000",
            "yes_ask": 65,
            "volume": 150000.0,
            "open_interest": 25000.0,
            "close_time": "2024-12-31T21:00:00Z",
            "status": "open"
        }
    })
}

#[tokio::test]
async fn test_fetch_markets_parses_response() {
    // #given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_markets_response()))
        .mount(&mock_server)
        .await;

    let config = KalshiConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Kalshi::new(config).unwrap();

    // #when
    let markets = exchange.fetch_markets(None).await.unwrap();

    // #then
    assert_eq!(markets.len(), 2);

    let first = &markets[0];
    assert_eq!(first.id, "INXD-24DEC31-B5000");
    assert_eq!(first.question, "S&P 500 above 5000 on Dec 31?");
    assert_eq!(first.outcomes, vec!["Yes", "No"]);
    assert_eq!(*first.prices.get("Yes").unwrap(), 0.65);
    assert_eq!(*first.prices.get("No").unwrap(), 0.35);
    assert_eq!(first.volume, 150000.0);
    assert_eq!(first.liquidity, 25000.0);
}

#[tokio::test]
async fn test_fetch_markets_with_limit() {
    // #given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_markets_response()))
        .mount(&mock_server)
        .await;

    let config = KalshiConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Kalshi::new(config).unwrap();

    // #when
    let params = FetchMarketsParams {
        limit: Some(5),
        active_only: false,
    };
    let markets = exchange.fetch_markets(Some(params)).await.unwrap();

    // #then
    assert_eq!(markets.len(), 2);
}

#[tokio::test]
async fn test_fetch_markets_active_only() {
    // #given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets"))
        .and(query_param("status", "open"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_markets_response()))
        .mount(&mock_server)
        .await;

    let config = KalshiConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Kalshi::new(config).unwrap();

    // #when
    let params = FetchMarketsParams {
        limit: None,
        active_only: true,
    };
    let markets = exchange.fetch_markets(Some(params)).await.unwrap();

    // #then
    assert!(!markets.is_empty());
}

#[tokio::test]
async fn test_fetch_market_by_ticker() {
    // #given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets/INXD-24DEC31-B5000"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_single_market_response()))
        .mount(&mock_server)
        .await;

    let config = KalshiConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Kalshi::new(config).unwrap();

    // #when
    let market = exchange.fetch_market("INXD-24DEC31-B5000").await.unwrap();

    // #then
    assert_eq!(market.id, "INXD-24DEC31-B5000");
    assert_eq!(market.question, "S&P 500 above 5000 on Dec 31?");
    assert_eq!(*market.prices.get("Yes").unwrap(), 0.65);
    assert_eq!(*market.prices.get("No").unwrap(), 0.35);
}

#[tokio::test]
async fn test_fetch_markets_by_event_ticker() {
    // #given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets"))
        .and(query_param("event_ticker", "INXD"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_markets_response()))
        .mount(&mock_server)
        .await;

    let config = KalshiConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Kalshi::new(config).unwrap();

    // #when
    let markets = exchange.fetch_markets_by_slug("INXD").await.unwrap();

    // #then
    assert_eq!(markets.len(), 2);
}

#[tokio::test]
async fn test_exchange_info() {
    // #given
    let config = KalshiConfig::new();
    let exchange = Kalshi::new(config).unwrap();

    // #when
    let info = exchange.describe();

    // #then
    assert_eq!(info.id, "kalshi");
    assert_eq!(info.name, "Kalshi");
    assert!(info.has_fetch_markets);
    assert!(!info.has_create_order);
    assert!(!info.has_websocket);
}

#[tokio::test]
async fn test_exchange_id_and_name() {
    // #given
    let config = KalshiConfig::new();
    let exchange = Kalshi::new(config).unwrap();

    // #when / #then
    assert_eq!(exchange.id(), "kalshi");
    assert_eq!(exchange.name(), "Kalshi");
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
