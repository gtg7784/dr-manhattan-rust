use std::env;

use crate::error::DrmError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExchangeId {
    Polymarket,
    Opinion,
    Limitless,
}

impl ExchangeId {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExchangeId::Polymarket => "polymarket",
            ExchangeId::Opinion => "opinion",
            ExchangeId::Limitless => "limitless",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "polymarket" => Some(ExchangeId::Polymarket),
            "opinion" => Some(ExchangeId::Opinion),
            "limitless" => Some(ExchangeId::Limitless),
            _ => None,
        }
    }

    pub fn env_prefix(&self) -> &'static str {
        match self {
            ExchangeId::Polymarket => "POLYMARKET",
            ExchangeId::Opinion => "OPINION",
            ExchangeId::Limitless => "LIMITLESS",
        }
    }

    pub fn required_env_vars(&self) -> Vec<&'static str> {
        match self {
            ExchangeId::Polymarket => vec!["POLYMARKET_PRIVATE_KEY", "POLYMARKET_FUNDER"],
            ExchangeId::Opinion => vec![
                "OPINION_API_KEY",
                "OPINION_PRIVATE_KEY",
                "OPINION_MULTI_SIG_ADDR",
            ],
            ExchangeId::Limitless => vec!["LIMITLESS_PRIVATE_KEY"],
        }
    }
}

pub fn list_exchanges() -> Vec<ExchangeId> {
    vec![
        ExchangeId::Polymarket,
        ExchangeId::Opinion,
        ExchangeId::Limitless,
    ]
}

pub fn list_exchange_names() -> Vec<&'static str> {
    vec!["polymarket", "opinion", "limitless"]
}

pub fn validate_env_config(exchange: ExchangeId) -> Result<(), DrmError> {
    let required = exchange.required_env_vars();
    let missing: Vec<_> = required
        .iter()
        .filter(|var| env::var(var).is_err())
        .collect();

    if !missing.is_empty() {
        return Err(DrmError::InvalidInput(format!(
            "Missing required environment variables for {}: {:?}",
            exchange.as_str(),
            missing
        )));
    }

    Ok(())
}

pub fn validate_private_key(key: &str) -> Result<(), DrmError> {
    if key.is_empty() {
        return Err(DrmError::InvalidInput(
            "Private key cannot be empty".to_string(),
        ));
    }

    let clean_key = key.strip_prefix("0x").unwrap_or(key);

    if clean_key.len() != 64 {
        return Err(DrmError::InvalidInput(format!(
            "Invalid private key length. Expected 64 hex characters, got {}",
            clean_key.len()
        )));
    }

    if !clean_key.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(DrmError::InvalidInput(
            "Invalid private key format. Must be valid hexadecimal".to_string(),
        ));
    }

    Ok(())
}

pub fn get_env_var(name: &str) -> Option<String> {
    env::var(name).ok()
}

pub fn get_env_var_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exchange_id_from_str() {
        assert_eq!(
            ExchangeId::from_str("polymarket"),
            Some(ExchangeId::Polymarket)
        );
        assert_eq!(
            ExchangeId::from_str("POLYMARKET"),
            Some(ExchangeId::Polymarket)
        );
        assert_eq!(ExchangeId::from_str("opinion"), Some(ExchangeId::Opinion));
        assert_eq!(
            ExchangeId::from_str("limitless"),
            Some(ExchangeId::Limitless)
        );
        assert_eq!(ExchangeId::from_str("unknown"), None);
    }

    #[test]
    fn test_list_exchanges() {
        let exchanges = list_exchanges();
        assert_eq!(exchanges.len(), 3);
    }

    #[test]
    fn test_validate_private_key_valid() {
        let valid_key = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        assert!(validate_private_key(valid_key).is_ok());

        let valid_key_no_prefix = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        assert!(validate_private_key(valid_key_no_prefix).is_ok());
    }

    #[test]
    fn test_validate_private_key_invalid() {
        assert!(validate_private_key("").is_err());
        assert!(validate_private_key("0x1234").is_err());
        assert!(validate_private_key("not_hex_at_all_gg").is_err());
    }
}
