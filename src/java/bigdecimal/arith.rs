//! Addition, multiplication and exponentiation.

use super::{check_scale, check_scale_saturating, rounding, BigDecimal, MathContext, RoundingMode, ARITHMETIC};
use crate::java::biginteger::BigInteger;
use crate::java::{JResult, Thrown};
use std::cmp::Ordering;

/// Largest `|n|` accepted by `pow`.
const MAX_POW_EXPONENT: i32 = 999_999_999;

fn invalid_operation() -> Thrown {
    Thrown::new(ARITHMETIC, "Invalid operation")
}

/// Brings two values to the scale of the one with more fraction digits.
fn align(left: &BigDecimal, right: &BigDecimal) -> JResult<(BigInteger, BigInteger, i32)> {
    let gap = i64::from(left.scale) - i64::from(right.scale);
    match gap.cmp(&0) {
        Ordering::Equal => Ok((left.unscaled.clone(), right.unscaled.clone(), left.scale)),
        Ordering::Less => {
            let raise = check_scale(left.is_zero(), -gap)?;
            Ok((left.raised(raise)?, right.unscaled.clone(), right.scale))
        }
        Ordering::Greater => {
            let raise = check_scale(right.is_zero(), gap)?;
            Ok((left.unscaled.clone(), right.raised(raise)?, left.scale))
        }
    }
}

/// Replaces the insignificant operand of an inexact sum by a sticky unit, as `preAlign` does.
fn condense_small(left: &BigDecimal, right: &BigDecimal, mc: &MathContext) -> JResult<BigDecimal> {
    let (big, small) = if left.scale < right.scale { (left, right) } else { (right, left) };
    let result_ulp_scale = i64::from(big.scale) - i64::from(big.precision()) + i64::from(mc.precision);
    let small_high_digit = i64::from(small.scale) - i64::from(small.precision()) + 1;
    // The JDK adds in `int` here, so the test passes when the sum wraps.
    let disjoint = small_high_digit > i64::from(big.scale.wrapping_add(2));
    if disjoint && small_high_digit > result_ulp_scale + 2 {
        let scale = check_scale_saturating(i64::from(big.scale).max(result_ulp_scale) + 3)?;
        return Ok(BigDecimal::compact(BigInteger::from_i64(i64::from(small.signum())), scale));
    }
    Ok(small.clone())
}

/// The sum of `augend` and a zero of `zero_scale`, rounded and rescaled for `mc`.
fn sum_with_zero(value: &BigDecimal, zero_scale: i32, mc: &MathContext) -> JResult<BigDecimal> {
    let preferred = value.scale.max(zero_scale);
    let result = value.round(mc)?;
    match result.scale.cmp(&preferred) {
        Ordering::Equal => Ok(result),
        Ordering::Greater => rounding::strip_zeros(result.unscaled, result.scale, i64::from(preferred)),
        Ordering::Less => {
            // Both differences are `int` arithmetic in the JDK and may wrap.
            let spare_digits = mc.precision.wrapping_sub(result.precision());
            let missing_digits = preferred.wrapping_sub(result.scale);
            let scale =
                if spare_digits >= missing_digits { preferred } else { result.scale.wrapping_add(spare_digits) };
            result.set_scale(scale, RoundingMode::Unnecessary)
        }
    }
}

impl BigDecimal {
    /// `add(augend)`.
    pub fn add(&self, augend: &BigDecimal) -> JResult<BigDecimal> {
        let (left, right, scale) = align(self, augend)?;
        let keeps_form = (self.inflated || augend.inflated) && self.signum() == augend.signum();
        Ok(BigDecimal::stored(left.add(&right)?, scale, keeps_form))
    }

    /// `subtract(subtrahend)`.
    pub fn subtract(&self, subtrahend: &BigDecimal) -> JResult<BigDecimal> {
        self.add(&subtrahend.negate())
    }

