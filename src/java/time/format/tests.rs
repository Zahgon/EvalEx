//! Tests of formatting and parsing. Expected values were observed on JDK 21.0.13 by
//! `fuzz/java/GenTests.java`, which generates this file.

use super::*;
use crate::java::js;

/// Pattern or constant name, locale tag, zone, epoch second, nanos, expected text.
type FormatCase = (&'static str, &'static str, &'static str, i64, i64, &'static str);
/// Pattern or constant name, text, default zone, expected instant or failure.
type ParseCase = (&'static str, &'static str, &'static str, &'static str);

#[rustfmt::skip]
const FORMAT_CASES: &[FormatCase] = &[
    ("@iso_date_time", "en-US", "Europe/Paris", 1710000000, 5000000, "2024-03-09T17:00:00.005+01:00[Europe/Paris]"),
    ("@iso_date_time", "en-US", "+05:30", 1710000000, 0, "2024-03-09T21:30:00+05:30"),
    ("@iso_date", "en-US", "Asia/Tokyo", 1710000000, 0, "2024-03-10+09:00"),
    ("@iso_local_date_time", "en-US", "UTC", 1710000000, 123456789, "2024-03-09T16:00:00.123456789"),
    ("@iso_local_date", "en-US", "UTC", -99999999999, 0, "-1199-02-15"),
    ("@rfc_1123", "de-DE", "America/New_York", 1710000000, 0, "Sat, 9 Mar 2024 11:00:00 -0500"),
    ("@rfc_1123", "en-US", "UTC", 1710000000, 0, "Sat, 9 Mar 2024 16:00:00 GMT"),
    ("EEEE d MMMM yyyy GGGG", "de-DE", "Europe/Berlin", 1710000000, 0, "Samstag 9 März 2024 n. Chr."),
    ("EEE, MMM d a", "fr-FR", "Europe/Paris", 1710000000, 0, "sam., mars 9 PM"),
    ("cccc LLLL QQQQ qqq", "pt-BR", "America/Sao_Paulo", 1710000000, 0, "sábado março 1º trimestre T1"),
    ("MMMMM EEEEE GGGGG", "en-US", "UTC", 1710000000, 0, "M S A"),
    ("YYYY-'W'ww-e W F", "de-DE", "UTC", 1609632000, 0, "2020-W53-7 0 1"),
    ("YYYY-'W'ww-e W F", "en-US", "UTC", 1609632000, 0, "2021-W02-1 2 1"),
    ("uuuu uu yyyy yy G", "en-US", "UTC", -99999999999, 0, "-1199 99 1200 00 BC"),
    ("uuuuu yyyy", "en-US", "UTC", 253402300800, 0, "10000 +10000"),
    ("h K k H m s S SSS n A N g D", "en-US", "UTC", 86399, 120000000, "11 11 23 23 59 59 1 120 120000000 86399120 86399120000000 40587 1"),
    ("X XX XXX XXXX XXXXX", "en-US", "-05:30", 0, 0, "-0530 -0530 -05:30 -0530 -05:30"),
    ("x xx xxx xxxx xxxxx Z ZZZZ ZZZZZ", "en-US", "+05:45:30", 0, 0, "+0545 +0545 +05:45 +054530 +05:45:30 +0545 GMT+05:45:30 +05:45:30"),
    ("x xxx X Z O OOOO", "fr-FR", "UTC", 0, 0, "+00 +00:00 Z +0000 UTC UTC"),
    ("O OOOO", "en-US", "-03:30", 0, 0, "GMT-3:30 GMT-03:30"),
    ("VV z v", "en-US", "America/Chicago", 0, 0, "America/Chicago America/Chicago America/Chicago"),
    ("ppppH|pppMM", "en-US", "UTC", 0, 0, "   0| 01"),
    ("[yyyy[-MM]]'q''x'''", "en-US", "UTC", 0, 0, "1970-01q'x'"),
];

#[rustfmt::skip]
const FORMAT_FAILURES: &[FormatCase] = &[
    ("ppuuuu", "en-US", "UTC", 253402300800, 0, "Cannot print as output of 6 characters exceeds pad width of 2"),
    ("@rfc_1123", "en-US", "UTC", 253402300800, 0, "Field Year cannot be printed as the value 10000 exceeds the maximum print width of 4"),
    ("@rfc_1123", "en-US", "UTC", -99999999999, 0, "Field Year cannot be printed as the value -1199 cannot be negative according to the SignStyle"),
];

