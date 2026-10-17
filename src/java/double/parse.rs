//! `Double.valueOf(String)` and `Float.valueOf(String)`.
//!
//! The value of the text is reduced to a binary mantissa, a sticky bit and an exponent, and
//! [`Format::round`] then rounds it once to the target format. This avoids the double rounding of
//! going through `f64` on the way to `f32`.

use super::super::{js, JResult, JString, Thrown};
use super::bigint::BigUint;
use crate::java::chars::trim;

const NUMBER_FORMAT_EXCEPTION: &str = "java.lang.NumberFormatException";

/// Significant decimal digits kept; everything beyond only acts as a sticky digit. Halfway points
/// between doubles have fewer digits than this.
const MAX_DIGITS: usize = 1100;
/// Decimal exponents from which on a non-zero `0.ddd * 10^e` overflows every format.
const OVERFLOW_POINT: i64 = 310;
/// Decimal exponents below which a non-zero `0.ddd * 10^e` underflows every format.
const UNDERFLOW_POINT: i64 = -330;
/// Decimal exponents with at least this magnitude when read as `int` make Java give up on them.
const EXPONENT_LIMIT: u64 = i32::MAX as u64 / 10;

/// Why text is not a number; the two cases have different exception messages.
enum Malformed {
    /// The digits of a decimal number contain a second decimal point.
    MultiplePoints,
    /// Anything else that does not follow the grammar.
    Syntax,
}

/// An IEEE binary format, addressed through bit patterns widened to `u64`.
struct Format {
    /// Mantissa bits including the implicit one.
    precision: u32,
    /// `2^min_exponent` is the smallest subnormal.
    min_exponent: i64,
    /// Every finite value is below `2^max_exponent`.
    max_exponent: i64,
    sign: u64,
    infinity: u64,
    nan: u64,
}

const DOUBLE: Format = Format {
    precision: 53,
    min_exponent: -1074,
    max_exponent: 1024,
    sign: 1 << 63,
    infinity: 0x7FF0_0000_0000_0000,
    nan: 0x7FF8_0000_0000_0000,
};

const FLOAT: Format = Format {
    precision: 24,
    min_exponent: -149,
    max_exponent: 128,
    sign: 1 << 31,
    infinity: 0x7F80_0000,
    nan: 0x7FC0_0000,
};

impl Format {
    /// The bits nearest to `mantissa * 2^exponent`, ties going to even. `sticky` says that the true
    /// value is slightly above that; it requires `mantissa` to have more bits than the format.
    fn round(&self, mantissa: u64, sticky: bool, exponent: i64) -> u64 {
        let top = exponent + i64::from(64 - mantissa.leading_zeros());
        if top > self.max_exponent {
            return self.infinity;
        }
        let lsb = (top - i64::from(self.precision)).max(self.min_exponent);
        let dropped = lsb - exponent;
        let rounded = if dropped <= 0 {
            mantissa << -dropped
        } else if dropped > 64 {
            return 0;
        } else {
            let wide = u128::from(mantissa);
            let kept = (wide >> dropped) as u64;
            let rest = wide & ((1 << dropped) - 1);
            let half = 1 << (dropped - 1);
            let up = rest > half || (rest == half && (sticky || kept & 1 == 1));
            kept + u64::from(up)
        };
        // The carry of a rounded-up mantissa runs into the exponent field, as it should.
        (((lsb - self.min_exponent) as u64) << (self.precision - 1)) + rounded
    }

    /// The bits nearest to `numerator / denominator`, both non-zero.
    fn round_ratio(&self, mut numerator: BigUint, mut denominator: BigUint) -> u64 {
        // Align the top bits, then long-divide: the first quotient bit has weight 2^offset.
        let offset = numerator.bit_length() as i64 - denominator.bit_length() as i64;
        if offset >= 0 {
            denominator.shl(offset as usize);
        } else {
            numerator.shl(offset.unsigned_abs() as usize);
        }
        // Two extra bits: the first quotient bit may be zero, and one more decides the rounding.
        let bits = self.precision + 2;
        let mut quotient = 0_u64;
        for _ in 0..bits {
            quotient <<= 1;
            if numerator >= denominator {
                numerator.sub_assign(&denominator);
                quotient |= 1;
            }
            numerator.shl(1);
        }
        self.round(quotient, !numerator.is_zero(), offset + 1 - i64::from(bits))
    }

