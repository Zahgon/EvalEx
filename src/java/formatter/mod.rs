//! `java.util.Formatter`, as `String.format(Locale, format, args)` behaves on JDK 21.
//!
//! The arguments are the value types of EvalEx. Numbers are always `BigDecimal`, so `%d`, `%x`
//! and `%o` only ever print `null` and `%c` likewise; `%a` is an error for every non-null value.
//! Text lengths, widths and precisions count UTF-16 code units, like Java.

mod datetime;
mod exceptions;
mod number;
mod spec;
pub(crate) mod symbols;

use super::chars::{self, to_upper_case};
use crate::java::bigdecimal::BigDecimal;
use crate::java::locale::Locale;
use crate::java::time::ZonedDateTime;
use crate::java::{js, JResult, JString};
use spec::{ArgumentRef, Flags, Piece, Specifier};
use symbols::NumberSymbols;

/// The longest output built; Java runs out of heap on the widths and precisions that exceed it.
const MAX_OUTPUT_UNITS: usize = 1 << 27;

/// An argument of [`format`].
#[derive(Clone, Debug)]
pub enum FormatArgument {
    /// `null`.
    Null,
    /// A `java.lang.String`.
    Text(JString),
    /// A `java.math.BigDecimal`.
    Decimal(BigDecimal),
    /// A `java.lang.Boolean`.
    Boolean(bool),
    /// A `java.time.ZonedDateTime`.
    DateTime(ZonedDateTime),
    /// Any other object, reduced to what `Formatter` reads from it.
    Other {
        /// `getClass().getName()`.
        class: &'static str,
        /// `toString()`.
        text: JString,
        /// `hashCode()`.
        hash: i32,
    },
}

impl FormatArgument {
    fn class_name(&self) -> &str {
        match self {
            FormatArgument::Null => "null",
            FormatArgument::Text(_) => "java.lang.String",
            FormatArgument::Decimal(_) => "java.math.BigDecimal",
            FormatArgument::Boolean(_) => "java.lang.Boolean",
            FormatArgument::DateTime(_) => "java.time.ZonedDateTime",
            FormatArgument::Other { class, .. } => class,
        }
    }

    /// `String.valueOf(argument)`.
    fn java_string(&self) -> JString {
        match self {
            FormatArgument::Null => js("null"),
            FormatArgument::Text(text) | FormatArgument::Other { text, .. } => text.clone(),
            FormatArgument::Decimal(value) => js(&value.to_string()),
            FormatArgument::Boolean(value) => js(&value.to_string()),
            FormatArgument::DateTime(value) => js(&datetime::to_iso_string(value)),
        }
    }

    /// `hashCode()`; `None` for `null`.
    fn hash_code(&self) -> Option<i32> {
        match self {
            FormatArgument::Null => None,
            FormatArgument::Text(text) => Some(chars::hash_code(text)),
            FormatArgument::Decimal(value) => Some(value.hash_code()),
            FormatArgument::Boolean(value) => Some(if *value { 1231 } else { 1237 }),
            FormatArgument::DateTime(value) => Some(datetime::hash_code(value)),
            FormatArgument::Other { hash, .. } => Some(*hash),
        }
    }
}

/// `String.format(locale, format, arguments)`.
///
/// The whole format string is validated before any argument is read. Output that would exceed
/// the memory Java can use is reported as `java.lang.OutOfMemoryError`.
pub fn format(locale: &Locale, format: &[u16], arguments: &[FormatArgument]) -> JResult<JString> {
    let pieces = spec::parse(format)?;
    let mut writer = Writer { locale, symbols: NumberSymbols::of(locale), out: Vec::with_capacity(format.len()) };
    let mut ordinary = 0_usize;
    let mut last: Option<usize> = None;
    for piece in pieces {
        let specifier = match piece {
            Piece::Literal(text) => {
                writer.out.extend_from_slice(text);
                continue;
            }
            Piece::Specifier(specifier) => specifier,
        };
        let position = match specifier.argument {
            ArgumentRef::Unused => {
                writer.write_argumentless(&specifier)?;
                continue;
            }
            ArgumentRef::Previous => last,
            ArgumentRef::Ordinary => {
                ordinary += 1;
                Some(ordinary - 1)
            }
            ArgumentRef::Explicit(index) => usize::try_from(index - 1).ok(),
        };
        last = position.or(last);
        let argument = position
            .and_then(|position| arguments.get(position))
            .ok_or_else(|| exceptions::missing_argument(&specifier))?;
        writer.write_argument(&specifier, argument)?;
    }
    Ok(writer.out)
}

