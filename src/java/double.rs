//! `java.lang.Double` and `java.lang.Float` conversions, plus the angle conversions of `Math`.
//!
//! Formatting follows the JDK 19+ specification of `Double.toString`: the shortest decimal that
//! rounds to the value, with at least two digits considered. Both formatting and parsing use exact
//! integer arithmetic, which keeps them short and correct at the cost of some speed.

mod bigint;
mod parse;

use bigint::BigUint;
pub use parse::{parse_double, parse_float};
use std::cmp::Ordering;
use std::num::FpCategory;

/// Degrees per radian as `Math.toDegrees` uses it.
const RADIANS_TO_DEGREES: f64 = 57.29577951308232;
/// Radians per degree as `Math.toRadians` uses it.
const DEGREES_TO_RADIANS: f64 = 0.017453292519943295;

/// A finite positive value `mantissa * 2^exponent` and the shape of its rounding interval.
struct Binary {
    mantissa: u64,
    exponent: i32,
    /// The value is a power of two, so the next lower value is only half as far away.
    narrow_below: bool,
}

/// The decimal `coefficient * 10^exponent`.
struct Decimal {
    coefficient: u64,
    exponent: i32,
}

/// `Double.toString(value)`.
pub fn to_string(value: f64) -> String {
    const MANTISSA_BITS: u32 = 52;
    let bits = value.to_bits();
    let biased = ((bits >> MANTISSA_BITS) & 0x7FF) as i32;
    let fraction = bits & ((1 << MANTISSA_BITS) - 1);
    let binary = decode(biased, fraction, MANTISSA_BITS, -1074);
    render(value.classify(), value.is_sign_negative(), &binary)
}

/// `Float.toString(value)`.
pub fn to_string_f32(value: f32) -> String {
    const MANTISSA_BITS: u32 = 23;
    let bits = value.to_bits();
    let biased = ((bits >> MANTISSA_BITS) & 0xFF) as i32;
    let fraction = u64::from(bits & ((1 << MANTISSA_BITS) - 1));
    let binary = decode(biased, fraction, MANTISSA_BITS, -149);
    render(value.classify(), value.is_sign_negative(), &binary)
}

/// `Math.toRadians(degrees)`.
pub fn to_radians(degrees: f64) -> f64 {
    degrees * DEGREES_TO_RADIANS
}

/// `Math.toDegrees(radians)`.
pub fn to_degrees(radians: f64) -> f64 {
    radians * RADIANS_TO_DEGREES
}

/// `Double.hashCode(value)`: all NaNs hash alike.
pub fn hash_code(value: f64) -> i32 {
    let bits = if value.is_nan() { f64::NAN.to_bits() } else { value.to_bits() };
    (bits ^ (bits >> 32)) as i32
}

/// Splits the fields of an IEEE format whose smallest subnormal is `2^min_exponent`.
fn decode(biased: i32, fraction: u64, mantissa_bits: u32, min_exponent: i32) -> Binary {
    if biased == 0 {
        Binary { mantissa: fraction, exponent: min_exponent, narrow_below: false }
    } else {
        Binary {
            mantissa: fraction | (1 << mantissa_bits),
            exponent: min_exponent + biased - 1,
            narrow_below: fraction == 0 && biased > 1,
        }
    }
}

/// The common shape of `toString`: the special values, then sign and digits.
fn render(category: FpCategory, negative: bool, binary: &Binary) -> String {
    let sign = if negative { "-" } else { "" };
    match category {
        FpCategory::Nan => "NaN".to_string(),
        FpCategory::Infinite => format!("{sign}Infinity"),
        FpCategory::Zero => format!("{sign}0.0"),
        FpCategory::Subnormal | FpCategory::Normal => {
            format!("{sign}{}", format_decimal(&shortest_decimal(binary)))
        }
    }
}

/// Picks the decimal the `toString` specification selects: among those of the smallest length of
/// at least two that round to the value, the closest, breaking ties towards an even last digit.
///
/// All quantities are integers over the common denominator `denominator`: the remaining value, and
/// the distances from the value to the ends of its rounding interval. Digit generation stops as
/// soon as one of the two decimals around the value lies inside that interval.
fn shortest_decimal(binary: &Binary) -> Decimal {
    // A value with an even mantissa wins round-half-even ties, so its interval is closed.
    let closed_interval = binary.mantissa & 1 == 0;
    let exponent = binary.exponent;
    let up_shift = exponent.max(0) as usize;
    let down_shift = (-exponent).max(0) as usize;

    let mut remainder = BigUint::from_u64(binary.mantissa);
    remainder.shl(up_shift + 2);
    let mut denominator = BigUint::from_u64(1);
    denominator.shl(down_shift + 2);
    let mut above = BigUint::from_u64(1);
    above.shl(up_shift + 1);
    let mut below = BigUint::from_u64(1);
    below.shl(up_shift + usize::from(!binary.narrow_below));

    // Find `decimal_point` with 10^(decimal_point - 1) <= value < 10^decimal_point. The estimate
    // floor((bits - 1) * log10(2)) + 1 is low by at most one.
    let top_bit = 63 - binary.mantissa.leading_zeros() as i32 + exponent;
    let mut decimal_point = ((top_bit * 78913) >> 18) + 1;
    if decimal_point >= 0 {
        denominator.mul_pow10(decimal_point.unsigned_abs());
    } else {
        let scale = decimal_point.unsigned_abs();
        remainder.mul_pow10(scale);
        above.mul_pow10(scale);
        below.mul_pow10(scale);
    }
    if remainder >= denominator {
        denominator.mul_pow10(1);
        decimal_point += 1;
    }

    let mut coefficient = 0_u64;
    let mut length = 0;
    loop {
        remainder.mul_small(10);
        above.mul_small(10);
        below.mul_small(10);
        let mut digit = 0;
        while remainder >= denominator {
            remainder.sub_assign(&denominator);
            digit += 1;
        }
        coefficient = coefficient * 10 + digit;
        length += 1;
        if length < 2 {
            continue;
        }

        let inside = |distance: &BigUint, limit: &BigUint| match distance.cmp(limit) {
            Ordering::Less => true,
            Ordering::Equal => closed_interval,
            Ordering::Greater => false,
        };
        let mut to_high = denominator.clone();
        to_high.sub_assign(&remainder);
        let low_inside = inside(&remainder, &below);
        let high_inside = inside(&to_high, &above);
        if low_inside || high_inside {
            let round_up = match (low_inside, high_inside) {
                (true, false) => false,
                (false, true) => true,
                _ => match to_high.cmp(&remainder) {
                    Ordering::Less => true,
                    Ordering::Equal => coefficient & 1 == 1,
                    Ordering::Greater => false,
                },
            };
            return Decimal { coefficient: coefficient + u64::from(round_up), exponent: decimal_point - length };
        }
    }
}