#[rustfmt::skip]
const PARSE_CASES: &[ParseCase] = &[
    ("@iso_date_time", "2024-03-09T17:00:00.5+01:00[Europe/Paris]", "UTC", "2024-03-09T16:00:00.500Z"),
    ("@iso_date_time", "2024-03-09T17:00", "Asia/Tokyo", "2024-03-09T08:00:00Z"),
    ("@iso_date_time", "2024-03-09t17:00:00z", "UTC", "2024-03-09T17:00:00Z"),
    ("@iso_date", "2024-03-09", "Europe/Paris", "2024-03-08T23:00:00Z"),
    ("@iso_local_date_time", "2024-03-31T02:30:00", "Europe/Paris", "2024-03-31T01:30:00Z"),
    ("@iso_local_date_time", "2024-10-27T02:30:00", "Europe/Paris", "2024-10-27T00:30:00Z"),
    ("@iso_local_date", "+12024-03-09", "UTC", "+12024-03-09T00:00:00Z"),
    ("@rfc_1123", "Sat, 9 Mar 2024 17:00:00 GMT", "UTC", "2024-03-09T17:00:00Z"),
    ("@rfc_1123", "9 mar 2024 17:00 +0130", "UTC", "2024-03-09T15:30:00Z"),
    ("yyyyMMddHHmmssSSS", "20240309170000123", "UTC", "2024-03-09T17:00:00.123Z"),
    ("uuuu-MM-dd HH:mm", "2024-03-09 24:00", "UTC", "2024-03-10T00:00:00Z"),
    ("uuuu-MM-dd", "2023-02-31", "UTC", "2023-02-28T00:00:00Z"),
    ("yy-M-d", "99-1-2", "UTC", "2099-01-02T00:00:00Z"),
    ("yyyy-MM-dd hh a", "2024-03-09 12 PM", "UTC", "2024-03-09T12:00:00Z"),
    ("uuuu-MM-dd a", "2024-03-09 PM", "UTC", "2024-03-09T18:00:00Z"),
    ("YYYY-'W'ww-e", "2024-W10-1", "UTC", "2024-03-03T00:00:00Z"),
    ("uuuu-DDD", "2024-366", "UTC", "2024-12-31T00:00:00Z"),
    ("g", "60000", "UTC", "2023-02-25T00:00:00Z"),
    ("MMMMM uuuu dd", "J 2024 01", "UTC", "2024-07-01T00:00:00Z"),
    ("[uuuu-MM-dd][dd/MM/uuuu]", "09/03/2024", "UTC", "2024-03-09T00:00:00Z"),
    ("uuuu-MM-dd HH:mm VV", "2024-03-09 12:00 UTC+02:00", "Asia/Tokyo", "2024-03-09T10:00:00Z"),
    ("uuuu-MM-dd HH:mm OOOO", "2024-03-09 12:00 GMT+02:00", "Asia/Tokyo", "2024-03-09T10:00:00Z"),
];

