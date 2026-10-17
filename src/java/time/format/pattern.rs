//! Compilation of `DateTimeFormatter.ofPattern` patterns into [`Op`] lists, including the width
//! bookkeeping of adjacent value parsing.

use super::{
    fixed_number, link_sections, week_field, FractionSpec, NumberSpec, OffsetLayout, Op, SignStyle, TextSpec,
    OPTIONAL_START,
};
use crate::java::locale::{Locale, TextField, TextStyle};
use crate::java::time::field::{Field, WeekKind};
use crate::java::{js, JResult, JString, Thrown};

const ILLEGAL_ARGUMENT: &str = "java.lang.IllegalArgumentException";
const CLASS_CAST: &str = "java.lang.ClassCastException";
const PAD_AS_NUMBER: &str = "class java.time.format.DateTimeFormatterBuilder$PadPrinterParserDecorator cannot be cast to class java.time.format.DateTimeFormatterBuilder$NumberPrinterParser (java.time.format.DateTimeFormatterBuilder$PadPrinterParserDecorator and java.time.format.DateTimeFormatterBuilder$NumberPrinterParser are in module java.base of loader 'bootstrap')";
const MAX_WIDTH: usize = 19;
const MAX_FRACTION_WIDTH: usize = 9;

fn illegal(message: String) -> Thrown {
    Thrown::new(ILLEGAL_ARGUMENT, &message)
}

fn illegal_units(prefix: &str, text: &[u16], suffix: &str) -> Thrown {
    let mut message = js(prefix);
    message.extend_from_slice(text);
    message.extend(js(suffix));
    Thrown::with_message(ILLEGAL_ARGUMENT, message)
}

fn too_many(letter: u16) -> Thrown {
    illegal_units("Too many pattern letters: ", &[letter], "")
}

fn is_letter(unit: u16) -> bool {
    u8::try_from(unit).is_ok_and(|b| b.is_ascii_alphabetic())
}

/// The state of `DateTimeFormatterBuilder` while a pattern is read.
struct Builder<'a> {
    ops: Vec<Op>,
    locale: &'a Locale,
    /// Indices of the unclosed [`Op::OptionalStart`]s.
    open_sections: Vec<usize>,
    /// Width of the padding for the next value.
    pad_next: usize,
    /// Index of the value that collects the widths of its adjacent followers.
    active_value: Option<usize>,
}

