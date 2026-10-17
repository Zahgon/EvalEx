//! Parsing and validation of format specifiers.
//!
//! The grammar is `%[index$][flags][width][.precision][t|T]conversion`. Everything `Formatter`
//! rejects before looking at an argument is rejected here, which is why an error in a late
//! specifier wins over a missing argument for an early one.

use super::exceptions;
use crate::java::JResult;

/// A set of format flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Flags(u8);

impl Flags {
    pub(super) const NONE: Flags = Flags(0);
    pub(super) const LEFT_JUSTIFY: Flags = Flags(1);
    pub(super) const ALTERNATE: Flags = Flags(1 << 1);
    pub(super) const PLUS: Flags = Flags(1 << 2);
    pub(super) const LEADING_SPACE: Flags = Flags(1 << 3);
    pub(super) const ZERO_PAD: Flags = Flags(1 << 4);
    pub(super) const GROUP: Flags = Flags(1 << 5);
    pub(super) const PARENTHESES: Flags = Flags(1 << 6);
    pub(super) const PREVIOUS: Flags = Flags(1 << 7);

    /// All flags with their characters, in the order Java prints them.
    const ALL: [(char, Flags); 8] = [
        ('-', Flags::LEFT_JUSTIFY),
        ('#', Flags::ALTERNATE),
        ('+', Flags::PLUS),
        (' ', Flags::LEADING_SPACE),
        ('0', Flags::ZERO_PAD),
        (',', Flags::GROUP),
        ('(', Flags::PARENTHESES),
        ('<', Flags::PREVIOUS),
    ];

    fn of_unit(unit: u16) -> Option<(char, Flags)> {
        Flags::ALL.into_iter().find(|(symbol, _)| u32::from(unit) == u32::from(*symbol))
    }

    pub(super) fn contains(self, other: Flags) -> bool {
        self.0 & other.0 != 0
    }

    fn insert(&mut self, other: Flags) {
        self.0 |= other.0;
    }

    /// The characters of the flags in canonical order.
    pub(super) fn render(self) -> String {
        Flags::ALL.into_iter().filter(|(_, flag)| self.contains(*flag)).map(|(symbol, _)| symbol).collect()
    }
}

/// How a specifier selects its argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArgumentRef {
    /// The next argument in sequence.
    Ordinary,
    /// The argument of the preceding specifier (`<`).
    Previous,
    /// A 1-based explicit position.
    Explicit(i32),
    /// No argument: `%n` and `%%`.
    Unused,
}

/// A validated format specifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Specifier {
    pub(super) argument: ArgumentRef,
    pub(super) flags: Flags,
    pub(super) uppercase: bool,
    pub(super) width: Option<i32>,
    pub(super) precision: Option<i32>,
    pub(super) date_time: bool,
    /// The conversion; lowercase except for date-time suffixes, which stay as written.
    pub(super) conversion: char,
}

/// One element of a parsed format string.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Piece<'a> {
    Literal(&'a [u16]),
    Specifier(Specifier),
}

/// The undigested parts of a specifier that matched the grammar.
struct RawSpecifier<'a> {
    index: Option<&'a [u16]>,
    flags: &'a [u16],
    width: Option<&'a [u16]>,
    precision: Option<&'a [u16]>,
    uppercase_date_time: Option<bool>,
    conversion: u16,
    end: usize,
}

/// Splits a format string into literal text and validated specifiers.
pub(super) fn parse(format: &[u16]) -> JResult<Vec<Piece<'_>>> {
    let mut pieces = Vec::new();
    let mut rest_start = 0;
    while let Some(tail) = format.get(rest_start..).filter(|tail| !tail.is_empty()) {
        let Some(offset) = tail.iter().position(|unit| *unit == u16::from(b'%')) else {
            pieces.push(Piece::Literal(tail));
            break;
        };
        if let Some(literal) = tail.get(..offset).filter(|literal| !literal.is_empty()) {
            pieces.push(Piece::Literal(literal));
        }
        let start = rest_start + offset + 1;
        let Some(&first) = format.get(start) else {
            return Err(exceptions::unknown_conversion(&[u16::from(b'%')]));
        };
        let raw = scan(format, start).ok_or_else(|| exceptions::unknown_conversion(&[first]))?;
        rest_start = raw.end;
        pieces.push(Piece::Specifier(Specifier::from_raw(&raw)?));
    }
    Ok(pieces)
}

