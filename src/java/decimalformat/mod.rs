//! `java.text.DecimalFormat` parsing with `setParseBigDecimal(true)`.
//!
//! Only the parsing half exists: a pattern contributes its affixes, its multiplier and whether
//! grouping is on; digit counts are validated but otherwise have no effect on `parse`.

mod pattern;

use crate::java::bigdecimal::{BigDecimal, RoundingMode};
use crate::java::biginteger::BigInteger;
use crate::java::chars::decimal_digit;
use crate::java::formatter::symbols::NumberSymbols;
use crate::java::locale::Locale;
use crate::java::{js, JResult, JString, Thrown};
use pattern::{AffixItem, Pattern};

const ARITHMETIC_EXCEPTION: &str = "java.lang.ArithmeticException";

const MINUS_SIGN: u16 = 0x2D;
const EXPONENT_SIGN: u16 = 0x45;
const NAN_TEXT: &str = "NaN";
const INFINITY_SIGN: u16 = 0x221E;
const PERCENT_SIGN: u16 = 0x25;
const PER_MILLE_SIGN: u16 = 0x2030;

/// The result of [`DecimalFormat::parse`].
#[derive(Clone, Debug)]
pub enum Parsed {
    /// A `BigDecimal` result.
    Number(BigDecimal),
    /// A `Double` result: the NaN or infinity symbol.
    NotFinite,
    /// The `ParseException` of `NumberFormat.parse`.
    NoMatch,
    /// A runtime exception escaping `parse`, for exponents beyond the range of a scale.
    Failed(Thrown),
}

/// A `DecimalFormat` with `parseBigDecimal` set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecimalFormat {
    positive_prefix: JString,
    positive_suffix: JString,
    negative_prefix: JString,
    negative_suffix: JString,
    grouping_used: bool,
    multiplier: i32,
    decimal: u16,
    grouping: u16,
}

impl DecimalFormat {
    /// `new DecimalFormat(pattern, DecimalFormatSymbols.getInstance(locale))`.
    pub fn new(pattern: &[u16], locale: &Locale) -> JResult<DecimalFormat> {
        let parsed: Pattern = pattern::parse(pattern)?;
        let symbols = NumberSymbols::of(locale);
        Ok(DecimalFormat {
            positive_prefix: expand_affix(&parsed.positive_prefix, &symbols),
            positive_suffix: expand_affix(&parsed.positive_suffix, &symbols),
            negative_prefix: expand_affix(&parsed.negative_prefix, &symbols),
            negative_suffix: expand_affix(&parsed.negative_suffix, &symbols),
            grouping_used: parsed.grouping_used,
            multiplier: parsed.multiplier,
            decimal: symbols.decimal,
            grouping: symbols.grouping,
        })
    }

    /// `NumberFormat.parse(String)`: parses a prefix of `text`, ignoring what follows.
    pub fn parse(&self, text: &[u16]) -> Parsed {
        if text.starts_with(&js(NAN_TEXT)) {
            return Parsed::NotFinite;
        }
        let prefixes = (&self.positive_prefix[..], &self.negative_prefix[..]);
        let (positive, negative) = longest_match((true, true), prefixes, text);
        let start = if positive {
            self.positive_prefix.len()
        } else if negative {
            self.negative_prefix.len()
        } else {
            return Parsed::NoMatch;
        };
        let tail = text.get(start..).unwrap_or_default();
        let body = if tail.first() == Some(&INFINITY_SIGN) {
            Body::Infinite { length: 1 }
        } else {
            match self.scan_digits(tail) {
                Some(digits) => Body::Finite(digits),
                None => return Parsed::NoMatch,
            }
        };
        let after = tail.get(body.length()..).unwrap_or_default();
        let suffixes = (&self.positive_suffix[..], &self.negative_suffix[..]);
        let (positive, negative) = longest_match((positive, negative), suffixes, after);
        if positive == negative {
            return Parsed::NoMatch;
        }
        match body {
            Body::Infinite { .. } => Parsed::NotFinite,
            Body::Finite(digits) => match self.to_value(&digits, positive) {
                Ok(value) => Parsed::Number(value),
                Err(thrown) => Parsed::Failed(thrown),
            },
        }
    }

    /// Reads digits, separators and an exponent. `None` when no digit is present.
    fn scan_digits(&self, text: &[u16]) -> Option<ScannedDigits> {
        let mut digits = String::new();
        let mut digit_count = 0_i32;
        let mut point: Option<i32> = None;
        let mut leading_fraction_zeros = 0_i32;
        let mut saw_digit = false;
        let mut pending_group: Option<usize> = None;
        let mut position = 0;
        let mut exponent = 0_i32;
        while let Some(&unit) = text.get(position) {
            if let Some(value) = decimal_digit(unit) {
                saw_digit = true;
                pending_group = None;
                if value != 0 || !digits.is_empty() {
                    digit_count = digit_count.wrapping_add(1);
                    digits.push(char::from(b'0' + value as u8));
                } else if point.is_some() {
                    leading_fraction_zeros = leading_fraction_zeros.wrapping_add(1);
                }
            } else if unit == self.decimal {
                if point.is_some() {
                    break;
                }
                point = Some(digit_count);
            } else if unit == self.grouping && self.grouping_used {
                if point.is_some() {
                    break;
                }
                pending_group = Some(position);
            } else if unit == EXPONENT_SIGN {
                if let Some(scan) = scan_exponent(text.get(position + 1..).unwrap_or_default()) {
                    position += 1 + scan.length;
                    exponent = scan.value;
                }
                break;
            } else {
                break;
            }
            position += 1;
        }
        if !saw_digit && digit_count == 0 {
            return None;
        }
        let position = pending_group.unwrap_or(position);
        let decimal_at = point.unwrap_or(digit_count).wrapping_sub(leading_fraction_zeros).wrapping_add(exponent);
        Some(ScannedDigits { digits, digit_count, decimal_at, length: position })
    }