impl Builder<'_> {
    fn append(&mut self, op: Op) -> usize {
        let op = if self.pad_next > 0 {
            let width = std::mem::take(&mut self.pad_next);
            Op::Padded { width, inner: Box::new(op) }
        } else {
            op
        };
        self.ops.push(op);
        self.active_value = None;
        self.ops.len() - 1
    }

    /// Appends a value, updating the widths of the value it is adjacent to.
    fn append_value(&mut self, op: Op) -> JResult<()> {
        let Some(active) = self.active_value else {
            self.active_value = Some(self.append(op));
            return Ok(());
        };
        let width = fixed_follower_width(&op);
        let Some(base) = self.ops.get_mut(active) else {
            return Ok(());
        };
        if matches!(base, Op::Padded { .. }) {
            return Err(Thrown::new(CLASS_CAST, PAD_AS_NUMBER));
        }
        match width {
            Some(width) => {
                widen_subsequent(base, width);
                self.append(with_fixed_width(op));
                self.active_value = Some(active);
            }
            None => {
                *base = with_fixed_width(base.clone());
                self.active_value = Some(self.append(op));
            }
        }
        Ok(())
    }

    fn append_number(&mut self, spec: NumberSpec) -> JResult<()> {
        self.append_value(Op::Number(spec))
    }

    /// `appendValue(field)`.
    fn value(&mut self, field: Field) -> JResult<()> {
        self.append_number(NumberSpec::plain(field))
    }

    /// `appendValue(field, width)`.
    fn fixed_value(&mut self, field: Field, width: usize) -> JResult<()> {
        self.append_value(fixed_number(field, width))
    }

    /// `appendValue(field, minWidth, maxWidth, signStyle)`.
    fn ranged_value(&mut self, field: Field, min: usize, sign: SignStyle) -> JResult<()> {
        if min > MAX_WIDTH {
            return Err(illegal(format!("The minimum width must be from 1 to 19 inclusive but was {min}")));
        }
        self.append_number(NumberSpec::new(field, min, MAX_WIDTH, sign))
    }

    fn reduced_value(&mut self, field: Field) -> JResult<()> {
        self.append_number(NumberSpec { reduced: true, ..NumberSpec::new(field, 2, 2, SignStyle::NotNegative) })
    }

    fn text(&mut self, field: Field, table: TextField, style: TextStyle) {
        self.append(Op::Text(TextSpec { field, table, style, english_only: false }));
    }

    fn offset(&mut self, layout: OffsetLayout, no_offset: &'static str) {
        self.append(Op::Offset { layout, no_offset });
    }

    fn start_optional(&mut self) {
        self.active_value = None;
        self.open_sections.push(self.ops.len());
        self.ops.push(OPTIONAL_START);
    }

    /// Closes the innermost section; an empty section leaves no trace.
    fn end_optional(&mut self) {
        let Some(start) = self.open_sections.pop() else {
            return;
        };
        if start + 1 == self.ops.len() {
            self.ops.pop();
            return;
        }
        self.ops.push(Op::OptionalEnd);
        self.active_value = None;
    }

    /// The letters shared by date, month, quarter and day-of-week fields: number or text by count.
    fn number_or_text(&mut self, letter: u16, count: usize, kind: LetterKind) -> JResult<()> {
        let LetterKind { field, table, standalone } = kind;
        let style = match count {
            1 | 2 if letter == u16::from(b'e') => {
                return self.append_week(WeekKind::DayOfWeek, count, count);
            }
            1 | 2 if letter == u16::from(b'E') => TextStyle::Short,
            1 => return self.value(field),
            2 => return self.fixed_value(field, 2),
            3 if standalone => TextStyle::ShortStandalone,
            3 => TextStyle::Short,
            4 if standalone => TextStyle::FullStandalone,
            4 => TextStyle::Full,
            5 if standalone => TextStyle::NarrowStandalone,
            5 => TextStyle::Narrow,
            _ => return Err(too_many(letter)),
        };
        self.text(field, table, style);
        Ok(())
    }

    fn append_week(&mut self, kind: WeekKind, min: usize, max: usize) -> JResult<()> {
        let field = week_field(kind, self.locale);
        self.append_number(NumberSpec::new(field, min, max, SignStyle::NotNegative))
    }

    /// `Y`: unlike `appendValue`, the JDK does not validate the width of this field.
    fn append_week_based_year(&mut self, count: usize) -> JResult<()> {
        let field = week_field(WeekKind::WeekBasedYear, self.locale);
        if count == 2 {
            return self.reduced_value(field);
        }
        let sign = if count < 4 { SignStyle::Normal } else { SignStyle::ExceedsPad };
        self.append_number(NumberSpec::new(field, count, MAX_WIDTH, sign))
    }

    fn append_year(&mut self, field: Field, count: usize) -> JResult<()> {
        match count {
            2 => self.reduced_value(field),
            1..=3 => self.ranged_value(field, count, SignStyle::Normal),
            _ => self.ranged_value(field, count, SignStyle::ExceedsPad),
        }
    }

    fn one_or_two_digits(&mut self, letter: u16, field: Field, count: usize) -> JResult<()> {
        match count {
            1 => self.value(field),
            2 => self.fixed_value(field, 2),
            _ => Err(too_many(letter)),
        }
    }

    fn append_fraction(&mut self, count: usize) -> JResult<()> {
        if count > MAX_FRACTION_WIDTH {
            return Err(illegal(format!("Minimum width must be from 0 to 9 inclusive but was {count}")));
        }
        self.append_value(Op::Fraction(FractionSpec {
            min_width: count,
            max_width: count,
            decimal_point: false,
            subsequent_width: 0,
        }))
    }

    /// Compiles a run of `count` equal pattern letters.
    fn append_letters(&mut self, letter: u16, count: usize) -> JResult<()> {
        let ascii = u8::try_from(letter).unwrap_or(0);
        match ascii {
            b'G' => {
                let style = match count {
                    1..=3 => TextStyle::Short,
                    4 => TextStyle::Full,
                    5 => TextStyle::Narrow,
                    _ => return Err(too_many(letter)),
                };
                self.text(Field::Era, TextField::Era, style);
                Ok(())
            }
            b'u' => self.append_year(Field::Year, count),
            b'y' => self.append_year(Field::YearOfEra, count),
            b'M' | b'L' | b'Q' | b'q' | b'E' | b'e' | b'c' => {
                self.append_month_quarter_or_weekday(letter, ascii, count)
            }
            b'D' => match count {
                1 => self.value(Field::DayOfYear),
                2 | 3 => self.append_number(NumberSpec::new(Field::DayOfYear, count, 3, SignStyle::NotNegative)),
                _ => Err(too_many(letter)),
            },
            b'd' => self.one_or_two_digits(letter, Field::DayOfMonth, count),
            b'h' => self.one_or_two_digits(letter, Field::ClockHourOfAmPm, count),
            b'H' => self.one_or_two_digits(letter, Field::HourOfDay, count),
            b'k' => self.one_or_two_digits(letter, Field::ClockHourOfDay, count),
            b'K' => self.one_or_two_digits(letter, Field::HourOfAmPm, count),
            b'm' => self.one_or_two_digits(letter, Field::MinuteOfHour, count),
            b's' => self.one_or_two_digits(letter, Field::SecondOfMinute, count),
            b'F' if count == 1 => self.value(Field::AlignedWeekOfMonth),
            b'a' if count == 1 => {
                self.text(Field::AmPmOfDay, TextField::AmPm, TextStyle::Short);
                Ok(())
            }
            b'F' | b'a' => Err(too_many(letter)),
            b'S' => self.append_fraction(count),
            b'g' => self.ranged_value(Field::ModifiedJulianDay, count, SignStyle::Normal),
            b'A' => self.ranged_value(Field::MilliOfDay, count, SignStyle::NotNegative),
            b'n' => self.ranged_value(Field::NanoOfSecond, count, SignStyle::NotNegative),
            b'N' => self.ranged_value(Field::NanoOfDay, count, SignStyle::NotNegative),
            b'z' if count > 4 => Err(too_many(letter)),
            b'z' => {
                self.append(Op::Zone { region_only: false });
                Ok(())
            }
            b'V' if count != 2 => Err(illegal_units("Pattern letter count must be 2: ", &[letter], "")),
            b'V' => {
                self.append(Op::Zone { region_only: false });
                Ok(())
            }
            b'v' if count == 1 || count == 4 => {
                self.append(Op::Zone { region_only: false });
                Ok(())
            }
            // Day periods have no support here: valid counts report the letter as unknown.
            b'B' if count == 1 || count == 4 || count == 5 => {
                Err(illegal_units("Unknown pattern letter: ", &[letter], ""))
            }
            b'v' | b'B' => Err(illegal_units("Wrong number of pattern letters: ", &[letter], "")),
            b'Z' | b'O' | b'X' | b'x' => self.append_offset_letters(letter, ascii, count),
            b'W' if count > 1 => Err(too_many(letter)),
            b'W' => self.append_week(WeekKind::WeekOfMonth, count, count),
            b'w' if count > 2 => Err(too_many(letter)),
            b'w' => self.append_week(WeekKind::WeekOfWeekBasedYear, count, 2),
            b'Y' => self.append_week_based_year(count),
            _ => Err(illegal_units("Unknown pattern letter: ", &[letter], "")),
        }
    }

    fn append_month_quarter_or_weekday(&mut self, letter: u16, ascii: u8, count: usize) -> JResult<()> {
        if ascii == b'c' {
            match count {
                1 => return self.append_week(WeekKind::DayOfWeek, 1, 1),
                2 => return Err(illegal("Invalid pattern \"cc\"".to_string())),
                _ => {}
            }
        }
        let (field, table) = match ascii {
            b'M' | b'L' => (Field::MonthOfYear, TextField::Month),
            b'Q' | b'q' => (Field::QuarterOfYear, TextField::Quarter),
            _ => (Field::DayOfWeek, TextField::DayOfWeek),
        };
        let standalone = matches!(ascii, b'L' | b'q' | b'c');
        self.number_or_text(letter, count, LetterKind { field, table, standalone })
    }

    fn append_offset_letters(&mut self, letter: u16, ascii: u8, count: usize) -> JResult<()> {
        const LAYOUTS: [OffsetLayout; 5] = [
            OffsetLayout::OptionalMinutes,
            OffsetLayout::Minutes,
            OffsetLayout::ColonMinutes,
            OffsetLayout::OptionalSeconds,
            OffsetLayout::ColonOptionalSeconds,
        ];
        match ascii {
            b'Z' => match count {
                1..=3 => self.offset(OffsetLayout::Minutes, "+0000"),
                4 => {
                    self.append(Op::LocalizedOffset { full: true });
                }
                5 => self.offset(OffsetLayout::ColonOptionalSeconds, "Z"),
                _ => return Err(too_many(letter)),
            },
            b'O' => match count {
                1 | 4 => {
                    self.append(Op::LocalizedOffset { full: count == 4 });
                }
                _ => return Err(illegal_units("Pattern letter count must be 1 or 4: ", &[letter], "")),
            },
            _ => {
                let Some(&layout) = LAYOUTS.get(count - 1) else {
                    return Err(too_many(letter));
                };
                let no_offset = match (ascii, count) {
                    (b'X', _) => "Z",
                    (_, 1) => "+00",
                    (_, 2 | 4) => "+0000",
                    _ => "+00:00",
                };
                self.offset(layout, no_offset);
            }
        }
        Ok(())
    }
}

