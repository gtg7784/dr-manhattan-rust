use ethers::prelude::*;
use ethers::utils::keccak256;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::CHAIN_ID;
use crate::error::LimitlessError;

/// Order side for Limitless CLOB
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitlessSide {
    Buy,
    Sell,
}

impl LimitlessSide {
    pub fn as_u8(&self) -> u8 {
        match self {
            LimitlessSide::Buy => 0,
            LimitlessSide::Sell => 1,
        }
    }
}

/// Order type for Limitless
#[derive(Debug, Clone, Copy, Default)]
pub enum LimitlessOrderType {
    #[default]
    Gtc,
    Fok,
}

impl LimitlessOrderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            LimitlessOrderType::Gtc => "GTC",
            LimitlessOrderType::Fok => "FOK",
        }
    }
}

/// Signed order for Limitless API
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedOrder {
    pub salt: u64,
    pub maker: String,
    pub signer: String,
    pub taker: String,
    pub token_id: String,
    pub maker_amount: u64,
    pub taker_amount: u64,
    pub expiration: String,
    pub nonce: u64,
    pub fee_rate_bps: u64,
    pub side: u8,
    pub signature_type: u8,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
}

/// Request payload for creating an order
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrderRequest {
    pub order: SignedOrder,
    pub order_type: String,
    pub market_slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
}

/// Order response from Limitless API
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderResponse {
    pub id: Option<String>,
    #[serde(rename = "orderId")]
    pub order_id: Option<String>,
    pub status: Option<String>,
    pub filled: Option<f64>,
    #[serde(rename = "errorMsg")]
    pub error_msg: Option<String>,
}

