use drm_core::{Exchange, FetchMarketsParams};
use drm_exchange_polymarket::{Polymarket, PolymarketConfig};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sample_markets_response() -> serde_json::Value {
    serde_json::json!([
        {
            "id": "123",
            "question": "Will it rain tomorrow?",
            "outcomes": "[\"Yes\", \"No\"]",
            "outcomePrices": "[\"0.65\", \"0.35\"]",
            "volumeNum": 50000.0,
            "liquidityNum": 10000.0,
            "minimum_tick_size": 0.01,
            "description": "Weather prediction market"
        },
        {
            "id": "456",
            "question": "Bitcoin > $100k by EOY?",
            "outcomes": "[\"Yes\", \"No\"]",
            "outcomePrices": "[\"0.42\", \"0.58\"]",
            "volumeNum": 1000000.0,
            "liquidityNum": 250000.0,
            "minimum_tick_size": 0.001,
            "description": "Crypto price prediction"
        }
    ])
}

fn sample_single_market_response() -> serde_json::Value {
    serde_json::json!({
        "id": "789",
        "question": "Single market test",
        "outcomes": "[\"Yes\", \"No\"]",
        "outcomePrices": "[\"0.80\", \"0.20\"]",
        "volumeNum": 75000.0,
        "liquidityNum": 15000.0,
        "minimum_tick_size": 0.01,
        "description": "Test market description"
    })
}

fn sample_event_response() -> serde_json::Value {
    serde_json::json!([
        {
            "slug": "test-event",
            "markets": [
                {
                    "id": "event-market-1",
                    "question": "Event market question",
                    "outcomes": "[\"Yes\", \"No\"]",
                    "outcomePrices": "[\"0.55\", \"0.45\"]",
                    "volumeNum": 30000.0,
                    "liquidityNum": 5000.0,
                    "minimum_tick_size": 0.01,
                    "description": "Event market"
                }
            ]
        }
    ])
}

#[tokio::test]
async fn test_fetch_markets_parses_response() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_markets_response()))
        .mount(&mock_server)
        .await;

    let config = PolymarketConfig::new()
        .with_gamma_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Polymarket::new(config).unwrap();

    // when
    let markets = exchange.fetch_markets(None).await.unwrap();

    // then
    assert_eq!(markets.len(), 2);

    let first = &markets[0];
    assert_eq!(first.id, "123");
    assert_eq!(first.question, "Will it rain tomorrow?");
    assert_eq!(first.outcomes, vec!["Yes", "No"]);
    assert_eq!(*first.prices.get("Yes").unwrap(), 0.65);
    assert_eq!(*first.prices.get("No").unwrap(), 0.35);
    assert_eq!(first.volume, 50000.0);
    assert_eq!(first.liquidity, 10000.0);
}

#[tokio::test]
async fn test_fetch_markets_with_limit() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_markets_response()))
        .mount(&mock_server)
        .await;

    let config = PolymarketConfig::new()
        .with_gamma_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Polymarket::new(config).unwrap();

    // when
    let params = FetchMarketsParams {
        limit: Some(5),
        active_only: false,
    };
    let markets = exchange.fetch_markets(Some(params)).await.unwrap();

    // then
    assert_eq!(markets.len(), 2);
}

#[tokio::test]
async fn test_fetch_markets_active_only() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets"))
        .and(query_param("active", "true"))
        .and(query_param("closed", "false"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_markets_response()))
        .mount(&mock_server)
        .await;

    let config = PolymarketConfig::new()
        .with_gamma_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Polymarket::new(config).unwrap();

    // when
    let params = FetchMarketsParams {
        limit: None,
        active_only: true,
    };
    let markets = exchange.fetch_markets(Some(params)).await.unwrap();

    // then
    assert!(!markets.is_empty());
}

#[tokio::test]
async fn test_fetch_market_by_id() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets/789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_single_market_response()))
        .mount(&mock_server)
        .await;

    let config = PolymarketConfig::new()
        .with_gamma_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Polymarket::new(config).unwrap();

    // when
    let market = exchange.fetch_market("789").await.unwrap();

    // then
    assert_eq!(market.id, "789");
    assert_eq!(market.question, "Single market test");
    assert_eq!(*market.prices.get("Yes").unwrap(), 0.80);
    assert_eq!(*market.prices.get("No").unwrap(), 0.20);
}

#[tokio::test]
async fn test_fetch_markets_by_slug() {
    // given
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .and(query_param("slug", "test-event"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_event_response()))
        .mount(&mock_server)
        .await;

    let config = PolymarketConfig::new()
        .with_gamma_url(mock_server.uri())
        .with_verbose(false);
    let exchange = Polymarket::new(config).unwrap();

    // when
    let markets = exchange.fetch_markets_by_slug("test-event").await.unwrap();

    // then
    assert_eq!(markets.len(), 1);
    assert_eq!(markets[0].id, "event-market-1");
    assert_eq!(*markets[0].prices.get("Yes").unwrap(), 0.55);
}

#[tokio::test]
async fn test_exchange_info() {
    // given
    let config = PolymarketConfig::new();
    let exchange = Polymarket::new(config).unwrap();

    // when
    let info = exchange.describe();

    // then
    assert_eq!(info.id, "polymarket");
    assert_eq!(info.name, "Polymarket");
    assert!(info.has_fetch_markets);
    assert!(!info.has_create_order);
    assert!(info.has_websocket);
}

#[tokio::test]
async fn test_exchange_id_and_name() {
    // given
    let config = PolymarketConfig::new();
    let exchange = Polymarket::new(config).unwrap();

    // when/then
    assert_eq!(exchange.id(), "polymarket");
    assert_eq!(exchange.name(), "Polymarket");
}
