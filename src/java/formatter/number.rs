//! `BigDecimal` under the `%e`, `%f` and `%g` conversions.
//!
//! All rounding is `HALF_UP` on the magnitude. The sign is handled by the caller; this module
//! produces the digits, with the locale's separators and any zero padding.

use super::exceptions;
use super::spec::{Flags, Specifier};
use super::symbols::NumberSymbols;
use super::MAX_OUTPUT_UNITS;
use crate::java::bigdecimal::{BigDecimal, MathContext, RoundingMode};
use crate::java::biginteger::BigInteger;
use crate::java::JResult;
use std::cmp::Ordering;

const DEFAULT_PRECISION: i32 = 6;

/// Appends a magnitude according to the conversion of `specifier`.
pub(super) fn write_magnitude(
    out: &mut Vec<u16>,
    specifier: &Specifier,
    symbols: &NumberSymbols,
    magnitude: &BigDecimal,
    negative: bool,
) -> JResult<()> {
    let writer = MagnitudeWriter { specifier, symbols, negative };
    let precision = specifier.precision;
    match specifier.conversion {
        'e' => writer.scientific(out, magnitude, precision.unwrap_or(DEFAULT_PRECISION)),
        'f' => writer.fixed(out, magnitude, precision.unwrap_or(DEFAULT_PRECISION)),
        _ => writer.general(out, magnitude, precision),
    }
}

struct MagnitudeWriter<'a> {
    specifier: &'a Specifier,
    symbols: &'a NumberSymbols,
    negative: bool,
}

impl MagnitudeWriter<'_> {
    fn has(&self, flag: Flags) -> bool {
        self.specifier.flags.contains(flag)
    }

    /// The width left for the digits, once a closing parenthesis is set aside.
    fn digits_width(&self, reserved: usize) -> Option<usize> {
        let width = usize::try_from(self.specifier.width?).ok()?;
        let parenthesis = usize::from(self.negative && self.has(Flags::PARENTHESES));
        Some(width.saturating_sub(reserved + parenthesis))
    }

    fn scientific(&self, out: &mut Vec<u16>, value: &BigDecimal, precision: i32) -> JResult<()> {
        let original_digits = value.precision();
        let kept = if precision > original_digits - 1 { original_digits } else { precision + 1 };
        let padding = precision - (original_digits - 1);
        let rounded = value.round(&half_up(kept)?)?;
        let digits = rounded.unscaled_value().to_string_radix(10);
        let mut mantissa = scientific_mantissa(&digits);
        if !mantissa.contains('.') && (padding > 0 || self.has(Flags::ALTERNATE)) {
            mantissa.push('.');
        }
        push_zeros(&mut mantissa, padding)?;
        let exponent = exponent_text(&digits, rounded.scale());
        self.localize(out, &mantissa, self.digits_width(exponent.len() + 1))?;
        out.push(u16::from(if self.specifier.uppercase { b'E' } else { b'e' }));
        out.extend(exponent.encode_utf16());
        Ok(())
    }

    fn fixed(&self, out: &mut Vec<u16>, value: &BigDecimal, precision: i32) -> JResult<()> {
        let rounded = round_to_scale(value, precision)?;
        let digits = rounded.unscaled_value().to_string_radix(10);
        let (mut mantissa, scale) = fixed_mantissa(&digits, rounded.scale())?;
        let padding = precision.saturating_sub(scale).max(0);
        if scale == 0 && (padding > 0 || self.has(Flags::ALTERNATE)) {
            mantissa.push('.');
        }
        push_zeros(&mut mantissa, padding)?;
        self.localize(out, &mantissa, self.digits_width(0))
    }

    fn general(&self, out: &mut Vec<u16>, value: &BigDecimal, precision: Option<i32>) -> JResult<()> {
        let precision = precision.unwrap_or(DEFAULT_PRECISION).max(1);
        let rounded = value.round(&half_up(precision)?)?;
        let lower = BigDecimal::from_big_integer(&BigInteger::from_i64(1), 4);
        let upper = BigDecimal::from_big_integer(&BigInteger::from_i64(1), -precision);
        let is_plain_zero = rounded.signum() == 0 && rounded.scale() == 0;
        let in_fixed_range =
            rounded.compare_to(&lower) != Ordering::Less && rounded.compare_to(&upper) == Ordering::Less;
        if is_plain_zero || in_fixed_range {
            let digit_count = rounded.unscaled_value().to_string_radix(10).len();
            let exponent = i64::try_from(digit_count).unwrap_or(i64::MAX) - 1 - i64::from(rounded.scale());
            let fraction = i64::from(precision) - exponent - 1;
            self.fixed(out, &rounded, i32::try_from(fraction).unwrap_or(i32::MAX))
        } else {
            self.scientific(out, &rounded, precision - 1)
        }
    }

    /// Copies a mantissa with localized separators, then zero-pads up to `width`.
    fn localize(&self, out: &mut Vec<u16>, mantissa: &str, width: Option<usize>) -> JResult<()> {
        let begin = out.len();
        let integer_digits = mantissa.find('.').unwrap_or(mantissa.len());
        let group = self.has(Flags::GROUP);
        for (position, byte) in mantissa.bytes().enumerate() {
            if byte == b'.' {
                out.push(self.symbols.decimal);
                continue;
            }
            out.push(u16::from(byte));
            let remaining = integer_digits.saturating_sub(position + 1);
            if group && remaining > 0 && remaining.is_multiple_of(3) {
                out.push(self.symbols.grouping);
            }
        }
        if let Some(width) = width.filter(|_| self.has(Flags::ZERO_PAD)) {
            if width > MAX_OUTPUT_UNITS {
                return Err(exceptions::out_of_memory());
            }
            let missing = width.saturating_sub(out.len());
            out.splice(begin..begin, std::iter::repeat_n(u16::from(b'0'), missing));
        }
        Ok(())
    }
}

