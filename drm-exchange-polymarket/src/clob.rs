use ethers::prelude::*;
use ethers::utils::keccak256;
use serde::{Deserialize, Serialize};

use crate::error::PolymarketError;

pub const CLOB_URL: &str = "https://clob.polymarket.com";
const CHAIN_ID: u64 = 137;
const CTF_EXCHANGE: &str = "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ClobOrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ClobOrderType {
    Gtc,
    Fok,
    Ioc,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderArgs {
    pub token_id: String,
    pub price: f64,
    pub size: f64,
    pub side: ClobOrderSide,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedOrder {
    pub salt: String,
    pub maker: String,
    pub signer: String,
    pub taker: String,
    pub token_id: String,
    pub maker_amount: String,
    pub taker_amount: String,
    pub expiration: String,
    pub nonce: String,
    pub fee_rate_bps: String,
    pub side: u8,
    pub signature_type: u8,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PostOrderRequest {
    order: SignedOrder,
    owner: String,
    order_type: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderResponse {
    #[serde(rename = "orderID")]
    pub order_id: Option<String>,
    pub status: Option<String>,
    pub error_msg: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiCredentials {
    #[serde(rename = "apiKey")]
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClobOrderData {
    #[serde(rename = "id", alias = "orderID")]
    pub id: Option<String>,
    pub market: Option<String>,
    pub asset_id: Option<String>,
    pub side: Option<String>,
    pub original_size: Option<String>,
    pub size_matched: Option<String>,
    pub price: Option<String>,
    pub status: Option<String>,
    pub outcome: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BalanceAllowance {
    pub balance: Option<String>,
    pub allowance: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenBalance {
    pub balance: Option<String>,
}

pub struct ClobClient {
    http: reqwest::Client,
    wallet: LocalWallet,
    address: Address,
    funder: Option<Address>,
    api_creds: Option<ApiCredentials>,
}

impl ClobClient {
    pub fn new(private_key: &str, funder: Option<&str>) -> Result<Self, PolymarketError> {
        let wallet: LocalWallet = private_key
            .parse()
            .map_err(|e| PolymarketError::Config(format!("invalid private key: {}", e)))?;

        let wallet = wallet.with_chain_id(CHAIN_ID);
        let address = wallet.address();

        let funder = funder
            .map(|f| f.parse::<Address>())
            .transpose()
            .map_err(|e| PolymarketError::Config(format!("invalid funder address: {}", e)))?;

        Ok(Self {
            http: reqwest::Client::new(),
            wallet,
            address,
            funder,
            api_creds: None,
        })
    }

    pub fn address(&self) -> Address {
        self.address
    }

    pub async fn derive_api_credentials(&mut self) -> Result<ApiCredentials, PolymarketError> {
        let nonce = chrono::Utc::now().timestamp_millis();
        let message = format!("I am signing this nonce: {}", nonce);
        let signature = self
            .wallet
            .sign_message(&message)
            .await
            .map_err(|e| PolymarketError::Signing(format!("signing failed: {}", e)))?;

        let url = format!("{}/auth/derive-api-key", CLOB_URL);
        let response = self
            .http
            .get(&url)
            .header("POLY_ADDRESS", format!("{:?}", self.address))
            .header("POLY_SIGNATURE", format!("0x{}", hex::encode(signature.to_vec())))
            .header("POLY_TIMESTAMP", nonce.to_string())
            .header("POLY_NONCE", nonce.to_string())
            .send()
            .await
            .map_err(|e| PolymarketError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(PolymarketError::Api(format!(
                "derive credentials failed: {} - {}",
                status, text
            )));
        }

        let creds: ApiCredentials = response
            .json()
            .await
            .map_err(|e| PolymarketError::Api(format!("parse credentials failed: {}", e)))?;

        self.api_creds = Some(creds.clone());
        Ok(creds)
    }

    pub fn set_api_credentials(&mut self, creds: ApiCredentials) {
        self.api_creds = Some(creds);
    }

    pub async fn create_order(&self, args: OrderArgs) -> Result<SignedOrder, PolymarketError> {
        let salt = chrono::Utc::now().timestamp_millis() as u128 * 1000;
        let maker = format!("{:?}", self.funder.unwrap_or(self.address));
        let signer = format!("{:?}", self.address);

        let side_int: u8 = match args.side {
            ClobOrderSide::Buy => 0,
            ClobOrderSide::Sell => 1,
        };

        let decimals = 6u32;
        let scale = 10u64.pow(decimals);

        let (maker_amount, taker_amount) = match args.side {
            ClobOrderSide::Buy => {
                let usdc_amount = (args.size * args.price * scale as f64) as u64;
                let shares = (args.size * scale as f64) as u64;
                (usdc_amount, shares)
            }
            ClobOrderSide::Sell => {
                let shares = (args.size * scale as f64) as u64;
                let usdc_amount = (args.size * args.price * scale as f64) as u64;
                (shares, usdc_amount)
            }
        };

        let token_id = U256::from_dec_str(&args.token_id)
            .map_err(|e| PolymarketError::Config(format!("invalid token_id: {}", e)))?;

        let order_hash = self.compute_order_hash(
            U256::from(salt),
            maker.parse().unwrap(),
            signer.parse().unwrap(),
            Address::zero(),
            token_id,
            U256::from(maker_amount),
            U256::from(taker_amount),
            U256::zero(),
            U256::zero(),
            U256::zero(),
            side_int,
            2u8,
        );

        let signature = self
            .wallet
            .sign_hash(order_hash.into())
            .map_err(|e| PolymarketError::Signing(format!("signing failed: {}", e)))?;

        Ok(SignedOrder {
            salt: salt.to_string(),
            maker,
            signer,
            taker: format!("{:?}", Address::zero()),
            token_id: args.token_id,
            maker_amount: maker_amount.to_string(),
            taker_amount: taker_amount.to_string(),
            expiration: "0".into(),
            nonce: "0".into(),
            fee_rate_bps: "0".into(),
            side: side_int,
            signature_type: 2,
            signature: format!("0x{}", hex::encode(signature.to_vec())),
        })
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
    ) -> [u8; 32] {
        let order_type_hash = keccak256(
            b"Order(uint256 salt,address maker,address signer,address taker,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType)"
        );

        let domain_separator = self.compute_domain_separator();

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

    fn compute_domain_separator(&self) -> [u8; 32] {
        let domain_type_hash = keccak256(
            b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
        );

        let name_hash = keccak256(b"Polymarket CTF Exchange");
        let version_hash = keccak256(b"1");
        let contract: Address = CTF_EXCHANGE.parse().unwrap();

        keccak256(ethers::abi::encode(&[
            ethers::abi::Token::FixedBytes(domain_type_hash.to_vec()),
            ethers::abi::Token::FixedBytes(name_hash.to_vec()),
            ethers::abi::Token::FixedBytes(version_hash.to_vec()),
            ethers::abi::Token::Uint(U256::from(CHAIN_ID)),
            ethers::abi::Token::Address(contract),
        ]))
    }

    pub async fn post_order(
        &self,
        order: SignedOrder,
        order_type: ClobOrderType,
    ) -> Result<OrderResponse, PolymarketError> {
        let creds = self
            .api_creds
            .as_ref()
            .ok_or_else(|| PolymarketError::Auth("API credentials not set".into()))?;

        let owner = format!("{:?}", self.funder.unwrap_or(self.address));
        let order_type_str = match order_type {
            ClobOrderType::Gtc => "GTC",
            ClobOrderType::Fok => "FOK",
            ClobOrderType::Ioc => "IOC",
        };

        let request = PostOrderRequest {
            order,
            owner,
            order_type: order_type_str.into(),
        };

        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        let body = serde_json::to_string(&request)
            .map_err(|e| PolymarketError::Api(format!("serialize failed: {}", e)))?;

        let sig_payload = format!("POST\n/order\n{}\n{}", timestamp, body);
        let hmac_sig = self.sign_hmac(&sig_payload, &creds.secret)?;

        let url = format!("{}/order", CLOB_URL);
        let response = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .header("POLY_ADDRESS", format!("{:?}", self.address))
            .header("POLY_SIGNATURE", &hmac_sig)
            .header("POLY_TIMESTAMP", &timestamp)
            .header("POLY_API_KEY", &creds.api_key)
            .header("POLY_PASSPHRASE", &creds.passphrase)
            .body(body)
            .send()
            .await
            .map_err(|e| PolymarketError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(PolymarketError::Api(format!(
                "post order failed: {} - {}",
                status, text
            )));
        }

        response
            .json()
            .await
            .map_err(|e| PolymarketError::Api(format!("parse response failed: {}", e)))
    }

    pub async fn cancel_order(&self, order_id: &str) -> Result<(), PolymarketError> {
        let creds = self
            .api_creds
            .as_ref()
            .ok_or_else(|| PolymarketError::Auth("API credentials not set".into()))?;

        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        let sig_payload = format!("DELETE\n/order/{}\n{}\n", order_id, timestamp);
        let hmac_sig = self.sign_hmac(&sig_payload, &creds.secret)?;

        let url = format!("{}/order/{}", CLOB_URL, order_id);
        let response = self
            .http
            .delete(&url)
            .header("POLY_ADDRESS", format!("{:?}", self.address))
            .header("POLY_SIGNATURE", &hmac_sig)
            .header("POLY_TIMESTAMP", &timestamp)
            .header("POLY_API_KEY", &creds.api_key)
            .header("POLY_PASSPHRASE", &creds.passphrase)
            .send()
            .await
            .map_err(|e| PolymarketError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(PolymarketError::Api(format!(
                "cancel order failed: {} - {}",
                status, text
            )));
        }

        Ok(())
    }

    pub async fn get_order(&self, order_id: &str) -> Result<ClobOrderData, PolymarketError> {
        let creds = self
            .api_creds
            .as_ref()
            .ok_or_else(|| PolymarketError::Auth("API credentials not set".into()))?;

        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        let sig_payload = format!("GET\n/order/{}\n{}\n", order_id, timestamp);
        let hmac_sig = self.sign_hmac(&sig_payload, &creds.secret)?;

        let url = format!("{}/order/{}", CLOB_URL, order_id);
        let response = self
            .http
            .get(&url)
            .header("POLY_ADDRESS", format!("{:?}", self.address))
            .header("POLY_SIGNATURE", &hmac_sig)
            .header("POLY_TIMESTAMP", &timestamp)
            .header("POLY_API_KEY", &creds.api_key)
            .header("POLY_PASSPHRASE", &creds.passphrase)
            .send()
            .await
            .map_err(|e| PolymarketError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(PolymarketError::Api(format!(
                "get order failed: {} - {}",
                status, text
            )));
        }

        response
            .json()
            .await
            .map_err(|e| PolymarketError::Api(format!("parse order failed: {}", e)))
    }

    pub async fn get_open_orders(&self) -> Result<Vec<ClobOrderData>, PolymarketError> {
        let creds = self
            .api_creds
            .as_ref()
            .ok_or_else(|| PolymarketError::Auth("API credentials not set".into()))?;

        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        let sig_payload = format!("GET\n/orders\n{}\n", timestamp);
        let hmac_sig = self.sign_hmac(&sig_payload, &creds.secret)?;

        let url = format!("{}/orders", CLOB_URL);
        let response = self
            .http
            .get(&url)
            .header("POLY_ADDRESS", format!("{:?}", self.address))
            .header("POLY_SIGNATURE", &hmac_sig)
            .header("POLY_TIMESTAMP", &timestamp)
            .header("POLY_API_KEY", &creds.api_key)
            .header("POLY_PASSPHRASE", &creds.passphrase)
            .send()
            .await
            .map_err(|e| PolymarketError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(PolymarketError::Api(format!(
                "get open orders failed: {} - {}",
                status, text
            )));
        }

        response
            .json()
            .await
            .map_err(|e| PolymarketError::Api(format!("parse orders failed: {}", e)))
    }

    pub async fn get_balance_allowance(&self) -> Result<BalanceAllowance, PolymarketError> {
        let creds = self
            .api_creds
            .as_ref()
            .ok_or_else(|| PolymarketError::Auth("API credentials not set".into()))?;

        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        let sig_payload = format!("GET\n/balance-allowance?asset_type=COLLATERAL\n{}\n", timestamp);
        let hmac_sig = self.sign_hmac(&sig_payload, &creds.secret)?;

        let url = format!("{}/balance-allowance?asset_type=COLLATERAL", CLOB_URL);
        let response = self
            .http
            .get(&url)
            .header("POLY_ADDRESS", format!("{:?}", self.address))
            .header("POLY_SIGNATURE", &hmac_sig)
            .header("POLY_TIMESTAMP", &timestamp)
            .header("POLY_API_KEY", &creds.api_key)
            .header("POLY_PASSPHRASE", &creds.passphrase)
            .send()
            .await
            .map_err(|e| PolymarketError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(PolymarketError::Api(format!(
                "get balance failed: {} - {}",
                status, text
            )));
        }

        response
            .json()
            .await
            .map_err(|e| PolymarketError::Api(format!("parse balance failed: {}", e)))
    }

    pub async fn get_token_balance(&self, token_id: &str) -> Result<f64, PolymarketError> {
        let creds = self
            .api_creds
            .as_ref()
            .ok_or_else(|| PolymarketError::Auth("API credentials not set".into()))?;

        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        let query = format!("asset_type=CONDITIONAL&token_id={}", token_id);
        let sig_payload = format!("GET\n/balance-allowance?{}\n{}\n", query, timestamp);
        let hmac_sig = self.sign_hmac(&sig_payload, &creds.secret)?;

        let url = format!("{}/balance-allowance?{}", CLOB_URL, query);
        let response = self
            .http
            .get(&url)
            .header("POLY_ADDRESS", format!("{:?}", self.address))
            .header("POLY_SIGNATURE", &hmac_sig)
            .header("POLY_TIMESTAMP", &timestamp)
            .header("POLY_API_KEY", &creds.api_key)
            .header("POLY_PASSPHRASE", &creds.passphrase)
            .send()
            .await
            .map_err(|e| PolymarketError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Ok(0.0);
        }

        let data: BalanceAllowance = response
            .json()
            .await
            .map_err(|e| PolymarketError::Api(format!("parse token balance failed: {}", e)))?;

        let balance = data
            .balance
            .and_then(|b| b.parse::<f64>().ok())
            .map(|b| b / 1_000_000.0)
            .unwrap_or(0.0);

        Ok(balance)
    }

    fn sign_hmac(&self, payload: &str, secret: &str) -> Result<String, PolymarketError> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let secret_bytes = STANDARD
            .decode(secret)
            .map_err(|e| PolymarketError::Signing(format!("decode secret failed: {}", e)))?;

        let mut mac = Hmac::<Sha256>::new_from_slice(&secret_bytes)
            .map_err(|e| PolymarketError::Signing(format!("hmac init failed: {}", e)))?;

        mac.update(payload.as_bytes());
        let result = mac.finalize();
        Ok(STANDARD.encode(result.into_bytes()))
    }
}
