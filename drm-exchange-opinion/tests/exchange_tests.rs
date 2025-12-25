use drm_core::Exchange;
use drm_exchange_opinion::{Opinion, OpinionConfig};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sample_markets_response() -> serde_json::Value {
    serde_json::json!({
        "errno": 0,
        "errmsg": null,
        "result": {
            "list": [
                {
                    "market_id": "123",
                    "market_title": "Will BTC reach $100k?",
                    "yes_token_id": "token_yes_123",
                    "no_token_id": "token_no_123",
                    "yes_label": "Yes",
                    "no_label": "No",
                    "volume": 50000.0,
                    "liquidity": 10000.0,
                    "description": "Bitcoin price prediction"
                },
                {
                    "market_id": "456",
                    "market_title": "ETH > $5k?",
                    "yes_token_id": "token_yes_456",
                    "no_token_id": "token_no_456",
                    "yes_label": "Yes",
                    "no_label": "No",
                    "volume": 30000.0,
                    "liquidity": 5000.0,
                    "description": "Ethereum price prediction"
                }
            ]
        }
    })
}

fn sample_single_market_response() -> serde_json::Value {
    serde_json::json!({
        "errno": 0,
        "errmsg": null,
        "result": {
            "data": {
                "market_id": "789",
                "market_title": "Test market question",
                "yes_token_id": "token_yes_789",
                "no_token_id": "token_no_789",
                "yes_label": "Yes",
                "no_label": "No",
                "volume": 25000.0,
                "liquidity": 8000.0,
                "description": "Test market description",
                "cutoff_at": 1735689600
            }
        }
    })
}

fn sample_orderbook_response() -> serde_json::Value {
    serde_json::json!({
        "errno": 0,
        "errmsg": null,
        "result": {
            "data": {
                "bids": [
                    {"price": 0.65, "size": 100.0},
                    {"price": 0.64, "size": 200.0}
                ],
                "asks": [
                    {"price": 0.66, "size": 150.0},
                    {"price": 0.67, "size": 250.0}
                ]
            }
        }
    })
}

#[tokio::test]
async fn test_fetch_markets_parses_response() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/markets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_markets_response()))
        .mount(&mock_server)
        .await;

    let config = OpinionConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Opinion::new(config).unwrap();

    // when
    let markets = exchange.fetch_markets(None).await.unwrap();

    // then
    assert_eq!(markets.len(), 2);

    let first = &markets[0];
    assert_eq!(first.id, "123");
    assert_eq!(first.question, "Will BTC reach $100k?");
    assert_eq!(first.outcomes, vec!["Yes", "No"]);
    assert_eq!(first.volume, 50000.0);
}

#[tokio::test]
async fn test_fetch_market_by_id() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/markets/789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_single_market_response()))
        .mount(&mock_server)
        .await;

    let config = OpinionConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Opinion::new(config).unwrap();

    // when
    let market = exchange.fetch_market("789").await.unwrap();

    // then
    assert_eq!(market.id, "789");
    assert_eq!(market.question, "Test market question");
    assert_eq!(market.volume, 25000.0);
}

#[tokio::test]
async fn test_get_orderbook() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/orderbook"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_orderbook_response()))
        .mount(&mock_server)
        .await;

    let config = OpinionConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Opinion::new(config).unwrap();

    // when
    let orderbook = exchange.get_orderbook("token_yes_123").await.unwrap();

    // then
    assert_eq!(orderbook.bids.len(), 2);
    assert_eq!(orderbook.asks.len(), 2);
    assert_eq!(orderbook.bids[0].price, 0.65);
    assert_eq!(orderbook.asks[0].price, 0.66);
}

#[tokio::test]
async fn test_exchange_info() {
    // given
    let config = OpinionConfig::new();
    let exchange = Opinion::new(config).unwrap();

    // when
    let info = exchange.describe();

    // then
    assert_eq!(info.id, "opinion");
    assert_eq!(info.name, "Opinion");
    assert!(info.has_fetch_markets);
    assert!(!info.has_create_order);
    assert!(!info.has_websocket);
}

#[tokio::test]
async fn test_exchange_id_and_name() {
    // given
    let config = OpinionConfig::new();
    let exchange = Opinion::new(config).unwrap();

    // when/then
    assert_eq!(exchange.id(), "opinion");
    assert_eq!(exchange.name(), "Opinion");
}

#[tokio::test]
async fn test_market_tick_size() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/markets/789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_single_market_response()))
        .mount(&mock_server)
        .await;

    let config = OpinionConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Opinion::new(config).unwrap();

    // when
    let market = exchange.fetch_market("789").await.unwrap();

    // then
    assert_eq!(market.tick_size, 0.001);
}

#[tokio::test]
async fn test_parse_token_ids_from_market() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/markets/789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_single_market_response()))
        .mount(&mock_server)
        .await;

    let config = OpinionConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Opinion::new(config).unwrap();

    // when
    let market = exchange.fetch_market("789").await.unwrap();
    let token_ids = market.get_token_ids();

    // then
    assert_eq!(token_ids.len(), 2);
    assert_eq!(token_ids[0], "token_yes_789");
    assert_eq!(token_ids[1], "token_no_789");
}