/// How a letter maps to a field and its texts.
struct LetterKind {
    field: Field,
    table: TextField,
    standalone: bool,
}

/// The width of a value that can follow another one without a separator: fixed width digits.
fn fixed_follower_width(op: &Op) -> Option<i32> {
    match op {
        Op::Number(spec) if spec.min_width == spec.max_width && spec.sign == SignStyle::NotNegative => {
            Some(spec.max_width as i32)
        }
        Op::Fraction(spec) if spec.min_width == spec.max_width => Some(spec.max_width as i32),
        _ => None,
    }
}

fn widen_subsequent(op: &mut Op, width: i32) {
    match op {
        Op::Number(spec) => spec.subsequent_width += width,
        Op::Fraction(spec) => spec.subsequent_width += width,
        _ => {}
    }
}

fn with_fixed_width(op: Op) -> Op {
    match op {
        Op::Number(spec) => Op::Number(NumberSpec { subsequent_width: -1, ..spec }),
        Op::Fraction(spec) => Op::Fraction(FractionSpec { subsequent_width: -1, ..spec }),
        other => other,
    }
}

/// Reads the quoted literal starting at `start`; returns its text and the index after it.
fn read_quoted(pattern: &[u16], start: usize) -> JResult<(JString, usize)> {
    const QUOTE: u16 = 0x27;
    let mut text = JString::new();
    let mut pos = start + 1;
    while let Some(&unit) = pattern.get(pos) {
        if unit != QUOTE {
            text.push(unit);
            pos += 1;
        } else if pattern.get(pos + 1) == Some(&QUOTE) {
            text.push(QUOTE);
            pos += 2;
        } else {
            let literal = if pos == start + 1 { vec![QUOTE] } else { text };
            return Ok((literal, pos + 1));
        }
    }
    Err(illegal_units("Pattern ends with an incomplete string literal: ", pattern, ""))
}