/// Lays out a non-zero decimal in the plain or the computerized scientific form of `toString`.
fn format_decimal(decimal: &Decimal) -> String {
    let digits = decimal.coefficient.to_string();
    let significant = digits.trim_end_matches('0');
    // `scientific` is the power of ten of the first digit.
    let scientific = digits.len() as i32 - 1 + decimal.exponent;
    let (first, rest) = significant.split_at(1);

    match scientific {
        0..=6 => {
            let integer_digits = scientific as usize + 1;
            if significant.len() <= integer_digits {
                let padding = integer_digits - significant.len();
                format!("{significant}{}.0", "0".repeat(padding))
            } else {
                let (integer, fraction) = significant.split_at(integer_digits);
                format!("{integer}.{fraction}")
            }
        }
        -3..=-1 => {
            let padding = (-scientific - 1) as usize;
            format!("0.{}{significant}", "0".repeat(padding))
        }
        _ if rest.is_empty() => format!("{first}.0E{scientific}"),
        _ => format!("{first}.{rest}E{scientific}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_string_uses_plain_form_between_the_thresholds() {
        assert_eq!(to_string(123456.0), "123456.0");
        assert_eq!(to_string(1.0), "1.0");
        assert_eq!(to_string(0.1), "0.1");
        assert_eq!(to_string(0.001), "0.001");
        assert_eq!(to_string(9999999.0), "9999999.0");
        assert_eq!(to_string(1234.5), "1234.5");
    }

    #[test]
    fn to_string_uses_scientific_form_outside_the_thresholds() {
        assert_eq!(to_string(1.0e7), "1.0E7");
        assert_eq!(to_string(1.0e10), "1.0E10");
        assert_eq!(to_string(9.999e-4), "9.999E-4");
        assert_eq!(to_string(1.5e300), "1.5E300");
    }

    #[test]
    fn to_string_applies_the_two_digit_rule() {
        assert_eq!(to_string(f64::from_bits(1)), "4.9E-324");
        assert_eq!(to_string(f64::from_bits(4)), "2.0E-323");
        assert_eq!(to_string(1.0e23), "1.0E23");
        assert_eq!(to_string(f64::MAX), "1.7976931348623157E308");
        assert_eq!(to_string(f64::MIN_POSITIVE), "2.2250738585072014E-308");
    }

    #[test]
    fn to_string_handles_special_values() {
        assert_eq!(to_string(f64::NAN), "NaN");
        assert_eq!(to_string(f64::INFINITY), "Infinity");
        assert_eq!(to_string(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(to_string(0.0), "0.0");
        assert_eq!(to_string(-0.0), "-0.0");
        assert_eq!(to_string(-2.5), "-2.5");
    }

    #[test]
    fn to_string_f32_is_shortest_for_the_float_format() {
        assert_eq!(to_string_f32(0.1), "0.1");
        assert_eq!(to_string_f32(1.0e10), "1.0E10");
        assert_eq!(to_string_f32(f32::from_bits(1)), "1.4E-45");
        assert_eq!(to_string_f32(f32::MAX), "3.4028235E38");
        assert_eq!(to_string_f32(-0.0), "-0.0");
        assert_eq!(to_string_f32(f32::NAN), "NaN");
    }

    #[test]
    fn angle_conversions_multiply_by_the_jdk_factor() {
        assert_eq!(to_radians(180.0).to_bits(), 0x4009_21FB_5444_2D18);
        assert_eq!(to_degrees(1.0), 57.29577951308232);
        assert!(to_radians(f64::NAN).is_nan());
    }

    #[test]
    fn hash_code_folds_the_bits() {
        assert_eq!(hash_code(0.0), 0);
        assert_eq!(hash_code(-0.0), -2147483648);
        assert_eq!(hash_code(1.0), 1072693248);
        assert_eq!(hash_code(f64::from_bits(0x7FF0_0000_0000_0001)), 2146959360);
    }
}
