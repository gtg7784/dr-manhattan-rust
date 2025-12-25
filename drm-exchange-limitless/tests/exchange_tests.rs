use drm_core::{Exchange, FetchMarketsParams};
use drm_exchange_limitless::{Limitless, LimitlessConfig};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sample_markets_response() -> serde_json::Value {
    serde_json::json!({
        "data": [
            {
                "slug": "will-btc-reach-100k",
                "title": "Will BTC reach $100k?",
                "yesPrice": 65.0,
                "noPrice": 35.0,
                "volume": 250000.0,
                "liquidity": 50000.0,
                "description": "Bitcoin price prediction market",
                "tokens": {
                    "yes": "0x123abc",
                    "no": "0x456def"
                }
            },
            {
                "slug": "eth-merge-success",
                "title": "ETH Merge successful?",
                "yesPrice": 0.85,
                "noPrice": 0.15,
                "volume": 100000.0,
                "liquidity": 20000.0,
                "description": "Ethereum merge outcome",
                "tokens": {
                    "yes": "0x789ghi",
                    "no": "0xabcjkl"
                }
            }
        ]
    })
}

fn sample_single_market_response() -> serde_json::Value {
    serde_json::json!({
        "slug": "test-market",
        "title": "Test market question",
        "yesPrice": 0.70,
        "noPrice": 0.30,
        "volume": 75000.0,
        "liquidity": 15000.0,
        "description": "Test market description",
        "tokens": {
            "yes": "0xyes123",
            "no": "0xno456"
        }
    })
}

fn sample_orderbook_response() -> serde_json::Value {
    serde_json::json!({
        "bids": [
            {"price": "0.65", "size": "100"},
            {"price": "0.64", "size": "200"}
        ],
        "asks": [
            {"price": "0.66", "size": "150"},
            {"price": "0.67", "size": "250"}
        ]
    })
}

#[tokio::test]
async fn test_fetch_markets_parses_response() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets/active"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_markets_response()))
        .mount(&mock_server)
        .await;

    let config = LimitlessConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Limitless::new(config).unwrap();

    // when
    let markets = exchange.fetch_markets(None).await.unwrap();

    // then
    assert_eq!(markets.len(), 2);

    let first = &markets[0];
    assert_eq!(first.id, "will-btc-reach-100k");
    assert_eq!(first.question, "Will BTC reach $100k?");
    assert_eq!(first.outcomes, vec!["Yes", "No"]);
    assert_eq!(*first.prices.get("Yes").unwrap(), 0.65);
    assert_eq!(*first.prices.get("No").unwrap(), 0.35);
}

#[tokio::test]
async fn test_fetch_market_by_slug() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets/test-market"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_single_market_response()))
        .mount(&mock_server)
        .await;

    let config = LimitlessConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Limitless::new(config).unwrap();

    // when
    let market = exchange.fetch_market("test-market").await.unwrap();

    // then
    assert_eq!(market.id, "test-market");
    assert_eq!(market.question, "Test market question");
    assert_eq!(*market.prices.get("Yes").unwrap(), 0.70);
    assert_eq!(*market.prices.get("No").unwrap(), 0.30);
}

#[tokio::test]
async fn test_get_orderbook() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets/test-market/orderbook"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_orderbook_response()))
        .mount(&mock_server)
        .await;

    let config = LimitlessConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Limitless::new(config).unwrap();

    // when
    let orderbook = exchange.get_orderbook("test-market").await.unwrap();

    // then
    assert_eq!(orderbook.bids.len(), 2);
    assert_eq!(orderbook.asks.len(), 2);
    assert_eq!(orderbook.bids[0].price, 0.65);
    assert_eq!(orderbook.asks[0].price, 0.66);
}

#[tokio::test]
async fn test_exchange_info() {
    // given
    let config = LimitlessConfig::new();
    let exchange = Limitless::new(config).unwrap();

    // when
    let info = exchange.describe();

    // then
    assert_eq!(info.id, "limitless");
    assert_eq!(info.name, "Limitless");
    assert!(info.has_fetch_markets);
    assert!(!info.has_create_order);
    assert!(info.has_websocket);
}

#[tokio::test]
async fn test_exchange_id_and_name() {
    // given
    let config = LimitlessConfig::new();
    let exchange = Limitless::new(config).unwrap();

    // when/then
    assert_eq!(exchange.id(), "limitless");
    assert_eq!(exchange.name(), "Limitless");
}

#[tokio::test]
async fn test_market_tick_size() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets/test-market"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_single_market_response()))
        .mount(&mock_server)
        .await;

    let config = LimitlessConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Limitless::new(config).unwrap();

    // when
    let market = exchange.fetch_market("test-market").await.unwrap();

    // then
    assert_eq!(market.tick_size, 0.001);
}

#[tokio::test]
async fn test_fetch_markets_with_limit() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets/active"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_markets_response()))
        .mount(&mock_server)
        .await;

    let config = LimitlessConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Limitless::new(config).unwrap();

    // when
    let params = FetchMarketsParams {
        limit: Some(10),
        active_only: true,
    };
    let markets = exchange.fetch_markets(Some(params)).await.unwrap();

    // then
    assert!(!markets.is_empty());
}
