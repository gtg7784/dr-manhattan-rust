use drm_core::{Exchange, FetchMarketsParams};
use drm_exchange_predictfun::{PredictFun, PredictFunConfig};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sample_markets_response() -> serde_json::Value {
    serde_json::json!({
        "data": [
            {
                "id": 123,
                "title": "Will BTC reach $150k?",
                "question": "Will BTC reach $150k?",
                "description": "Bitcoin price prediction market",
                "outcomes": [
                    {"name": "Yes", "onChainId": "12345"},
                    {"name": "No", "onChainId": "67890"}
                ],
                "status": "ACTIVE",
                "decimalPrecision": 2,
                "isNegRisk": false,
                "isYieldBearing": true,
                "volume": 50000.0,
                "liquidity": 10000.0,
                "feeRateBps": 50
            },
            {
                "id": 456,
                "title": "ETH above $5k EOY?",
                "question": "ETH above $5k EOY?",
                "description": "Ethereum price prediction",
                "outcomes": [
                    {"name": "Yes", "onChainId": "11111"},
                    {"name": "No", "onChainId": "22222"}
                ],
                "status": "ACTIVE",
                "decimalPrecision": 2,
                "isNegRisk": false,
                "isYieldBearing": true,
                "volume": 100000.0,
                "liquidity": 25000.0,
                "feeRateBps": 50
            }
        ]
    })
}

fn sample_single_market_response() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "id": 789,
            "title": "Single market test",
            "question": "Single market test",
            "description": "Test market description",
            "outcomes": [
                {"name": "Yes", "onChainId": "99999"},
                {"name": "No", "onChainId": "88888"}
            ],
            "status": "ACTIVE",
            "decimalPrecision": 2,
            "isNegRisk": false,
            "isYieldBearing": true,
            "volume": 75000.0,
            "liquidity": 15000.0,
            "feeRateBps": 25
        }
    })
}

fn sample_orderbook_response() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "bids": [[0.45, 100.0], [0.44, 200.0], [0.43, 150.0]],
            "asks": [[0.55, 100.0], [0.56, 200.0], [0.57, 150.0]]
        }
    })
}

#[tokio::test]
async fn test_fetch_markets_parses_response() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/markets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_markets_response()))
        .mount(&mock_server)
        .await;

    let config = PredictFunConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = PredictFun::new(config).unwrap();

    // when
    let markets = exchange.fetch_markets(None).await.unwrap();

    // then
    assert_eq!(markets.len(), 2);

    let first = &markets[0];
    assert_eq!(first.id, "123");
    assert_eq!(first.question, "Will BTC reach $150k?");
    assert_eq!(first.outcomes, vec!["Yes", "No"]);
    assert_eq!(first.volume, 50000.0);
    assert_eq!(first.liquidity, 10000.0);
}

#[tokio::test]
async fn test_fetch_markets_with_limit() {
    // given
    let mock_server = MockServer::start().await;
    // Pagination always uses first=100, then truncates results to limit
    Mock::given(method("GET"))
        .and(path("/v1/markets"))
        .and(query_param("first", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_markets_response()))
        .mount(&mock_server)
        .await;

    let config = PredictFunConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = PredictFun::new(config).unwrap();

    // when
    let params = FetchMarketsParams {
        limit: Some(1),
        active_only: false,
    };
    let markets = exchange.fetch_markets(Some(params)).await.unwrap();

    // then - limit=1 should truncate to 1 market
    assert_eq!(markets.len(), 1);
}

#[tokio::test]
async fn test_fetch_market_by_id() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/markets/789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_single_market_response()))
        .mount(&mock_server)
        .await;

    let config = PredictFunConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = PredictFun::new(config).unwrap();

    // when
    let market = exchange.fetch_market("789").await.unwrap();

    // then
    assert_eq!(market.id, "789");
    assert_eq!(market.question, "Single market test");
    assert_eq!(market.volume, 75000.0);
    assert_eq!(market.liquidity, 15000.0);
}

#[tokio::test]
async fn test_get_orderbook() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/markets/123/orderbook"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_orderbook_response()))
        .mount(&mock_server)
        .await;

    let config = PredictFunConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = PredictFun::new(config).unwrap();

    // when
    let orderbook = exchange.get_orderbook("123").await.unwrap();

    // then
    assert_eq!(orderbook.bids.len(), 3);
    assert_eq!(orderbook.asks.len(), 3);
    assert_eq!(orderbook.bids[0].price, 0.45);
    assert_eq!(orderbook.bids[0].size, 100.0);
    assert_eq!(orderbook.asks[0].price, 0.55);
    assert_eq!(orderbook.asks[0].size, 100.0);
}

#[tokio::test]
async fn test_exchange_info() {
    // given
    let config = PredictFunConfig::new();
    let exchange = PredictFun::new(config).unwrap();

    // when
    let info = exchange.describe();

    // then
    assert_eq!(info.id, "predictfun");
    assert_eq!(info.name, "Predict.fun");
    assert!(info.has_fetch_markets);
    assert!(!info.has_create_order);
    assert!(!info.has_websocket);
}

#[tokio::test]
async fn test_exchange_id_and_name() {
    // given
    let config = PredictFunConfig::new();
    let exchange = PredictFun::new(config).unwrap();

    // when/then
    assert_eq!(exchange.id(), "predictfun");
    assert_eq!(exchange.name(), "Predict.fun");
}

#[tokio::test]
async fn test_config_testnet() {
    // given
    let config = PredictFunConfig::testnet();

    // then
    assert!(config.testnet);
    assert_eq!(config.chain_id, 97);
    assert!(config.api_url.contains("testnet"));
}

#[tokio::test]
async fn test_config_mainnet() {
    // given
    let config = PredictFunConfig::new();

    // then
    assert!(!config.testnet);
    assert_eq!(config.chain_id, 56);
    assert!(!config.api_url.contains("testnet"));
}

#[tokio::test]
async fn test_config_with_credentials() {
    // given
    let config = PredictFunConfig::new()
        .with_api_key("test-api-key")
        .with_private_key("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef");

    // then
    assert!(config.is_authenticated());
    assert_eq!(config.api_key, Some("test-api-key".to_string()));
}

#[tokio::test]
async fn test_describe_with_auth() {
    // given
    let config = PredictFunConfig::new()
        .with_api_key("test-api-key")
        .with_private_key("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef");
    let exchange = PredictFun::new(config).unwrap();

    // when
    let info = exchange.describe();

    // then
    assert!(info.has_create_order);
}

#[tokio::test]
async fn test_market_metadata_contains_token_ids() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/markets/789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_single_market_response()))
        .mount(&mock_server)
        .await;

    let config = PredictFunConfig::new()
        .with_api_url(mock_server.uri())
        .with_verbose(false);
    let exchange = PredictFun::new(config).unwrap();

    // when
    let market = exchange.fetch_market("789").await.unwrap();

    // then
    let token_ids = market.get_token_ids();
    assert_eq!(token_ids.len(), 2);
    assert_eq!(token_ids[0], "99999");
    assert_eq!(token_ids[1], "88888");
}
