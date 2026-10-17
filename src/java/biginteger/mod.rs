//! `java.math.BigInteger`: arbitrary precision integers in sign-magnitude form.

mod limbs;
mod radix;

use crate::java::{JResult, Thrown};
use std::cmp::Ordering;
use std::fmt;

const ARITHMETIC: &str = "java.lang.ArithmeticException";
/// Magnitudes must stay below 2^(2^31 - 1), the JDK's supported range.
const MAX_BIT_LENGTH: u64 = i32::MAX as u64;
/// `pow` of a base that is not a power of two gives up when this bound is reached by
/// the bit length of the base times the exponent.
const POW_BIT_ESTIMATE_LIMIT: u64 = (1 << 31) + 32;

fn overflow() -> Thrown {
    Thrown::new(ARITHMETIC, "BigInteger would overflow supported range")
}

/// An arbitrary precision integer with the behaviour of `java.math.BigInteger`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigInteger {
    negative: bool,
    magnitude: Vec<u64>,
}

impl BigInteger {
    /// Builds a value from a normalized magnitude; zero is never negative.
    fn from_parts(negative: bool, magnitude: Vec<u64>) -> BigInteger {
        BigInteger { negative: negative && !magnitude.is_empty(), magnitude }
    }

    /// Like [`Self::from_parts`], rejecting results outside the supported range.
    fn checked(negative: bool, magnitude: Vec<u64>) -> JResult<BigInteger> {
        if limbs::bit_length(&magnitude) > MAX_BIT_LENGTH {
            return Err(overflow());
        }
        Ok(BigInteger::from_parts(negative, magnitude))
    }

    /// `BigInteger.ZERO`.
    pub fn zero() -> BigInteger {
        BigInteger::from_parts(false, Vec::new())
    }

    /// `BigInteger.valueOf(value)`.
    pub fn from_i64(value: i64) -> BigInteger {
        BigInteger::from_u64(value < 0, value.unsigned_abs())
    }

    /// A value from its sign and absolute value.
    pub(crate) fn from_u64(negative: bool, value: u64) -> BigInteger {
        let magnitude = if value == 0 { Vec::new() } else { vec![value] };
        BigInteger::from_parts(negative, magnitude)
    }

    /// A value from decimal digit values, most significant first.
    pub(crate) fn from_decimal_digits(negative: bool, digits: &[u8]) -> JResult<BigInteger> {
        BigInteger::checked(negative, radix::magnitude_of_digits(digits, 10))
    }

    /// `new BigInteger(text, radix)`.
    pub fn parse(text: &[u16], radix: i32) -> JResult<BigInteger> {
        let parsed = radix::parse(text, radix)?;
        BigInteger::checked(parsed.negative, parsed.magnitude)
    }

    /// `this + other`.
    pub fn add(&self, other: &BigInteger) -> JResult<BigInteger> {
        if self.negative == other.negative {
            let sum = limbs::add(&self.magnitude, &other.magnitude);
            return BigInteger::checked(self.negative, sum);
        }
        Ok(match limbs::compare(&self.magnitude, &other.magnitude) {
            Ordering::Less => BigInteger::from_parts(other.negative, limbs::sub(&other.magnitude, &self.magnitude)),
            _ => BigInteger::from_parts(self.negative, limbs::sub(&self.magnitude, &other.magnitude)),
        })
    }

    /// `this - other`.
    pub fn subtract(&self, other: &BigInteger) -> JResult<BigInteger> {
        self.add(&other.negate())
    }

    /// `this * other`.
    pub fn multiply(&self, other: &BigInteger) -> JResult<BigInteger> {
        let bits = limbs::bit_length(&self.magnitude) + limbs::bit_length(&other.magnitude);
        if bits > MAX_BIT_LENGTH + 1 {
            return Err(overflow());
        }
        BigInteger::checked(self.negative != other.negative, limbs::mul(&self.magnitude, &other.magnitude))
    }