struct Writer<'a> {
    locale: &'a Locale,
    symbols: NumberSymbols,
    out: Vec<u16>,
}

impl Writer<'_> {
    fn write_argumentless(&mut self, specifier: &Specifier) -> JResult<()> {
        if specifier.conversion == 'n' {
            self.out.push(u16::from(b'\n'));
            return Ok(());
        }
        self.write_justified(specifier, &[u16::from(b'%')])
    }

    fn write_argument(&mut self, specifier: &Specifier, argument: &FormatArgument) -> JResult<()> {
        if specifier.date_time {
            return self.write_date_time(specifier, argument);
        }
        match (specifier.conversion, argument) {
            ('s', _) => {
                if specifier.flags.contains(Flags::ALTERNATE) {
                    return Err(exceptions::flag_mismatch(Flags::ALTERNATE, 's'));
                }
                self.write_text(specifier, &argument.java_string())
            }
            ('b', _) => {
                let value = match argument {
                    FormatArgument::Null => false,
                    FormatArgument::Boolean(value) => *value,
                    _ => true,
                };
                self.write_text(specifier, &js(&value.to_string()))
            }
            ('h', _) => match argument.hash_code() {
                Some(hash) => self.write_text(specifier, &js(&format!("{hash:x}"))),
                None => self.write_text(specifier, &js("null")),
            },
            (_, FormatArgument::Null) => self.write_text(specifier, &js("null")),
            ('e' | 'f' | 'g', FormatArgument::Decimal(value)) => self.write_decimal(specifier, value),
            (conversion, _) => Err(exceptions::illegal_conversion(conversion, argument.class_name())),
        }
    }

    fn write_date_time(&mut self, specifier: &Specifier, argument: &FormatArgument) -> JResult<()> {
        match argument {
            FormatArgument::Null => self.write_text(specifier, &js("null")),
            FormatArgument::DateTime(value) => {
                let mut text = String::new();
                datetime::write(&mut text, value, specifier.conversion, self.locale);
                let units = js(&text);
                if specifier.uppercase {
                    self.write_justified(specifier, &to_upper_case(&units))
                } else {
                    self.write_justified(specifier, &units)
                }
            }
            _ => Err(exceptions::illegal_conversion(specifier.conversion, argument.class_name())),
        }
    }

    /// Text conversions: the precision truncates first, then case, then the width.
    fn write_text(&mut self, specifier: &Specifier, text: &[u16]) -> JResult<()> {
        let limit = specifier.precision.and_then(|precision| usize::try_from(precision).ok()).unwrap_or(text.len());
        let visible = text.get(..limit).unwrap_or(text);
        if specifier.uppercase {
            self.write_justified(specifier, &to_upper_case(visible))
        } else {
            self.write_justified(specifier, visible)
        }
    }

    fn write_decimal(&mut self, specifier: &Specifier, value: &BigDecimal) -> JResult<()> {
        let negative = value.signum() < 0;
        let magnitude = BigDecimal::from_big_integer(&value.unscaled_value().abs(), value.scale());
        let flags = specifier.flags;
        let mut text = Vec::new();
        if negative {
            let opening = if flags.contains(Flags::PARENTHESES) { b'(' } else { b'-' };
            text.push(u16::from(opening));
        } else if flags.contains(Flags::PLUS) {
            text.push(u16::from(b'+'));
        } else if flags.contains(Flags::LEADING_SPACE) {
            text.push(u16::from(b' '));
        }
        number::write_magnitude(&mut text, specifier, &self.symbols, &magnitude, negative)?;
        if negative && flags.contains(Flags::PARENTHESES) {
            text.push(u16::from(b')'));
        }
        self.write_justified(specifier, &text)
    }

    fn write_justified(&mut self, specifier: &Specifier, text: &[u16]) -> JResult<()> {
        let width = specifier.width.and_then(|width| usize::try_from(width).ok()).unwrap_or(0);
        if width.max(text.len()) + self.out.len() > MAX_OUTPUT_UNITS {
            return Err(exceptions::out_of_memory());
        }
        let padding = std::iter::repeat_n(u16::from(b' '), width.saturating_sub(text.len()));
        if specifier.flags.contains(Flags::LEFT_JUSTIFY) {
            self.out.extend_from_slice(text);
            self.out.extend(padding);
        } else {
            self.out.extend(padding);
            self.out.extend_from_slice(text);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
