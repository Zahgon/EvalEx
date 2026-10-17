//! `java.time.format.DateTimeFormatter`.
//!
//! A formatter is a flat list of [`Op`]s. Optional sections are bracketed by jump instructions
//! rather than nested, so patterns of any size are processed without recursion.

mod parse;
mod pattern;
mod print;
mod resolve;

use super::field::{Field, WeekKind};
use super::instant::Instant;
use super::zone::ZoneId;
use super::zoned::ZonedDateTime;
use crate::java::locale::{Locale, TextField, TextStyle};
use crate::java::{js, JResult, JString, Thrown};

pub(crate) use parse::parse_instant;
use resolve::Resolved;

const NULL_POINTER_EXCEPTION: &str = "java.lang.NullPointerException";
/// Helpful message of the JVM for the date-less, time-less case of `parseToInstant`.
const NULL_PARSED_DATE: &str = "Cannot invoke \"java.time.LocalDate.atStartOfDay()\" because \"parsedDate\" is null";
/// Message of `Objects.requireNonNull` in `LocalDateTime.of(date, time)`.
const NULL_DATE: &str = "date";

/// `java.time.format.SignStyle`, as far as formatters reach it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SignStyle {
    Normal,
    NotNegative,
    ExceedsPad,
}

/// `java.time.format.ResolverStyle`, as far as formatters reach it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResolverStyle {
    Strict,
    Smart,
}

/// A numeric value, optionally reduced to its last digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NumberSpec {
    field: Field,
    min_width: usize,
    max_width: usize,
    sign: SignStyle,
    /// Width reserved for adjacent values that follow; -1 marks this value as fixed width.
    subsequent_width: i32,
    /// Two-digit form relative to the value of the field in the year 2000.
    reduced: bool,
}

impl NumberSpec {
    fn new(field: Field, min_width: usize, max_width: usize, sign: SignStyle) -> NumberSpec {
        NumberSpec { field, min_width, max_width, sign, subsequent_width: 0, reduced: false }
    }

    /// The fallback used for fields without text: `Value(field)`.
    fn plain(field: Field) -> NumberSpec {
        NumberSpec::new(field, 1, 19, SignStyle::Normal)
    }
}

/// The fraction of a second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FractionSpec {
    min_width: usize,
    max_width: usize,
    decimal_point: bool,
    /// Same meaning as in [`NumberSpec`].
    subsequent_width: i32,
}

/// A field printed as text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TextSpec {
    field: Field,
    table: TextField,
    style: TextStyle,
    /// RFC 1123 names, which do not depend on the formatter's locale.
    english_only: bool,
}

/// The offset layouts that patterns and the ISO formatters use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OffsetLayout {
    /// `+HHmm`
    OptionalMinutes,
    /// `+HHMM`
    Minutes,
    /// `+HH:MM`
    ColonMinutes,
    /// `+HHMMss`
    OptionalSeconds,
    /// `+HH:MM:ss`
    ColonOptionalSeconds,
}

impl OffsetLayout {
    fn has_colon(self) -> bool {
        matches!(self, OffsetLayout::ColonMinutes | OffsetLayout::ColonOptionalSeconds)
    }
}

/// One step of a formatter.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Op {
    Literal(JString),
    Number(NumberSpec),
    Fraction(FractionSpec),
    Text(TextSpec),
    Offset {
        layout: OffsetLayout,
        no_offset: &'static str,
    },
    LocalizedOffset {
        full: bool,
    },
    /// A zone id; `region_only` skips offset-like zones when printing.
    Zone {
        region_only: bool,
    },
    Padded {
        width: usize,
        inner: Box<Op>,
    },
    CaseSensitive(bool),
    Strict(bool),
    /// Start of an optional section; `end` is the index of its [`Op::OptionalEnd`].
    OptionalStart {
        end: usize,
    },
    OptionalEnd,
}