    /// `divideAndRemainder`: the quotient truncated toward zero and the remainder, which
    /// takes the sign of `this`.
    pub fn divide_and_remainder(&self, other: &BigInteger) -> JResult<(BigInteger, BigInteger)> {
        if other.is_zero() {
            return Err(Thrown::new(ARITHMETIC, "BigInteger divide by zero"));
        }
        let (quotient, remainder) = limbs::div_rem(&self.magnitude, &other.magnitude);
        Ok((
            BigInteger::from_parts(self.negative != other.negative, quotient),
            BigInteger::from_parts(self.negative, remainder),
        ))
    }

    /// `this.pow(exponent)`.
    pub fn pow(&self, exponent: i32) -> JResult<BigInteger> {
        let Ok(exponent) = u32::try_from(exponent) else {
            return Err(Thrown::new(ARITHMETIC, "Negative exponent"));
        };
        if self.is_zero() {
            return Ok(if exponent == 0 { BigInteger::from_i64(1) } else { BigInteger::zero() });
        }
        let twos = limbs::trailing_zeros(&self.magnitude);
        let shift = twos * u64::from(exponent);
        if shift > MAX_BIT_LENGTH {
            return Err(overflow());
        }
        let odd_part = limbs::shr(&self.magnitude, twos);
        if odd_part != [1] {
            let estimate = limbs::bit_length(&self.magnitude) * u64::from(exponent);
            if estimate >= POW_BIT_ESTIMATE_LIMIT {
                return Err(overflow());
            }
        }
        let negative = self.negative && exponent % 2 == 1;
        let power = limbs::pow(&odd_part, exponent);
        BigInteger::checked(negative, limbs::shl(&power, shift))
    }

    /// `BigInteger.TEN.pow(exponent)`.
    pub(crate) fn pow10(exponent: u32) -> JResult<BigInteger> {
        BigInteger::from_i64(10).pow(exponent.min(i32::MAX as u32) as i32)
    }

    /// The magnitude of `this * 10^exponent` for comparisons, free of range limits.
    pub(crate) fn magnitude_times_pow10(&self, exponent: u32) -> BigInteger {
        let power = limbs::shl(&limbs::pow(&[5], exponent), u64::from(exponent));
        BigInteger::from_parts(false, limbs::mul(&self.magnitude, &power))
    }

    /// Shifts left by a distance that may be negative.
    fn shifted(&self, distance: i64) -> JResult<BigInteger> {
        if self.is_zero() {
            return Ok(BigInteger::zero());
        }
        let bits = distance.unsigned_abs();
        if distance >= 0 {
            if limbs::bit_length(&self.magnitude) + bits > MAX_BIT_LENGTH {
                return Err(overflow());
            }
            return Ok(BigInteger::from_parts(self.negative, limbs::shl(&self.magnitude, bits)));
        }
        let mut magnitude = limbs::shr(&self.magnitude, bits);
        if self.negative && limbs::trailing_zeros(&self.magnitude) < bits {
            // Arithmetic shifts round toward negative infinity.
            magnitude = limbs::add(&magnitude, &[1]);
        }
        Ok(BigInteger::from_parts(self.negative, magnitude))
    }

    /// `this << distance`.
    pub fn shift_left(&self, distance: i32) -> JResult<BigInteger> {
        self.shifted(i64::from(distance))
    }

    /// `this >> distance`, an arithmetic shift.
    pub fn shift_right(&self, distance: i32) -> JResult<BigInteger> {
        self.shifted(-i64::from(distance))
    }

    /// `bitLength()`: bits of the two's complement form, excluding the sign bit.
    pub fn bit_length(&self) -> i32 {
        let bits = limbs::bit_length(&self.magnitude);
        let power_of_two = limbs::trailing_zeros(&self.magnitude) + 1 == bits;
        let bits = if self.negative && power_of_two { bits - 1 } else { bits };
        bits.min(MAX_BIT_LENGTH) as i32
    }

    /// `abs()`.
    pub fn abs(&self) -> BigInteger {
        BigInteger::from_parts(false, self.magnitude.clone())
    }

    /// `negate()`.
    pub fn negate(&self) -> BigInteger {
        BigInteger::from_parts(!self.negative, self.magnitude.clone())
    }

