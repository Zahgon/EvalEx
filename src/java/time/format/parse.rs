//! Parsing text into unresolved fields, one [`Op`] at a time.

use super::print::year_2000_value;
use super::resolve::{Parsed, Resolved};
use super::{
    fixed_number, iso_date_ops, DateTimeFormatter, FractionSpec, NumberSpec, OffsetLayout, Op, SignStyle, TextSpec,
    OFFSET_ID,
};
use crate::java::chars::{lower_unit, upper_unit};
use crate::java::locale::Locale;
use crate::java::time::field::Field;
use crate::java::time::instant::Instant;
use crate::java::time::local::LocalDateTime;
use crate::java::time::multiply_exact;
use crate::java::time::zone::{region_ids, ZoneId};
use crate::java::{js, JResult, JString, Thrown};

const PARSE_EXCEPTION: &str = "java.time.format.DateTimeParseException";
const INDEX_EXCEPTION: &str = "java.lang.StringIndexOutOfBoundsException";
const SECONDS_PER_10000_YEARS: i64 = 146_097 * 25 * 86_400;

const PLUS: u16 = 0x2B;
const MINUS: u16 = 0x2D;
const COLON: u16 = 0x3A;
const ZERO: u16 = 0x30;
const PERIOD: u16 = 0x2E;
const LETTER_Z: u16 = 0x5A;
const SPACE: u16 = 0x20;

/// Why a parse step stopped.
pub(super) enum Failure {
    /// The text does not match; the value is the error index.
    At(usize),
    /// A runtime exception, reported with the text it occurred in.
    Thrown(Thrown),
}

impl From<Thrown> for Failure {
    fn from(thrown: Thrown) -> Failure {
        Failure::Thrown(thrown)
    }
}

/// The result of a step: the position after the consumed text.
type Step = Result<usize, Failure>;

/// `DateTimeParseContext`.
struct Context {
    locale: Locale,
    case_sensitive: bool,
    strict: bool,
    parsed: Parsed,
}

fn digit(unit: u16) -> Option<i64> {
    (ZERO..ZERO + 10).contains(&unit).then(|| i64::from(unit - ZERO))
}

impl Context {
    fn char_equals(&self, a: u16, b: u16) -> bool {
        a == b || (!self.case_sensitive && (upper_unit(a) == upper_unit(b) || lower_unit(a) == lower_unit(b)))
    }

    fn matches_at(&self, text: &[u16], pos: usize, expected: impl Iterator<Item = u16>) -> bool {
        let mut offset = pos;
        for unit in expected {
            match text.get(offset) {
                Some(&actual) if self.char_equals(actual, unit) => offset += 1,
                _ => return false,
            }
        }
        true
    }

    /// `setParsedField`: a second, different value for a field fails at `error_pos`.
    fn set_field(&mut self, field: Field, value: i64, error_pos: usize, success: usize) -> Step {
        match self.parsed.insert(field, value) {
            Some(previous) if previous != value => Err(Failure::At(error_pos)),
            _ => Ok(success),
        }
    }

    fn parse_literal(&self, literal: &[u16], text: &[u16], pos: usize) -> Step {
        if self.matches_at(text, pos, literal.iter().copied()) {
            Ok(pos + literal.len())
        } else {
            Err(Failure::At(pos))
        }
    }

    fn is_fixed_width(&self, spec: &NumberSpec) -> bool {
        if spec.reduced && !self.strict {
            return false;
        }
        spec.subsequent_width == -1
            || (spec.subsequent_width > 0 && spec.min_width == spec.max_width && spec.sign == SignStyle::NotNegative)
    }

    fn sign_allowed(&self, spec: &NumberSpec, positive: bool) -> bool {
        match spec.sign {
            SignStyle::Normal => !positive || !self.strict,
            SignStyle::ExceedsPad => true,
            SignStyle::NotNegative => !self.strict && spec.min_width != spec.max_width,
        }
    }

