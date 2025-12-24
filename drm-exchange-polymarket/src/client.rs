use reqwest::Client;
use serde::de::DeserializeOwned;

use crate::config::PolymarketConfig;
use crate::error::PolymarketError;

pub struct HttpClient {
    client: Client,
    gamma_url: String,
    clob_url: String,
    verbose: bool,
}

impl HttpClient {
    pub fn new(config: &PolymarketConfig) -> Result<Self, PolymarketError> {
        let client = Client::builder()
            .timeout(config.base.timeout)
            .build()?;

        Ok(Self {
            client,
            gamma_url: config.gamma_url.clone(),
            clob_url: config.clob_url.clone(),
            verbose: config.base.verbose,
        })
    }

    pub async fn get_gamma<T: DeserializeOwned>(
        &self,
        endpoint: &str,
    ) -> Result<T, PolymarketError> {
        let url = format!("{}{}", self.gamma_url, endpoint);
        self.get(&url).await
    }

    pub async fn get_clob<T: DeserializeOwned>(
        &self,
        endpoint: &str,
    ) -> Result<T, PolymarketError> {
        let url = format!("{}{}", self.clob_url, endpoint);
        self.get(&url).await
    }

    async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, PolymarketError> {
        if self.verbose {
            tracing::debug!("GET {}", url);
        }

        let response = self.client.get(url).send().await?;
        let status = response.status();

        if status == 429 {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);
            return Err(PolymarketError::RateLimited { retry_after });
        }

        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(PolymarketError::Api {
                status: status.as_u16(),
                message,
            });
        }

        let body = response.json().await?;
        Ok(body)
    }
}
