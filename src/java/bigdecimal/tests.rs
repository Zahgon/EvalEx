use super::*;
use crate::java::js;

const NUMBER_FORMAT: &str = "java.lang.NumberFormatException";

fn dec(text: &str) -> BigDecimal {
    BigDecimal::parse(&js(text)).expect("valid literal")
}

fn mc(precision: i32, rounding: RoundingMode) -> MathContext {
    MathContext::new(precision, rounding).expect("valid precision")
}

fn arithmetic(message: &str) -> Thrown {
    Thrown::new(ARITHMETIC, message)
}

fn format_error(text: &str, message: &str) {
    let error = BigDecimal::parse(&js(text)).expect_err("invalid literal");
    assert_eq!(error, Thrown::new(NUMBER_FORMAT, message), "{text}");
}

fn text(result: JResult<BigDecimal>) -> String {
    result.expect("a value").to_string()
}

#[test]
fn parse_errors_without_message() {
    for literal in ["", "e", "1e", "1e+", "1e-", "1234567890123456789e"] {
        let error = BigDecimal::parse(&js(literal)).expect_err("invalid literal");
        assert_eq!(error, Thrown::no_message(NUMBER_FORMAT), "{literal}");
    }
}

#[test]
fn parse_error_messages() {
    format_error("-", "No digits found.");
    format_error(".", "No digits found.");
    format_error("1e1x", "Not a digit.");
    format_error("1e--1", "Not a digit.");
    format_error("1e99999999999", "Too many nonzero exponent digits.");
    format_error("1e2147483649", "Exponent overflow.");
    format_error("0.1e-2147483647", "Exponent overflow.");
    format_error("12345678901234567.89.1", "Character array contains more than one decimal point.");
    format_error(
        "NaN",
        "Character N is neither a decimal digit number, decimal point, nor \"e\" notation exponential mark.",
    );
    format_error("1234567890123456789x", "Character array is missing \"e\" notation exponential mark.");
}

#[test]
fn parse_exponent_limits() {
    assert_eq!(dec("1e2147483648").scale(), i32::MIN);
    assert_eq!(dec("10e2147483648").scale(), i32::MIN);
    assert_eq!(dec("1e-2147483647").scale(), i32::MAX);
    assert_eq!(dec("1e00000000000005").to_string(), "1E+5");
}

#[test]
fn parse_rounding_can_bring_the_scale_back_in_range() {
    let literal = "796497018181707152904492756.120547659E-2147483647";
    let value = BigDecimal::parse_mc(&js(literal), &mc(18, RoundingMode::Down)).unwrap();
    assert_eq!(value.to_string(), "7.96497018181707152E-2147483621");
    assert_eq!(value.scale(), 2147483638);
}

#[test]
fn parse_accepts_non_ascii_digits() {
    assert_eq!(dec("١٢.٥").to_string(), "12.5");
    assert_eq!(dec("1e٣").to_string(), "1E+3");
}

#[test]
fn value_of_double_uses_the_shortest_text() {
    assert_eq!(text(BigDecimal::value_of_double(0.1)), "0.1");
    assert_eq!(text(BigDecimal::value_of_double(1e23)), "1.0E+23");
    let error = BigDecimal::value_of_double(f64::NAN).expect_err("not a number");
    assert_eq!(error.class, NUMBER_FORMAT);
}

#[test]
fn string_forms() {
    assert_eq!(dec("0.000001").to_string(), "0.000001");
    assert_eq!(dec("0.0000001").to_string(), "1E-7");
    assert_eq!(dec("1.5E+10").to_string(), "1.5E+10");
    assert_eq!(dec("-12.50E+3").to_string(), "-1.250E+4");
    assert_eq!(dec("12.5E+4").to_engineering_string(), "125E+3");
    assert_eq!(dec("1E+4").to_engineering_string(), "10E+3");
    assert_eq!(dec("0E+4").to_engineering_string(), "0.00E+6");
    assert_eq!(dec("0E+5").to_engineering_string(), "0.0E+6");
    assert_eq!(dec("0E+3").to_engineering_string(), "0E+3");
    assert_eq!(dec("1.5E+3").to_plain_string().unwrap(), "1500");
    assert_eq!(dec("-1E-3").to_plain_string().unwrap(), "-0.001");
    assert_eq!(dec("0E+3").to_plain_string().unwrap(), "0");
    assert_eq!(dec("0E-3").to_plain_string().unwrap(), "0.000");
}

