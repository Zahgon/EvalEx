//! Division in all its forms: to a scale, to a precision, exact, and integral.

use super::rounding::{divide_rounded, strip_zeros};
use super::{check_scale, check_scale_truncating, saturate, BigDecimal, MathContext, RoundingMode, ARITHMETIC};
use crate::java::biginteger::BigInteger;
use crate::java::{JResult, Thrown};
use std::cmp::Ordering;

/// The message of `long` division by zero in the JDK's compact path.
const LONG_DIVIDE_BY_ZERO: &str = "/ by zero";
const BIG_DIVIDE_BY_ZERO: &str = "BigInteger divide by zero";
/// Decimal shifts below this size stay in the JDK's `long` division path.
const LONG_POWERS_OF_TEN: i32 = 19;

/// `value * 10^raise`; the power is computed even for a zero value, as it can overflow.
fn scaled_up(value: &BigInteger, raise: i32) -> JResult<BigInteger> {
    if raise <= 0 {
        return Ok(value.clone());
    }
    BigInteger::pow10(raise.unsigned_abs())?.multiply(value)
}

fn zero_divisor_error(dividend: &BigDecimal) -> Thrown {
    let message = if dividend.is_zero() { "Division undefined" } else { "Division by zero" };
    Thrown::new(ARITHMETIC, message)
}

/// The digit budget beyond the dividend that any terminating quotient fits in.
fn divisor_digit_budget(divisor: &BigDecimal) -> i64 {
    (10.0 * f64::from(divisor.precision()) / 3.0).ceil() as i64
}

/// Quotient at `scale`, stripped toward `preferred` when exact.
fn divide_and_round(
    dividend: &BigInteger,
    divisor: &BigInteger,
    scale: i32,
    mode: RoundingMode,
    preferred: i32,
) -> JResult<BigDecimal> {
    let (quotient, exact) = divide_rounded(dividend, divisor, mode)?;
    if exact && preferred != scale {
        strip_zeros(quotient, scale, i64::from(preferred))
    } else {
        Ok(BigDecimal::compact(quotient, scale))
    }
}

/// True when `x` read as a fraction of `x_digits` digits exceeds `y` of `y_digits` digits.
fn exceeds_normalized(x: &BigInteger, x_digits: i64, y: &BigInteger, y_digits: i64) -> bool {
    let gap = x_digits - y_digits;
    let raise = gap.unsigned_abs() as u32;
    let ordering = if gap < 0 {
        x.magnitude_times_pow10(raise).compare_magnitude(y)
    } else {
        x.compare_magnitude(&y.magnitude_times_pow10(raise))
    };
    ordering == Ordering::Greater
}

impl BigDecimal {
    /// `divide(divisor, scale, mode)`.
    pub fn divide_scale(&self, divisor: &BigDecimal, scale: i32, mode: RoundingMode) -> JResult<BigDecimal> {
        let target = check_scale(self.is_zero(), i64::from(scale) + i64::from(divisor.scale))?;
        let (dividend, scaled_divisor, raise) = if target > self.scale {
            let raise = scale.wrapping_add(divisor.scale).wrapping_sub(self.scale);
            (scaled_up(&self.unscaled, raise)?, divisor.unscaled.clone(), raise)
        } else {
            let gap = i64::from(self.scale) - i64::from(scale);
            let raise = check_scale(divisor.is_zero(), gap)?.wrapping_sub(divisor.scale);
            (self.unscaled.clone(), scaled_up(&divisor.unscaled, raise)?, raise)
        };
        if scaled_divisor.is_zero() {
            let in_long = !self.inflated && !divisor.inflated && raise < LONG_POWERS_OF_TEN && dividend.fits_compact();
            let message = if in_long { LONG_DIVIDE_BY_ZERO } else { BIG_DIVIDE_BY_ZERO };
            return Err(Thrown::new(ARITHMETIC, message));
        }
        divide_and_round(&dividend, &scaled_divisor, scale, mode, scale)
    }

    /// `divide(divisor)`: the exact quotient.
    pub fn divide(&self, divisor: &BigDecimal) -> JResult<BigDecimal> {
        if divisor.is_zero() {
            return Err(zero_divisor_error(self));
        }
        let preferred = saturate(i64::from(self.scale) - i64::from(divisor.scale));
        if self.is_zero() {
            return Ok(BigDecimal::zero_with_scale(preferred));
        }
        let digits = i64::from(self.precision()) + divisor_digit_budget(divisor);
        let mc = MathContext { precision: digits.min(i64::from(i32::MAX)) as i32, rounding: RoundingMode::Unnecessary };
        // Like the JDK, this reports every failure of the bounded division the same way.
        let quotient = self.divide_mc(divisor, &mc).map_err(|_| {
            Thrown::new(ARITHMETIC, "Non-terminating decimal expansion; no exact representable decimal result.")
        })?;
        if preferred > quotient.scale {
            quotient.set_scale(preferred, RoundingMode::Unnecessary)
        } else {
            Ok(quotient)
        }
    }