fn is_digit(unit: u16) -> bool {
    (0x30..=0x39).contains(&unit)
}

fn is_ascii_letter(unit: u16) -> bool {
    u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_alphabetic())
}

fn skip_while(format: &[u16], start: usize, accept: impl Fn(u16) -> bool) -> usize {
    let tail = format.get(start..).unwrap_or_default();
    start + tail.iter().take_while(|unit| accept(**unit)).count()
}

/// Matches the specifier grammar at `start`, just after the `%`.
fn scan(format: &[u16], start: usize) -> Option<RawSpecifier<'_>> {
    let mut pos = start;
    let index_end = skip_while(format, pos, is_digit);
    let mut index = None;
    if index_end > pos && format.get(index_end) == Some(&u16::from(b'$')) {
        index = format.get(pos..index_end);
        pos = index_end + 1;
    }
    let flags_end = skip_while(format, pos, |unit| Flags::of_unit(unit).is_some());
    let flags = format.get(pos..flags_end)?;
    pos = flags_end;
    let width_end = skip_while(format, pos, is_digit);
    let width = format.get(pos..width_end).filter(|w| !w.is_empty());
    pos = width_end;
    let mut precision = None;
    if format.get(pos) == Some(&u16::from(b'.')) {
        let precision_end = skip_while(format, pos + 1, is_digit);
        precision = Some(format.get(pos + 1..precision_end).filter(|p| !p.is_empty())?);
        pos = precision_end;
    }
    let is_conversion = |unit: &&u16| is_ascii_letter(**unit) || **unit == u16::from(b'%');
    let first = format.get(pos).filter(is_conversion)?;
    let after_prefix = format.get(pos + 1).filter(is_conversion);
    let (uppercase_date_time, conversion) = match (*first, after_prefix) {
        (0x74, Some(suffix)) => (Some(false), *suffix),
        (0x54, Some(suffix)) => (Some(true), *suffix),
        (unit, _) => (None, unit),
    };
    Some(RawSpecifier {
        index,
        flags,
        width,
        precision,
        uppercase_date_time,
        conversion,
        end: pos + if uppercase_date_time.is_some() { 2 } else { 1 },
    })
}

/// `Integer.parseInt` of ASCII digits; `None` when the value overflows.
fn parse_int(digits: &[u16]) -> Option<i32> {
    String::from_utf16_lossy(digits).parse().ok()
}

fn parse_flags(units: &[u16]) -> JResult<Flags> {
    let mut flags = Flags::NONE;
    for (symbol, flag) in units.iter().filter_map(|unit| Flags::of_unit(*unit)) {
        if flags.contains(flag) {
            return Err(exceptions::duplicate_flag(symbol));
        }
        flags.insert(flag);
    }
    Ok(flags)
}

fn parse_argument(raw: &RawSpecifier<'_>) -> JResult<ArgumentRef> {
    match raw.index {
        Some(digits) => match parse_int(digits) {
            Some(index) if index > 0 => Ok(ArgumentRef::Explicit(index)),
            Some(index) => Err(exceptions::illegal_argument_index(index)),
            None => Err(exceptions::unrepresentable_argument_index()),
        },
        None => Ok(ArgumentRef::Ordinary),
    }
}

impl Specifier {
    fn from_raw(raw: &RawSpecifier<'_>) -> JResult<Specifier> {
        // The index is converted first although flags decide whether it counts: a bad index
        // wins over a bad flag.
        let explicit = parse_argument(raw)?;
        let flags = parse_flags(raw.flags)?;
        let argument = if flags.contains(Flags::PREVIOUS) { ArgumentRef::Previous } else { explicit };
        let width = match raw.width {
            Some(digits) => Some(parse_int(digits).ok_or_else(|| exceptions::illegal_width(i32::MIN))?),
            None => None,
        };
        let precision = match raw.precision {
            Some(digits) => Some(parse_int(digits).ok_or_else(|| exceptions::illegal_precision(i32::MIN))?),
            None => None,
        };
        let date_time = raw.uppercase_date_time.is_some();
        let (conversion, uppercase) = match raw.uppercase_date_time {
            Some(uppercase) => (conversion_char(raw.conversion), uppercase),
            None => split_case(raw.conversion),
        };
        let mut specifier = Specifier { argument, flags, uppercase, width, precision, date_time, conversion };
        if matches!(conversion, '%' | 'n') && !date_time {
            specifier.argument = ArgumentRef::Unused;
        }
        specifier.validate(raw.conversion)?;
        Ok(specifier)
    }