    fn parse_number(&mut self, spec: &NumberSpec, text: &[u16], start: usize) -> Step {
        let Some(&first) = text.get(start) else {
            return Err(Failure::At(start));
        };
        let positive = first == PLUS;
        let negative = first == MINUS;
        if (positive || negative) && !self.sign_allowed(spec, positive) {
            return Err(Failure::At(start));
        }
        let position = start + usize::from(positive || negative);
        let bounded = self.strict || self.is_fixed_width(spec);
        let min_width = if bounded { spec.min_width } else { 1 };
        let min_end = position + min_width;
        if min_end > text.len() {
            return Err(Failure::At(position));
        }
        let subsequent = spec.subsequent_width.max(0) as usize;
        let mut max_width = if bounded { spec.max_width } else { 9 } + subsequent;
        let mut pass = 0;
        let (mut pos, mut magnitude);
        loop {
            pos = position;
            magnitude = 0u128;
            let limit = (position + max_width).min(text.len());
            while pos < limit {
                let Some(value) = text.get(pos).copied().and_then(digit) else {
                    if pos < min_end {
                        return Err(Failure::At(position));
                    }
                    break;
                };
                magnitude = magnitude.wrapping_mul(10).wrapping_add(value as u128);
                pos += 1;
            }
            if subsequent > 0 && pass == 0 {
                max_width = min_width.max((pos - position).saturating_sub(subsequent));
                pass = 1;
            } else {
                break;
            }
        }
        let digits = pos - position;
        if negative {
            if magnitude == 0 && self.strict {
                return Err(Failure::At(position - 1));
            }
        } else if spec.sign == SignStyle::ExceedsPad && self.strict {
            if positive && digits <= spec.min_width {
                return Err(Failure::At(position - 1));
            }
            if !positive && digits > spec.min_width {
                return Err(Failure::At(position));
            }
        }
        let limit = if negative { 1u128 << 63 } else { (1u128 << 63) - 1 };
        if magnitude > limit {
            magnitude /= 10;
            pos -= 1;
        }
        let value = if negative { (magnitude as i128).wrapping_neg() as i64 } else { magnitude as i64 };
        self.set_number(spec, value, position, pos)
    }

    fn set_number(&mut self, spec: &NumberSpec, value: i64, error_pos: usize, end: usize) -> Step {
        let mut value = value;
        if spec.reduced && end - error_pos == spec.min_width && value >= 0 {
            let base = year_2000_value(spec.field);
            let range = 10i64.pow(spec.min_width as u32);
            let last_part = base % range;
            value = if base > 0 { base - last_part + value } else { base - last_part - value };
            if value < base {
                value += range;
            }
        }
        self.set_field(spec.field, value, error_pos, end)
    }

    fn parse_fraction(&mut self, spec: &FractionSpec, text: &[u16], start: usize) -> Step {
        let fixed = self.strict && spec.min_width == spec.max_width && !spec.decimal_point;
        let bounded = self.strict || fixed;
        let min_width = if bounded { spec.min_width } else { 0 };
        let max_width = if bounded { spec.max_width } else { 9 };
        if start >= text.len() {
            return if min_width > 0 { Err(Failure::At(start)) } else { Ok(start) };
        }
        let mut position = start;
        if spec.decimal_point {
            if text.get(position) != Some(&PERIOD) {
                return if min_width > 0 { Err(Failure::At(position)) } else { Ok(position) };
            }
            position += 1;
        }
        let min_end = position + min_width;
        if min_end > text.len() {
            return Err(Failure::At(position));
        }
        let limit = (position + max_width).min(text.len());
        let mut pos = position;
        let mut total = 0i64;
        while pos < limit {
            let Some(value) = text.get(pos).copied().and_then(digit) else {
                break;
            };
            total = total * 10 + value;
            pos += 1;
        }
        if pos < min_end {
            return Err(Failure::At(position));
        }
        for _ in (pos - position)..9 {
            total *= 10;
        }
        self.set_field(Field::NanoOfSecond, total, position, pos)
    }

