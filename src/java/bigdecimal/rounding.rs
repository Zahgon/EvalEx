//! Rounding primitives: quotient rounding, precision reduction, scale changes.

use super::{check_scale_saturating, check_scale_truncating, BigDecimal, MathContext, RoundingMode, ARITHMETIC};
use crate::java::biginteger::BigInteger;
use crate::java::{JResult, Thrown};
use std::cmp::Ordering;

/// Largest power of ten stripped in one step.
const STRIP_CHUNK: u32 = 19;

/// Whether a non-zero remainder moves the truncated quotient away from zero.
fn rounds_away(
    mode: RoundingMode,
    negative: bool,
    quotient: &BigInteger,
    remainder: &BigInteger,
    divisor: &BigInteger,
) -> JResult<bool> {
    let tie_breaker = |on_tie: bool| -> JResult<bool> {
        let doubled = remainder.shift_left(1)?;
        Ok(match doubled.compare_magnitude(divisor) {
            Ordering::Less => false,
            Ordering::Greater => true,
            Ordering::Equal => on_tie,
        })
    };
    match mode {
        RoundingMode::Unnecessary => Err(Thrown::new(ARITHMETIC, "Rounding necessary")),
        RoundingMode::Up => Ok(true),
        RoundingMode::Down => Ok(false),
        RoundingMode::Ceiling => Ok(!negative),
        RoundingMode::Floor => Ok(negative),
        RoundingMode::HalfUp => tie_breaker(true),
        RoundingMode::HalfDown => tie_breaker(false),
        RoundingMode::HalfEven => tie_breaker(!quotient.is_even()),
    }
}

/// `dividend / divisor` rounded to an integer; the flag tells whether it was exact.
pub(super) fn divide_rounded(
    dividend: &BigInteger,
    divisor: &BigInteger,
    mode: RoundingMode,
) -> JResult<(BigInteger, bool)> {
    let (quotient, remainder) = dividend.divide_and_remainder(divisor)?;
    if remainder.is_zero() {
        return Ok((quotient, true));
    }
    let negative = dividend.signum() != divisor.signum();
    if !rounds_away(mode, negative, &quotient, &remainder, divisor)? {
        return Ok((quotient, false));
    }
    let step = BigInteger::from_i64(if negative { -1 } else { 1 });
    Ok((quotient.add(&step)?, false))
}

/// `value / divisor` when the division leaves no remainder.
fn exact_quotient(value: &BigInteger, divisor: &BigInteger) -> JResult<Option<BigInteger>> {
    let (quotient, remainder) = value.divide_and_remainder(divisor)?;
    Ok(remainder.is_zero().then_some(quotient))
}

/// Removes up to `scale - preferred` trailing zeros, lowering the scale accordingly.
pub(super) fn strip_zeros(mut unscaled: BigInteger, scale: i32, preferred: i64) -> JResult<BigDecimal> {
    let mut scale = i64::from(scale);
    let chunk = BigInteger::pow10(STRIP_CHUNK)?;
    let ten = BigInteger::from_i64(10);
    while !unscaled.is_zero() && unscaled.is_even() && scale > preferred {
        let whole_chunk =
            if scale - preferred >= i64::from(STRIP_CHUNK) { exact_quotient(&unscaled, &chunk)? } else { None };
        let (quotient, digits) = match whole_chunk {
            Some(quotient) => (quotient, STRIP_CHUNK),
            None => match exact_quotient(&unscaled, &ten)? {
                Some(quotient) => (quotient, 1),
                None => break,
            },
        };
        scale = i64::from(check_scale_saturating(scale - i64::from(digits))?);
        unscaled = quotient;
    }
    Ok(BigDecimal::compact(unscaled, scale as i32))
}

/// Rounds to `mc.precision` digits, validating the scale before each digit drop.
///
/// Returns the new unscaled value and scale; `None` when the value already fits.
pub(super) fn reduce_digits(
    unscaled: &BigInteger,
    scale: i64,
    mc: &MathContext,
    check: impl Fn(i64) -> JResult<i64>,
) -> JResult<Option<(BigInteger, i64)>> {
    if mc.precision == 0 {
        return Ok(None);
    }
    let mut digits = unscaled.decimal_digits();
    if digits <= mc.precision {
        return Ok(None);
    }
    let mut value = unscaled.clone();
    let mut scale = scale;
    while digits > mc.precision {
        let drop = digits - mc.precision;
        scale = check(scale - i64::from(drop))?;
        let power = BigInteger::pow10(drop.unsigned_abs())?;
        value = divide_rounded(&value, &power, mc.rounding)?.0;
        digits = value.decimal_digits();
    }
    Ok(Some((value, scale)))
}

/// [`reduce_digits`] for a value with a valid scale.
pub(super) fn reduce_precision(unscaled: &BigInteger, scale: i32, mc: &MathContext) -> JResult<Option<BigDecimal>> {
    let checked = |scale| check_scale_truncating(scale).map(i64::from);
    let reduced = reduce_digits(unscaled, i64::from(scale), mc, checked)?;
    Ok(reduced.map(|(value, scale)| BigDecimal::compact(value, scale as i32)))
}

impl BigDecimal {
    /// `round(mc)`, also known as `plus(mc)`.
    pub fn round(&self, mc: &MathContext) -> JResult<BigDecimal> {
        let reduced = reduce_precision(&self.unscaled, self.scale, mc)?;
        Ok(reduced.unwrap_or_else(|| self.clone()))
    }

    /// `plus(mc)`.
    pub fn plus_mc(&self, mc: &MathContext) -> JResult<BigDecimal> {
        self.round(mc)
    }

    /// `negate(mc)`.
    pub fn negate_mc(&self, mc: &MathContext) -> JResult<BigDecimal> {
        self.negate().round(mc)
    }

    /// `abs(mc)`.
    pub fn abs_mc(&self, mc: &MathContext) -> JResult<BigDecimal> {
        if self.signum() < 0 {
            self.negate_mc(mc)
        } else {
            self.plus_mc(mc)
        }
    }

    /// `doRound` of a freshly computed integer: an unrounded result is stored compactly.
    pub(super) fn round_integer(unscaled: BigInteger, scale: i32, mc: &MathContext) -> JResult<BigDecimal> {
        let reduced = reduce_precision(&unscaled, scale, mc)?;
        Ok(reduced.unwrap_or_else(|| BigDecimal::compact(unscaled, scale)))
    }

    /// `this.unscaled * 10^raise`; the compact zero never computes the power.
    pub(super) fn raised(&self, raise: i32) -> JResult<BigInteger> {
        if raise <= 0 || (self.is_zero() && !self.inflated) {
            return Ok(self.unscaled.clone());
        }
        BigInteger::pow10(raise.unsigned_abs())?.multiply(&self.unscaled)
    }

    /// `setScale(scale, mode)`.
    pub fn set_scale(&self, scale: i32, mode: RoundingMode) -> JResult<BigDecimal> {
        if scale == self.scale {
            return Ok(self.clone());
        }
        if self.is_zero() {
            return Ok(BigDecimal::zero_with_scale(scale));
        }
        let gap = i64::from(scale) - i64::from(self.scale);
        if gap > 0 {
            let raised = self.raised(check_scale_saturating(gap)?)?;
            return Ok(BigDecimal::stored(raised, scale, self.inflated));
        }
        let drop = check_scale_saturating(-gap)?;
        let power = BigInteger::pow10(drop.unsigned_abs())?;
        let (quotient, _) = divide_rounded(&self.unscaled, &power, mode)?;
        Ok(BigDecimal::compact(quotient, scale))
    }
}