    /// `divide(divisor, mc)`.
    pub fn divide_mc(&self, divisor: &BigDecimal, mc: &MathContext) -> JResult<BigDecimal> {
        if mc.precision == 0 {
            return self.divide(divisor);
        }
        let preferred = i64::from(self.scale) - i64::from(divisor.scale);
        if divisor.is_zero() {
            return Err(zero_divisor_error(self));
        }
        if self.is_zero() {
            return Ok(BigDecimal::zero_with_scale(saturate(preferred)));
        }
        let (x, y) = (&self.unscaled, &divisor.unscaled);
        let x_digits = i64::from(self.precision());
        let mut y_digits = i64::from(divisor.precision());
        if exceeds_normalized(x, x_digits, y, y_digits) {
            y_digits -= 1;
        }
        let precision = i64::from(mc.precision);
        let scale = check_scale_truncating(preferred + y_digits - x_digits + precision)?;
        let growth = check_scale_truncating(precision + y_digits - x_digits)?;
        let quotient = if growth > 0 {
            let raised = self.raised(growth)?;
            let preferred = check_scale_truncating(preferred)?;
            divide_and_round(&raised, y, scale, mc.rounding, preferred)?
        } else {
            let divisor_scale = check_scale_truncating(x_digits - precision)?;
            let raise = check_scale_truncating(i64::from(divisor_scale) - y_digits)?;
            let raised = divisor.raised(raise)?;
            let preferred = check_scale_truncating(preferred)?;
            divide_and_round(x, &raised, scale, mc.rounding, preferred)?
        };
        quotient.round(mc)
    }

    /// `divideToIntegralValue(divisor)`.
    fn divide_to_integral(&self, divisor: &BigDecimal) -> JResult<BigDecimal> {
        let preferred = saturate(i64::from(self.scale) - i64::from(divisor.scale));
        if self.compare_magnitude(divisor) == Ordering::Less {
            return Ok(BigDecimal::zero_with_scale(preferred));
        }
        if self.is_zero() && !divisor.is_zero() {
            return self.set_scale(preferred, RoundingMode::Unnecessary);
        }
        let scale_gap = (i64::from(self.scale) - i64::from(divisor.scale)).abs();
        let digits = i64::from(self.precision()) + divisor_digit_budget(divisor) + scale_gap + 2;
        let mc = MathContext { precision: digits.min(i64::from(i32::MAX)) as i32, rounding: RoundingMode::Down };
        let mut quotient = self.divide_mc(divisor, &mc)?;
        if quotient.scale > 0 {
            let truncated = quotient.set_scale(0, RoundingMode::Down)?;
            quotient = strip_zeros(truncated.unscaled, truncated.scale, i64::from(preferred))?;
        }
        if quotient.scale < preferred {
            quotient = quotient.set_scale(preferred, RoundingMode::Unnecessary)?;
        }
        Ok(quotient)
    }

    /// `divideToIntegralValue(divisor, mc)`.
    fn divide_to_integral_mc(&self, divisor: &BigDecimal, mc: &MathContext) -> JResult<BigDecimal> {
        if mc.precision == 0 || self.compare_magnitude(divisor) == Ordering::Less {
            return self.divide_to_integral(divisor);
        }
        let preferred = saturate(i64::from(self.scale) - i64::from(divisor.scale));
        let truncating = MathContext { precision: mc.precision, rounding: RoundingMode::Down };
        let mut result = self.divide_mc(divisor, &truncating)?;
        if result.scale < 0 {
            let product = result.multiply(divisor)?;
            let remainder = self.subtract(&product)?;
            if remainder.compare_magnitude(divisor) != Ordering::Less {
                return Err(Thrown::new(ARITHMETIC, "Division impossible"));
            }
        } else if result.scale > 0 {
            result = result.set_scale(0, RoundingMode::Down)?;
        }
        // Both differences are `int` arithmetic in the JDK and may wrap.
        let spare_digits = mc.precision.wrapping_sub(result.precision());
        if preferred > result.scale && spare_digits > 0 {
            let missing_digits = preferred.wrapping_sub(result.scale);
            let scale = result.scale.wrapping_add(spare_digits.min(missing_digits));
            return result.set_scale(scale, RoundingMode::Unnecessary);
        }
        strip_zeros(result.unscaled, result.scale, i64::from(preferred))
    }

    /// `remainder(divisor)`.
    pub fn remainder(&self, divisor: &BigDecimal) -> JResult<BigDecimal> {
        let quotient = self.divide_to_integral(divisor)?;
        self.subtract(&quotient.multiply(divisor)?)
    }

    /// `remainder(divisor, mc)`: the quotient is limited by `mc`, the result is not rounded.
    pub fn remainder_mc(&self, divisor: &BigDecimal, mc: &MathContext) -> JResult<BigDecimal> {
        if mc.precision == 0 {
            return self.remainder(divisor);
        }
        let quotient = self.divide_to_integral_mc(divisor, mc)?;
        self.subtract(&quotient.multiply(divisor)?)
    }
}