impl Op {
    fn with_week_rules(&self, locale: &Locale) -> Op {
        let relocated = |field: Field| match field {
            Field::Week(kind, _) => Field::Week(kind, locale.week_def()),
            other => other,
        };
        match self {
            Op::Number(spec) => Op::Number(NumberSpec { field: relocated(spec.field), ..*spec }),
            Op::Padded { width, inner } => Op::Padded { width: *width, inner: Box::new(inner.with_week_rules(locale)) },
            other => other.clone(),
        }
    }
}

/// A compiled date-time format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateTimeFormatter {
    ops: Vec<Op>,
    locale: Locale,
    resolver: ResolverStyle,
}

fn literal(text: &str) -> Op {
    Op::Literal(js(text))
}

/// `appendValue(field, width)`.
fn fixed_number(field: Field, width: usize) -> Op {
    Op::Number(NumberSpec::new(field, width, width, SignStyle::NotNegative))
}

/// `ISO_LOCAL_DATE`'s operations.
fn iso_date_ops() -> Vec<Op> {
    vec![
        Op::Number(NumberSpec::new(Field::Year, 4, 10, SignStyle::ExceedsPad)),
        literal("-"),
        fixed_number(Field::MonthOfYear, 2),
        literal("-"),
        fixed_number(Field::DayOfMonth, 2),
    ]
}

/// `ISO_LOCAL_DATE_TIME`'s operations.
fn iso_local_date_time_ops() -> Vec<Op> {
    let mut ops = vec![Op::CaseSensitive(false)];
    ops.extend(iso_date_ops());
    ops.extend([
        literal("T"),
        fixed_number(Field::HourOfDay, 2),
        literal(":"),
        fixed_number(Field::MinuteOfHour, 2),
        OPTIONAL_START,
        literal(":"),
        fixed_number(Field::SecondOfMinute, 2),
        Op::Fraction(FractionSpec { min_width: 0, max_width: 9, decimal_point: true, subsequent_width: 0 }),
        Op::OptionalEnd,
    ]);
    ops
}

/// Points every [`Op::OptionalStart`] at its [`Op::OptionalEnd`].
fn link_sections(ops: &mut [Op]) {
    let mut open = Vec::new();
    for index in 0..ops.len() {
        match ops.get(index) {
            Some(Op::OptionalStart { .. }) => open.push(index),
            Some(Op::OptionalEnd) => {
                if let Some(start) = open.pop().and_then(|start| ops.get_mut(start)) {
                    *start = Op::OptionalStart { end: index };
                }
            }
            _ => {}
        }
    }
}

const OPTIONAL_START: Op = Op::OptionalStart { end: 0 };

const OFFSET_ID: Op = Op::Offset { layout: OffsetLayout::ColonOptionalSeconds, no_offset: "Z" };

fn week_field(kind: WeekKind, locale: &Locale) -> Field {
    Field::Week(kind, locale.week_def())
}

impl DateTimeFormatter {
    fn iso(mut ops: Vec<Op>) -> DateTimeFormatter {
        link_sections(&mut ops);
        DateTimeFormatter { ops, locale: Locale::US, resolver: ResolverStyle::Strict }
    }

    /// `DateTimeFormatter.ofPattern(pattern, locale)`.
    pub fn of_pattern(pattern: &[u16], locale: &Locale) -> JResult<DateTimeFormatter> {
        Ok(DateTimeFormatter {
            ops: pattern::compile(pattern, locale)?,
            locale: *locale,
            resolver: ResolverStyle::Smart,
        })
    }

    /// `DateTimeFormatter.withLocale(locale)`.
    pub fn with_locale(&self, locale: &Locale) -> DateTimeFormatter {
        DateTimeFormatter {
            ops: self.ops.iter().map(|op| op.with_week_rules(locale)).collect(),
            locale: *locale,
            resolver: self.resolver,
        }
    }

