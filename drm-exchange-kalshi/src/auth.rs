use crate::error::KalshiError;
use base64::{engine::general_purpose::STANDARD, Engine};
use pkcs8::DecodePrivateKey;
use rsa::pss::SigningKey;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::RsaPrivateKey;
use sha2::Sha256;
use std::fs;
use std::path::Path;

pub struct KalshiAuth {
    signing_key: SigningKey<Sha256>,
}

impl KalshiAuth {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, KalshiError> {
        let pem = fs::read_to_string(path)?;
        Self::from_pem(&pem)
    }

    pub fn from_pem(pem: &str) -> Result<Self, KalshiError> {
        let private_key = RsaPrivateKey::from_pkcs8_pem(pem)
            .map_err(|e| KalshiError::Rsa(format!("failed to parse RSA private key: {e}")))?;

        let signing_key = SigningKey::<Sha256>::new(private_key);

        Ok(Self { signing_key })
    }

    pub fn sign(&self, timestamp_ms: i64, method: &str, path: &str) -> String {
        let path_without_query = path.split('?').next().unwrap_or(path);
        let message = format!("{}{}{}", timestamp_ms, method.to_uppercase(), path_without_query);
        let signature = self.signing_key.sign(message.as_bytes());
        STANDARD.encode(signature.to_bytes())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn test_sign_format() {
        let _expected_format = "base64_encoded_signature";
    }
}