#[test]
fn plain_string_limits_follow_the_jvm() {
    let out_of_memory = "java.lang.OutOfMemoryError";
    assert_eq!(dec("1E+2147483647").to_plain_string(), Err(Thrown::new(out_of_memory, "too large to fit in a String")));
    assert_eq!(
        dec("1E+2147483646").to_plain_string(),
        Err(Thrown::new(out_of_memory, "Requested array size exceeds VM limit"))
    );
    assert_eq!(dec("1E+2147483648").to_plain_string(), Err(arithmetic("Overflow")));
}

#[test]
fn addition_prefers_the_larger_scale() {
    assert_eq!(text(dec("1.10").add(&dec("2.2"))), "3.30");
    assert_eq!(text(dec("1E+3").add(&dec("1"))), "1001");
    assert_eq!(text(dec("0E+3").add(&dec("1.5"))), "1.5");
}

#[test]
fn addition_with_context_condenses_the_small_operand() {
    let context = mc(3, RoundingMode::Ceiling);
    assert_eq!(text(dec("1E+2147483647").add_mc(&dec("1E-2147483647"), &context)), "1.01E+2147483647");
    let context = mc(3, RoundingMode::HalfUp);
    assert_eq!(text(dec("1000").subtract_mc(&dec("0.000001"), &context)), "1.00E+3");
}

#[test]
fn addition_with_zero_keeps_the_preferred_scale_within_precision() {
    let context = mc(5, RoundingMode::HalfUp);
    assert_eq!(text(dec("0.000").add_mc(&dec("1.5"), &context)), "1.500");
    assert_eq!(text(dec("0.0000000").add_mc(&dec("1.5"), &context)), "1.5000");
    assert_eq!(text(dec("0E+5").add_mc(&dec("0.0"), &context)), "0.0");
}

#[test]
fn addition_near_the_scale_limit_wraps_like_int_arithmetic() {
    let context = mc(4, RoundingMode::HalfDown);
    let left = dec("-9223372036854775806E-2147483646");
    let right = dec("1E-2147483647");
    assert_eq!(left.add_mc(&right, &context).map(|v| v.scale()), Err(arithmetic("Underflow")));
}

#[test]
fn product_scale_excuses_only_a_zero_left_operand() {
    assert_eq!(text(dec("0E+2147483647").multiply(&dec("1E+1"))), "0E+2147483648");
    assert_eq!(dec("1E+2147483647").multiply(&dec("1E+2147483647")).map(|v| v.scale()), Err(arithmetic("Overflow")));
}

#[test]
fn multiplication_rounds_with_carry() {
    let context = mc(2, RoundingMode::HalfUp);
    assert_eq!(text(dec("99").multiply_mc(&dec("1.01"), &context)), "1.0E+2");
}

#[test]
fn every_rounding_mode() {
    let cases = [
        (RoundingMode::Up, "3", "-3"),
        (RoundingMode::Down, "2", "-2"),
        (RoundingMode::Ceiling, "3", "-2"),
        (RoundingMode::Floor, "2", "-3"),
        (RoundingMode::HalfUp, "3", "-3"),
        (RoundingMode::HalfDown, "2", "-2"),
        (RoundingMode::HalfEven, "2", "-2"),
    ];
    for (mode, positive, negative) in cases {
        assert_eq!(text(dec("2.5").set_scale(0, mode)), positive, "{mode:?}");
        assert_eq!(text(dec("-2.5").set_scale(0, mode)), negative, "{mode:?}");
    }
    assert_eq!(
        dec("2.5").set_scale(0, RoundingMode::Unnecessary).map(|v| v.scale()),
        Err(arithmetic("Rounding necessary"))
    );
}

#[test]
fn set_scale_failures() {
    let up = RoundingMode::Up;
    assert_eq!(
        dec("1").set_scale(i32::MIN, up).and_then(|v| v.set_scale(i32::MAX, up)).map(|v| v.scale()),
        Err(arithmetic("Underflow"))
    );
    assert_eq!(
        dec("1E-2147483647").set_scale(0, RoundingMode::Down).map(|v| v.scale()),
        Err(arithmetic("BigInteger would overflow supported range"))
    );
    assert_eq!(text(dec("0E+5").set_scale(i32::MAX, up)), "0E-2147483647");
}

