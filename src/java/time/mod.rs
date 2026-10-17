//! The subset of `java.time` that EvalEx depends on, with the JDK 21 behaviour.
//!
//! Types mirror their Java namesakes. Fallible operations return [`crate::java::JResult`] whose
//! error carries the Java exception class and message.

pub mod duration;
pub(crate) mod field;
pub mod format;
pub mod instant;
pub mod local;
mod tzdata;
pub(crate) mod week;
pub mod zone;
pub mod zoned;

pub use duration::Duration;
pub use format::{parse_to_instant, DateTimeFormatter};
pub use instant::Instant;
pub use local::{LocalDate, LocalDateTime, LocalTime};
pub use zone::{time_zone_ids_contains, ZoneId};
pub use zoned::ZonedDateTime;

use crate::java::{JResult, Thrown};

const ARITHMETIC_EXCEPTION: &str = "java.lang.ArithmeticException";
pub(crate) const DATE_TIME_EXCEPTION: &str = "java.time.DateTimeException";

/// The `java.time.temporal.ChronoUnit` values a date-time can be truncated to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// `ChronoUnit.DAYS`.
    Days,
    /// `ChronoUnit.HOURS`.
    Hours,
    /// `ChronoUnit.MINUTES`.
    Minutes,
    /// `ChronoUnit.SECONDS`.
    Seconds,
}

/// A `java.time.DateTimeException`.
pub(crate) fn date_time_error(message: &str) -> Thrown {
    Thrown::new(DATE_TIME_EXCEPTION, message)
}

/// `Math.addExact(long, long)`.
fn add_exact(a: i64, b: i64) -> JResult<i64> {
    a.checked_add(b).ok_or_else(|| Thrown::new(ARITHMETIC_EXCEPTION, "long overflow"))
}

/// `Math.multiplyExact(long, long)`.
pub(crate) fn multiply_exact(a: i64, b: i64) -> JResult<i64> {
    a.checked_mul(b).ok_or_else(|| Thrown::new(ARITHMETIC_EXCEPTION, "long overflow"))
}

/// `Math.toIntExact(long)`.
pub(crate) fn to_int_exact(value: i64) -> JResult<i32> {
    i32::try_from(value).map_err(|_| Thrown::new(ARITHMETIC_EXCEPTION, "integer overflow"))
}
