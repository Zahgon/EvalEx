//! `java.math.BigDecimal`: an arbitrary precision integer with a 32-bit decimal scale.
//!
//! The value is `unscaled * 10^-scale`. Operations reproduce the JDK's results, preferred
//! scales and exceptions, including the order in which its scale checks fire.

mod arith;
mod context;
mod convert;
mod divide;
mod format;
mod parse;
mod rounding;

#[cfg(test)]
mod tests;

pub use context::{MathContext, RoundingMode};

use super::biginteger::BigInteger;
use super::{JResult, Thrown};
use std::cmp::Ordering;

const ARITHMETIC: &str = "java.lang.ArithmeticException";

/// An arbitrary precision decimal with the behaviour of `java.math.BigDecimal`.
#[derive(Clone, Debug)]
pub struct BigDecimal {
    unscaled: BigInteger,
    scale: i32,
    /// True when the JDK would keep this value in its `BigInteger` field.
    ///
    /// Values beyond `long` always are. Smaller ones can be too, for example a difference
    /// of two huge numbers, and division by zero then reports a different message.
    inflated: bool,
}

/// `checkScale`: validates a scale computed for a value that is zero or not.
///
/// Out of range scales of zero saturate. Others throw, naming the direction of the
/// exponent: a too large scale is an underflow.
fn check_scale(is_zero: bool, scale: i64) -> JResult<i32> {
    match i32::try_from(scale) {
        Ok(scale) => Ok(scale),
        Err(_) if is_zero => Ok(saturate(scale)),
        Err(_) if scale > 0 => Err(Thrown::new(ARITHMETIC, "Underflow")),
        Err(_) => Err(Thrown::new(ARITHMETIC, "Overflow")),
    }
}

/// [`check_scale`] for a non-zero value.
fn check_scale_saturating(scale: i64) -> JResult<i32> {
    check_scale(false, scale)
}

/// `checkScaleNonZero`: as [`check_scale_saturating`], but the direction is read from the
/// value truncated to 32 bits.
fn check_scale_truncating(scale: i64) -> JResult<i32> {
    let truncated = scale as i32;
    if i64::from(truncated) == scale {
        Ok(truncated)
    } else if truncated > 0 {
        Err(Thrown::new(ARITHMETIC, "Underflow"))
    } else {
        Err(Thrown::new(ARITHMETIC, "Overflow"))
    }
}