/// The last column is the exception class and message.
#[rustfmt::skip]
const PARSE_FAILURES: &[ParseCase] = &[
    ("@iso_local_date", "2023-02-30", "UTC", "java.time.format.DateTimeParseException: Text '2023-02-30' could not be parsed: Invalid date 'FEBRUARY 30'"),
    ("@iso_date_time", "2024-03-09T24:00:00", "UTC", "java.time.format.DateTimeParseException: Text '2024-03-09T24:00:00' could not be parsed: Invalid value for HourOfDay (valid values 0 - 23): 24"),
    ("uuuu-MM-dd", "2024-13-01", "UTC", "java.time.format.DateTimeParseException: Text '2024-13-01' could not be parsed: Invalid value for MonthOfYear (valid values 1 - 12): 13"),
    ("uuuu-MM-dd", "2024-01-01x", "UTC", "java.time.format.DateTimeParseException: Text '2024-01-01x' could not be parsed, unparsed text found at index 10"),
    ("uuuu-MM-dd", "2024-1-01", "UTC", "java.time.format.DateTimeParseException: Text '2024-1-01' could not be parsed at index 5"),
    ("MM", "03", "UTC", "java.lang.NullPointerException: Cannot invoke \"java.time.LocalDate.atStartOfDay()\" because \"parsedDate\" is null"),
    ("a", "AM", "UTC", "java.lang.NullPointerException: date"),
    ("uuuu-MM-dd XXX", "2024-01-01 +01:00", "UTC", "java.time.DateTimeException: Unable to obtain Instant from TemporalAccessor: {OffsetSeconds=3600},ISO resolved to 2024-01-01 of type java.time.format.Parsed"),
    ("uuuu-MM-dd HH:mm XXX", "2024-01-01 00:00 +24:00", "UTC", "java.time.format.DateTimeParseException: Text '2024-01-01 00:00 +24:00' could not be parsed: Value out of range: Hour[0-23], Minute[0-59], Second[0-59]"),
    ("uuuu-MM-dd HH:mm XXX", "2024-01-01 00:00 +19:00", "UTC", "java.time.format.DateTimeParseException: Text '2024-01-01 00:00 +19:00' could not be parsed: Zone offset not in valid range: -18:00 to +18:00"),
    ("uuuu-MM-dd E", "2024-01-01 Tue", "UTC", "java.time.format.DateTimeParseException: Text '2024-01-01 Tue' could not be parsed: Conflict found: Field DayOfWeek 1 differs from DayOfWeek 2 derived from 2024-01-01"),
    ("uuuu-MM-dd HH:mm OOOO", "2024-03-09 12:00 GMT+02", "UTC", "java.time.format.DateTimeParseException: Text '2024-03-09 12:00 GMT+02' could not be parsed: Index 23 out of bounds for length 23"),
    ("YYYY-'W'ww-e", "2018-W53-1", "UTC", "java.time.format.DateTimeParseException: Text '2018-W53-1' could not be parsed: Invalid value for WeekOfWeekBasedYear[WeekFields[SUNDAY,1]] (valid values 1 - 52): 53"),
    ("uuuu-MM-dd", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "UTC", "java.time.format.DateTimeParseException: Text 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa...' could not be parsed at index 0"),
];

/// Pattern and the class and message of its rejection.
#[rustfmt::skip]
const BAD_PATTERNS: &[(&str, &str)] = &[
    ("b", "java.lang.IllegalArgumentException: Unknown pattern letter: b"),
    ("MMMMMM", "java.lang.IllegalArgumentException: Too many pattern letters: M"),
    ("'abc", "java.lang.IllegalArgumentException: Pattern ends with an incomplete string literal: 'abc"),
    ("#", "java.lang.IllegalArgumentException: Pattern includes reserved character: '#'"),
    ("]", "java.lang.IllegalArgumentException: Pattern invalid as it contains ] without previous ["),
    ("cc", "java.lang.IllegalArgumentException: Invalid pattern \"cc\""),
    ("V", "java.lang.IllegalArgumentException: Pattern letter count must be 2: V"),
    ("OO", "java.lang.IllegalArgumentException: Pattern letter count must be 1 or 4: O"),
    ("vv", "java.lang.IllegalArgumentException: Wrong number of pattern letters: v"),
    ("BB", "java.lang.IllegalArgumentException: Wrong number of pattern letters: B"),
    ("p", "java.lang.IllegalArgumentException: Pad letter 'p' must be followed by valid pad pattern: p"),
    ("ppHmm", "java.lang.ClassCastException: class java.time.format.DateTimeFormatterBuilder$PadPrinterParserDecorator cannot be cast to class java.time.format.DateTimeFormatterBuilder$NumberPrinterParser (java.time.format.DateTimeFormatterBuilder$PadPrinterParserDecorator and java.time.format.DateTimeFormatterBuilder$NumberPrinterParser are in module java.base of loader 'bootstrap')"),
    ("uuuuuuuuuuuuuuuuuuuu", "java.lang.IllegalArgumentException: The minimum width must be from 1 to 19 inclusive but was 20"),
    ("SSSSSSSSSS", "java.lang.IllegalArgumentException: Minimum width must be from 0 to 9 inclusive but was 10"),
];

