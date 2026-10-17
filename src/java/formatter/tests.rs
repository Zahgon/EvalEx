use super::*;
use crate::java::time::{LocalDateTime, ZoneId};
use FormatArgument::{Boolean, Decimal, Null, Other, Text};

fn decimal(text: &str) -> FormatArgument {
    Decimal(BigDecimal::parse(&js(text)).expect("valid decimal"))
}

fn date_time(zone: &str) -> FormatArgument {
    let local = LocalDateTime::of(2024, 3, 5, 14, 7, 9, 123_456_789).expect("valid date");
    let zone = ZoneId::of(&js(zone)).expect("valid zone");
    FormatArgument::DateTime(local.at_zone(&zone).expect("valid instant"))
}

fn locale(tag: &str) -> Locale {
    Locale::for_language_tag(&js(tag))
}

fn formatted(tag: &str, pattern: &str, arguments: &[FormatArgument]) -> String {
    let units = format(&locale(tag), &js(pattern), arguments).expect("format must succeed");
    String::from_utf16_lossy(&units)
}

fn failure(pattern: &str, arguments: &[FormatArgument]) -> (String, String) {
    let error = format(&Locale::US, &js(pattern), arguments).expect_err("format must fail");
    let message = error.message.unwrap_or_default();
    (error.class, String::from_utf16_lossy(&message))
}

#[test]
fn decimals_round_half_up() {
    assert_eq!(formatted("en-US", "%.2f", &[decimal("2.345")]), "2.35");
    assert_eq!(formatted("en-US", "%.0f", &[decimal("0.5")]), "1");
    assert_eq!(formatted("en-US", "%.2e", &[decimal("-12345")]), "-1.23e+04");
    assert_eq!(formatted("en-US", "%f", &[decimal("1E+3")]), "1000.000000");
}

#[test]
fn general_switches_between_fixed_and_scientific() {
    assert_eq!(formatted("en-US", "%g", &[decimal("1.5")]), "1.50000");
    assert_eq!(formatted("en-US", "%g", &[decimal("0.00001")]), "1.00000e-05");
    assert_eq!(formatted("en-US", "%g", &[decimal("0.00")]), "0.00000e-02");
    assert_eq!(formatted("en-US", "%.3G", &[decimal("123456")]), "1.23E+05");
}

#[test]
fn ten_digit_integers_show_the_short_exponent() {
    assert_eq!(formatted("en-US", "%.1e", &[decimal("1234567890")]), "1.2e+09");
    assert_eq!(formatted("en-US", "%.12e", &[decimal("1234567890")]), "1.234567890000e+9");
}

#[test]
fn signs_padding_and_grouping_combine() {
    assert_eq!(formatted("en-US", "%(,12.2f", &[decimal("-1234.5")]), "  (1,234.50)");
    assert_eq!(formatted("en-US", "%+010.2f", &[decimal("1.5")]), "+000001.50");
    assert_eq!(formatted("en-US", "%(010.1f", &[decimal("-1.5")]), "(000001.5)");
    assert_eq!(formatted("en-US", "% .1f|%-6.1f|", &[decimal("1"), decimal("2")]), " 1.0|2.0   |");
}

#[test]
fn locales_supply_their_separators() {
    let value = [decimal("1234567.5")];
    assert_eq!(formatted("de-DE", "%,.2f", &value), "1.234.567,50");
    assert_eq!(formatted("fr-FR", "%,.2f", &value), "1\u{202F}234\u{202F}567,50");
    assert_eq!(formatted("pt-BR", "%,.1f", &value), "1.234.567,5");
}

#[test]
fn general_conversions_cover_every_argument_kind() {
    let other = Other { class: "java.time.Duration", text: js("PT1H"), hash: -1 };
    assert_eq!(formatted("en-US", "%s %S", &[other.clone(), Text(js("straße"))]), "PT1H STRASSE");
    assert_eq!(formatted("en-US", "%h %H", &[other.clone(), Text(js("hello"))]), "ffffffff 5E918D2");
    assert_eq!(formatted("en-US", "%b %B %b", &[Null, other, Boolean(false)]), "false TRUE false");
    assert_eq!(formatted("en-US", "%h %h", &[decimal("1.5"), Boolean(true)]), "1d2 4cf");
    assert_eq!(formatted("en-US", "%.2s|%5.1S|", &[Text(js("abc")), Null]), "ab|    N|");
    assert_eq!(formatted("en-US", "%d %X %c %.2e", &[Null, Null, Null, Null]), "null NULL null nu");
}