#[test]
fn exact_division() {
    assert_eq!(text(dec("1").divide(&dec("8"))), "0.125");
    assert_eq!(text(dec("1.00").divide(&dec("2"))), "0.50");
    assert_eq!(text(dec("6E+3").divide(&dec("2"))), "3E+3");
    assert_eq!(text(dec("0.00").divide(&dec("3"))), "0.00");
    assert_eq!(
        dec("1").divide(&dec("3")).map(|v| v.scale()),
        Err(arithmetic("Non-terminating decimal expansion; no exact representable decimal result."))
    );
}

#[test]
fn zero_divisor_messages() {
    let context = mc(5, RoundingMode::HalfUp);
    let by_zero = arithmetic("Division by zero");
    let undefined = arithmetic("Division undefined");
    assert_eq!(dec("1").divide(&dec("0")).map(|v| v.scale()), Err(by_zero.clone()));
    assert_eq!(dec("0").divide(&dec("0")).map(|v| v.scale()), Err(undefined.clone()));
    assert_eq!(dec("1").divide_mc(&dec("0"), &context).map(|v| v.scale()), Err(by_zero.clone()));
    assert_eq!(dec("1").remainder(&dec("0")).map(|v| v.scale()), Err(by_zero));
    assert_eq!(dec("0").remainder(&dec("0")).map(|v| v.scale()), Err(undefined));
}

#[test]
fn division_to_a_scale_names_the_path_it_took() {
    let down = RoundingMode::Down;
    let long_path = arithmetic("/ by zero");
    let big_path = arithmetic("BigInteger divide by zero");
    let by_scale = |x: &str, scale| dec(x).divide_scale(&dec("0"), scale, down).map(|v| v.scale());
    assert_eq!(by_scale("1", 2), Err(long_path.clone()));
    assert_eq!(by_scale("0", 2), Err(long_path));
    assert_eq!(by_scale("1", 30), Err(big_path.clone()));
    assert_eq!(by_scale("100000000000000000000", 2), Err(big_path));
}

#[test]
fn a_cancelled_sum_of_huge_values_stays_inflated() {
    let huge = dec("100000000000000000000");
    let small = huge.subtract(&dec("99999999999999999999")).unwrap();
    assert_eq!(small.to_string(), "1");
    let same_sign = dec("100000000000000000000").add(&dec("1")).unwrap();
    assert_eq!(
        same_sign.divide_scale(&dec("0"), 0, RoundingMode::Down).map(|v| v.scale()),
        Err(arithmetic("BigInteger divide by zero"))
    );
}

#[test]
fn division_with_context() {
    let context = mc(3, RoundingMode::HalfUp);
    assert_eq!(text(dec("1").divide_mc(&dec("3"), &context)), "0.333");
    assert_eq!(text(dec("2").divide_mc(&dec("3"), &context)), "0.667");
    assert_eq!(text(dec("10").divide_mc(&dec("2"), &context)), "5");
    assert_eq!(text(dec("999.5").divide_mc(&dec("1"), &context)), "1.00E+3");
    assert_eq!(text(dec("0.00").divide_mc(&dec("4E+2"), &context)), "0.0000");
}

#[test]
fn remainder_keeps_the_dividend_sign() {
    assert_eq!(text(dec("7.5").remainder(&dec("2"))), "1.5");
    assert_eq!(text(dec("-7.5").remainder(&dec("2"))), "-1.5");
    assert_eq!(text(dec("1").remainder(&dec("1E+3"))), "1");
}

#[test]
fn remainder_with_context_needs_the_whole_integer_quotient() {
    let context = mc(2, RoundingMode::HalfUp);
    assert_eq!(
        dec("12345").remainder_mc(&dec("7"), &context).map(|v| v.scale()),
        Err(arithmetic("Division impossible"))
    );
    assert_eq!(text(dec("75").remainder_mc(&dec("7"), &context)), "5");
}

#[test]
fn pow_follows_x3_274() {
    let context = mc(5, RoundingMode::HalfUp);
    assert_eq!(text(dec("2").pow_mc(10, &context)), "1024");
    assert_eq!(text(dec("2").pow_mc(-2, &context)), "0.25");
    assert_eq!(text(dec("1.5").pow_mc(0, &context)), "1");
    assert_eq!(text(dec("1.1").pow_mc(100, &context)), "13781");
    assert_eq!(dec("2").pow_mc(100000, &context).map(|v| v.scale()), Err(arithmetic("Invalid operation")));
    assert_eq!(dec("2").pow_mc(1_000_000_000, &context).map(|v| v.scale()), Err(arithmetic("Invalid operation")));
    assert_eq!(dec("0").pow_mc(-1, &context).map(|v| v.scale()), Err(arithmetic("Division by zero")));
}

