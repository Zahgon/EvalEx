//! `new BigDecimal(String)` and `new BigDecimal(String, MathContext)`.

use super::rounding::reduce_digits;
use super::{BigDecimal, MathContext};
use crate::java::biginteger::BigInteger;
use crate::java::chars::decimal_digit;
use crate::java::{js, JResult, JString, Thrown};

const NUMBER_FORMAT: &str = "java.lang.NumberFormatException";
/// Coefficients with at most this many characters take the JDK's `long` scanner, whose
/// message for an unexpected character differs from the general one.
const COMPACT_SCANNER_LENGTH: usize = 18;
/// An exponent can have this many digits after its leading zeros.
const MAX_EXPONENT_DIGITS: usize = 10;

const PLUS: u16 = b'+' as u16;
const MINUS: u16 = b'-' as u16;
const POINT: u16 = b'.' as u16;
const LOWER_E: u16 = b'e' as u16;
const UPPER_E: u16 = b'E' as u16;

fn format_error(message: &str) -> Thrown {
    Thrown::new(NUMBER_FORMAT, message)
}

/// The exception of running off the end of the input, which carries no message.
fn truncated() -> Thrown {
    Thrown::no_message(NUMBER_FORMAT)
}

fn unexpected_character(unit: u16, compact_scanner: bool) -> Thrown {
    if !compact_scanner {
        return format_error("Character array is missing \"e\" notation exponential mark.");
    }
    let mut message: JString = js("Character ");
    message.push(unit);
    message.extend(js(" is neither a decimal digit number, decimal point, nor \"e\" notation exponential mark."));
    Thrown::with_message(NUMBER_FORMAT, message)
}

/// Parses what follows the exponent mark.
fn parse_exponent(text: &[u16]) -> JResult<i64> {
    let (negative, digits) = match text.split_first() {
        None => return Err(truncated()),
        Some((&MINUS, rest)) => (true, rest),
        Some((&PLUS, rest)) => (false, rest),
        Some(_) => (false, text),
    };
    if digits.is_empty() {
        return Err(truncated());
    }
    let zeros_to_skip = digits
        .iter()
        .take(digits.len().saturating_sub(MAX_EXPONENT_DIGITS))
        .take_while(|unit| decimal_digit(**unit) == Some(0))
        .count();
    let digits = digits.get(zeros_to_skip..).unwrap_or_default();
    if digits.len() > MAX_EXPONENT_DIGITS {
        return Err(format_error("Too many nonzero exponent digits."));
    }
    let magnitude = digits.iter().try_fold(0i64, |value, unit| {
        decimal_digit(*unit).map(|digit| value * 10 + i64::from(digit)).ok_or_else(|| format_error("Not a digit."))
    })?;
    Ok(if negative { -magnitude } else { magnitude })
}

/// The pieces of a decimal literal.
struct Literal {
    /// Coefficient digits without leading zeros.
    digits: Vec<u8>,
    /// Digits seen after the point.
    fraction_digits: i64,
    exponent: i64,
}

/// Scans a coefficient and optional exponent; the sign is already removed.
fn scan(text: &[u16]) -> JResult<Literal> {
    let compact_scanner = text.len() <= COMPACT_SCANNER_LENGTH;
    let mut literal = Literal { digits: Vec::with_capacity(text.len()), fraction_digits: 0, exponent: 0 };
    let mut seen_digit = false;
    let mut seen_point = false;
    for (index, &unit) in text.iter().enumerate() {
        if let Some(digit) = decimal_digit(unit) {
            seen_digit = true;
            if digit != 0 || !literal.digits.is_empty() {
                literal.digits.push(digit as u8);
            }
            literal.fraction_digits += i64::from(seen_point);
        } else if unit == POINT {
            if seen_point {
                return Err(format_error("Character array contains more than one decimal point."));
            }
            seen_point = true;
        } else if unit == LOWER_E || unit == UPPER_E {
            let rest = text.get(index + 1..).unwrap_or_default();
            literal.exponent = parse_exponent(rest)?;
            break;
        } else {
            return Err(unexpected_character(unit, compact_scanner));
        }
    }
    if !seen_digit {
        return Err(format_error("No digits found."));
    }
    Ok(literal)
}

impl BigDecimal {
    /// `new BigDecimal(text)`.
    pub fn parse(text: &[u16]) -> JResult<BigDecimal> {
        BigDecimal::parse_mc(text, &MathContext { precision: 0, rounding: super::RoundingMode::HalfUp })
    }

    /// `new BigDecimal(text, mc)`.
    pub fn parse_mc(text: &[u16], mc: &MathContext) -> JResult<BigDecimal> {
        let (negative, body) = match text.split_first() {
            None => return Err(truncated()),
            Some((&MINUS, rest)) => (true, rest),
            Some((&PLUS, rest)) => (false, rest),
            Some(_) => (false, text),
        };
        let literal = scan(body)?;
        let scale = literal.fraction_digits - literal.exponent;
        let unscaled = BigInteger::from_decimal_digits(negative, &literal.digits)?;
        // The scale is only validated once rounding has moved it for the last time.
        let reduced = reduce_digits(&unscaled, scale, mc, Ok)?;
        let (unscaled, scale) = reduced.unwrap_or((unscaled, scale));
        let scale = i32::try_from(scale).map_err(|_| format_error("Exponent overflow."))?;
        Ok(BigDecimal::compact(unscaled, scale))
    }
}