    /// `signum()`.
    pub fn signum(&self) -> i32 {
        match (self.is_zero(), self.negative) {
            (true, _) => 0,
            (false, true) => -1,
            (false, false) => 1,
        }
    }

    /// `compareTo`.
    pub fn compare_to(&self, other: &BigInteger) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => self.compare_magnitude(other),
            (true, true) => other.compare_magnitude(self),
        }
    }

    /// Compares absolute values.
    pub(crate) fn compare_magnitude(&self, other: &BigInteger) -> Ordering {
        limbs::compare(&self.magnitude, &other.magnitude)
    }

    /// `toString(radix)`; a radix outside 2..=36 means 10.
    pub fn to_string_radix(&self, radix: i32) -> String {
        let radix = u32::try_from(radix).ok().filter(|radix| (2..=36).contains(radix)).unwrap_or(10);
        let digits = radix::format(&self.magnitude, radix);
        if self.negative {
            format!("-{digits}")
        } else {
            digits
        }
    }

    /// `longValue()`: the low 64 bits of the two's complement form.
    pub fn long_value(&self) -> i64 {
        let low = self.magnitude.first().copied().unwrap_or(0);
        (if self.negative { low.wrapping_neg() } else { low }) as i64
    }

    /// `intValue()`: the low 32 bits of the two's complement form.
    pub fn int_value(&self) -> i32 {
        self.long_value() as i32
    }

    /// `doubleValue()`, rounded half-even and infinite beyond the double range.
    pub fn double_value(&self) -> f64 {
        let bits = limbs::bit_length(&self.magnitude);
        let shift = bits.saturating_sub(u64::from(u64::BITS));
        let top = limbs::shr(&self.magnitude, shift).first().copied().unwrap_or(0);
        // A sticky bit keeps the cast to f64 rounding like the exact value.
        let sticky = limbs::trailing_zeros(&self.magnitude) < shift;
        let rounded = (top | u64::from(sticky)) as f64;
        let magnitude = if bits > 1024 { f64::INFINITY } else { rounded * f64::from_bits((1023 + shift) << 52) };
        if self.negative {
            -magnitude
        } else {
            magnitude
        }
    }

    /// `hashCode()`.
    pub fn hash_code(&self) -> i32 {
        let hash = self
            .magnitude
            .iter()
            .rev()
            .flat_map(|limb| [(limb >> u32::BITS) as u32, *limb as u32])
            .fold(0u32, |hash, word| hash.wrapping_mul(31).wrapping_add(word));
        (hash as i32).wrapping_mul(self.signum())
    }

    /// True for zero.
    pub fn is_zero(&self) -> bool {
        self.magnitude.is_empty()
    }

    /// True for an even value.
    pub(crate) fn is_even(&self) -> bool {
        self.magnitude.first().is_none_or(|limb| limb & 1 == 0)
    }

    /// True when the JDK can hold the value in a `long` without its sentinel `Long.MIN_VALUE`.
    pub(crate) fn fits_compact(&self) -> bool {
        match *self.magnitude.as_slice() {
            [] => true,
            [limb] => limb <= i64::MAX as u64,
            _ => false,
        }
    }

    /// Number of decimal digits of the absolute value; 1 for zero.
    pub(crate) fn decimal_digits(&self) -> i32 {
        /// log10(2) as a 32-bit binary fraction, rounded down.
        const LOG10_2_Q32: u64 = 1_292_913_986;
        let bits = limbs::bit_length(&self.magnitude);
        if let [] | [_] = *self.magnitude.as_slice() {
            let value = self.magnitude.first().copied().unwrap_or(0);
            return value.checked_ilog10().map_or(1, |log| log as i32 + 1);
        }
        let low_bound = (((bits - 1) * LOG10_2_Q32) >> u32::BITS) as u32 + 1;
        let next_power = BigInteger::from_i64(1).magnitude_times_pow10(low_bound);
        let below = self.compare_magnitude(&next_power) == Ordering::Less;
        (low_bound + u32::from(!below)) as i32
    }
}

impl fmt::Display for BigInteger {
    /// `toString()`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_string_radix(10))
    }
}

#[cfg(test)]
mod tests;
