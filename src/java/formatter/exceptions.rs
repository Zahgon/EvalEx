//! The exceptions of `java.util.Formatter` with their exact messages.

use super::spec::{Flags, Specifier};
use crate::java::{js, JString, Thrown};

fn utf16_with(prefix: &str, units: &[u16], suffix: &str) -> JString {
    let mut message = js(prefix);
    message.extend_from_slice(units);
    message.extend(suffix.encode_utf16());
    message
}

/// `UnknownFormatConversionException` for the given conversion text.
pub(super) fn unknown_conversion(conversion: &[u16]) -> Thrown {
    Thrown::with_message("java.util.UnknownFormatConversionException", utf16_with("Conversion = '", conversion, "'"))
}

/// `UnknownFormatConversionException` for a date-time suffix.
pub(super) fn unknown_date_time_conversion(suffix: char) -> Thrown {
    Thrown::with_message("java.util.UnknownFormatConversionException", js(&format!("Conversion = 't{suffix}'")))
}

/// `DuplicateFormatFlagsException`.
pub(super) fn duplicate_flag(flag: char) -> Thrown {
    Thrown::new("java.util.DuplicateFormatFlagsException", &format!("Flags = '{flag}'"))
}

/// `IllegalFormatFlagsException`.
pub(super) fn illegal_flags(flags: String) -> Thrown {
    Thrown::new("java.util.IllegalFormatFlagsException", &format!("Flags = '{flags}'"))
}

/// `FormatFlagsConversionMismatchException`.
pub(super) fn flag_mismatch(flag: Flags, conversion: char) -> Thrown {
    Thrown::new(
        "java.util.FormatFlagsConversionMismatchException",
        &format!("Conversion = {conversion}, Flags = {}", flag.render()),
    )
}

/// `IllegalFormatWidthException`.
pub(super) fn illegal_width(width: i32) -> Thrown {
    Thrown::new("java.util.IllegalFormatWidthException", &width.to_string())
}

/// `IllegalFormatPrecisionException`.
pub(super) fn illegal_precision(precision: i32) -> Thrown {
    Thrown::new("java.util.IllegalFormatPrecisionException", &precision.to_string())
}

/// `IllegalFormatArgumentIndexException` for an index below one.
pub(super) fn illegal_argument_index(index: i32) -> Thrown {
    Thrown::new("java.util.IllegalFormatArgumentIndexException", &format!("Illegal format argument index = {index}"))
}

/// `IllegalFormatArgumentIndexException` for an index beyond `int`.
pub(super) fn unrepresentable_argument_index() -> Thrown {
    Thrown::new("java.util.IllegalFormatArgumentIndexException", "Format argument index: (not representable as int)")
}

/// `MissingFormatWidthException`.
pub(super) fn missing_width(specifier: &Specifier) -> Thrown {
    Thrown::new("java.util.MissingFormatWidthException", &specifier.render())
}

/// `MissingFormatArgumentException`.
pub(super) fn missing_argument(specifier: &Specifier) -> Thrown {
    Thrown::new("java.util.MissingFormatArgumentException", &format!("Format specifier '{}'", specifier.render()))
}

/// `IllegalFormatConversionException`.
pub(super) fn illegal_conversion(conversion: char, class: &str) -> Thrown {
    Thrown::new("java.util.IllegalFormatConversionException", &format!("{conversion} != {class}"))
}

/// The `OutOfMemoryError` standing in for output too long to build.
pub(super) fn out_of_memory() -> Thrown {
    Thrown::new("java.lang.OutOfMemoryError", "Java heap space")
}