    /// Bits of the decimal `0.digits * 10^point`, for non-empty `digits` without leading zeros.
    fn round_decimal(&self, digits: &[u8], point: i64) -> u64 {
        if point >= OVERFLOW_POINT {
            return self.infinity;
        }
        if point < UNDERFLOW_POINT {
            return 0;
        }
        let mut numerator = BigUint::from_digits(digits);
        let mut denominator = BigUint::from_u64(1);
        let scale = point - digits.len() as i64;
        if scale >= 0 {
            numerator.mul_pow10(scale as u32);
        } else {
            denominator.mul_pow10(scale.unsigned_abs() as u32);
        }
        self.round_ratio(numerator, denominator)
    }
}

/// `Double.valueOf(text)`.
pub fn parse_double(text: &[u16]) -> JResult<f64> {
    parse(text, &DOUBLE).map(f64::from_bits)
}

/// `Float.valueOf(text)`.
pub fn parse_float(text: &[u16]) -> JResult<f32> {
    parse(text, &FLOAT).map(|bits| f32::from_bits(bits as u32))
}

fn parse(text: &[u16], format: &Format) -> JResult<u64> {
    let trimmed = trim(text);
    if trimmed.is_empty() {
        return Err(Thrown::new(NUMBER_FORMAT_EXCEPTION, "empty String"));
    }
    // Every character of the grammar is ASCII, so anything else may collapse into NUL.
    let bytes: Vec<u8> = trimmed.iter().map(|&unit| u8::try_from(unit).unwrap_or(0)).collect();
    parse_number(&bytes, format).map_err(|malformed| match malformed {
        Malformed::MultiplePoints => Thrown::new(NUMBER_FORMAT_EXCEPTION, "multiple points"),
        Malformed::Syntax => invalid_input(trimmed),
    })
}

fn invalid_input(trimmed: &[u16]) -> Thrown {
    let mut message: JString = js("For input string: \"");
    message.extend_from_slice(trimmed);
    message.extend(js("\""));
    Thrown::with_message(NUMBER_FORMAT_EXCEPTION, message)
}

/// Parses trimmed text.
fn parse_number(bytes: &[u8], format: &Format) -> Result<u64, Malformed> {
    let (negative, rest) = match bytes {
        [b'-', rest @ ..] => (true, rest),
        [b'+', rest @ ..] => (false, rest),
        _ => (false, bytes),
    };
    let magnitude = match rest {
        b"NaN" => return Ok(format.nan),
        b"Infinity" => format.infinity,
        [b'0', b'x' | b'X', hex @ ..] => parse_hex(hex, format)?,
        _ => parse_decimal(rest, format)?,
    };
    Ok(if negative { magnitude | format.sign } else { magnitude })
}

/// Splits a leading run of bytes satisfying `accept` from `bytes`.
fn take_while(bytes: &[u8], accept: fn(&u8) -> bool) -> (&[u8], &[u8]) {
    let length = bytes.iter().position(|b| !accept(b));
    bytes.split_at(length.unwrap_or(bytes.len()))
}

/// Accepts what may follow the last digit of a number: nothing, or one type suffix.
fn at_end(bytes: &[u8]) -> bool {
    matches!(bytes, [] | [b'f' | b'F' | b'd' | b'D'])
}

/// Removes an optional exponent sign, returning whether the exponent is negative.
fn split_exponent_sign(bytes: &[u8]) -> (bool, &[u8]) {
    match bytes {
        [b'-', rest @ ..] => (true, rest),
        [b'+', rest @ ..] => (false, rest),
        _ => (false, bytes),
    }
}

/// Splits `digits[.digits]`, where `.digits` may also come without an integer part.
fn split_significand(bytes: &[u8], is_digit: fn(&u8) -> bool) -> (&[u8], &[u8], &[u8]) {
    let (integer, rest) = take_while(bytes, is_digit);
    let (fraction, rest) = match rest {
        [b'.', after_point @ ..] => take_while(after_point, is_digit),
        _ => (&rest[..0], rest),
    };
    (integer, fraction, rest)
}

