//! Text conversion of magnitudes: `new BigInteger(String, radix)` and `toString(radix)`.

use super::limbs;
use crate::java::chars::digit;
use crate::java::{js, JResult, JString, Thrown};

const NUMBER_FORMAT: &str = "java.lang.NumberFormatException";
const MIN_RADIX: i32 = 2;
const MAX_RADIX: i32 = 36;
/// Magnitudes up to this many limbs are converted to text limb by limb.
const DIRECT_FORMAT_LIMBS: usize = 16;

/// Digits per group the JDK hands to `Integer.parseInt`, indexed by radix.
const JAVA_GROUP_DIGITS: [usize; 37] = [
    0, 0, 30, 19, 15, 13, 11, 11, 10, 9, 9, 8, 8, 8, 8, 7, 7, 7, 7, 7, 7, 7, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6,
    5,
];

/// The sign and magnitude denoted by a Java string.
pub(super) struct Parsed {
    pub(super) negative: bool,
    pub(super) magnitude: Vec<u64>,
}

/// The largest power of `radix` that fits a limb, with its exponent.
fn limb_power(radix: u32) -> (u64, usize) {
    let radix = u64::from(radix);
    let mut power = radix;
    let mut digits = 1;
    while let Some(next) = power.checked_mul(radix) {
        power = next;
        digits += 1;
    }
    (power, digits)
}

/// Finds the sign prefix; returns whether it is minus and the digits after it.
fn split_sign(text: &[u16]) -> JResult<(bool, &[u16])> {
    let minus = text.iter().rposition(|unit| *unit == u16::from(b'-'));
    let plus = text.iter().rposition(|unit| *unit == u16::from(b'+'));
    let embedded = || Thrown::new(NUMBER_FORMAT, "Illegal embedded sign character");
    match (minus, plus) {
        (Some(0), None) => Ok((true, text.get(1..).unwrap_or_default())),
        (None, Some(0)) => Ok((false, text.get(1..).unwrap_or_default())),
        (None, None) => Ok((false, text)),
        _ => Err(embedded()),
    }
}

/// Reads the digits group by group so that the first bad group is the one reported.
fn digit_values(digits: &[u16], radix: u32) -> JResult<Vec<u8>> {
    let group_digits = JAVA_GROUP_DIGITS.get(radix as usize).copied().unwrap_or(1).max(1);
    let first_group = match digits.len() % group_digits {
        0 => group_digits,
        partial => partial,
    };
    let (head, tail) = digits.split_at(first_group.min(digits.len()));
    let mut values = Vec::with_capacity(digits.len());
    for group in std::iter::once(head).chain(tail.chunks(group_digits)) {
        for unit in group {
            let value = digit(*unit, radix).ok_or_else(|| bad_group(group, radix))?;
            values.push(value as u8);
        }
    }
    Ok(values)
}

/// The `NumberFormatException` of `Integer.parseInt(group, radix)`.
fn bad_group(group: &[u16], radix: u32) -> Thrown {
    let mut message: JString = js("For input string: \"");
    message.extend_from_slice(group);
    message.extend(js("\""));
    if radix != 10 {
        message.extend(js(&format!(" under radix {radix}")));
    }
    Thrown::with_message(NUMBER_FORMAT, message)
}

/// Accumulates digit values into a magnitude.
pub(super) fn magnitude_of_digits(values: &[u8], radix: u32) -> Vec<u64> {
    let (power, digits_per_limb) = limb_power(radix);
    let mut magnitude = Vec::with_capacity(values.len() / digits_per_limb + 1);
    let first_chunk = match values.len() % digits_per_limb {
        0 => digits_per_limb,
        partial => partial,
    };
    let (head, tail) = values.split_at(first_chunk.min(values.len()));
    for chunk in std::iter::once(head).chain(tail.chunks(digits_per_limb)) {
        let value = chunk.iter().fold(0u64, |acc, value| acc * u64::from(radix) + u64::from(*value));
        limbs::mul_add_small(&mut magnitude, power, value);
    }
    limbs::normalize(&mut magnitude);
    magnitude
}