#[test]
fn pow_without_context_is_exact_and_rejects_negative_exponents() {
    let unlimited = mc(0, RoundingMode::HalfUp);
    assert_eq!(text(dec("1.5").pow_mc(3, &unlimited)), "3.375");
    assert_eq!(dec("2").pow_mc(-1, &unlimited).map(|v| v.scale()), Err(arithmetic("Invalid operation")));
}

#[test]
fn unary_context_operations() {
    let context = mc(2, RoundingMode::HalfUp);
    assert_eq!(text(dec("1.25").negate_mc(&context)), "-1.3");
    assert_eq!(text(dec("-1.25").abs_mc(&context)), "1.3");
    assert_eq!(text(dec("1.25").plus_mc(&context)), "1.3");
    assert_eq!(text(dec("999").round(&context)), "1.0E+3");
    assert_eq!(text(BigDecimal::from_i32_mc(12345, &context)), "1.2E+4");
}

#[test]
fn rounding_past_the_lowest_scale_reports_the_truncated_direction() {
    let context = mc(1, RoundingMode::Up);
    assert_eq!(dec("123E+2147483648").round(&context).map(|v| v.scale()), Err(arithmetic("Underflow")));
}

#[test]
fn strip_trailing_zeros() {
    assert_eq!(text(dec("100").strip_trailing_zeros()), "1E+2");
    assert_eq!(text(dec("1.500").strip_trailing_zeros()), "1.5");
    assert_eq!(text(dec("0.000").strip_trailing_zeros()), "0");
    assert_eq!(dec("10E+2147483648").strip_trailing_zeros().map(|v| v.scale()), Err(arithmetic("Overflow")));
}

#[test]
fn move_point_right_normalizes_negative_scales() {
    assert_eq!(text(dec("1.5").move_point_right(3)), "1500");
    assert_eq!(text(dec("1.5").move_point_right(1)), "15");
    assert_eq!(text(dec("1E+2").move_point_right(0)), "100");
}

#[test]
fn comparison_ignores_scale_but_equality_does_not() {
    assert_eq!(dec("1.0").compare_to(&dec("1.00")), Ordering::Equal);
    assert!(!dec("1.0").java_equals(&dec("1.00")));
    assert!(dec("1.0").java_equals(&dec("1.0")));
    assert_eq!(dec("-1").compare_to(&dec("0")), Ordering::Less);
    assert_eq!(dec("1E+2147483647").compare_to(&dec("1E-2147483647")), Ordering::Greater);
    assert_eq!(dec("-1E+3").compare_to(&dec("-999")), Ordering::Less);
}

#[test]
fn numeric_properties() {
    let value = dec("-12.50");
    assert_eq!(value.signum(), -1);
    assert_eq!(value.scale(), 2);
    assert_eq!(value.precision(), 4);
    assert_eq!(value.unscaled_value().to_string(), "-1250");
    assert_eq!(dec("0.000").precision(), 1);
    assert_eq!(dec("1.5").hash_code(), 466);
    assert_eq!(BigDecimal::zero().to_string(), "0");
    assert_eq!(BigDecimal::one().to_string(), "1");
}

#[test]
fn conversions() {
    assert_eq!(dec("12.9").int_value(), 12);
    assert_eq!(dec("-12.9").long_value(), -12);
    assert_eq!(dec("18446744073709551617.5").long_value(), 1);
    assert_eq!(dec("1E+100").long_value(), 0);
    assert_eq!(dec("1E-2147483647").long_value(), 0);
    assert_eq!(dec("0.1").double_value(), 0.1);
    assert_eq!(dec("0.1").float_value(), 0.1f32);
    assert_eq!(dec("1E+2147483647").double_value(), f64::INFINITY);
    assert_eq!(dec("1.5").to_big_integer().unwrap().to_string(), "1");
}

#[test]
fn exact_int_conversion() {
    assert_eq!(dec("12.0").int_value_exact(), Ok(12));
    assert_eq!(dec("0E+5").int_value_exact(), Ok(0));
    assert_eq!(dec("1.5").int_value_exact(), Err(arithmetic("Rounding necessary")));
    assert_eq!(dec("0.5").int_value_exact(), Err(arithmetic("Rounding necessary")));
    assert_eq!(dec("2147483648").int_value_exact(), Err(arithmetic("Overflow")));
    assert_eq!(dec("1E+20").int_value_exact(), Err(arithmetic("Overflow")));
    assert_eq!(dec("9223372036854775808").int_value_exact(), Err(arithmetic("Overflow")));
}