fn formatter(name: &str, tag: &str) -> DateTimeFormatter {
    match name {
        "@iso_date_time" => DateTimeFormatter::iso_date_time(),
        "@iso_date" => DateTimeFormatter::iso_date(),
        "@iso_local_date_time" => DateTimeFormatter::iso_local_date_time(),
        "@iso_local_date" => DateTimeFormatter::iso_local_date(),
        "@rfc_1123" => DateTimeFormatter::rfc_1123_date_time(),
        pattern => {
            let locale = Locale::for_language_tag(&js(tag));
            DateTimeFormatter::of_pattern(&js(pattern), &locale).unwrap()
        }
    }
}

fn zone(id: &str) -> ZoneId {
    ZoneId::of(&js(id)).unwrap()
}

fn render(thrown: &Thrown) -> String {
    let message = thrown.message.as_deref().unwrap_or(&[]);
    format!("{}: {}", thrown.class, String::from_utf16_lossy(message))
}

fn format_case(case: &FormatCase) -> JResult<String> {
    let &(name, tag, zone_id, seconds, nanos, _) = case;
    let value = Instant::of_epoch_second(seconds, nanos)?.at_zone(&zone(zone_id))?;
    let text = formatter(name, tag).format(&value)?;
    Ok(String::from_utf16_lossy(&text))
}

#[test]
fn format_matches_the_jdk() {
    for case in FORMAT_CASES {
        assert_eq!(format_case(case).as_deref(), Ok(case.5), "{case:?}");
    }
}

#[test]
fn format_failures_match_the_jdk() {
    for case in FORMAT_FAILURES {
        let thrown = format_case(case).unwrap_err();
        assert_eq!(thrown.class, "java.time.DateTimeException");
        assert_eq!(thrown.message, Some(js(case.5)), "{case:?}");
    }
}

#[test]
fn parse_matches_the_jdk() {
    for &(name, text, zone_id, expected) in PARSE_CASES {
        let parsed = parse_to_instant(&formatter(name, "en-US"), &js(text), &zone(zone_id));
        assert_eq!(parsed.unwrap().to_string(), expected, "{name} {text}");
    }
}

#[test]
fn parse_failures_match_the_jdk() {
    for &(name, text, zone_id, expected) in PARSE_FAILURES {
        let thrown = parse_to_instant(&formatter(name, "en-US"), &js(text), &zone(zone_id));
        assert_eq!(render(&thrown.unwrap_err()), expected, "{name} {text}");
    }
}

#[test]
fn malformed_patterns_are_rejected_like_the_jdk() {
    for &(pattern, expected) in BAD_PATTERNS {
        let thrown = DateTimeFormatter::of_pattern(&js(pattern), &Locale::US).unwrap_err();
        assert_eq!(render(&thrown), expected, "{pattern}");
    }
}

#[test]
fn with_locale_switches_texts_and_week_rules() {
    let us = DateTimeFormatter::of_pattern(&js("MMMM e"), &Locale::US).unwrap();
    let german = us.with_locale(&Locale::for_language_tag(&js("de-DE")));
    let value = Instant::EPOCH.at_zone(&zone("UTC")).unwrap();
    assert_eq!(us.format(&value), Ok(js("January 5")));
    assert_eq!(german.format(&value), Ok(js("Januar 4")));
}

#[test]
fn day_periods_are_reported_as_unsupported() {
    let thrown = DateTimeFormatter::of_pattern(&js("B"), &Locale::US).unwrap_err();
    assert_eq!(thrown.message, Some(js("Unknown pattern letter: B")));
}

#[test]
fn deeply_nested_sections_need_no_recursion() {
    let mut pattern = "[".repeat(100_000);
    pattern.push_str("uuuu");
    let formatter = DateTimeFormatter::of_pattern(&js(&pattern), &Locale::US).unwrap();
    let value = Instant::EPOCH.at_zone(&zone("UTC")).unwrap();
    assert_eq!(formatter.format(&value), Ok(js("1970")));
    let parsed = parse_to_instant(&formatter, &js("x"), &zone("UTC"));
    assert_eq!(parsed.unwrap_err().class, "java.time.format.DateTimeParseException");
}