/// Parses like `new BigInteger(text, radix)`.
pub(super) fn parse(text: &[u16], radix: i32) -> JResult<Parsed> {
    if !(MIN_RADIX..=MAX_RADIX).contains(&radix) {
        return Err(Thrown::new(NUMBER_FORMAT, "Radix out of range"));
    }
    let radix = radix.unsigned_abs();
    let zero_length = || Thrown::new(NUMBER_FORMAT, "Zero length BigInteger");
    if text.is_empty() {
        return Err(zero_length());
    }
    let (negative, digits) = split_sign(text)?;
    if digits.is_empty() {
        return Err(zero_length());
    }
    let significant = digits.iter().position(|unit| digit(*unit, radix) != Some(0)).unwrap_or(digits.len());
    let digits = digits.get(significant..).unwrap_or_default();
    if digits.is_empty() {
        return Ok(Parsed { negative: false, magnitude: Vec::new() });
    }
    let values = digit_values(digits, radix)?;
    Ok(Parsed { negative, magnitude: magnitude_of_digits(&values, radix) })
}

/// The digits of a magnitude in `radix`, most significant first, without a sign.
pub(super) fn format(magnitude: &[u64], radix: u32) -> String {
    if magnitude.is_empty() {
        return "0".to_string();
    }
    let (power, digits_per_limb) = limb_power(radix);
    let mut powers = vec![vec![power]];
    while powers.last().is_some_and(|top| top.len() * 2 <= magnitude.len()) {
        let next = powers.last().map(|top| limbs::mul(top, top));
        powers.extend(next);
    }
    let mut text = Vec::with_capacity(magnitude.len() * 20);
    write_digits(magnitude, radix, &powers, digits_per_limb, 0, &mut text);
    let start = text.iter().position(|byte| *byte != b'0').unwrap_or(0);
    text.drain(..start);
    String::from_utf8_lossy(&text).into_owned()
}

/// Appends `magnitude` padded to `width` digits (no padding when `width` is 0).
///
/// `powers[k]` is the limb power of the radix squared `k` times, so each level halves the
/// operand and the recursion depth is logarithmic.
fn write_digits(
    magnitude: &[u64],
    radix: u32,
    powers: &[Vec<u64>],
    digits_per_limb: usize,
    width: usize,
    text: &mut Vec<u8>,
) {
    if let Some((divisor, smaller)) = powers.split_last() {
        if magnitude.len() > DIRECT_FORMAT_LIMBS && magnitude.len() >= divisor.len() {
            let low_width = digits_per_limb << smaller.len();
            let (high, low) = limbs::div_rem(magnitude, divisor);
            let high_width = width.saturating_sub(low_width);
            write_digits(&high, radix, powers, digits_per_limb, high_width, text);
            write_digits(&low, radix, smaller, digits_per_limb, low_width, text);
            return;
        }
        if !smaller.is_empty() {
            write_digits(magnitude, radix, smaller, digits_per_limb, width, text);
            return;
        }
    }
    write_direct(magnitude, radix, digits_per_limb, width, text);
}

/// Appends a small magnitude by peeling one limb power at a time.
fn write_direct(magnitude: &[u64], radix: u32, digits_per_limb: usize, width: usize, text: &mut Vec<u8>) {
    let (power, _) = limb_power(radix);
    let mut rest = magnitude.to_vec();
    let mut reversed = Vec::with_capacity(rest.len() * digits_per_limb + digits_per_limb);
    while !rest.is_empty() {
        let mut chunk = limbs::div_small(&mut rest, power);
        for _ in 0..digits_per_limb {
            let value = (chunk % u64::from(radix)) as u32;
            reversed.push(char::from_digit(value, radix).map_or(b'0', |c| c as u8));
            chunk /= u64::from(radix);
        }
    }
    reversed.resize(reversed.len().max(width), b'0');
    text.extend(reversed.iter().rev());
}