    fn validate(&self, written: u16) -> JResult<()> {
        if self.date_time {
            return self.check_date_time();
        }
        match self.conversion {
            'b' | 'h' | 's' => self.check_general(),
            'c' => self.check_character(),
            'd' | 'o' | 'x' => self.check_integer(),
            'e' | 'f' | 'g' | 'a' => self.check_float(),
            '%' => self.check_percent(),
            'n' => self.check_line_separator(),
            _ => Err(exceptions::unknown_conversion(&[written])),
        }
    }

    fn has(&self, flag: Flags) -> bool {
        self.flags.contains(flag)
    }

    /// Fails on the flags of `rejected` that are present, naming all of them.
    fn reject_flags(&self, rejected: Flags) -> JResult<()> {
        let present = Flags(self.flags.0 & rejected.0);
        if present == Flags::NONE {
            return Ok(());
        }
        Err(exceptions::flag_mismatch(present, self.conversion))
    }

    fn require_width_for(&self, flags: &[Flags]) -> JResult<()> {
        if self.width.is_none() && flags.iter().any(|flag| self.has(*flag)) {
            return Err(exceptions::missing_width(self));
        }
        Ok(())
    }

    fn reject_precision(&self) -> JResult<()> {
        match self.precision {
            Some(precision) => Err(exceptions::illegal_precision(precision)),
            None => Ok(()),
        }
    }

    fn check_general(&self) -> JResult<()> {
        if self.conversion != 's' {
            self.reject_flags(Flags::ALTERNATE)?;
        }
        self.require_width_for(&[Flags::LEFT_JUSTIFY])?;
        self.reject_flags(Flags(ALL_BUT_LEFT_JUSTIFY.0 & !Flags::ALTERNATE.0))
    }

    fn check_character(&self) -> JResult<()> {
        self.reject_precision()?;
        self.reject_flags(ALL_BUT_LEFT_JUSTIFY)?;
        self.require_width_for(&[Flags::LEFT_JUSTIFY])
    }

    fn check_date_time(&self) -> JResult<()> {
        self.reject_precision()?;
        if !"HIklMSLNpzZsQBbhAaCYyjmdeRTrDFc".contains(self.conversion) {
            return Err(exceptions::unknown_date_time_conversion(self.conversion));
        }
        self.reject_flags(ALL_BUT_LEFT_JUSTIFY)?;
        self.require_width_for(&[Flags::LEFT_JUSTIFY])
    }

    fn check_numeric(&self) -> JResult<()> {
        self.require_width_for(&[Flags::LEFT_JUSTIFY, Flags::ZERO_PAD])?;
        let both = |a, b| self.has(a) && self.has(b);
        if both(Flags::PLUS, Flags::LEADING_SPACE) || both(Flags::LEFT_JUSTIFY, Flags::ZERO_PAD) {
            return Err(exceptions::illegal_flags(self.render_flags_with_case()));
        }
        Ok(())
    }

    /// The flags as `Flags.toString` shows them, where uppercase is a flag `^` after `-`.
    fn render_flags_with_case(&self) -> String {
        let rendered = self.flags.render();
        if !self.uppercase {
            return rendered;
        }
        match rendered.strip_prefix('-') {
            Some(rest) => format!("-^{rest}"),
            None => format!("^{rendered}"),
        }
    }

    fn check_integer(&self) -> JResult<()> {
        self.check_numeric()?;
        self.reject_precision()?;
        match self.conversion {
            'd' => self.reject_flags(Flags::ALTERNATE),
            _ => self.reject_flags(Flags::GROUP),
        }
    }

    fn check_float(&self) -> JResult<()> {
        self.check_numeric()?;
        match self.conversion {
            'a' => self.reject_flags(Flags(Flags::PARENTHESES.0 | Flags::GROUP.0)),
            'e' => self.reject_flags(Flags::GROUP),
            'g' => self.reject_flags(Flags::ALTERNATE),
            _ => Ok(()),
        }
    }