    /// Builds the value as `DigitList.getBigDecimal`, then applies multiplier and sign.
    fn to_value(&self, scanned: &ScannedDigits, positive: bool) -> JResult<BigDecimal> {
        let mut value = scanned.to_big_decimal()?;
        if self.multiplier != 1 {
            let multiplier = BigDecimal::from_i64(i64::from(self.multiplier));
            value = match value.divide(&multiplier) {
                Ok(quotient) => quotient,
                Err(_) => value.divide_scale(&multiplier, value.scale(), RoundingMode::HalfEven)?,
            };
        }
        if !positive {
            value = BigDecimal::from_big_integer(&value.unscaled_value().negate(), value.scale());
        }
        Ok(value)
    }
}

/// What stands between prefix and suffix.
enum Body {
    Finite(ScannedDigits),
    Infinite { length: usize },
}

impl Body {
    fn length(&self) -> usize {
        match self {
            Body::Finite(digits) => digits.length,
            Body::Infinite { length } => *length,
        }
    }
}

/// The significant digits of a number and where its decimal point falls.
struct ScannedDigits {
    /// Digits without leading zeros.
    digits: String,
    digit_count: i32,
    /// The position of the decimal point relative to the start of `digits`.
    decimal_at: i32,
    /// Units of text consumed.
    length: usize,
}

impl ScannedDigits {
    fn to_big_decimal(&self) -> JResult<BigDecimal> {
        if self.digits.is_empty() {
            return if self.decimal_at == 0 {
                Ok(BigDecimal::zero())
            } else {
                BigDecimal::parse(&js(&format!("0E{}", self.decimal_at)))
            };
        }
        let unscaled = BigInteger::parse(&js(&self.digits), 10)?;
        let shift = self.decimal_at.wrapping_sub(self.digit_count);
        match i32::try_from(-i64::from(shift)) {
            Ok(scale) => Ok(BigDecimal::from_big_integer(&unscaled, scale)),
            Err(_) => Err(Thrown::new(ARITHMETIC_EXCEPTION, if shift < 0 { "Underflow" } else { "Overflow" })),
        }
    }
}

/// An exponent read after the exponent sign.
struct ScannedExponent {
    value: i32,
    length: usize,
}

/// Reads `-?digits`. Values beyond `long` are not exponents; others are truncated to `int`.
fn scan_exponent(text: &[u16]) -> Option<ScannedExponent> {
    let negative = text.first() == Some(&MINUS_SIGN);
    let sign_length = usize::from(negative);
    let digits: Vec<u32> =
        text.get(sign_length..).unwrap_or_default().iter().map_while(|unit| decimal_digit(*unit)).collect();
    if digits.is_empty() {
        return None;
    }
    let magnitude =
        digits.iter().try_fold(0_u64, |value, digit| value.checked_mul(10)?.checked_add(u64::from(*digit)))?;
    let limit = (i64::MAX as u64) + u64::from(negative);
    if magnitude > limit {
        return None;
    }
    let truncated = magnitude as i32;
    Some(ScannedExponent {
        value: if negative { truncated.wrapping_neg() } else { truncated },
        length: sign_length + digits.len(),
    })
}

/// Which of the still possible signs have their affix at the start of `text`; when both do, the
/// longer affix alone counts.
fn longest_match(
    (positive, negative): (bool, bool),
    (positive_affix, negative_affix): (&[u16], &[u16]),
    text: &[u16],
) -> (bool, bool) {
    let with_positive = positive && text.starts_with(positive_affix);
    let with_negative = negative && text.starts_with(negative_affix);
    if with_positive && with_negative && positive_affix.len() != negative_affix.len() {
        let positive_longer = positive_affix.len() > negative_affix.len();
        return (positive_longer, !positive_longer);
    }
    (with_positive, with_negative)
}

fn expand_affix(items: &[AffixItem], symbols: &NumberSymbols) -> JString {
    let mut text = JString::with_capacity(items.len());
    for item in items {
        match item {
            AffixItem::Literal(unit) => text.push(*unit),
            AffixItem::Minus => text.push(MINUS_SIGN),
            AffixItem::Percent => text.push(PERCENT_SIGN),
            AffixItem::PerMille => text.push(PER_MILLE_SIGN),
            AffixItem::Currency => text.extend(symbols.currency.encode_utf16()),
            AffixItem::InternationalCurrency => {
                text.extend(symbols.international_currency.encode_utf16());
            }
        }
    }
    text
}

#[cfg(test)]
mod tests;