    /// `DateTimeFormatter.ISO_DATE_TIME`.
    pub fn iso_date_time() -> DateTimeFormatter {
        let mut ops = iso_local_date_time_ops();
        ops.extend([
            OPTIONAL_START,
            OFFSET_ID,
            OPTIONAL_START,
            literal("["),
            Op::CaseSensitive(true),
            Op::Zone { region_only: true },
            literal("]"),
            Op::OptionalEnd,
            Op::OptionalEnd,
        ]);
        DateTimeFormatter::iso(ops)
    }

    /// `DateTimeFormatter.ISO_DATE`.
    pub fn iso_date() -> DateTimeFormatter {
        let mut ops = vec![Op::CaseSensitive(false)];
        ops.extend(iso_date_ops());
        ops.extend([OPTIONAL_START, OFFSET_ID, Op::OptionalEnd]);
        DateTimeFormatter::iso(ops)
    }

    /// `DateTimeFormatter.ISO_LOCAL_DATE_TIME`.
    pub fn iso_local_date_time() -> DateTimeFormatter {
        DateTimeFormatter::iso(iso_local_date_time_ops())
    }

    /// `DateTimeFormatter.ISO_LOCAL_DATE`.
    pub fn iso_local_date() -> DateTimeFormatter {
        DateTimeFormatter::iso(iso_date_ops())
    }

    /// `DateTimeFormatter.RFC_1123_DATE_TIME`.
    pub fn rfc_1123_date_time() -> DateTimeFormatter {
        let english = |field, table| Op::Text(TextSpec { field, table, style: TextStyle::Short, english_only: true });
        let mut ops = vec![
            Op::CaseSensitive(false),
            Op::Strict(false),
            OPTIONAL_START,
            english(Field::DayOfWeek, TextField::DayOfWeek),
            literal(", "),
            Op::OptionalEnd,
            Op::Number(NumberSpec::new(Field::DayOfMonth, 1, 2, SignStyle::NotNegative)),
            literal(" "),
            english(Field::MonthOfYear, TextField::Month),
            literal(" "),
            fixed_number(Field::Year, 4),
            literal(" "),
            fixed_number(Field::HourOfDay, 2),
            literal(":"),
            fixed_number(Field::MinuteOfHour, 2),
            OPTIONAL_START,
            literal(":"),
            fixed_number(Field::SecondOfMinute, 2),
            Op::OptionalEnd,
            literal(" "),
            Op::Offset { layout: OffsetLayout::Minutes, no_offset: "GMT" },
        ];
        link_sections(&mut ops);
        DateTimeFormatter { ops, locale: Locale::US, resolver: ResolverStyle::Smart }
    }

    /// `DateTimeFormatter.format(temporal)` for a zoned date-time.
    pub fn format(&self, value: &ZonedDateTime) -> JResult<JString> {
        print::format(self, value)
    }

    /// `DateTimeFormatter.parse(text)` with the parsed fields resolved.
    fn parse(&self, text: &[u16]) -> JResult<Resolved> {
        parse::parse_resolved(self, text)
    }
}

/// `DateTimeConverter.parseToInstant`: the zone of the text wins over `default_zone`, a missing
/// time means the start of the day, and text without a date fails with the JVM's
/// `NullPointerException` (the caller only catches `DateTimeException`).
pub fn parse_to_instant(formatter: &DateTimeFormatter, value: &[u16], default_zone: &ZoneId) -> JResult<Instant> {
    let parsed = formatter.parse(value)?;
    if parsed.has_zone()? {
        return parsed.to_instant();
    }
    let date = parsed.date().ok_or_else(|| {
        let message = if parsed.time().is_some() { NULL_DATE } else { NULL_PARSED_DATE };
        Thrown::new(NULL_POINTER_EXCEPTION, message)
    })?;
    let time = parsed.time().unwrap_or(super::local::LocalTime::MIDNIGHT);
    let local = super::local::LocalDateTime::new(date, time);
    Ok(local.at_zone(default_zone)?.to_instant())
}

#[cfg(test)]
mod tests;