fn parse_decimal(bytes: &[u8], format: &Format) -> Result<u64, Malformed> {
    let (integer, fraction, rest) = split_significand(bytes, u8::is_ascii_digit);
    if rest.first() == Some(&b'.') {
        return Err(Malformed::MultiplePoints);
    }
    if integer.is_empty() && fraction.is_empty() {
        return Err(Malformed::Syntax);
    }

    let (exponent, rest) = match rest {
        [b'e' | b'E', after_e @ ..] => {
            let (negative, unsigned) = split_exponent_sign(after_e);
            let (digits, rest) = take_while(unsigned, u8::is_ascii_digit);
            if digits.is_empty() {
                return Err(Malformed::Syntax);
            }
            (Some((negative, digits)), rest)
        }
        _ => (None, rest),
    };
    if !at_end(rest) {
        return Err(Malformed::Syntax);
    }

    // Collect the significant digits; `point` positions the decimal point before the first one.
    let mut digits = Vec::new();
    let mut point = integer.len() as i64;
    let mut sticky = false;
    for &digit in integer.iter().chain(fraction) {
        if digits.is_empty() && digit == b'0' {
            point -= 1;
        } else if digits.len() < MAX_DIGITS {
            digits.push(digit);
        } else {
            sticky |= digit != b'0';
        }
    }
    if sticky {
        digits.push(b'1');
    }
    while digits.last() == Some(&b'0') {
        digits.pop();
    }
    if digits.is_empty() {
        return Ok(0);
    }

    if let Some((negative, exponent_digits)) = exponent {
        let mut value = 0_u64;
        for digit in exponent_digits {
            if value >= EXPONENT_LIMIT {
                return Ok(if negative { 0 } else { format.infinity });
            }
            value = value * 10 + u64::from(digit - b'0');
        }
        point += if negative { -(value as i64) } else { value as i64 };
    }
    Ok(format.round_decimal(&digits, point))
}

fn parse_hex(bytes: &[u8], format: &Format) -> Result<u64, Malformed> {
    let (integer, fraction, rest) = split_significand(bytes, u8::is_ascii_hexdigit);
    if integer.is_empty() && fraction.is_empty() {
        return Err(Malformed::Syntax);
    }
    let [b'p' | b'P', after_p @ ..] = rest else {
        return Err(Malformed::Syntax);
    };
    let (exponent_negative, unsigned) = split_exponent_sign(after_p);
    let (exponent_digits, rest) = take_while(unsigned, u8::is_ascii_digit);
    if exponent_digits.is_empty() || !at_end(rest) {
        return Err(Malformed::Syntax);
    }

    // Keep the first 64 significant bits; the rest only sets the sticky flag.
    let mut mantissa = 0_u64;
    let mut sticky = false;
    let mut dropped_digits = 0_i64;
    for &digit in integer.iter().chain(fraction) {
        let value = hex_value(digit);
        if mantissa >> 60 == 0 {
            mantissa = mantissa << 4 | value;
        } else {
            dropped_digits += 1;
            sticky |= value != 0;
        }
    }
    if mantissa == 0 {
        return Ok(0);
    }

    // Java reads the exponent as an `int` and treats anything bigger as out of range.
    let exponent = exponent_digits
        .iter()
        .try_fold(0_i32, |value, digit| value.checked_mul(10)?.checked_add(i32::from(digit - b'0')));
    let Some(exponent) = exponent else {
        return Ok(if exponent_negative { 0 } else { format.infinity });
    };
    let exponent = if exponent_negative { -i64::from(exponent) } else { i64::from(exponent) };
    let scale = 4 * (dropped_digits - fraction.len() as i64);
    Ok(format.round(mantissa, sticky, exponent + scale))
}

