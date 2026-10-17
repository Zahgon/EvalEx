use super::*;

fn parsed(pattern: &str, tag: &str, text: &str) -> String {
    let locale = Locale::for_language_tag(&js(tag));
    let format = DecimalFormat::new(&js(pattern), &locale).expect("valid pattern");
    match format.parse(&js(text)) {
        Parsed::Number(value) => value.to_string(),
        Parsed::NotFinite => "not finite".to_string(),
        Parsed::NoMatch => "no match".to_string(),
        Parsed::Failed(thrown) => thrown.class,
    }
}

fn pattern_error(pattern: &str) -> String {
    let error = DecimalFormat::new(&js(pattern), &Locale::US).expect_err("invalid pattern");
    assert_eq!(error.class, "java.lang.IllegalArgumentException");
    String::from_utf16_lossy(&error.message.unwrap_or_default())
}

#[test]
fn locale_separators_and_grouping() {
    assert_eq!(parsed("###,##0.00", "de", "1.234,56"), "1234.56");
    assert_eq!(parsed("###,##0.00", "en-US", "1,234.56"), "1234.56");
    assert_eq!(parsed("###,##0.00", "pt-BR", "1234,56"), "1234.56");
    assert_eq!(parsed("#,##0", "fr-FR", "1\u{202F}234"), "1234");
    assert_eq!(parsed("#,##0", "fr-FR", "1\u{A0}234"), "1");
    assert_eq!(parsed("0", "en-US", "1,234"), "1");
}

#[test]
fn scale_is_preserved() {
    assert_eq!(parsed("0", "en-US", "00.100"), "0.100");
    assert_eq!(parsed("0", "en-US", "0.00"), "0.00");
    assert_eq!(parsed("0", "en-US", "1.5E2"), "1.5E+2");
    assert_eq!(parsed("0", "en-US", "0E5"), "0E+5");
}

#[test]
fn negative_subpatterns_and_affixes() {
    assert_eq!(parsed("###,##0.00;(###,##0.00)", "en-US", "(1,250.50)"), "-1250.50");
    assert_eq!(parsed("###,##0.00;(###,##0.00)", "en-US", "1234)"), "1234");
    assert_eq!(parsed("a0;ab0", "en-US", "ab1"), "-1");
    assert_eq!(parsed("-0;-0", "en-US", "-1"), "1");
    assert_eq!(parsed("0E0E0", "en-US", "1"), "-1");
    assert_eq!(parsed("'#'0", "en-US", "#5"), "5");
    assert_eq!(parsed("\u{A4}0", "pt-BR", "R$5"), "5");
}

#[test]
fn multipliers_divide_exactly() {
    assert_eq!(parsed("###.0%", "en-US", "85.5%"), "0.855");
    assert_eq!(parsed("0%", "en-US", "0.00%"), "0.00");
    assert_eq!(parsed("0\u{2030}", "en-US", "5\u{2030}"), "0.005");
    assert_eq!(parsed("0%", "en-US", "5"), "no match");
}

#[test]
fn non_finite_symbols() {
    assert_eq!(parsed("0", "en-US", "NaN"), "not finite");
    assert_eq!(parsed("0", "en-US", "-NaN"), "no match");
    assert_eq!(parsed("0", "en-US", "-\u{221E}"), "not finite");
    assert_eq!(parsed("0%", "en-US", "\u{221E}"), "no match");
}

#[test]
fn exponents_and_trailing_text() {
    assert_eq!(parsed("0", "en-US", "1E+3"), "1");
    assert_eq!(parsed("0", "en-US", "1E-3x"), "0.001");
    assert_eq!(parsed("0", "en-US", "1E4294967296"), "1");
    assert_eq!(parsed("0", "en-US", "1E9223372036854775808"), "1");
    assert_eq!(parsed("0", "en-US", ""), "no match");
    assert_eq!(parsed("0", "en-US", "\u{661}\u{FF12}"), "12");
}

#[test]
fn scales_out_of_range_fail_like_java() {
    assert_eq!(parsed("0", "en-US", "1E2147483648"), "java.lang.ArithmeticException");
    assert_eq!(parsed("0", "en-US", "0E-2147483648"), "java.lang.NumberFormatException");
    assert_eq!(parsed("0%", "en-US", "1E-2147483647%"), "0E-2147483647");
}

#[test]
fn malformed_patterns_are_rejected() {
    assert_eq!(pattern_error("0#0"), "Unexpected '0' in pattern \"0#0\"");
    assert_eq!(pattern_error("#,"), "Malformed pattern \"#,\"");
    assert_eq!(pattern_error("00#.##"), "Malformed pattern \"00#.##\"");
    assert_eq!(pattern_error("0E"), "Malformed exponential pattern \"0E\"");
    assert_eq!(pattern_error("0.0.0"), "Multiple decimal separators in pattern \"0.0.0\"");
    assert_eq!(pattern_error("a;b"), "Unquoted special character ';' in pattern \"a;b\"");
    assert_eq!(pattern_error("%%"), "Too many percent/per mille characters in pattern \"%%\"");
    assert_eq!(pattern_error("0E0#E0"), "Multiple exponential symbols in pattern \"0E0#E0\"");
}

#[test]
fn unusual_patterns_are_accepted() {
    assert_eq!(parsed("", "en-US", "-1,000"), "-1000");
    assert_eq!(parsed("0a0", "en-US", "5a"), "5");
    assert_eq!(parsed("x", "en-US", "x7"), "7");
}