fn half_up(precision: i32) -> JResult<MathContext> {
    MathContext::new(precision, RoundingMode::HalfUp)
}

/// Cuts the fraction of a value down to `precision` digits; a shorter fraction stays as it is.
fn round_to_scale(value: &BigDecimal, precision: i32) -> JResult<BigDecimal> {
    let scale = value.scale();
    if scale <= precision {
        return Ok(value.clone());
    }
    let digit_count = value.precision();
    if digit_count <= scale {
        value.set_scale(precision, RoundingMode::HalfUp)
    } else {
        value.round(&half_up(digit_count - (scale - precision))?)
    }
}

fn push_zeros(text: &mut String, count: i32) -> JResult<()> {
    let count = usize::try_from(count).unwrap_or(0);
    if count > MAX_OUTPUT_UNITS {
        return Err(exceptions::out_of_memory());
    }
    text.extend(std::iter::repeat_n('0', count));
    Ok(())
}

/// `d.ddd`, or `d` for a single digit.
fn scientific_mantissa(digits: &str) -> String {
    let (first, rest) = digits.split_at(digits.len().min(1));
    if rest.is_empty() {
        first.to_string()
    } else {
        format!("{first}.{rest}")
    }
}

/// The signed exponent of at least two digits.
///
/// A scale of zero takes a separate path in Java whose two-digit test is off by one: exactly ten
/// digits give the one-digit exponent `+9`.
fn exponent_text(digits: &str, scale: i32) -> String {
    if scale == 0 && digits.len() == 10 {
        return "+9".to_string();
    }
    let adjusted = i64::try_from(digits.len()).unwrap_or(i64::MAX) - 1 - i64::from(scale);
    let sign = if adjusted < 0 { '-' } else { '+' };
    format!("{sign}{:02}", adjusted.unsigned_abs())
}

/// The plain digits of a scaled value, with the number of fraction digits written. A negative
/// scale writes the trailing zeros, except that a zero stays a single digit.
fn fixed_mantissa(digits: &str, scale: i32) -> JResult<(String, i32)> {
    let length = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    if scale == 0 {
        return Ok((digits.to_string(), 0));
    }
    if scale < 0 {
        let mut text = digits.to_string();
        if digits != "0" {
            push_zeros(&mut text, scale.saturating_neg())?;
        }
        return Ok((text, 0));
    }
    if scale >= length {
        let mut text = String::from("0.");
        push_zeros(&mut text, scale - length)?;
        text.push_str(digits);
        return Ok((text, scale));
    }
    let (integer, fraction) = digits.split_at(digits.len() - scale.unsigned_abs() as usize);
    Ok((format!("{integer}.{fraction}"), scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponents_keep_two_digits_except_for_the_ten_digit_quirk() {
        assert_eq!(exponent_text("15", 1), "+00");
        assert_eq!(exponent_text("15", 12), "-11");
        assert_eq!(exponent_text("123456789", 0), "+08");
        assert_eq!(exponent_text("1234567890", 0), "+9");
        assert_eq!(exponent_text("12345678901", 0), "+10");
    }

    #[test]
    fn fixed_mantissas_cover_every_scale_shape() {
        assert_eq!(fixed_mantissa("15", 3).unwrap(), ("0.015".to_string(), 3));
        assert_eq!(fixed_mantissa("15", 1).unwrap(), ("1.5".to_string(), 1));
        assert_eq!(fixed_mantissa("15", -2).unwrap(), ("1500".to_string(), 0));
        assert_eq!(fixed_mantissa("0", -2).unwrap(), ("0".to_string(), 0));
    }
}
