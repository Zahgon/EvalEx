//! Conversions to integers and floating point.

use super::{BigDecimal, RoundingMode, ARITHMETIC};
use crate::java::biginteger::BigInteger;
use crate::java::{JResult, Thrown};

/// Digits in the integer part beyond which a value cannot fit a `long`.
const MAX_LONG_DIGITS: i64 = 19;
/// A scale at or below this makes the low 64 bits of the value zero.
const SCALE_WITH_ZERO_LOW_BITS: i32 = -64;

fn numeric_overflow() -> Thrown {
    Thrown::new(ARITHMETIC, "Overflow")
}

impl BigDecimal {
    /// Number of digits left of the decimal point, not positive for pure fractions.
    fn integer_digits(&self) -> i64 {
        i64::from(self.precision()) - i64::from(self.scale)
    }

    /// `toBigInteger()`: discards any fraction.
    pub fn to_big_integer(&self) -> JResult<BigInteger> {
        Ok(self.set_scale(0, RoundingMode::Down)?.unscaled)
    }

    /// `longValue()`: the low-order 64 bits of the integer part.
    pub fn long_value(&self) -> i64 {
        if self.is_zero() || self.integer_digits() <= 0 || self.scale <= SCALE_WITH_ZERO_LOW_BITS {
            return 0;
        }
        // The conversion only fails for an integer part too large for a BigInteger, whose low 64 bits are all zero.
        self.to_big_integer().map_or(0, |integer| integer.long_value())
    }

    /// `intValue()`: the low-order 32 bits of the integer part.
    pub fn int_value(&self) -> i32 {
        self.long_value() as i32
    }

    /// `longValueExact()`.
    fn long_value_exact(&self) -> JResult<i64> {
        if self.is_zero() {
            return Ok(0);
        }
        if self.integer_digits() <= 0 {
            return Err(Thrown::new(ARITHMETIC, "Rounding necessary"));
        }
        if self.integer_digits() > MAX_LONG_DIGITS {
            return Err(numeric_overflow());
        }
        let integer = self.set_scale(0, RoundingMode::Unnecessary)?.unscaled;
        let in_range = integer.bit_length() < i64::BITS as i32;
        if in_range {
            Ok(integer.long_value())
        } else {
            Err(numeric_overflow())
        }
    }

    /// `intValueExact()`.
    pub fn int_value_exact(&self) -> JResult<i32> {
        i32::try_from(self.long_value_exact()?).map_err(|_| numeric_overflow())
    }

    /// `doubleValue()`, correctly rounded; `toString` output always parses.
    pub fn double_value(&self) -> f64 {
        self.to_string().parse().unwrap_or(f64::NAN)
    }

    /// `floatValue()`, correctly rounded; `toString` output always parses.
    pub fn float_value(&self) -> f32 {
        self.to_string().parse().unwrap_or(f32::NAN)
    }
}
