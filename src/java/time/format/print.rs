//! Printing a [`ZonedDateTime`] with a formatter.

use super::{DateTimeFormatter, FractionSpec, NumberSpec, OffsetLayout, Op, SignStyle, TextSpec};
use crate::java::locale::Locale;
use crate::java::time::date_time_error;
use crate::java::time::field::Field;
use crate::java::time::local::LocalDate;
use crate::java::time::zoned::ZonedDateTime;
use crate::java::{JResult, JString};

/// The value of a field on a date-time.
fn field_value(value: &ZonedDateTime, field: Field) -> JResult<i64> {
    if field == Field::OffsetSeconds {
        return Ok(i64::from(value.offset_seconds()));
    }
    if let Some(result) = field.of_date(&value.date()) {
        return result;
    }
    Ok(field.of_time(&value.time()).unwrap_or(0))
}

/// The value a reduced field prints: its last digits.
fn reduced_value(value: i64, width: usize) -> i64 {
    value.unsigned_abs() as i64 % 10i64.pow(width as u32)
}

/// The value of a field on 2000-01-01, the base date of two-digit years.
pub(super) fn year_2000_value(field: Field) -> i64 {
    LocalDate::of(2000, 1, 1).ok().and_then(|date| field.of_date(&date)).and_then(Result::ok).unwrap_or(2000)
}

fn number_error(spec: &NumberSpec, value: i64, detail: String) -> crate::java::Thrown {
    let field = spec.field;
    date_time_error(&format!("Field {field} cannot be printed as the value {value} {detail}"))
}

fn print_number(spec: &NumberSpec, value: i64, out: &mut JString) -> JResult<()> {
    let printed = if spec.reduced { reduced_value(value, spec.max_width) } else { value };
    let digits = printed.unsigned_abs().to_string();
    if digits.len() > spec.max_width {
        let detail = format!("exceeds the maximum print width of {}", spec.max_width);
        return Err(number_error(spec, printed, detail));
    }
    if printed >= 0 {
        let exceeds = spec.min_width < 19 && printed >= 10i64.pow(spec.min_width as u32);
        if spec.sign == SignStyle::ExceedsPad && exceeds {
            out.push(u16::from(b'+'));
        }
    } else if spec.sign == SignStyle::NotNegative {
        let detail = "cannot be negative according to the SignStyle".to_string();
        return Err(number_error(spec, printed, detail));
    } else {
        out.push(u16::from(b'-'));
    }
    let padding = spec.min_width.saturating_sub(digits.len());
    out.extend(std::iter::repeat_n(u16::from(b'0'), padding));
    out.extend(digits.bytes().map(u16::from));
    Ok(())
}

fn print_fraction(spec: &FractionSpec, nanos: i64, out: &mut JString) {
    let digits = format!("{nanos:09}");
    let significant = digits.trim_end_matches('0').len();
    let width = significant.clamp(spec.min_width, spec.max_width);
    if width == 0 {
        return;
    }
    if spec.decimal_point {
        out.push(u16::from(b'.'));
    }
    out.extend(digits.bytes().take(width).map(u16::from));
}

fn print_text(spec: &TextSpec, locale: &Locale, value: i64, out: &mut JString) -> JResult<()> {
    let texts =
        if spec.english_only { Locale::US.texts(spec.table, spec.style) } else { locale.texts(spec.table, spec.style) };
    let first = i64::from(!matches!(spec.field, Field::Era | Field::AmPmOfDay));
    let text = usize::try_from(value - first).ok().and_then(|i| texts.get(i));
    match text {
        Some(text) => {
            out.extend(text.encode_utf16());
            Ok(())
        }
        None => print_number(&NumberSpec::plain(spec.field), value, out),
    }
}

fn push_two_digits(prefix: Option<char>, value: i32, out: &mut JString) {
    let text = match prefix {
        Some(prefix) => format!("{prefix}{value:02}"),
        None => format!("{value:02}"),
    };
    out.extend(text.bytes().map(u16::from));
}

/// Splits an offset into absolute hours, minutes and seconds.
fn offset_parts(total: i32) -> (i32, i32, i32) {
    ((total / 3600 % 100).abs(), (total / 60 % 60).abs(), (total % 60).abs())
}