fn hex_value(digit: u8) -> u64 {
    u64::from(char::from(digit).to_digit(16).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn double(text: &str) -> JResult<f64> {
        parse_double(&js(text))
    }

    fn float(text: &str) -> JResult<f32> {
        parse_float(&js(text))
    }

    fn invalid(text: &str) -> Thrown {
        Thrown::new(NUMBER_FORMAT_EXCEPTION, &format!("For input string: \"{text}\""))
    }

    #[test]
    fn decimals_are_correctly_rounded() {
        assert_eq!(double("0.1"), Ok(0.1));
        assert_eq!(double("1e23"), Ok(1e23));
        assert_eq!(double("2.2250738585072012e-308"), Ok(f64::MIN_POSITIVE));
        assert_eq!(double("4.9e-324"), Ok(f64::from_bits(1)));
        assert_eq!(double("2.4703282292062328e-324"), Ok(f64::from_bits(1)));
        assert_eq!(double("2.4703282292062327e-324"), Ok(0.0));
        assert_eq!(double("1.7976931348623159e308"), Ok(f64::INFINITY));
        assert_eq!(double("9007199254740993"), Ok(9007199254740992.0));
        assert_eq!(double("9007199254740993.000000000000001"), Ok(9007199254740994.0));
    }

    #[test]
    fn floats_are_rounded_directly_from_the_text() {
        assert_eq!(float("0.1"), Ok(0.1));
        assert_eq!(float("1.17549435E-38"), Ok(f32::MIN_POSITIVE));
        assert_eq!(float("1.4e-45"), Ok(f32::from_bits(1)));
        assert_eq!(float("3.40282356e38"), Ok(f32::MAX));
        assert_eq!(float("3.40282357e38"), Ok(f32::INFINITY));
        assert_eq!(float("1e39"), Ok(f32::INFINITY));
        assert_eq!(float("1.00000017881393432617187499"), Ok(f32::from_bits(0x3F80_0001)));
    }

    #[test]
    fn surroundings_of_the_digits_are_accepted() {
        assert_eq!(double(" \t\n1.5\u{1}"), Ok(1.5));
        assert_eq!(double("+1.5d"), Ok(1.5));
        assert_eq!(double("-1.5F"), Ok(-1.5));
        assert_eq!(double("1."), Ok(1.0));
        assert_eq!(double(".5"), Ok(0.5));
        assert_eq!(double("1E+2"), Ok(100.0));
        assert_eq!(double("0e999999999999"), Ok(0.0));
        assert_eq!(double("1e999999999999"), Ok(f64::INFINITY));
        assert_eq!(double("1e-999999999999"), Ok(0.0));
    }

    #[test]
    fn special_values_ignore_or_keep_the_sign() {
        assert_eq!(double("Infinity"), Ok(f64::INFINITY));
        assert_eq!(double("-Infinity"), Ok(f64::NEG_INFINITY));
        assert_eq!(double("-NaN").map(f64::to_bits), Ok(0x7FF8_0000_0000_0000));
        assert_eq!(double("-0").map(f64::to_bits), Ok(0x8000_0000_0000_0000));
        assert_eq!(float("-0x0p0").map(f32::to_bits), Ok(0x8000_0000));
    }

    #[test]
    fn hexadecimal_floats_round_like_decimals() {
        assert_eq!(double("0x1p0"), Ok(1.0));
        assert_eq!(double("0X1.8P1D"), Ok(3.0));
        assert_eq!(double("0x.8p1f"), Ok(1.0));
        assert_eq!(double("0x1.p-1074"), Ok(f64::from_bits(1)));
        assert_eq!(double("0x1p-1075"), Ok(0.0));
        assert_eq!(double("0x1.8p-1074"), Ok(f64::from_bits(2)));
        assert_eq!(double("0x1.00000000000008p0"), Ok(1.0));
        assert_eq!(double("0x1.000000000000080000000000000000001p0"), Ok(f64::from_bits(0x3FF0_0000_0000_0001)));
        assert_eq!(double("0x1p99999999999"), Ok(f64::INFINITY));
        assert_eq!(double("0x1p-99999999999"), Ok(0.0));
        assert_eq!(double("0x0p99999999999"), Ok(0.0));
        assert_eq!(float("0x1.000001p0"), Ok(1.0));
    }

    #[test]
    fn blank_text_reports_an_empty_string() {
        let expected = Thrown::new(NUMBER_FORMAT_EXCEPTION, "empty String");
        assert_eq!(double(""), Err(expected.clone()));
        assert_eq!(double(" \t "), Err(expected));
    }

    #[test]
    fn malformed_text_is_quoted_after_trimming() {
        for text in [
            "x",
            ".",
            "e5",
            "1e",
            "1e+",
            "1 2",
            "+-1",
            "1ff",
            "0x",
            "0x1",
            "0x.p1",
            "0x1p",
            "NaNf",
            "nan",
            "infinity",
            "Infinityd",
            "1_0",
            "١",
        ] {
            assert_eq!(double(text), Err(invalid(text)));
        }
        assert_eq!(double("  abc  "), Err(invalid("abc")));
    }

    #[test]
    fn a_second_decimal_point_has_its_own_message() {
        let expected = Thrown::new(NUMBER_FORMAT_EXCEPTION, "multiple points");
        for text in ["1.2.3", "..", " -.0.1x ", "1..e5"] {
            assert_eq!(double(text), Err(expected.clone()));
        }
        assert_eq!(double("1.2e3.4"), Err(invalid("1.2e3.4")));
        assert_eq!(double("0x1.2.3p0"), Err(invalid("0x1.2.3p0")));
    }

    #[test]
    fn lone_surrogates_survive_in_the_message() {
        let error = parse_double(&[0x31, 0xD800]).unwrap_err();
        let mut expected = js("For input string: \"1");
        expected.push(0xD800);
        expected.extend(js("\""));
        assert_eq!(error.message, Some(expected));
    }
}