    fn parse_text(&mut self, spec: &TextSpec, text: &[u16], pos: usize) -> Step {
        let locale = if spec.english_only { Locale::US } else { self.locale };
        let names = locale.texts(spec.table, spec.style);
        let first = i64::from(!matches!(spec.field, Field::Era | Field::AmPmOfDay));
        if !names.is_empty() {
            for (index, name) in longest_first(names) {
                if self.matches_at(text, pos, name.encode_utf16()) {
                    let end = pos + name.encode_utf16().count();
                    return self.set_field(spec.field, index as i64 + first, pos, end);
                }
            }
            if self.strict {
                return Err(Failure::At(pos));
            }
        }
        self.parse_number(&NumberSpec::plain(spec.field), text, pos)
    }

    fn parse_offset(&mut self, layout: OffsetLayout, no_offset: &str, text: &[u16], pos: usize) -> Step {
        if pos == text.len() {
            return Err(Failure::At(pos));
        }
        if self.matches_at(text, pos, no_offset.encode_utf16()) {
            let end = pos + no_offset.len();
            return self.set_field(Field::OffsetSeconds, 0, pos, end);
        }
        let sign = match text.get(pos).copied() {
            Some(PLUS) => 1,
            Some(MINUS) => -1,
            _ => return Err(Failure::At(pos)),
        };
        let mut reader = OffsetReader::new(text, pos + 1);
        reader.read_for(layout, self.strict);
        let Some(end) = reader.end() else {
            return Err(Failure::At(pos));
        };
        let [hours, minutes, seconds] = reader.parts;
        if hours > 23 || minutes > 59 || seconds > 59 {
            return Err(Thrown::new(
                crate::java::time::DATE_TIME_EXCEPTION,
                "Value out of range: Hour[0-23], Minute[0-59], Second[0-59]",
            )
            .into());
        }
        let total = sign * (hours * 3600 + minutes * 60 + seconds);
        self.set_field(Field::OffsetSeconds, total, pos, end)
    }

    fn parse_localized_offset(&mut self, full: bool, text: &[u16], start: usize) -> Step {
        let prefix = self.locale.gmt_text();
        if !self.matches_at(text, start, prefix.encode_utf16()) {
            return Err(Failure::At(start));
        }
        let mut pos = start + prefix.encode_utf16().count();
        let sign = match text.get(pos).copied() {
            Some(PLUS) => 1,
            Some(MINUS) => -1,
            _ => return self.set_field(Field::OffsetSeconds, 0, start, pos),
        };
        pos += 1;
        let parts = if full { read_full_offset(text, &mut pos)? } else { read_short_offset(text, &mut pos)? };
        let Some((hours, minutes, seconds)) = parts else {
            return Err(Failure::At(start));
        };
        let total = sign * (hours * 3600 + minutes * 60 + seconds);
        self.set_field(Field::OffsetSeconds, total, start, pos)
    }

    /// A zone id: an offset, a prefixed offset or the longest matching region.
    fn parse_zone(&mut self, text: &[u16], pos: usize) -> Step {
        let Some(&first) = text.get(pos) else {
            return Err(Failure::At(pos));
        };
        if first == PLUS || first == MINUS {
            return self.parse_prefixed_zone(text, pos, pos, true);
        }
        let is_char = |index: usize, expected: u8| {
            text.get(index).is_some_and(|&unit| self.char_equals(unit, u16::from(expected)))
        };
        if is_char(pos, b'U') && is_char(pos + 1, b'T') {
            let prefix_end = if is_char(pos + 2, b'C') { pos + 3 } else { pos + 2 };
            return self.parse_prefixed_zone(text, pos, prefix_end, false);
        }
        if is_char(pos, b'G') && is_char(pos + 1, b'M') && is_char(pos + 2, b'T') {
            if is_char(pos + 3, b'0') {
                self.parsed.set_zone(ZoneId::of(&js("GMT0"))?);
                return Ok(pos + 4);
            }
            return self.parse_prefixed_zone(text, pos, pos + 3, false);
        }
        let longest =
            region_ids().iter().filter(|id| self.matches_at(text, pos, id.encode_utf16())).max_by_key(|id| id.len());
        if let Some(id) = longest {
            self.parsed.set_zone(ZoneId::of(&js(id))?);
            return Ok(pos + id.len());
        }
        if self.char_equals(first, LETTER_Z) {
            self.parsed.set_zone(ZoneId::utc());
            return Ok(pos + 1);
        }
        Err(Failure::At(pos))
    }

