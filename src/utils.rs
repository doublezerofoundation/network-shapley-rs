use rust_decimal::{
    Decimal,
    prelude::{FromPrimitive, ToPrimitive},
};

/// Utility functions for Decimal/f64 conversion
#[inline]
pub fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

#[inline]
pub fn f64_to_decimal(f: f64) -> Decimal {
    Decimal::from_f64(f).unwrap_or(Decimal::ZERO)
}

/// Round a Decimal to 4 decimal places (matching Python behavior)
#[inline]
pub fn round_decimal(d: Decimal) -> Decimal {
    d.round_dp(4)
}
