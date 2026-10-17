//! Text output: `toString`, `toEngineeringString`, `toPlainString`.

use super::{check_scale_truncating, BigDecimal};
use crate::java::{JResult, Thrown};
use std::fmt;

const OUT_OF_MEMORY: &str = "java.lang.OutOfMemoryError";
/// The longest string the JVM can allocate.
const MAX_STRING_LENGTH: u64 = 2_147_483_645;

/// Rejects lengths that make the JVM give up.
fn check_length(length: u64) -> JResult<()> {
    if length > i32::MAX as u64 {
        return Err(Thrown::new(OUT_OF_MEMORY, "too large to fit in a String"));
    }
    if length > MAX_STRING_LENGTH {
        return Err(Thrown::new(OUT_OF_MEMORY, "Requested array size exceeds VM limit"));
    }
    Ok(())
}

/// Digits with `scale` of them after a decimal point, padded with zeros on the left.
fn insert_point(digits: &str, scale: usize, out: &mut String) {
    match digits.len().checked_sub(scale) {
        Some(0) => {
            out.push_str("0.");
            out.push_str(digits);
        }
        Some(integer_digits) => {
            let (integer, fraction) = digits.split_at(integer_digits);
            out.push_str(integer);
            out.push('.');
            out.push_str(fraction);
        }
        None => {
            out.push_str("0.");
            out.extend(std::iter::repeat_n('0', scale - digits.len()));
            out.push_str(digits);
        }
    }
}

impl BigDecimal {
    /// The digits of the unscaled absolute value.
    fn coefficient(&self) -> String {
        self.unscaled.abs().to_string_radix(10)
    }

    /// `toEngineeringString()`.
    pub fn to_engineering_string(&self) -> String {
        self.layout(true)
    }

    /// `toPlainString()`.
    pub fn to_plain_string(&self) -> JResult<String> {
        if self.scale == 0 {
            return Ok(self.unscaled.to_string_radix(10));
        }
        let sign = if self.signum() < 0 { "-" } else { "" };
        let digits = self.coefficient();
        let sign_length = sign.len() as u64;
        let digit_length = digits.len() as u64;
        if self.scale < 0 {
            if self.is_zero() {
                return Ok("0".to_string());
            }
            let zeros = check_scale_truncating(-i64::from(self.scale))?.unsigned_abs();
            check_length(sign_length + digit_length + u64::from(zeros))?;
            let mut text = format!("{sign}{digits}");
            text.extend(std::iter::repeat_n('0', zeros as usize));
            return Ok(text);
        }
        let scale = self.scale.unsigned_abs();
        let integer_length = digit_length.saturating_sub(u64::from(scale)).max(1);
        check_length(sign_length + integer_length + 1 + u64::from(scale))?;
        let mut text = sign.to_string();
        insert_point(&digits, scale as usize, &mut text);
        Ok(text)
    }

    /// Scientific or engineering notation as specified by `toString`.
    fn layout(&self, engineering: bool) -> String {
        if self.scale == 0 {
            return self.unscaled.to_string_radix(10);
        }
        let digits = self.coefficient();
        let mut text = String::with_capacity(digits.len() + 16);
        if self.signum() < 0 {
            text.push('-');
        }
        let adjusted = digits.len() as i64 - 1 - i64::from(self.scale);
        if self.scale >= 0 && adjusted >= -6 {
            insert_point(&digits, self.scale.unsigned_abs() as usize, &mut text);
            return text;
        }
        let exponent = if engineering {
            self.engineering_mantissa(&digits, adjusted, &mut text)
        } else {
            let (first, rest) = digits.split_at(1);
            text.push_str(first);
            if !rest.is_empty() {
                text.push('.');
                text.push_str(rest);
            }
            adjusted
        };
        if exponent != 0 {
            text.push('E');
            if exponent > 0 {
                text.push('+');
            }
            text.push_str(&exponent.to_string());
        }
        text
    }

    /// Writes a mantissa with 1 to 3 integer digits; returns the matching exponent.
    fn engineering_mantissa(&self, digits: &str, adjusted: i64, text: &mut String) -> i64 {
        let excess = adjusted.rem_euclid(3);
        let exponent = adjusted - excess;
        let integer_digits = excess as usize + 1;
        if self.is_zero() {
            return match excess {
                0 => {
                    text.push_str(digits);
                    exponent
                }
                1 => {
                    text.push_str("0.00");
                    exponent + 3
                }
                _ => {
                    text.push_str("0.0");
                    exponent + 3
                }
            };
        }
        if integer_digits >= digits.len() {
            text.push_str(digits);
            text.extend(std::iter::repeat_n('0', integer_digits - digits.len()));
        } else {
            let (integer, fraction) = digits.split_at(integer_digits);
            text.push_str(integer);
            text.push('.');
            text.push_str(fraction);
        }
        exponent
    }
}

impl fmt::Display for BigDecimal {
    /// `toString()`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.layout(false))
    }
}