    /// The offset part of a zone id after a prefix of UTC, GMT or UT, or after nothing.
    fn parse_prefixed_zone(&mut self, text: &[u16], prefix_start: usize, pos: usize, offset_required: bool) -> Step {
        let prefix: JString = text.get(prefix_start..pos).unwrap_or(&[]).iter().map(|&unit| upper_unit(unit)).collect();
        let bare_prefix = match text.get(pos).copied() {
            None => true,
            Some(unit) => unit == ZERO || self.char_equals(unit, LETTER_Z),
        };
        if bare_prefix {
            self.parsed.set_zone(ZoneId::of(&prefix)?);
            return Ok(pos);
        }
        let mut nested = Context {
            locale: self.locale,
            case_sensitive: self.case_sensitive,
            strict: self.strict,
            parsed: Parsed::default(),
        };
        let no_offset = if offset_required { "Z" } else { "0" };
        let layout = OffsetLayout::ColonOptionalSeconds;
        let total = match nested.parse_offset(layout, no_offset, text, pos) {
            Ok(end) => nested.parsed.get(Field::OffsetSeconds).map(|total| (total, end)),
            Err(Failure::Thrown(thrown)) => return Err(thrown.into()),
            Err(Failure::At(_)) => None,
        };
        let Some((total, end)) = total else {
            if offset_required {
                return Err(Failure::At(prefix_start));
            }
            self.parsed.set_zone(ZoneId::of(&prefix)?);
            return Ok(pos);
        };
        let Ok(offset) = ZoneId::of_total_seconds(total as i32) else {
            return Err(Failure::At(prefix_start));
        };
        let zone = if prefix.is_empty() { offset } else { ZoneId::of_prefixed(&prefix, total as i32) };
        self.parsed.set_zone(zone);
        Ok(end)
    }

    fn parse_padded(&mut self, width: usize, inner: &Op, text: &[u16], position: usize) -> Step {
        if position == text.len() {
            return Err(Failure::At(position));
        }
        let mut end = position + width;
        if end > text.len() {
            if self.strict {
                return Err(Failure::At(position));
            }
            end = text.len();
        }
        let mut pos = position;
        while pos < end && text.get(pos).is_some_and(|&unit| self.char_equals(unit, SPACE)) {
            pos += 1;
        }
        let bounded = text.get(..end).unwrap_or(text);
        match self.parse_op(inner, bounded, pos) {
            Err(Failure::At(_)) if self.strict => Err(Failure::At(position + pos)),
            Ok(result) if result != end && self.strict => Err(Failure::At(position + pos)),
            other => other,
        }
    }

    fn parse_op(&mut self, op: &Op, text: &[u16], pos: usize) -> Step {
        match op {
            Op::Literal(literal) => self.parse_literal(literal, text, pos),
            Op::Number(spec) => self.parse_number(spec, text, pos),
            Op::Fraction(spec) => self.parse_fraction(spec, text, pos),
            Op::Text(spec) => self.parse_text(spec, text, pos),
            Op::Offset { layout, no_offset } => self.parse_offset(*layout, no_offset, text, pos),
            Op::LocalizedOffset { full } => self.parse_localized_offset(*full, text, pos),
            Op::Zone { .. } => self.parse_zone(text, pos),
            Op::Padded { width, inner } => self.parse_padded(*width, inner, text, pos),
            Op::CaseSensitive(value) => {
                self.case_sensitive = *value;
                Ok(pos)
            }
            Op::Strict(value) => {
                self.strict = *value;
                Ok(pos)
            }
            Op::OptionalStart { .. } | Op::OptionalEnd => Ok(pos),
        }
    }
}