/// Clamps a scale to the `int` range.
fn saturate(scale: i64) -> i32 {
    scale.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

impl BigDecimal {
    /// A value in the compact form whenever it fits.
    fn compact(unscaled: BigInteger, scale: i32) -> BigDecimal {
        BigDecimal::stored(unscaled, scale, false)
    }

    /// A value that stays in `BigInteger` form if `inflated`, whatever its size.
    fn stored(unscaled: BigInteger, scale: i32, inflated: bool) -> BigDecimal {
        let inflated = inflated || !unscaled.fits_compact();
        BigDecimal { unscaled, scale, inflated }
    }

    /// Zero with the given scale.
    fn zero_with_scale(scale: i32) -> BigDecimal {
        BigDecimal::compact(BigInteger::zero(), scale)
    }

    /// `BigDecimal.ZERO`.
    pub fn zero() -> BigDecimal {
        BigDecimal::zero_with_scale(0)
    }

    /// `BigDecimal.ONE`.
    pub fn one() -> BigDecimal {
        BigDecimal::from_i64(1)
    }

    /// `new BigDecimal(long)`.
    pub fn from_i64(value: i64) -> BigDecimal {
        BigDecimal::compact(BigInteger::from_i64(value), 0)
    }

    /// `new BigDecimal(int, MathContext)`.
    pub fn from_i32_mc(value: i32, mc: &MathContext) -> JResult<BigDecimal> {
        BigDecimal::from_i64(i64::from(value)).round(mc)
    }

    /// `new BigDecimal(BigInteger, int)`.
    pub fn from_big_integer(unscaled: &BigInteger, scale: i32) -> BigDecimal {
        BigDecimal::compact(unscaled.clone(), scale)
    }

    /// `new BigDecimal(BigInteger, MathContext)`.
    pub fn from_big_integer_mc(value: &BigInteger, mc: &MathContext) -> JResult<BigDecimal> {
        BigDecimal::from_big_integer(value, 0).round(mc)
    }

    /// `BigDecimal.valueOf(double)`: the value `Double.toString` shows.
    pub fn value_of_double(value: f64) -> JResult<BigDecimal> {
        let text = super::double::to_string(value);
        BigDecimal::parse(&super::js(&text))
    }

    /// `signum()`.
    pub fn signum(&self) -> i32 {
        self.unscaled.signum()
    }

    /// `scale()`.
    pub fn scale(&self) -> i32 {
        self.scale
    }

    /// `precision()`: the number of digits of the unscaled value; 1 for zero.
    pub fn precision(&self) -> i32 {
        self.unscaled.decimal_digits()
    }

    /// `unscaledValue()`.
    pub fn unscaled_value(&self) -> BigInteger {
        self.unscaled.clone()
    }

    /// True for any zero.
    fn is_zero(&self) -> bool {
        self.unscaled.is_zero()
    }

    /// `negate()`.
    fn negate(&self) -> BigDecimal {
        BigDecimal::stored(self.unscaled.negate(), self.scale, self.inflated)
    }

    /// `movePointRight(n)`.
    pub fn move_point_right(&self, n: i32) -> JResult<BigDecimal> {
        let scale = check_scale(self.is_zero(), i64::from(self.scale) - i64::from(n))?;
        let moved = BigDecimal::stored(self.unscaled.clone(), scale, self.inflated);
        if scale < 0 {
            moved.set_scale(0, RoundingMode::Unnecessary)
        } else {
            Ok(moved)
        }
    }

    /// `stripTrailingZeros()`.
    pub fn strip_trailing_zeros(&self) -> JResult<BigDecimal> {
        if self.is_zero() {
            return Ok(BigDecimal::zero());
        }
        // A target below every valid scale: all zeros go, or the scale check throws.
        rounding::strip_zeros(self.unscaled.clone(), self.scale, i64::from(i32::MIN) - 1)
    }

    /// `compareTo`: numeric order, ignoring scale.
    pub fn compare_to(&self, other: &BigDecimal) -> Ordering {
        match self.signum().cmp(&other.signum()) {
            Ordering::Equal if self.signum() < 0 => other.compare_magnitude(self),
            Ordering::Equal => self.compare_magnitude(other),
            unequal => unequal,
        }
    }

    /// Compares absolute values without materializing large powers of ten.
    fn compare_magnitude(&self, other: &BigDecimal) -> Ordering {
        match (self.is_zero(), other.is_zero()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => {}
        }
        let exponent = i64::from(self.precision()) - i64::from(self.scale);
        let other_exponent = i64::from(other.precision()) - i64::from(other.scale);
        if exponent != other_exponent {
            return exponent.cmp(&other_exponent);
        }
        // With equal exponents the scales differ by no more than the digit counts do.
        let gap = i64::from(self.scale) - i64::from(other.scale);
        let raise = gap.unsigned_abs() as u32;
        if gap < 0 {
            self.unscaled.magnitude_times_pow10(raise).compare_magnitude(&other.unscaled)
        } else {
            self.unscaled.compare_magnitude(&other.unscaled.magnitude_times_pow10(raise))
        }
    }

    /// `equals`: same value and same scale.
    pub fn java_equals(&self, other: &BigDecimal) -> bool {
        self.scale == other.scale && self.unscaled == other.unscaled
    }

    /// `hashCode()`.
    pub fn hash_code(&self) -> i32 {
        self.unscaled.hash_code().wrapping_mul(31).wrapping_add(self.scale)
    }
}