    /// `add(augend, mc)`.
    pub fn add_mc(&self, augend: &BigDecimal, mc: &MathContext) -> JResult<BigDecimal> {
        if mc.precision == 0 {
            return self.add(augend);
        }
        match (self.is_zero(), augend.is_zero()) {
            (true, true) => {
                return Ok(BigDecimal::zero_with_scale(self.scale.max(augend.scale)));
            }
            (true, false) => return sum_with_zero(augend, self.scale, mc),
            (false, true) => return sum_with_zero(self, augend.scale, mc),
            (false, false) => {}
        }
        let (big, small) = if self.scale == augend.scale {
            (self.clone(), augend.clone())
        } else if self.scale < augend.scale {
            (self.clone(), condense_small(self, augend, mc)?)
        } else {
            (augend.clone(), condense_small(self, augend, mc)?)
        };
        let unnecessary = RoundingMode::Unnecessary;
        let scale = big.scale.max(small.scale);
        let left = big.set_scale(scale, unnecessary)?;
        let right = small.set_scale(scale, unnecessary)?;
        BigDecimal::round_integer(left.unscaled.add(&right.unscaled)?, scale, mc)
    }

    /// `subtract(subtrahend, mc)`.
    pub fn subtract_mc(&self, subtrahend: &BigDecimal, mc: &MathContext) -> JResult<BigDecimal> {
        if mc.precision == 0 {
            return self.subtract(subtrahend);
        }
        self.add_mc(&subtrahend.negate(), mc)
    }

    /// `multiply(multiplicand)`.
    pub fn multiply(&self, multiplicand: &BigDecimal) -> JResult<BigDecimal> {
        let scale = self.product_scale(multiplicand)?;
        let product = self.unscaled.multiply(&multiplicand.unscaled)?;
        let compact_zero = (self.is_zero() && !self.inflated) || (multiplicand.is_zero() && !multiplicand.inflated);
        let keeps_form = (self.inflated || multiplicand.inflated) && !compact_zero;
        Ok(BigDecimal::stored(product, scale, keeps_form))
    }

    /// The scale of a product; only the left operand excuses an overflow by being zero.
    fn product_scale(&self, multiplicand: &BigDecimal) -> JResult<i32> {
        check_scale(self.is_zero(), i64::from(self.scale) + i64::from(multiplicand.scale))
    }

    /// `multiply(multiplicand, mc)`.
    pub fn multiply_mc(&self, multiplicand: &BigDecimal, mc: &MathContext) -> JResult<BigDecimal> {
        if mc.precision == 0 {
            return self.multiply(multiplicand);
        }
        let scale = self.product_scale(multiplicand)?;
        let product = self.unscaled.multiply(&multiplicand.unscaled)?;
        if self.is_zero() || multiplicand.is_zero() {
            return Ok(BigDecimal::compact(product, scale));
        }
        BigDecimal::round_integer(product, scale, mc)
    }

    /// `pow(n)`.
    pub fn pow(&self, n: i32) -> JResult<BigDecimal> {
        if !(0..=MAX_POW_EXPONENT).contains(&n) {
            return Err(invalid_operation());
        }
        let scale = check_scale(self.is_zero(), i64::from(self.scale) * i64::from(n))?;
        Ok(BigDecimal::compact(self.unscaled.pow(n)?, scale))
    }

    /// `pow(n, mc)`: the ANSI X3.274 algorithm.
    pub fn pow_mc(&self, n: i32, mc: &MathContext) -> JResult<BigDecimal> {
        if mc.precision == 0 {
            return self.pow(n);
        }
        if !(-MAX_POW_EXPONENT..=MAX_POW_EXPONENT).contains(&n) {
            return Err(invalid_operation());
        }
        if n == 0 {
            return Ok(BigDecimal::one());
        }
        let magnitude = n.unsigned_abs();
        let exponent_digits = magnitude.ilog10() as i32 + 1;
        if exponent_digits > mc.precision {
            return Err(invalid_operation());
        }
        let working_precision = mc.precision.wrapping_add(exponent_digits).wrapping_add(1);
        let working = MathContext::new(working_precision, mc.rounding)?;
        let mut accumulator = BigDecimal::one();
        let mut started = false;
        for bit in (0..u32::BITS - 1).rev() {
            if magnitude >> bit & 1 == 1 {
                started = true;
                accumulator = accumulator.multiply_mc(self, &working)?;
            }
            if started && bit > 0 {
                accumulator = accumulator.multiply_mc(&accumulator, &working)?;
            }
        }
        if n < 0 {
            accumulator = BigDecimal::one().divide_mc(&accumulator, &working)?;
        }
        accumulator.round(mc)
    }
}