/// Order data from Limitless API
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitlessOrderData {
    pub id: Option<String>,
    #[serde(rename = "orderId")]
    pub order_id: Option<String>,
    pub market_slug: Option<String>,
    pub token_id: Option<String>,
    pub side: Option<String>,
    pub price: Option<f64>,
    pub size: Option<f64>,
    pub original_size: Option<f64>,
    pub filled: Option<f64>,
    pub status: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Orderbook level
#[derive(Debug, Clone, Deserialize)]
pub struct OrderbookLevel {
    pub price: String,
    pub size: String,
}

/// Orderbook response
#[derive(Debug, Clone, Deserialize)]
pub struct OrderbookResponse {
    pub bids: Vec<OrderbookLevel>,
    pub asks: Vec<OrderbookLevel>,
}

/// Position data from Limitless API
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitlessPosition {
    pub market_slug: Option<String>,
    pub token_id: Option<String>,
    pub outcome: Option<String>,
    pub size: Option<f64>,
    pub average_price: Option<f64>,
    pub current_price: Option<f64>,
}

/// Balance data from Limitless API
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceResponse {
    pub balance: Option<String>,
    pub available: Option<String>,
}

/// User profile from login response
#[derive(Debug, Clone, Deserialize)]
pub struct UserProfile {
    pub id: String,
    #[serde(default)]
    pub address: Option<String>,
}

/// Login response
#[derive(Debug, Clone, Deserialize)]
pub struct LoginResponse {
    pub user: Option<UserProfile>,
    pub id: Option<String>,
}

/// Limitless CLOB client for authenticated operations
pub struct LimitlessClobClient {
    http: reqwest::Client,
    wallet: LocalWallet,
    address: Address,
    host: String,
    chain_id: u64,
    authenticated: bool,
    owner_id: Option<String>,
    token_to_slug: std::collections::HashMap<String, String>,
    no_tokens: HashSet<String>,
}

impl LimitlessClobClient {
    /// Create a new CLOB client with private key
    pub fn new(private_key: &str, host: &str) -> Result<Self, LimitlessError> {
        let wallet: LocalWallet = private_key
            .parse()
            .map_err(|e| LimitlessError::Auth(format!("invalid private key: {}", e)))?;

        let wallet = wallet.with_chain_id(CHAIN_ID);
        let address = wallet.address();

        Ok(Self {
            http: reqwest::Client::new(),
            wallet,
            address,
            host: host.to_string(),
            chain_id: CHAIN_ID,
            authenticated: false,
            owner_id: None,
            token_to_slug: std::collections::HashMap::new(),
            no_tokens: HashSet::new(),
        })
    }

    /// Get wallet address
    pub fn address(&self) -> Address {
        self.address
    }

    /// Check if authenticated
    pub fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    /// Get owner ID from login
    pub fn owner_id(&self) -> Option<&str> {
        self.owner_id.as_deref()
    }

    /// Authenticate with Limitless API
    pub async fn authenticate(&mut self) -> Result<(), LimitlessError> {
        // Get signing message
        let url = format!("{}/auth/signing-message", self.host);
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| LimitlessError::Http(e))?;

        if !response.status().is_success() {
            return Err(LimitlessError::Auth("failed to get signing message".into()));
        }

        let message = response
            .text()
            .await
            .map_err(|e| LimitlessError::Http(e))?
            .trim()
            .to_string();

        if message.is_empty() {
            return Err(LimitlessError::Auth("empty signing message".into()));
        }

        // Sign the message using EIP-191 personal sign
        let signature = self
            .wallet
            .sign_message(&message)
            .await
            .map_err(|e| LimitlessError::Auth(format!("signing failed: {}", e)))?;

        let sig_hex = format!("0x{}", hex::encode(signature.to_vec()));
        let message_hex = format!("0x{}", hex::encode(message.as_bytes()));

        // Login with signature
        let login_url = format!("{}/auth/login", self.host);
        let login_response = self
            .http
            .post(&login_url)
            .header("x-account", format!("{:?}", self.address))
            .header("x-signing-message", message_hex)
            .header("x-signature", sig_hex)
            .json(&serde_json::json!({"client": "eoa"}))
            .send()
            .await
            .map_err(|e| LimitlessError::Http(e))?;

        if !login_response.status().is_success() {
            let text = login_response.text().await.unwrap_or_default();
            return Err(LimitlessError::Auth(format!("login failed: {}", text)));
        }

        // Extract owner ID
        if let Ok(login_data) = login_response.json::<LoginResponse>().await {
            self.owner_id = login_data.user.map(|u| u.id).or(login_data.id);
        }

        self.authenticated = true;
        Ok(())
    }

    /// Register token ID to slug mapping
    pub fn register_token_mapping(&mut self, token_id: &str, slug: &str, is_no_token: bool) {
        self.token_to_slug.insert(token_id.to_string(), slug.to_string());
        if is_no_token {
            self.no_tokens.insert(token_id.to_string());
        }
    }

    /// Get market slug for token ID
    pub fn get_slug_for_token(&self, token_id: &str) -> Option<&str> {
        self.token_to_slug.get(token_id).map(|s| s.as_str())
    }

    /// Check if token is a No token (needs inverted orderbook)
    pub fn is_no_token(&self, token_id: &str) -> bool {
        self.no_tokens.contains(token_id)
    }

    /// Build and sign an order
    pub fn build_signed_order(
        &self,
        token_id: &str,
        price: f64,
        size: f64,
        side: LimitlessSide,
        order_type: LimitlessOrderType,
        exchange_address: &str,
        fee_rate_bps: u64,
    ) -> Result<SignedOrder, LimitlessError> {
        // Generate salt (timestamp-based)
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let salt = timestamp_ms * 1000 + (timestamp_ms % 1000) + 86400000; // +1 day offset

        // Scale factors
        let shares_scale: u64 = 1_000_000;
        let collateral_scale: u64 = 1_000_000;
        let price_scale: u64 = 1_000_000;
        let price_tick: u64 = 1000; // 0.001 * 1_000_000

        // Scale inputs
        let shares = (size * shares_scale as f64) as u64;
        let price_int = (price * price_scale as f64) as u64;

        // Align shares to tick
        let shares_step = price_scale / price_tick;
        let shares = (shares / shares_step) * shares_step;

        // Calculate collateral
        let numerator = shares as u128 * price_int as u128 * collateral_scale as u128;
        let denominator = shares_scale as u128 * price_scale as u128;

        let (maker_amount, taker_amount) = match side {
            LimitlessSide::Buy => {
                // BUY: Round UP
                let collateral = ((numerator + denominator - 1) / denominator) as u64;
                (collateral, shares)
            }
            LimitlessSide::Sell => {
                // SELL: Round DOWN
                let collateral = (numerator / denominator) as u64;
                (shares, collateral)
            }
        };

        let token_id_u256 = U256::from_dec_str(token_id)
            .map_err(|e| LimitlessError::Api(format!("invalid token_id: {}", e)))?;

        // Compute order hash
        let order_hash = self.compute_order_hash(
            U256::from(salt),
            self.address,
            self.address,
            Address::zero(),
            token_id_u256,
            U256::from(maker_amount),
            U256::from(taker_amount),
            U256::zero(), // expiration
            U256::zero(), // nonce
            U256::from(fee_rate_bps),
            side.as_u8(),
            0u8, // EOA signature type
            exchange_address,
        );

        // Sign the hash
        let signature = self
            .wallet
            .sign_hash(order_hash.into())
            .map_err(|e| LimitlessError::Auth(format!("signing failed: {}", e)))?;

        let mut order = SignedOrder {
            salt,
            maker: format!("{:?}", self.address),
            signer: format!("{:?}", self.address),
            taker: format!("{:?}", Address::zero()),
            token_id: token_id.to_string(),
            maker_amount,
            taker_amount,
            expiration: "0".to_string(),
            nonce: 0,
            fee_rate_bps,
            side: side.as_u8(),
            signature_type: 0,
            signature: format!("0x{}", hex::encode(signature.to_vec())),
            price: None,
        };

        // Add price for GTC orders
        if matches!(order_type, LimitlessOrderType::Gtc) {
            order.price = Some((price * 1000.0).round() / 1000.0);
        }

        Ok(order)
    }

    fn compute_order_hash(
        &self,
        salt: U256,
        maker: Address,
        signer: Address,
        taker: Address,
        token_id: U256,
        maker_amount: U256,
        taker_amount: U256,
        expiration: U256,
        nonce: U256,
        fee_rate_bps: U256,
        side: u8,
        signature_type: u8,
        exchange_address: &str,
    ) -> [u8; 32] {
        let order_type_hash = keccak256(
            b"Order(uint256 salt,address maker,address signer,address taker,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType)"
        );

        let domain_separator = self.compute_domain_separator(exchange_address);

        let struct_hash = keccak256(ethers::abi::encode(&[
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
        ]));

        let mut payload = vec![0x19, 0x01];
        payload.extend_from_slice(&domain_separator);
        payload.extend_from_slice(&struct_hash);

        keccak256(&payload)
    }

    fn compute_domain_separator(&self, exchange_address: &str) -> [u8; 32] {
        let domain_type_hash = keccak256(
            b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
        );

        let name_hash = keccak256(b"Limitless CTF Exchange");
        let version_hash = keccak256(b"1");
        let contract: Address = exchange_address
            .parse()
            .expect("invalid exchange address");

        keccak256(ethers::abi::encode(&[
            ethers::abi::Token::FixedBytes(domain_type_hash.to_vec()),
            ethers::abi::Token::FixedBytes(name_hash.to_vec()),
            ethers::abi::Token::FixedBytes(version_hash.to_vec()),
            ethers::abi::Token::Uint(U256::from(self.chain_id)),
            ethers::abi::Token::Address(contract),
        ]))
    }

    /// Create and post an order
    pub async fn post_order(
        &self,
        order: SignedOrder,
        order_type: LimitlessOrderType,
        market_slug: &str,
    ) -> Result<OrderResponse, LimitlessError> {
        if !self.authenticated {
            return Err(LimitlessError::AuthRequired);
        }

        let request = CreateOrderRequest {
            order,
            order_type: order_type.as_str().to_string(),
            market_slug: market_slug.to_string(),
            owner_id: self.owner_id.clone(),
        };

        let url = format!("{}/orders", self.host);
        let response = self
            .http
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| LimitlessError::Http(e))?;

        if response.status() == 429 {
            return Err(LimitlessError::RateLimited);
        }

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(LimitlessError::Api(format!("post order failed: {}", text)));
        }

        response
            .json()
            .await
            .map_err(|e| LimitlessError::Api(format!("parse response failed: {}", e)))
    }

    /// Cancel an order
    pub async fn cancel_order(&self, order_id: &str) -> Result<(), LimitlessError> {
        if !self.authenticated {
            return Err(LimitlessError::AuthRequired);
        }

        let url = format!("{}/orders/{}", self.host, order_id);
        let response = self
            .http
            .delete(&url)
            .send()
            .await
            .map_err(|e| LimitlessError::Http(e))?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(LimitlessError::Api(format!("cancel order failed: {}", text)));
        }

        Ok(())
    }

    /// Cancel all orders for a market
    pub async fn cancel_all_orders(&self, market_slug: &str) -> Result<(), LimitlessError> {
        if !self.authenticated {
            return Err(LimitlessError::AuthRequired);
        }

        let url = format!("{}/orders/all/{}", self.host, market_slug);
        let response = self
            .http
            .delete(&url)
            .send()
            .await
            .map_err(|e| LimitlessError::Http(e))?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(LimitlessError::Api(format!("cancel all orders failed: {}", text)));
        }

        Ok(())
    }

    /// Get order by ID
    pub async fn get_order(&self, order_id: &str) -> Result<LimitlessOrderData, LimitlessError> {
        if !self.authenticated {
            return Err(LimitlessError::AuthRequired);
        }

        let url = format!("{}/orders/{}", self.host, order_id);
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| LimitlessError::Http(e))?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(LimitlessError::Api(format!("get order failed: {}", text)));
        }

        response
            .json()
            .await
            .map_err(|e| LimitlessError::Api(format!("parse order failed: {}", e)))
    }

    /// Get open orders
    pub async fn get_open_orders(
        &self,
        market_slug: Option<&str>,
    ) -> Result<Vec<LimitlessOrderData>, LimitlessError> {
        if !self.authenticated {
            return Err(LimitlessError::AuthRequired);
        }

        let mut url = format!("{}/orders", self.host);
        if let Some(slug) = market_slug {
            url.push_str(&format!("?marketSlug={}", slug));
        }

        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| LimitlessError::Http(e))?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(LimitlessError::Api(format!("get orders failed: {}", text)));
        }

        // Response may be { "data": [...] } or just [...]
        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LimitlessError::Api(format!("parse orders failed: {}", e)))?;

        let orders_arr = data
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_else(|| {
                data.as_array().cloned().unwrap_or_default()
            });

        let orders: Vec<LimitlessOrderData> = orders_arr
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();

        Ok(orders)
    }

    /// Get positions
    pub async fn get_positions(
        &self,
        market_slug: Option<&str>,
    ) -> Result<Vec<LimitlessPosition>, LimitlessError> {
        if !self.authenticated {
            return Err(LimitlessError::AuthRequired);
        }

        let mut url = format!("{}/positions", self.host);
        if let Some(slug) = market_slug {
            url.push_str(&format!("?marketSlug={}", slug));
        }

        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| LimitlessError::Http(e))?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(LimitlessError::Api(format!("get positions failed: {}", text)));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LimitlessError::Api(format!("parse positions failed: {}", e)))?;

        let positions_arr = data
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_else(|| {
                data.as_array().cloned().unwrap_or_default()
            });

        let positions: Vec<LimitlessPosition> = positions_arr
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();

        Ok(positions)
    }

    /// Get balance
    pub async fn get_balance(&self) -> Result<BalanceResponse, LimitlessError> {
        if !self.authenticated {
            return Err(LimitlessError::AuthRequired);
        }

        let url = format!("{}/balance", self.host);
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| LimitlessError::Http(e))?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(LimitlessError::Api(format!("get balance failed: {}", text)));
        }

        response
            .json()
            .await
            .map_err(|e| LimitlessError::Api(format!("parse balance failed: {}", e)))
    }
}
