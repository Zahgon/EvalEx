use super::*;
use crate::java::js;

const NUMBER_FORMAT: &str = "java.lang.NumberFormatException";

fn big(text: &str) -> BigInteger {
    BigInteger::parse(&js(text), 10).expect("valid literal")
}

fn parse_message(text: &str, radix: i32) -> String {
    let error = BigInteger::parse(&js(text), radix).expect_err("invalid literal");
    assert_eq!(error.class, NUMBER_FORMAT);
    String::from_utf16_lossy(&error.message.expect("a message"))
}

#[test]
fn parse_messages_match_the_jdk() {
    assert_eq!(parse_message("", 10), "Zero length BigInteger");
    assert_eq!(parse_message("-", 10), "Zero length BigInteger");
    assert_eq!(parse_message("1-", 10), "Illegal embedded sign character");
    assert_eq!(parse_message("+-1", 10), "Illegal embedded sign character");
    assert_eq!(parse_message("1", 99), "Radix out of range");
    assert_eq!(parse_message("12x", 10), "For input string: \"12x\"");
    assert_eq!(parse_message("xy", 16), "For input string: \"xy\" under radix 16");
}

#[test]
fn parse_reports_the_whole_digit_group() {
    assert_eq!(parse_message("1234567890123x56789012345", 10), "For input string: \"890123x56\"");
}

#[test]
fn parse_accepts_non_ascii_digits() {
    assert_eq!(BigInteger::parse(&js("١٢"), 10), Ok(big("12")));
    assert_eq!(BigInteger::parse(&js("ｚ"), 36), Ok(big("35")));
}

#[test]
fn zero_is_never_negative() {
    assert_eq!(big("-000"), BigInteger::zero());
    assert_eq!(big("-000").signum(), 0);
}

#[test]
fn arithmetic_crosses_limb_boundaries() {
    let top = big("18446744073709551615");
    assert_eq!(top.add(&big("1")).unwrap().to_string(), "18446744073709551616");
    assert_eq!(top.multiply(&top).unwrap().to_string(), "340282366920938463426481119284349108225");
    assert_eq!(big("-5").subtract(&big("-7")).unwrap().to_string(), "2");
}

#[test]
fn division_truncates_and_the_remainder_follows_the_dividend() {
    let (quotient, remainder) = big("-7").divide_and_remainder(&big("2")).unwrap();
    assert_eq!(quotient.to_string(), "-3");
    assert_eq!(remainder.to_string(), "-1");
    let error = big("1").divide_and_remainder(&BigInteger::zero());
    assert_eq!(error, Err(Thrown::new(ARITHMETIC, "BigInteger divide by zero")));
}

#[test]
fn knuth_division_handles_the_add_back_case() {
    let dividend = big("340282366920938463463374607431768211455");
    let divisor = big("18446744073709551616");
    let (quotient, remainder) = dividend.divide_and_remainder(&divisor).unwrap();
    assert_eq!(quotient.to_string(), "18446744073709551615");
    assert_eq!(remainder.to_string(), "18446744073709551615");
}

#[test]
fn karatsuba_agrees_with_powers() {
    let power = BigInteger::pow10(2000).unwrap();
    let square = power.multiply(&power).unwrap();
    assert_eq!(square, BigInteger::pow10(4000).unwrap());
    assert_eq!(square.to_string().len(), 4001);
}

#[test]
fn shifts_are_arithmetic_and_cover_int_extremes() {
    assert_eq!(big("-5").shift_right(1).unwrap().to_string(), "-3");
    assert_eq!(big("-1").shift_left(i32::MIN).unwrap().to_string(), "-1");
    assert_eq!(big("1").shift_left(i32::MIN).unwrap().to_string(), "0");
    assert_eq!(big("1").shift_right(i32::MIN), Err(overflow()));
    assert_eq!(big("1").shift_left(i32::MAX), Err(overflow()));
    assert_eq!(BigInteger::zero().shift_right(i32::MIN), Ok(BigInteger::zero()));
}

#[test]
fn the_largest_supported_shift_succeeds() {
    let value = big("1").shift_left(i32::MAX - 1).unwrap();
    assert_eq!(value.bit_length(), i32::MAX);
}

#[test]
fn pow_applies_the_jdk_estimate() {
    assert_eq!(big("10").pow(536_870_920), Err(overflow()));
    assert_eq!(big("3").pow(1_073_741_840), Err(overflow()));
    assert_eq!(big("4").pow(1_073_741_824), Err(overflow()));
    assert_eq!(big("-1").pow(i32::MAX), Ok(big("-1")));
    assert_eq!(big("2").pow(-1), Err(Thrown::new(ARITHMETIC, "Negative exponent")));
    assert_eq!(BigInteger::zero().pow(0), Ok(big("1")));
}

#[test]
fn bit_length_is_that_of_twos_complement() {
    assert_eq!(big("0").bit_length(), 0);
    assert_eq!(big("-1").bit_length(), 0);
    assert_eq!(big("-256").bit_length(), 8);
    assert_eq!(big("-257").bit_length(), 9);
    assert_eq!(big("256").bit_length(), 9);
}

#[test]
fn radix_output_is_lower_case_and_falls_back_to_ten() {
    assert_eq!(big("-255").to_string_radix(16), "-ff");
    assert_eq!(big("35").to_string_radix(36), "z");
    assert_eq!(big("1180591620717411303424").to_string_radix(99), "1180591620717411303424");
}

#[test]
fn narrowing_keeps_the_low_bits() {
    assert_eq!(big("18446744073709551617").long_value(), 1);
    assert_eq!(big("-18446744073709551617").long_value(), -1);
    assert_eq!(big("4294967298").int_value(), 2);
    assert_eq!(big("2147483648").int_value(), i32::MIN);
}

#[test]
fn double_value_rounds_half_even_with_sticky_bits() {
    assert_eq!(big("9007199254740993").double_value(), 9007199254740992.0);
    assert_eq!(big("9007199254740995").double_value(), 9007199254740996.0);
    assert_eq!(big("-1").shift_left(1024).unwrap().double_value(), f64::NEG_INFINITY);
    let near_max = big("1").shift_left(1024).unwrap().subtract(&big("1")).unwrap();
    assert_eq!(near_max.double_value(), f64::INFINITY);
}

#[test]
fn hash_code_folds_32_bit_words() {
    assert_eq!(big("5").hash_code(), 5);
    assert_eq!(big("-5").hash_code(), -5);
    assert_eq!(big("4294967296").hash_code(), 31);
}

#[test]
fn decimal_digit_counts_straddle_powers_of_ten() {
    assert_eq!(big("0").decimal_digits(), 1);
    assert_eq!(big("9999999999999999999").decimal_digits(), 19);
    assert_eq!(big("10000000000000000000").decimal_digits(), 20);
    assert_eq!(big("-99999999999999999999").decimal_digits(), 20);
    assert_eq!(big("100000000000000000000").decimal_digits(), 21);
}