fn print_offset(layout: OffsetLayout, no_offset: &str, total: i32, out: &mut JString) {
    let (hours, minutes, seconds) = offset_parts(total);
    let colon = layout.has_colon().then_some(':');
    let shows_minutes = minutes > 0 || !matches!(layout, OffsetLayout::OptionalMinutes);
    let may_show_seconds = matches!(layout, OffsetLayout::OptionalSeconds | OffsetLayout::ColonOptionalSeconds);
    if hours + if shows_minutes { minutes } else { 0 } + if may_show_seconds { seconds } else { 0 } == 0 {
        out.extend(no_offset.bytes().map(u16::from));
        return;
    }
    out.push(u16::from(if total < 0 { b'-' } else { b'+' }));
    push_two_digits(None, hours, out);
    if shows_minutes {
        push_two_digits(colon, minutes, out);
        if may_show_seconds && seconds > 0 {
            push_two_digits(colon, seconds, out);
        }
    }
}

fn print_localized_offset(full: bool, locale: &Locale, total: i32, out: &mut JString) {
    out.extend(locale.gmt_text().encode_utf16());
    if total == 0 {
        return;
    }
    let (hours, minutes, seconds) = offset_parts(total);
    out.push(u16::from(if total < 0 { b'-' } else { b'+' }));
    if full {
        push_two_digits(None, hours, out);
        push_two_digits(Some(':'), minutes, out);
    } else {
        out.extend(hours.to_string().bytes().map(u16::from));
        if minutes != 0 || seconds != 0 {
            push_two_digits(Some(':'), minutes, out);
        }
    }
    if seconds != 0 {
        push_two_digits(Some(':'), seconds, out);
    }
}

/// Prints one operation; `Ok(false)` means the value is missing and the section is dropped.
fn print_op(
    op: &Op,
    formatter: &DateTimeFormatter,
    value: &ZonedDateTime,
    optional_depth: usize,
    out: &mut JString,
) -> JResult<bool> {
    let read = |field| match field_value(value, field) {
        Err(_) if optional_depth > 0 => Ok(None),
        other => other.map(Some),
    };
    match op {
        Op::Literal(text) => out.extend_from_slice(text),
        Op::Number(spec) => match read(spec.field)? {
            Some(number) => print_number(spec, number, out)?,
            None => return Ok(false),
        },
        Op::Fraction(spec) => print_fraction(spec, i64::from(value.nano()), out),
        Op::Text(spec) => match read(spec.field)? {
            Some(number) => print_text(spec, &formatter.locale, number, out)?,
            None => return Ok(false),
        },
        Op::Offset { layout, no_offset } => {
            print_offset(*layout, no_offset, value.offset_seconds(), out);
        }
        Op::LocalizedOffset { full } => {
            print_localized_offset(*full, &formatter.locale, value.offset_seconds(), out);
        }
        Op::Zone { region_only } => {
            if *region_only && value.zone().is_offset() {
                return Ok(false);
            }
            out.extend_from_slice(value.zone().id());
        }
        Op::Padded { width, inner } => {
            let start = out.len();
            if !print_op(inner, formatter, value, optional_depth, out)? {
                return Ok(false);
            }
            let length = out.len() - start;
            if length > *width {
                return Err(date_time_error(&format!(
                    "Cannot print as output of {length} characters exceeds pad width of {width}"
                )));
            }
            let padding = vec![u16::from(b' '); width - length];
            out.splice(start..start, padding);
        }
        Op::CaseSensitive(_) | Op::Strict(_) | Op::OptionalStart { .. } | Op::OptionalEnd => {}
    }
    Ok(true)
}

/// A section being printed: its end and the output length to restore if it is dropped.
struct OpenSection {
    end: usize,
    output_length: usize,
}

/// `DateTimeFormatter.format`.
pub(super) fn format(formatter: &DateTimeFormatter, value: &ZonedDateTime) -> JResult<JString> {
    let mut out = JString::new();
    let mut sections: Vec<OpenSection> = Vec::new();
    let mut index = 0;
    while let Some(op) = formatter.ops.get(index) {
        index += 1;
        match op {
            Op::OptionalStart { end } => sections.push(OpenSection { end: *end, output_length: out.len() }),
            Op::OptionalEnd => {
                sections.pop();
            }
            _ => {
                if !print_op(op, formatter, value, sections.len(), &mut out)? {
                    if let Some(section) = sections.pop() {
                        out.truncate(section.output_length);
                        index = section.end + 1;
                    }
                }
            }
        }
    }
    Ok(out)
}
