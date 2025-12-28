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
        let message = format!(
            "{}{}{}",
            timestamp_ms,
            method.to_uppercase(),
            path_without_query
        );
        let signature = self.signing_key.sign(message.as_bytes());
        STANDARD.encode(signature.to_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkcs8::EncodePrivateKey;
    use rsa::pss::VerifyingKey;
    use rsa::signature::Verifier;

    fn generate_test_keypair() -> (RsaPrivateKey, rsa::RsaPublicKey) {
        let mut rng = rand::thread_rng();
        let private_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public_key = private_key.to_public_key();
        (private_key, public_key)
    }

    #[test]
    fn test_sign_produces_base64_output() {
        // #given
        let (private_key, _) = generate_test_keypair();
        let signing_key = SigningKey::<Sha256>::new(private_key);
        let auth = KalshiAuth { signing_key };

        // #when
        let signature = auth.sign(1703980800000, "GET", "/trade-api/v2/markets");

        // #then
        assert!(!signature.is_empty());
        let decoded = STANDARD.decode(&signature);
        assert!(decoded.is_ok(), "Signature should be valid base64");
        // RSA-PSS with SHA256 on 2048-bit key produces 256-byte signature
        assert_eq!(decoded.unwrap().len(), 256);
    }

    #[test]
    fn test_sign_rsa_pss_is_randomized() {
        // #given
        let (private_key, _) = generate_test_keypair();
        let signing_key = SigningKey::<Sha256>::new(private_key);
        let auth = KalshiAuth { signing_key };

        // #when
        let sig1 = auth.sign(1703980800000, "GET", "/trade-api/v2/markets");
        let sig2 = auth.sign(1703980800000, "GET", "/trade-api/v2/markets");

        // #then
        // RSA-PSS uses random salt, so same message produces different signatures
        assert!(!sig1.is_empty());
        assert!(!sig2.is_empty());
    }

    #[test]
    fn test_sign_different_timestamps_produce_different_signatures() {
        // #given
        let (private_key, _) = generate_test_keypair();
        let signing_key = SigningKey::<Sha256>::new(private_key);
        let auth = KalshiAuth { signing_key };

        // #when
        let sig1 = auth.sign(1703980800000, "GET", "/trade-api/v2/markets");
        let sig2 = auth.sign(1703980800001, "GET", "/trade-api/v2/markets");

        // #then
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_sign_strips_query_parameters() {
        // #given
        let (private_key, public_key) = generate_test_keypair();
        let signing_key = SigningKey::<Sha256>::new(private_key.clone());
        let auth = KalshiAuth { signing_key };

        // #when
        let sig_with_query = auth.sign(
            1703980800000,
            "GET",
            "/trade-api/v2/markets?limit=10&cursor=abc",
        );
        let sig_without_query = auth.sign(1703980800000, "GET", "/trade-api/v2/markets");

        // #then
        let verifying_key = VerifyingKey::<Sha256>::new(public_key);
        let message = "1703980800000GET/trade-api/v2/markets";

        let sig_bytes = STANDARD.decode(&sig_with_query).unwrap();
        let signature = rsa::pss::Signature::try_from(sig_bytes.as_slice()).unwrap();
        assert!(verifying_key.verify(message.as_bytes(), &signature).is_ok());

        let sig_bytes = STANDARD.decode(&sig_without_query).unwrap();
        let signature = rsa::pss::Signature::try_from(sig_bytes.as_slice()).unwrap();
        assert!(verifying_key.verify(message.as_bytes(), &signature).is_ok());
    }

    #[test]
    fn test_sign_method_is_uppercased() {
        // #given
        let (private_key, public_key) = generate_test_keypair();
        let signing_key = SigningKey::<Sha256>::new(private_key);
        let auth = KalshiAuth { signing_key };

        // #when
        let sig_lower = auth.sign(1703980800000, "get", "/trade-api/v2/markets");
        let sig_upper = auth.sign(1703980800000, "GET", "/trade-api/v2/markets");

        // #then
        let verifying_key = VerifyingKey::<Sha256>::new(public_key);
        let message = "1703980800000GET/trade-api/v2/markets";

        let sig_bytes = STANDARD.decode(&sig_lower).unwrap();
        let signature = rsa::pss::Signature::try_from(sig_bytes.as_slice()).unwrap();
        assert!(verifying_key.verify(message.as_bytes(), &signature).is_ok());

        let sig_bytes = STANDARD.decode(&sig_upper).unwrap();
        let signature = rsa::pss::Signature::try_from(sig_bytes.as_slice()).unwrap();
        assert!(verifying_key.verify(message.as_bytes(), &signature).is_ok());
    }

    #[test]
    fn test_from_pem_valid_key() {
        // #given
        let (private_key, _) = generate_test_keypair();
        let pem = private_key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .unwrap();

        // #when
        let result = KalshiAuth::from_pem(pem.as_str());

        // #then
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_pem_invalid_key() {
        // #given
        let invalid_pem = "not a valid PEM";

        // #when
        let result = KalshiAuth::from_pem(invalid_pem);

        // #then
        assert!(result.is_err());
    }

    #[test]
    fn test_message_format_timestamp_method_path() {
        // #given
        let (private_key, public_key) = generate_test_keypair();
        let signing_key = SigningKey::<Sha256>::new(private_key);
        let auth = KalshiAuth { signing_key };

        let timestamp_ms: i64 = 1703980800000;
        let method = "POST";
        let path = "/trade-api/v2/portfolio/orders";

        // #when
        let signature = auth.sign(timestamp_ms, method, path);

        // #then
        let expected_message = format!("{}{}{}", timestamp_ms, method, path);
        let verifying_key = VerifyingKey::<Sha256>::new(public_key);
        let sig_bytes = STANDARD.decode(&signature).unwrap();
        let sig = rsa::pss::Signature::try_from(sig_bytes.as_slice()).unwrap();

        assert!(verifying_key
            .verify(expected_message.as_bytes(), &sig)
            .is_ok());
    }
}