fn run_length(pattern: &[u16], start: usize) -> usize {
    let first = pattern.get(start);
    pattern.iter().skip(start).take_while(|&unit| Some(unit) == first).count()
}

/// Compiles a pattern; `locale` supplies the week rules of localized fields.
pub(super) fn compile(pattern: &[u16], locale: &Locale) -> JResult<Vec<Op>> {
    let mut builder = Builder { ops: Vec::new(), locale, open_sections: Vec::new(), pad_next: 0, active_value: None };
    let mut pos = 0;
    while let Some(&unit) = pattern.get(pos) {
        if is_letter(unit) {
            let mut letter = unit;
            let mut count = run_length(pattern, pos);
            pos += count;
            if letter == u16::from(b'p') {
                match pattern.get(pos) {
                    Some(&next) if is_letter(next) => {
                        builder.pad_next = count;
                        builder.active_value = None;
                        letter = next;
                        count = run_length(pattern, pos);
                        pos += count;
                    }
                    _ => {
                        return Err(illegal_units(
                            "Pad letter 'p' must be followed by valid pad pattern: ",
                            pattern,
                            "",
                        ))
                    }
                }
            }
            builder.append_letters(letter, count)?;
            continue;
        }
        pos += 1;
        match u8::try_from(unit).unwrap_or(0) {
            b'\'' => {
                let (text, next) = read_quoted(pattern, pos - 1)?;
                builder.append(Op::Literal(text));
                pos = next;
            }
            b'[' => builder.start_optional(),
            b']' if builder.open_sections.is_empty() => {
                return Err(illegal("Pattern invalid as it contains ] without previous [".to_string()))
            }
            b']' => builder.end_optional(),
            b'{' | b'}' | b'#' => return Err(illegal_units("Pattern includes reserved character: '", &[unit], "'")),
            _ => {
                builder.append(Op::Literal(vec![unit]));
            }
        }
    }
    while !builder.open_sections.is_empty() {
        builder.end_optional();
    }
    link_sections(&mut builder.ops);
    Ok(builder.ops)
}
