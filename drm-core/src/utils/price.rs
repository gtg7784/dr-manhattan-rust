//! Price utility functions for tick size rounding and validation.

use crate::error::DrmError;

/// Round a price to the nearest valid tick increment.
///
/// # Arguments
/// * `price` - The price to round
/// * `tick_size` - The minimum tick size
///
/// # Returns
/// Price rounded to nearest tick
///
/// # Example
/// ```
/// use drm_core::utils::round_to_tick_size;
///
/// let rounded = round_to_tick_size(0.1234, 0.01).unwrap();
/// assert!((rounded - 0.12).abs() < 1e-10);
/// ```
pub fn round_to_tick_size(price: f64, tick_size: f64) -> Result<f64, DrmError> {
    if tick_size <= 0.0 {
        return Err(DrmError::InvalidInput(
            "tick_size must be positive".to_string(),
        ));
    }

    Ok((price / tick_size).round() * tick_size)
}

/// Check if a price is valid for the given tick size.
///
/// # Arguments
/// * `price` - Price to check
/// * `tick_size` - Minimum tick size
///
/// # Returns
/// True if price is valid (aligned to tick size)
///
/// # Example
/// ```
/// use drm_core::utils::is_valid_price;
///
/// assert!(is_valid_price(0.12, 0.01).unwrap());
/// assert!(!is_valid_price(0.123, 0.01).unwrap());
/// ```
pub fn is_valid_price(price: f64, tick_size: f64) -> Result<bool, DrmError> {
    if tick_size <= 0.0 {
        return Err(DrmError::InvalidInput(
            "tick_size must be positive".to_string(),
        ));
    }

    let rounded = round_to_tick_size(price, tick_size)?;
    Ok((price - rounded).abs() < (tick_size / 10.0))
}

/// Clamp a price to be within valid bounds.
///
/// # Arguments
/// * `price` - Price to clamp
/// * `min_price` - Minimum allowed price
/// * `max_price` - Maximum allowed price
/// * `tick_size` - Tick size to round to
///
/// # Returns
/// Price clamped to bounds and rounded to tick size
pub fn clamp_price(
    price: f64,
    min_price: f64,
    max_price: f64,
    tick_size: f64,
) -> Result<f64, DrmError> {
    let clamped = price.clamp(min_price, max_price);
    round_to_tick_size(clamped, tick_size)
}

/// Calculate mid price from best bid and ask.
///
/// # Arguments
/// * `best_bid` - Best bid price
/// * `best_ask` - Best ask price
///
/// # Returns
/// Mid price, or None if either price is missing
pub fn mid_price(best_bid: Option<f64>, best_ask: Option<f64>) -> Option<f64> {
    match (best_bid, best_ask) {
        (Some(bid), Some(ask)) => Some((bid + ask) / 2.0),
        _ => None,
    }
}

/// Calculate spread in basis points.
///
/// # Arguments
/// * `best_bid` - Best bid price
/// * `best_ask` - Best ask price
///
/// # Returns
/// Spread in basis points, or None if either price is missing
pub fn spread_bps(best_bid: Option<f64>, best_ask: Option<f64>) -> Option<f64> {
    match (best_bid, best_ask) {
        (Some(bid), Some(ask)) if bid > 0.0 => {
            let mid = (bid + ask) / 2.0;
            Some((ask - bid) / mid * 10000.0)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_to_tick_size() {
        assert!((round_to_tick_size(0.1234, 0.01).unwrap() - 0.12).abs() < 1e-10);
        assert!((round_to_tick_size(0.1256, 0.01).unwrap() - 0.13).abs() < 1e-10);
        assert!((round_to_tick_size(0.5, 0.1).unwrap() - 0.5).abs() < 1e-10);
        assert!((round_to_tick_size(0.55, 0.1).unwrap() - 0.6).abs() < 1e-10);
    }

    #[test]
    fn test_round_to_tick_size_invalid() {
        assert!(round_to_tick_size(0.5, 0.0).is_err());
        assert!(round_to_tick_size(0.5, -0.01).is_err());
    }

    #[test]
    fn test_is_valid_price() {
        assert!(is_valid_price(0.12, 0.01).unwrap());
        assert!(is_valid_price(0.50, 0.01).unwrap());
        assert!(!is_valid_price(0.123, 0.01).unwrap());
        assert!(!is_valid_price(0.1234, 0.01).unwrap());
    }

    #[test]
    fn test_clamp_price() {
        assert!((clamp_price(0.15, 0.10, 0.90, 0.01).unwrap() - 0.15).abs() < 1e-10);
        assert!((clamp_price(0.05, 0.10, 0.90, 0.01).unwrap() - 0.10).abs() < 1e-10);
        assert!((clamp_price(0.95, 0.10, 0.90, 0.01).unwrap() - 0.90).abs() < 1e-10);
    }

    #[test]
    fn test_mid_price() {
        assert!((mid_price(Some(0.40), Some(0.60)).unwrap() - 0.50).abs() < 1e-10);
        assert!(mid_price(None, Some(0.60)).is_none());
        assert!(mid_price(Some(0.40), None).is_none());
    }

    #[test]
    fn test_spread_bps() {
        // Spread = 0.60 - 0.40 = 0.20, Mid = 0.50
        // BPS = 0.20 / 0.50 * 10000 = 4000
        let spread = spread_bps(Some(0.40), Some(0.60)).unwrap();
        assert!((spread - 4000.0).abs() < 1e-10);
    }
}
