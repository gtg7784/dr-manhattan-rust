use async_trait::async_trait;
use ethers::prelude::*;
use ethers::utils::keccak256;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use drm_core::{
    DrmError, Exchange, ExchangeInfo, FetchMarketsParams, FetchOrdersParams, Market, Order,
    OrderSide, OrderStatus, Position, RateLimiter,
};

use crate::config::{PredictFunConfig, PROTOCOL_NAME, PROTOCOL_VERSION};
use crate::error::PredictFunError;

pub struct PredictFun {
    config: PredictFunConfig,
    client: reqwest::Client,
    rate_limiter: Arc<Mutex<RateLimiter>>,
    wallet: Option<LocalWallet>,
    address: Option<Address>,
    jwt_token: Arc<Mutex<Option<String>>>,
    authenticated: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct AuthMessageResponse {
    data: Option<AuthMessageData>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct AuthMessageData {
    message: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct AuthTokenResponse {
    data: Option<AuthTokenData>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct AuthTokenData {
    token: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct AuthRequest {
    signer: String,
    message: String,
    signature: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct MarketsResponse {
    data: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct MarketResponse {
    data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OrderbookResponse {
    data: Option<OrderbookData>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OrderbookData {
    bids: Option<Vec<(f64, f64)>>,
    asks: Option<Vec<(f64, f64)>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OrderResponse {
    data: Option<OrderData>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OrderData {
    hash: Option<String>,
    #[serde(rename = "orderHash")]
    order_hash: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OrdersResponse {
    data: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PositionsResponse {
    data: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct BalanceResponse {
    data: Option<BalanceData>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct BalanceData {
    balance: Option<String>,
    available: Option<String>,
}

impl PredictFun {
    pub fn new(config: PredictFunConfig) -> Result<Self, PredictFunError> {
        let client = reqwest::Client::builder()
            .timeout(config.base.timeout)
            .build()?;

        let rate_limiter = Arc::new(Mutex::new(RateLimiter::new(
            config.base.rate_limit_per_second,
        )));

        let (wallet, address) = if let Some(ref pk) = config.private_key {
            let wallet: LocalWallet = pk
                .parse()
                .map_err(|e| PredictFunError::Config(format!("invalid private key: {e}")))?;
            let wallet = wallet.with_chain_id(config.chain_id);
            let addr = wallet.address();
            (Some(wallet), Some(addr))
        } else {
            (None, None)
        };

        Ok(Self {
            config,
            client,
            rate_limiter,
            wallet,
            address,
            jwt_token: Arc::new(Mutex::new(None)),
            authenticated: Arc::new(Mutex::new(false)),
        })
    }

    pub fn with_default_config() -> Result<Self, PredictFunError> {
        Self::new(PredictFunConfig::default())
    }

    pub fn with_testnet_config() -> Result<Self, PredictFunError> {
        Self::new(PredictFunConfig::testnet())
    }

    async fn rate_limit(&self) {
        self.rate_limiter.lock().await.wait().await;
    }

    fn get_headers(&self, require_auth: bool) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );

        if let Some(ref api_key) = self.config.api_key {
            headers.insert("x-api-key", api_key.parse().unwrap());
        }

        if require_auth {
            if let Ok(token_guard) = self.jwt_token.try_lock() {
                if let Some(ref token) = *token_guard {
                    headers.insert(
                        reqwest::header::AUTHORIZATION,
                        format!("Bearer {token}").parse().unwrap(),
                    );
                }
            }
        }

        headers
    }

    pub async fn authenticate(&self) -> Result<(), PredictFunError> {
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(PredictFunError::Auth("API key required".into()))?;

        let wallet = self
            .wallet
            .as_ref()
            .ok_or(PredictFunError::Auth("private key required".into()))?;

        let address = self
            .address
            .ok_or(PredictFunError::Auth("wallet address not set".into()))?;

        let msg_url = format!("{}/v1/auth/message", self.config.api_url);
        let msg_response = self
            .client
            .get(&msg_url)
            .header("x-api-key", api_key)
            .send()
            .await
            .map_err(|e| PredictFunError::Network(e.to_string()))?;

        if !msg_response.status().is_success() {
            return Err(PredictFunError::Auth("failed to get signing message".into()));
        }

        let msg_data: AuthMessageResponse = msg_response
            .json()
            .await
            .map_err(|e| PredictFunError::Api(e.to_string()))?;

        let message = msg_data
            .data
            .and_then(|d| d.message)
            .ok_or_else(|| PredictFunError::Auth("empty signing message".into()))?;

        let signature = wallet
            .sign_message(&message)
            .await
            .map_err(|e| PredictFunError::Signing(format!("signing failed: {e}")))?;

        let auth_url = format!("{}/v1/auth", self.config.api_url);
        let auth_request = AuthRequest {
            signer: format!("{address:?}"),
            message,
            signature: format!("0x{}", hex::encode(signature.to_vec())),
        };

        let auth_response = self
            .client
            .post(&auth_url)
            .header("Content-Type", "application/json")
            .header("x-api-key", api_key)
            .json(&auth_request)
            .send()
            .await
            .map_err(|e| PredictFunError::Network(e.to_string()))?;

        if !auth_response.status().is_success() {
            return Err(PredictFunError::Auth("JWT authentication failed".into()));
        }

        let token_data: AuthTokenResponse = auth_response
            .json()
            .await
            .map_err(|e| PredictFunError::Api(e.to_string()))?;

        let token = token_data
            .data
            .and_then(|d| d.token)
            .ok_or_else(|| PredictFunError::Auth("failed to get JWT token".into()))?;

        *self.jwt_token.lock().await = Some(token);
        *self.authenticated.lock().await = true;

        if self.config.base.verbose {
            tracing::debug!("Authenticated as {:?}", address);
        }

        Ok(())
    }

    async fn ensure_auth(&self) -> Result<(), PredictFunError> {
        let is_auth = *self.authenticated.lock().await;
        if !is_auth {
            if self.wallet.is_none() || self.config.api_key.is_none() {
                return Err(PredictFunError::Auth(
                    "API key and private key required".into(),
                ));
            }
            self.authenticate().await?;
        }
        Ok(())
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        require_auth: bool,
    ) -> Result<T, PredictFunError> {
        if require_auth {
            self.ensure_auth().await?;
        }

        self.rate_limit().await;

        let url = format!("{}{}", self.config.api_url, endpoint);

        if self.config.base.verbose {
            tracing::debug!("GET {}", url);
        }

        let headers = self.get_headers(require_auth);
        let response = self.client.get(&url).headers(headers).send().await?;

        if response.status() == 429 {
            return Err(PredictFunError::RateLimited);
        }

        if response.status() == 401 {
            *self.authenticated.lock().await = false;
            return Err(PredictFunError::Auth("authentication failed".into()));
        }

        if response.status() == 404 {
            return Err(PredictFunError::Api(format!("not found: {endpoint}")));
        }

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(PredictFunError::Api(text));
        }

        response
            .json()
            .await
            .map_err(|e| PredictFunError::Api(e.to_string()))
    }

    async fn post<T: serde::de::DeserializeOwned, B: serde::Serialize>(
        &self,
        endpoint: &str,
        body: &B,
        require_auth: bool,
    ) -> Result<T, PredictFunError> {
        if require_auth {
            self.ensure_auth().await?;
        }

        self.rate_limit().await;

        let url = format!("{}{}", self.config.api_url, endpoint);

        if self.config.base.verbose {
            tracing::debug!("POST {}", url);
        }

        let headers = self.get_headers(require_auth);
        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(body)
            .send()
            .await?;

        if response.status() == 429 {
            return Err(PredictFunError::RateLimited);
        }

        if response.status() == 401 {
            *self.authenticated.lock().await = false;
            return Err(PredictFunError::Auth("authentication failed".into()));
        }

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(PredictFunError::Api(text));
        }

        response
            .json()
            .await
            .map_err(|e| PredictFunError::Api(e.to_string()))
    }

    async fn delete<T: serde::de::DeserializeOwned, B: serde::Serialize>(
        &self,
        endpoint: &str,
        body: &B,
        require_auth: bool,
    ) -> Result<T, PredictFunError> {
        if require_auth {
            self.ensure_auth().await?;
        }

        self.rate_limit().await;

        let url = format!("{}{}", self.config.api_url, endpoint);

        if self.config.base.verbose {
            tracing::debug!("DELETE {}", url);
        }

        let headers = self.get_headers(require_auth);
        let response = self
            .client
            .delete(&url)
            .headers(headers)
            .json(body)
            .send()
            .await?;

        if response.status() == 429 {
            return Err(PredictFunError::RateLimited);
        }

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(PredictFunError::Api(text));
        }

        response
            .json()
            .await
            .map_err(|e| PredictFunError::Api(e.to_string()))
    }

    fn parse_market(&self, data: serde_json::Value) -> Option<Market> {
        let obj = data.as_object()?;

        let id = obj
            .get("id")
            .and_then(|v| v.as_i64().map(|n| n.to_string()).or_else(|| v.as_str().map(String::from)))?;

        let question = obj
            .get("question")
            .or_else(|| obj.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let description = obj
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let outcomes_data = obj.get("outcomes").and_then(|v| v.as_array());
        let outcomes: Vec<String> = outcomes_data
            .map(|arr| {
                arr.iter()
                    .filter_map(|o| o.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| vec!["Yes".into(), "No".into()]);

        let token_ids: Vec<String> = outcomes_data
            .map(|arr| {
                arr.iter()
                    .filter_map(|o| {
                        o.get("onChainId")
                            .and_then(|n| n.as_str().map(String::from))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let status = obj
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let closed = status == "RESOLVED" || status == "PAUSED";

        let decimal_precision = obj
            .get("decimalPrecision")
            .and_then(|v| v.as_u64())
            .unwrap_or(2) as u32;
        let tick_size = 10f64.powi(-(decimal_precision as i32));

        let volume = obj
            .get("volume")
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let liquidity = obj
            .get("liquidity")
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let is_neg_risk = obj
            .get("isNegRisk")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let is_yield_bearing = obj
            .get("isYieldBearing")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let condition_id = obj
            .get("conditionId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let fee_rate_bps = obj
            .get("feeRateBps")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let mut metadata = data.clone();
        if let Some(meta_obj) = metadata.as_object_mut() {
            meta_obj.insert("clobTokenIds".to_string(), serde_json::json!(token_ids));
            meta_obj.insert("isNegRisk".to_string(), serde_json::json!(is_neg_risk));
            meta_obj.insert(
                "isYieldBearing".to_string(),
                serde_json::json!(is_yield_bearing),
            );
            meta_obj.insert("conditionId".to_string(), serde_json::json!(condition_id));
            meta_obj.insert("feeRateBps".to_string(), serde_json::json!(fee_rate_bps));
            meta_obj.insert("closed".to_string(), serde_json::json!(closed));
            meta_obj.insert("minimum_tick_size".to_string(), serde_json::json!(tick_size));
        }

        Some(Market {
            id,
            question,
            outcomes,
            close_time: None,
            volume,
            liquidity,
            prices: HashMap::new(),
            metadata,
            tick_size,
            description,
        })
    }

    fn parse_order(&self, data: &serde_json::Value, outcome: Option<&str>) -> Order {
        let obj = data.as_object();

        let order_id = obj
            .and_then(|o| {
                o.get("hash")
                    .or_else(|| o.get("orderHash"))
                    .or_else(|| o.get("id"))
            })
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let market_id = obj
            .and_then(|o| o.get("marketId"))
            .and_then(|v| v.as_i64().map(|n| n.to_string()).or_else(|| v.as_str().map(String::from)))
            .unwrap_or_default();

        let side_val = obj.and_then(|o| o.get("side"));
        let side = match side_val {
            Some(serde_json::Value::Number(n)) => {
                if n.as_u64() == Some(0) {
                    OrderSide::Buy
                } else {
                    OrderSide::Sell
                }
            }
            Some(serde_json::Value::String(s)) => {
                if s.to_lowercase() == "buy" {
                    OrderSide::Buy
                } else {
                    OrderSide::Sell
                }
            }
            _ => OrderSide::Buy,
        };

        let status = obj
            .and_then(|o| o.get("status"))
            .and_then(|v| v.as_str())
            .map(|s| self.parse_order_status(s))
            .unwrap_or(OrderStatus::Open);

        let price = obj
            .and_then(|o| o.get("pricePerShare").or_else(|| o.get("price")))
            .and_then(|v| {
                if let Some(s) = v.as_str() {
                    if let Ok(wei) = s.parse::<u128>() {
                        Some(wei as f64 / 1e18)
                    } else {
                        s.parse().ok()
                    }
                } else {
                    v.as_f64()
                }
            })
            .unwrap_or(0.0);

        let size = obj
            .and_then(|o| o.get("amount"))
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let filled = obj
            .and_then(|o| o.get("amountFilled"))
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let created_at = obj
            .and_then(|o| o.get("createdAt"))
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);

        let updated_at = obj
            .and_then(|o| o.get("updatedAt"))
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        Order {
            id: order_id,
            market_id,
            outcome: outcome.unwrap_or("").to_string(),
            side,
            price,
            size,
            filled,
            status,
            created_at,
            updated_at,
        }
    }

    fn parse_order_status(&self, status: &str) -> OrderStatus {
        match status.to_uppercase().as_str() {
            "PENDING" => OrderStatus::Pending,
            "OPEN" | "LIVE" | "ACTIVE" => OrderStatus::Open,
            "FILLED" | "MATCHED" => OrderStatus::Filled,
            "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
            "CANCELLED" | "CANCELED" | "EXPIRED" => OrderStatus::Cancelled,
            "INVALIDATED" => OrderStatus::Rejected,
            _ => OrderStatus::Open,
        }
    }

    fn parse_position(&self, data: &serde_json::Value) -> Position {
        let obj = data.as_object();

        let market_id = obj
            .and_then(|o| o.get("marketId"))
            .and_then(|v| v.as_i64().map(|n| n.to_string()).or_else(|| v.as_str().map(String::from)))
            .unwrap_or_default();

        let outcome = obj
            .and_then(|o| o.get("outcome"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let size = obj
            .and_then(|o| o.get("size"))
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let average_price = obj
            .and_then(|o| o.get("avgPrice"))
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let current_price = obj
            .and_then(|o| o.get("currentPrice"))
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        Position {
            market_id,
            outcome,
            size,
            average_price,
            current_price,
        }
    }

    pub async fn get_orderbook(
        &self,
        market_id: &str,
    ) -> Result<drm_core::Orderbook, PredictFunError> {
        let endpoint = format!("/v1/markets/{market_id}/orderbook");
        let response: OrderbookResponse = self.get(&endpoint, false).await?;

        let data = response.data.unwrap_or(OrderbookData {
            bids: None,
            asks: None,
        });

        let bids: Vec<drm_core::PriceLevel> = data
            .bids
            .unwrap_or_default()
            .into_iter()
            .map(|(price, size)| drm_core::PriceLevel { price, size })
            .collect();

        let asks: Vec<drm_core::PriceLevel> = data
            .asks
            .unwrap_or_default()
            .into_iter()
            .map(|(price, size)| drm_core::PriceLevel { price, size })
            .collect();

        Ok(drm_core::Orderbook {
            market_id: market_id.to_string(),
            asset_id: market_id.to_string(),
            bids,
            asks,
            last_update_id: None,
            timestamp: Some(chrono::Utc::now()),
        })
    }

    pub async fn fetch_token_ids(&self, market_id: &str) -> Result<Vec<String>, PredictFunError> {
        let market = self
            .fetch_market(market_id)
            .await
            .map_err(|e| PredictFunError::Api(format!("{e}")))?;

        let token_ids: Vec<String> = market
            .metadata
            .get("clobTokenIds")
            .and_then(|v| {
                v.as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
            })
            .unwrap_or_default();

        if token_ids.is_empty() {
            return Err(PredictFunError::Api(format!(
                "no token IDs found for market {market_id}"
            )));
        }

        Ok(token_ids)
    }

    fn get_exchange_address(&self, is_neg_risk: bool, is_yield_bearing: bool) -> &'static str {
        if is_yield_bearing {
            if is_neg_risk {
                self.config.get_yield_bearing_neg_risk_ctf_exchange()
            } else {
                self.config.get_yield_bearing_ctf_exchange()
            }
        } else if is_neg_risk {
            self.config.get_neg_risk_ctf_exchange()
        } else {
            self.config.get_ctf_exchange()
        }
    }

    async fn build_signed_order(
        &self,
        token_id: &str,
        price: f64,
        size: f64,
        side: OrderSide,
        fee_rate_bps: u64,
        exchange_address: &str,
    ) -> Result<serde_json::Value, PredictFunError> {
        let wallet = self
            .wallet
            .as_ref()
            .ok_or(PredictFunError::Auth("wallet not initialized".into()))?;

        let address = self
            .address
            .ok_or(PredictFunError::Auth("address not set".into()))?;

        let salt = chrono::Utc::now().timestamp_millis() as u128 * 1_000_000
            + (rand::random::<u32>() % 1_000_000) as u128;

        let shares_wei = (size * 1e18) as u128;
        let price_wei = (price * 1e18) as u128;

        let side_int: u8 = match side {
            OrderSide::Buy => 0,
            OrderSide::Sell => 1,
        };

        let (maker_amount, taker_amount) = match side {
            OrderSide::Buy => {
                let maker = (shares_wei as f64 * price_wei as f64 / 1e18) as u128;
                (maker, shares_wei)
            }
            OrderSide::Sell => {
                let taker = (shares_wei as f64 * price_wei as f64 / 1e18) as u128;
                (shares_wei, taker)
            }
        };

        let order = serde_json::json!({
            "salt": salt.to_string(),
            "maker": format!("{:?}", address),
            "signer": format!("{:?}", address),
            "taker": format!("{:?}", Address::zero()),
            "tokenId": token_id,
            "makerAmount": maker_amount.to_string(),
            "takerAmount": taker_amount.to_string(),
            "expiration": "0",
            "nonce": "0",
            "feeRateBps": fee_rate_bps.to_string(),
            "side": side_int,
            "signatureType": 0,
        });

        let signature =
            self.sign_order_eip712(&order, exchange_address, wallet, address)?;

        let mut signed_order = order;
        signed_order["signature"] = serde_json::json!(signature);

        Ok(signed_order)
    }

    fn sign_order_eip712(
        &self,
        order: &serde_json::Value,
        exchange_address: &str,
        wallet: &LocalWallet,
        _address: Address,
    ) -> Result<String, PredictFunError> {
        let domain_separator = self.compute_domain_separator(exchange_address);
        let struct_hash = self.compute_struct_hash(order)?;

        let mut payload = vec![0x19, 0x01];
        payload.extend_from_slice(&domain_separator);
        payload.extend_from_slice(&struct_hash);

        let hash = keccak256(&payload);

        let signature = wallet
            .sign_hash(hash.into())
            .map_err(|e| PredictFunError::Signing(format!("signing failed: {e}")))?;

        Ok(format!("0x{}", hex::encode(signature.to_vec())))
    }

    fn compute_domain_separator(&self, exchange_address: &str) -> [u8; 32] {
        let domain_type_hash = keccak256(
            b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
        );

        let name_hash = keccak256(PROTOCOL_NAME.as_bytes());
        let version_hash = keccak256(PROTOCOL_VERSION.as_bytes());
        let contract: Address = exchange_address.parse().unwrap();

        keccak256(ethers::abi::encode(&[
            ethers::abi::Token::FixedBytes(domain_type_hash.to_vec()),
            ethers::abi::Token::FixedBytes(name_hash.to_vec()),
            ethers::abi::Token::FixedBytes(version_hash.to_vec()),
            ethers::abi::Token::Uint(U256::from(self.config.chain_id)),
            ethers::abi::Token::Address(contract),
        ]))
    }

    fn compute_struct_hash(&self, order: &serde_json::Value) -> Result<[u8; 32], PredictFunError> {
        let order_type_hash = keccak256(
            b"Order(uint256 salt,address maker,address signer,address taker,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType)"
        );

        let salt = U256::from_dec_str(order["salt"].as_str().unwrap_or("0"))
            .map_err(|e| PredictFunError::Signing(format!("invalid salt: {e}")))?;

        let maker: Address = order["maker"]
            .as_str()
            .unwrap_or("0x0000000000000000000000000000000000000000")
            .parse()
            .map_err(|e| PredictFunError::Signing(format!("invalid maker: {e}")))?;

        let signer: Address = order["signer"]
            .as_str()
            .unwrap_or("0x0000000000000000000000000000000000000000")
            .parse()
            .map_err(|e| PredictFunError::Signing(format!("invalid signer: {e}")))?;

        let taker: Address = order["taker"]
            .as_str()
            .unwrap_or("0x0000000000000000000000000000000000000000")
            .parse()
            .map_err(|e| PredictFunError::Signing(format!("invalid taker: {e}")))?;

        let token_id = U256::from_dec_str(order["tokenId"].as_str().unwrap_or("0"))
            .map_err(|e| PredictFunError::Signing(format!("invalid tokenId: {e}")))?;

        let maker_amount = U256::from_dec_str(order["makerAmount"].as_str().unwrap_or("0"))
            .map_err(|e| PredictFunError::Signing(format!("invalid makerAmount: {e}")))?;

        let taker_amount = U256::from_dec_str(order["takerAmount"].as_str().unwrap_or("0"))
            .map_err(|e| PredictFunError::Signing(format!("invalid takerAmount: {e}")))?;

        let expiration = U256::from_dec_str(order["expiration"].as_str().unwrap_or("0"))
            .map_err(|e| PredictFunError::Signing(format!("invalid expiration: {e}")))?;

        let nonce = U256::from_dec_str(order["nonce"].as_str().unwrap_or("0"))
            .map_err(|e| PredictFunError::Signing(format!("invalid nonce: {e}")))?;

        let fee_rate_bps = U256::from_dec_str(order["feeRateBps"].as_str().unwrap_or("0"))
            .map_err(|e| PredictFunError::Signing(format!("invalid feeRateBps: {e}")))?;

        let side = order["side"].as_u64().unwrap_or(0) as u8;
        let signature_type = order["signatureType"].as_u64().unwrap_or(0) as u8;

        Ok(keccak256(ethers::abi::encode(&[
            ethers::abi::Token::FixedBytes(order_type_hash.to_vec()),
            ethers::abi::Token::Uint(salt),
            ethers::abi::Token::Address(maker),
            ethers::abi::Token::Address(signer),
            ethers::abi::Token::Address(taker),
            ethers::abi::Token::Uint(token_id),
            ethers::abi::Token::Uint(maker_amount),
            ethers::abi::Token::Uint(taker_amount),
            ethers::abi::Token::Uint(expiration),
            ethers::abi::Token::Uint(nonce),
            ethers::abi::Token::Uint(fee_rate_bps),
            ethers::abi::Token::Uint(U256::from(side)),
            ethers::abi::Token::Uint(U256::from(signature_type)),
        ])))
    }
}

#[async_trait]
impl Exchange for PredictFun {
    fn id(&self) -> &'static str {
        "predictfun"
    }

    fn name(&self) -> &'static str {
        "Predict.fun"
    }

    async fn fetch_markets(
        &self,
        params: Option<FetchMarketsParams>,
    ) -> Result<Vec<Market>, DrmError> {
        let params = params.unwrap_or_default();
        let mut query = String::new();

        if let Some(limit) = params.limit {
            query.push_str(&format!("?first={limit}"));
        }

        let endpoint = format!("/v1/markets{query}");
        let response: MarketsResponse = self
            .get(&endpoint, false)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let markets_data = response.data.unwrap_or_default();
        let mut markets: Vec<Market> = markets_data
            .into_iter()
            .filter_map(|v| self.parse_market(v))
            .collect();

        if params.active_only {
            markets.retain(|m| {
                !m.metadata
                    .get("closed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            });
        }

        if let Some(limit) = params.limit {
            markets.truncate(limit);
        }

        Ok(markets)
    }

    async fn fetch_market(&self, market_id: &str) -> Result<Market, DrmError> {
        let endpoint = format!("/v1/markets/{market_id}");
        let response: MarketResponse = self
            .get(&endpoint, false)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let market_data = response.data.ok_or_else(|| {
            DrmError::Exchange(drm_core::ExchangeError::MarketNotFound(market_id.into()))
        })?;

        self.parse_market(market_data).ok_or_else(|| {
            DrmError::Exchange(drm_core::ExchangeError::MarketNotFound(market_id.into()))
        })
    }

    async fn create_order(
        &self,
        market_id: &str,
        outcome: &str,
        side: OrderSide,
        price: f64,
        size: f64,
        params: HashMap<String, String>,
    ) -> Result<Order, DrmError> {
        self.ensure_auth()
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let market = self.fetch_market(market_id).await?;
        let outcomes = &market.outcomes;
        let token_ids: Vec<String> = market
            .metadata
            .get("clobTokenIds")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let token_id = params
            .get("token_id")
            .cloned()
            .or_else(|| {
                let idx = outcomes.iter().position(|o| o == outcome)?;
                token_ids.get(idx).cloned()
            })
            .ok_or_else(|| {
                DrmError::Exchange(drm_core::ExchangeError::InvalidOrder(format!(
                    "could not find token_id for outcome '{outcome}'"
                )))
            })?;

        if price <= 0.0 || price > 1.0 {
            return Err(DrmError::Exchange(drm_core::ExchangeError::InvalidOrder(
                format!("price must be between 0 and 1, got: {price}"),
            )));
        }

        let fee_rate_bps = market
            .metadata
            .get("feeRateBps")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let is_yield_bearing = market
            .metadata
            .get("isYieldBearing")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let is_neg_risk = market
            .metadata
            .get("isNegRisk")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let exchange_address = self.get_exchange_address(is_neg_risk, is_yield_bearing);

        let strategy = params
            .get("strategy")
            .map(|s| s.to_uppercase())
            .unwrap_or_else(|| "LIMIT".into());

        let signed_order = self
            .build_signed_order(&token_id, price, size, side, fee_rate_bps, exchange_address)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let price_per_share_wei = (price * 1e18) as u128;

        let payload = serde_json::json!({
            "data": {
                "pricePerShare": price_per_share_wei.to_string(),
                "strategy": strategy,
                "slippageBps": params.get("slippageBps").unwrap_or(&"0".to_string()),
                "order": signed_order,
            }
        });

        let response: OrderResponse = self
            .post("/v1/orders", &payload, true)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let order_data = response.data.unwrap_or(OrderData {
            hash: None,
            order_hash: None,
            id: None,
        });

        let order_id = order_data
            .hash
            .or(order_data.order_hash)
            .or(order_data.id)
            .unwrap_or_default();

        Ok(Order {
            id: order_id,
            market_id: market_id.to_string(),
            outcome: outcome.to_string(),
            side,
            price,
            size,
            filled: 0.0,
            status: OrderStatus::Open,
            created_at: chrono::Utc::now(),
            updated_at: None,
        })
    }

    async fn cancel_order(
        &self,
        order_id: &str,
        market_id: Option<&str>,
    ) -> Result<Order, DrmError> {
        self.ensure_auth()
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let payload = serde_json::json!({
            "orderHashes": [order_id]
        });

        let _: serde_json::Value = self
            .delete("/v1/orders", &payload, true)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        Ok(Order {
            id: order_id.to_string(),
            market_id: market_id.unwrap_or("").to_string(),
            outcome: String::new(),
            side: OrderSide::Buy,
            price: 0.0,
            size: 0.0,
            filled: 0.0,
            status: OrderStatus::Cancelled,
            created_at: chrono::Utc::now(),
            updated_at: Some(chrono::Utc::now()),
        })
    }

    async fn fetch_order(
        &self,
        order_id: &str,
        _market_id: Option<&str>,
    ) -> Result<Order, DrmError> {
        self.ensure_auth()
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let endpoint = format!("/v1/orders/{order_id}");
        let response: serde_json::Value = self
            .get(&endpoint, true)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let order_data = response.get("data").cloned().unwrap_or(response);
        Ok(self.parse_order(&order_data, None))
    }

    async fn fetch_open_orders(
        &self,
        params: Option<FetchOrdersParams>,
    ) -> Result<Vec<Order>, DrmError> {
        self.ensure_auth()
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let mut query = "?status=OPEN".to_string();
        if let Some(ref p) = params {
            if let Some(ref market_id) = p.market_id {
                query.push_str(&format!("&marketId={market_id}"));
            }
        }

        let endpoint = format!("/v1/orders{query}");
        let response: OrdersResponse = self
            .get(&endpoint, true)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let orders_data = response.data.unwrap_or_default();
        let orders: Vec<Order> = orders_data
            .iter()
            .map(|o| self.parse_order(o, None))
            .collect();

        Ok(orders)
    }

    async fn fetch_positions(&self, market_id: Option<&str>) -> Result<Vec<Position>, DrmError> {
        self.ensure_auth()
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let mut query = String::new();
        if let Some(mid) = market_id {
            query.push_str(&format!("?marketId={mid}"));
        }

        let endpoint = format!("/v1/positions{query}");
        let response: PositionsResponse = self
            .get(&endpoint, true)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let positions_data = response.data.unwrap_or_default();
        let positions: Vec<Position> = positions_data
            .iter()
            .map(|p| self.parse_position(p))
            .collect();

        Ok(positions)
    }

    async fn fetch_balance(&self) -> Result<HashMap<String, f64>, DrmError> {
        self.ensure_auth()
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let response: BalanceResponse = self
            .get("/v1/balance", true)
            .await
            .map_err(|e| DrmError::Exchange(e.into()))?;

        let mut result = HashMap::new();

        if let Some(data) = response.data {
            if let Some(balance) = data.balance {
                if let Ok(val) = balance.parse::<f64>() {
                    result.insert("USDC".to_string(), val);
                }
            }
            if let Some(available) = data.available {
                if let Ok(val) = available.parse::<f64>() {
                    result.insert("USDC_AVAILABLE".to_string(), val);
                }
            }
        }

        Ok(result)
    }

    fn describe(&self) -> ExchangeInfo {
        ExchangeInfo {
            id: self.id(),
            name: self.name(),
            has_fetch_markets: true,
            has_create_order: self.config.is_authenticated(),
            has_websocket: false,
        }
    }
}