/// The texts of a style with their indices, longest first; among equal texts (narrow names repeat)
/// the highest value comes first.
fn longest_first(names: &'static [&'static str]) -> Vec<(usize, &'static str)> {
    let mut entries: Vec<(usize, &'static str)> = names.iter().copied().enumerate().rev().collect();
    entries.sort_by_key(|&(_, name)| std::cmp::Reverse(name.encode_utf16().count()));
    entries
}

/// A digit of a localized offset, which the JDK reads without checking the text length.
fn digit_or_index_error(text: &[u16], pos: usize) -> Result<Option<i64>, Failure> {
    match text.get(pos) {
        Some(&unit) => Ok(digit(unit)),
        None => {
            Err(Thrown::new(INDEX_EXCEPTION, &format!("Index {pos} out of bounds for length {}", text.len())).into())
        }
    }
}

/// The two digits after a colon at `pos`, when the whole group fits in the text.
fn colon_group(text: &[u16], pos: usize) -> Option<i64> {
    if text.get(pos) != Some(&COLON) || pos + 2 >= text.len() {
        return None;
    }
    let high = text.get(pos + 1).copied().and_then(digit)?;
    let low = text.get(pos + 2).copied().and_then(digit)?;
    Some(high * 10 + low)
}

type OffsetParts = Option<(i64, i64, i64)>;

/// `HH:mm[:ss]` of `OOOO`; `Ok(None)` is a mismatch.
fn read_full_offset(text: &[u16], pos: &mut usize) -> Result<OffsetParts, Failure> {
    let hour_high = digit_or_index_error(text, *pos)?;
    let hour_low = digit_or_index_error(text, *pos + 1)?;
    let (Some(hour_high), Some(hour_low)) = (hour_high, hour_low) else {
        return Ok(None);
    };
    match text.get(*pos + 2) {
        Some(&COLON) => {}
        Some(_) => return Ok(None),
        None => {
            digit_or_index_error(text, *pos + 2)?;
        }
    }
    let minute_high = digit_or_index_error(text, *pos + 3)?;
    let minute_low = digit_or_index_error(text, *pos + 4)?;
    let (Some(minute_high), Some(minute_low)) = (minute_high, minute_low) else {
        return Ok(None);
    };
    *pos += 5;
    let seconds = colon_group(text, *pos);
    if seconds.is_some() {
        *pos += 3;
    }
    let hours = hour_high * 10 + hour_low;
    let minutes = minute_high * 10 + minute_low;
    Ok(Some((hours, minutes, seconds.unwrap_or(0))))
}

/// `h[h][:mm[:ss]]` of `O`; `Ok(None)` is a mismatch.
fn read_short_offset(text: &[u16], pos: &mut usize) -> Result<OffsetParts, Failure> {
    let Some(mut hours) = digit_or_index_error(text, *pos)? else {
        return Ok(None);
    };
    *pos += 1;
    if let Some(low) = text.get(*pos).copied().and_then(digit) {
        hours = hours * 10 + low;
        *pos += 1;
    }
    let Some(minutes) = colon_group(text, *pos) else {
        return Ok(Some((hours, 0, 0)));
    };
    *pos += 3;
    let seconds = colon_group(text, *pos);
    if seconds.is_some() {
        *pos += 3;
    }
    Ok(Some((hours, minutes, seconds.unwrap_or(0))))
}

/// Reads hour, minute and second groups of an offset; `next` becomes `None` on a mismatch.
struct OffsetReader<'a> {
    text: &'a [u16],
    next: Option<usize>,
    /// Hour, minute and second.
    parts: [i64; 3],
}