    fn check_percent(&self) -> JResult<()> {
        self.reject_precision()?;
        if self.flags != Flags::NONE && self.flags != Flags::LEFT_JUSTIFY {
            return Err(exceptions::illegal_flags(self.flags.render()));
        }
        self.require_width_for(&[Flags::LEFT_JUSTIFY])
    }

    fn check_line_separator(&self) -> JResult<()> {
        self.reject_precision()?;
        if let Some(width) = self.width {
            return Err(exceptions::illegal_width(width));
        }
        if self.flags != Flags::NONE {
            return Err(exceptions::illegal_flags(self.flags.render()));
        }
        Ok(())
    }

    /// `FormatSpecifier.toString`: the canonical spelling used in exception messages.
    pub(super) fn render(&self) -> String {
        let mut text = String::from("%");
        text.push_str(&self.flags.render());
        if let ArgumentRef::Explicit(index) = self.argument {
            text.push_str(&format!("{index}$"));
        }
        if let Some(width) = self.width {
            text.push_str(&width.to_string());
        }
        if let Some(precision) = self.precision {
            text.push_str(&format!(".{precision}"));
        }
        if self.date_time {
            text.push(if self.uppercase { 'T' } else { 't' });
        }
        if self.uppercase {
            text.push(self.conversion.to_ascii_uppercase());
        } else {
            text.push(self.conversion);
        }
        text
    }
}

const ALL_BUT_LEFT_JUSTIFY: Flags = Flags(
    Flags::ALTERNATE.0
        | Flags::PLUS.0
        | Flags::LEADING_SPACE.0
        | Flags::ZERO_PAD.0
        | Flags::GROUP.0
        | Flags::PARENTHESES.0,
);

fn conversion_char(unit: u16) -> char {
    char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER)
}

/// Separates a conversion into its lowercase form and whether it was uppercase. Only the
/// uppercase forms Java accepts are folded, so `%D` stays invalid.
fn split_case(unit: u16) -> (char, bool) {
    let conversion = conversion_char(unit);
    if "BHSCXEGA".contains(conversion) {
        (conversion.to_ascii_lowercase(), true)
    } else {
        (conversion, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::js;

    fn failure(format: &str) -> (String, String) {
        let units = js(format);
        let error = parse(&units).expect_err("format must be rejected");
        let message = String::from_utf16_lossy(&error.message.unwrap_or_default());
        (error.class, message)
    }

    #[test]
    fn grammar_failures_name_the_first_unit_after_the_percent() {
        let unknown = "java.util.UnknownFormatConversionException";
        assert_eq!(failure("%"), (unknown.into(), "Conversion = '%'".into()));
        assert_eq!(failure("%.q"), (unknown.into(), "Conversion = '.'".into()));
        assert_eq!(failure("%<1$s"), (unknown.into(), "Conversion = '<'".into()));
        assert_eq!(failure("%-q"), (unknown.into(), "Conversion = 'q'".into()));
        assert_eq!(failure("%tq"), (unknown.into(), "Conversion = 'tq'".into()));
    }

    #[test]
    fn flags_render_in_canonical_order() {
        let units = js("%0-+5d");
        let pieces = parse(&units);
        assert_eq!(pieces.expect_err("left and zero conflict").message, Some(js("Flags = '-+0'")));
    }

    #[test]
    fn canonical_rendering_drops_ignored_index_and_upcases_suffix() {
        let units = js("%1$<-8.3S %Tb");
        let pieces = parse(&units).expect("valid format");
        let Some(Piece::Specifier(first)) = pieces.first() else {
            panic!("specifier expected");
        };
        assert_eq!(first.render(), "%-<8.3S");
        let Some(Piece::Specifier(second)) = pieces.get(2) else {
            panic!("specifier expected");
        };
        assert_eq!(second.render(), "%TB");
    }

    #[test]
    fn numeric_limits_are_reported() {
        assert_eq!(failure("%99999999999s").1, "-2147483648", "width overflow is reported as Integer.MIN_VALUE");
        assert_eq!(failure("%0$s").1, "Illegal format argument index = 0");
    }
}