#[test]
fn argument_indexes_follow_java() {
    let arguments = [Text(js("a")), Text(js("b"))];
    assert_eq!(formatted("en-US", "%2$s %s %<s %s", &arguments), "b a a b");
    assert_eq!(formatted("en-US", "%5%|%-3%|%n", &[]), "    %|%  |\n");
}

#[test]
fn date_times_are_localized() {
    let value = [date_time("UTC")];
    assert_eq!(formatted("en-US", "%tc", &value), "Tue Mar 05 14:07:09 UTC 2024");
    assert_eq!(formatted("de-DE", "%tc", &value), "Di. März 05 14:07:09 UTC 2024");
    assert_eq!(formatted("de-DE", "%TB", &value), "MÄRZ");
    assert_eq!(formatted("pt-BR", "%tA", &value), "terça-feira");
    assert_eq!(
        formatted("en-US", "%tr|%tp|%Tp", &[value[0].clone(), value[0].clone(), value[0].clone()]),
        "02:07:09 PM|pm|PM"
    );
    assert_eq!(formatted("en-US", "%tL %1$tN %1$tj %1$te %1$ty", &value), "123 123456789 065 5 24");
    assert_eq!(formatted("en-US", "%tF %1$tD %1$tR", &value), "2024-03-05 03/05/24 14:07");
    assert_eq!(formatted("en-US", "%ts %1$tQ", &value), "1709647629 1709647629123");
}

#[test]
fn zones_print_ids_or_time_zone_names() {
    assert_eq!(formatted("en-US", "%tZ %1$tz", &[date_time("+05:30")]), "+05:30 +0530");
    assert_eq!(formatted("en-US", "%tZ", &[date_time("GMT+05:00")]), "GMT+05:00");
    assert_eq!(formatted("en-US", "%tZ", &[date_time("UTC+01:00")]), "GMT");
    assert_eq!(formatted("fr-FR", "%tZ", &[date_time("Etc/GMT")]), "UTC");
}

#[test]
fn date_times_print_and_hash_like_java() {
    let value = [date_time("UTC")];
    assert_eq!(formatted("en-US", "%s", &value), "2024-03-05T14:07:09.123456789Z[UTC]");
    assert_eq!(formatted("en-US", "%h", &value), "93ec2dca");
}

#[test]
fn argument_type_mismatches_name_the_class() {
    let illegal = "java.util.IllegalFormatConversionException";
    assert_eq!(failure("%d", &[Text(js("a"))]), (illegal.into(), "d != java.lang.String".into()));
    assert_eq!(failure("%X", &[decimal("1")]), (illegal.into(), "x != java.math.BigDecimal".into()));
    assert_eq!(failure("%A", &[decimal("1")]), (illegal.into(), "a != java.math.BigDecimal".into()));
    assert_eq!(failure("%tH", &[Boolean(true)]), (illegal.into(), "H != java.lang.Boolean".into()));
}

#[test]
fn missing_arguments_use_the_canonical_specifier() {
    let missing = "java.util.MissingFormatArgumentException";
    assert_eq!(failure("%1$<s", &[]), (missing.into(), "Format specifier '%<s'".into()));
    assert_eq!(failure("%-5.2S", &[]), (missing.into(), "Format specifier '%-5.2S'".into()));
}

#[test]
fn format_errors_win_over_argument_errors() {
    let (class, message) = failure("%s %q", &[]);
    assert_eq!(class, "java.util.UnknownFormatConversionException");
    assert_eq!(message, "Conversion = 'q'");
}

#[test]
fn flag_errors_list_all_offending_flags() {
    assert_eq!(failure("%+ s", &[]).1, "Conversion = s, Flags = + ");
    assert_eq!(failure("%-08.2X", &[]).1, "Flags = '-^0'");
    assert_eq!(failure("%#s", &[Null]).1, "Conversion = s, Flags = #");
}

#[test]
fn oversized_output_reports_exhausted_memory() {
    let out_of_memory = "java.lang.OutOfMemoryError";
    assert_eq!(failure("%2147483647s", &[Null]).0, out_of_memory);
    assert_eq!(failure("%.2147483647f", &[decimal("1.5")]).0, out_of_memory);
    assert_eq!(failure("%.2147483647e", &[decimal("1.5")]).0, out_of_memory);
    assert_eq!(failure("%.2147483647g", &[decimal("1.5")]).0, out_of_memory);
    assert_eq!(failure("%f", &[decimal("1E+2147483647")]).0, out_of_memory);
}