impl<'a> OffsetReader<'a> {
    fn new(text: &'a [u16], start: usize) -> OffsetReader<'a> {
        OffsetReader { text, next: Some(start), parts: [0; 3] }
    }

    fn end(&self) -> Option<usize> {
        self.next
    }

    /// Reads a two-digit group; on a mismatch the position stays and `false` is returned.
    fn read_group(&mut self, colon: bool, index: usize) -> bool {
        let Some(mut pos) = self.next else {
            return true;
        };
        if colon && index > 0 {
            if self.text.get(pos) != Some(&COLON) {
                return false;
            }
            pos += 1;
        }
        let high = self.text.get(pos).copied().and_then(digit);
        let low = self.text.get(pos + 1).copied().and_then(digit);
        let (Some(high), Some(low)) = (high, low) else {
            return false;
        };
        let value = high * 10 + low;
        if value > 59 {
            return false;
        }
        if let Some(part) = self.parts.get_mut(index) {
            *part = value;
        }
        self.next = Some(pos + 2);
        true
    }

    fn require_group(&mut self, colon: bool, index: usize) {
        if !self.read_group(colon, index) {
            self.next = None;
        }
    }

    fn read_for(&mut self, layout: OffsetLayout, strict: bool) {
        let colon = layout.has_colon();
        self.require_group(false, 0);
        if !strict {
            if self.read_group(colon, 1) {
                self.read_group(colon, 2);
            }
            return;
        }
        match layout {
            OffsetLayout::OptionalMinutes => {
                self.read_group(colon, 1);
            }
            OffsetLayout::Minutes | OffsetLayout::ColonMinutes => self.require_group(colon, 1),
            OffsetLayout::OptionalSeconds | OffsetLayout::ColonOptionalSeconds => {
                self.require_group(colon, 1);
                self.read_group(colon, 2);
            }
        }
    }
}

/// `text` for messages: long values are cut after 64 units.
fn abbreviate(text: &[u16]) -> JString {
    match text.get(..64) {
        Some(head) if text.len() > 64 => {
            let mut short = head.to_vec();
            short.extend(js("..."));
            short
        }
        _ => text.to_vec(),
    }
}

fn parse_exception(text: &[u16], detail: &str) -> Thrown {
    let mut message = js("Text '");
    message.extend(abbreviate(text));
    message.extend(js(detail));
    Thrown::with_message(PARSE_EXCEPTION, message)
}

/// `createError`: a runtime exception raised while parsing `text`.
fn wrapped_exception(text: &[u16], thrown: &Thrown) -> Thrown {
    let mut message = js("Text '");
    message.extend(abbreviate(text));
    message.extend(js("' could not be parsed: "));
    message.extend_from_slice(thrown.message.as_deref().unwrap_or(&[]));
    Thrown::with_message(PARSE_EXCEPTION, message)
}

/// Runs the operations of a formatter from the start of `text`.
fn run_ops(ops: &[Op], context: &mut Context, text: &[u16]) -> Step {
    struct OpenSection {
        end: usize,
        start_pos: usize,
        saved: Parsed,
    }
    let mut sections: Vec<OpenSection> = Vec::new();
    let mut pos = 0;
    let mut index = 0;
    while let Some(op) = ops.get(index) {
        index += 1;
        match op {
            Op::OptionalStart { end } => {
                sections.push(OpenSection { end: *end, start_pos: pos, saved: context.parsed.clone() })
            }
            Op::OptionalEnd => {
                sections.pop();
            }
            _ => match context.parse_op(op, text, pos) {
                Ok(next) => pos = next,
                Err(Failure::At(error)) => {
                    let Some(section) = sections.pop() else {
                        return Err(Failure::At(error));
                    };
                    context.parsed = section.saved;
                    pos = section.start_pos;
                    index = section.end + 1;
                }
                Err(failure) => return Err(failure),
            },
        }
    }
    Ok(pos)
}

/// The error of a failed run of the operations.
fn run_error(text: &[u16], failure: &Failure) -> Thrown {
    match failure {
        Failure::At(index) => parse_exception(text, &format!("' could not be parsed at index {index}")),
        Failure::Thrown(thrown) => wrapped_exception(text, thrown),
    }
}

/// Requires the whole text to be consumed.
fn check_consumed(text: &[u16], end: usize) -> JResult<()> {
    if end < text.len() {
        let detail = format!("' could not be parsed, unparsed text found at index {end}");
        return Err(parse_exception(text, &detail));
    }
    Ok(())
}

/// `DateTimeFormatter.parse(text)`.
pub(super) fn parse_resolved(formatter: &DateTimeFormatter, text: &[u16]) -> JResult<Resolved> {
    let mut context =
        Context { locale: formatter.locale, case_sensitive: true, strict: true, parsed: Parsed::default() };
    let end = run_ops(&formatter.ops, &mut context, text).map_err(|f| run_error(text, &f))?;
    check_consumed(text, end)?;
    context.parsed.resolve(formatter.resolver).map_err(|thrown| wrapped_exception(text, &thrown))
}

/// The instant of the fields read by the `ISO_INSTANT` parser, `None` if they do not form one.
/// The year wraps to an `int` before the 10,000-year cycles are split off, as in the JDK.
fn instant_seconds(parsed: &Parsed) -> Option<i64> {
    let field = |field| parsed.get(field);
    let year_field = field(Field::Year)?;
    let (mut hour, minute, mut second) =
        (field(Field::HourOfDay)? as i32, field(Field::MinuteOfHour)? as i32, field(Field::SecondOfMinute)? as i32);
    let nano = field(Field::NanoOfSecond).unwrap_or(0);
    let mut days = 0;
    if hour == 24 && minute == 0 && second == 0 && nano == 0 {
        hour = 0;
        days = 1;
    } else if hour == 23 && minute == 59 && second == 60 {
        second = 59;
    }
    let date_time = LocalDateTime::of(
        (year_field as i32) % 10_000,
        field(Field::MonthOfYear)? as i32,
        field(Field::DayOfMonth)? as i32,
        hour,
        minute,
        second,
        0,
    )
    .ok()?;
    let date = date_time.date().plus_days(days).ok()?;
    let local = LocalDateTime::new(date, date_time.time()).local_epoch_second();
    let offset = ZoneId::of_total_seconds(field(Field::OffsetSeconds)? as i32).ok()?;
    let seconds = local - i64::from(offset.offset_at(0).ok()?);
    let cycles = multiply_exact(year_field / 10_000, SECONDS_PER_10000_YEARS).ok()?;
    seconds.checked_add(cycles)
}

/// `Instant.parse`, through the `ISO_INSTANT` grammar: a seconds-precision date-time with a fraction
/// of up to nine digits and an offset, where 24:00:00 and a leap second are accepted.
pub(crate) fn parse_instant(text: &[u16]) -> JResult<Instant> {
    let mut ops = vec![Op::CaseSensitive(false)];
    ops.extend(iso_date_ops());
    ops.extend([
        Op::Literal(js("T")),
        fixed_number(Field::HourOfDay, 2),
        Op::Literal(js(":")),
        fixed_number(Field::MinuteOfHour, 2),
        Op::Literal(js(":")),
        fixed_number(Field::SecondOfMinute, 2),
        Op::Fraction(FractionSpec { min_width: 0, max_width: 9, decimal_point: true, subsequent_width: 0 }),
        OFFSET_ID,
    ]);
    let mut context = Context { locale: Locale::US, case_sensitive: true, strict: true, parsed: Parsed::default() };
    let end = run_ops(&ops, &mut context, text).map_err(|f| run_error(text, &f))?;
    let Some(seconds) = instant_seconds(&context.parsed) else {
        return Err(run_error(text, &Failure::At(0)));
    };
    check_consumed(text, end)?;
    let nano = context.parsed.get(Field::NanoOfSecond).unwrap_or(0);
    let resolved = Parsed::of_instant_fields(seconds, nano);
    Instant::of_epoch_second(seconds, nano).map_err(|_| wrapped_exception(text, &resolved.instant_error()))
}
