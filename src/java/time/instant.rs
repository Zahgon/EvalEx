//! `java.time.Instant`.

use std::cmp::Ordering;
use std::fmt;

use super::duration::Duration;
use super::format::parse_instant;
use super::local::{civil_from_epoch_day, SECONDS_PER_DAY};
use super::zone::ZoneId;
use super::zoned::ZonedDateTime;
use super::{add_exact, date_time_error, multiply_exact};
use crate::java::JResult;

const MIN_SECOND: i64 = -31_557_014_167_219_200;
const MAX_SECOND: i64 = 31_556_889_864_403_199;
const NANOS_PER_SECOND: i64 = 1_000_000_000;
const NANOS_PER_MILLI: i64 = 1_000_000;

/// A point on the time line; `nanos` is always in `0..1_000_000_000`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant {
    /// Seconds since 1970-01-01T00:00:00Z.
    pub seconds: i64,
    /// Nanoseconds later along the time line.
    pub nanos: i32,
}

impl Instant {
    /// `Instant.EPOCH`.
    pub const EPOCH: Instant = Instant { seconds: 0, nanos: 0 };

    /// `Instant.ofEpochSecond(epochSecond, nanoAdjustment)`.
    pub fn of_epoch_second(seconds: i64, nano_adjustment: i64) -> JResult<Instant> {
        let seconds = add_exact(seconds, nano_adjustment.div_euclid(NANOS_PER_SECOND))?;
        if !(MIN_SECOND..=MAX_SECOND).contains(&seconds) {
            return Err(date_time_error("Instant exceeds minimum or maximum instant"));
        }
        Ok(Instant { seconds, nanos: nano_adjustment.rem_euclid(NANOS_PER_SECOND) as i32 })
    }

    /// `Instant.ofEpochMilli`.
    pub fn of_epoch_milli(millis: i64) -> Instant {
        Instant { seconds: millis.div_euclid(1000), nanos: (millis.rem_euclid(1000) * NANOS_PER_MILLI) as i32 }
    }

    /// `Instant.toEpochMilli`.
    pub fn to_epoch_milli(&self) -> JResult<i64> {
        let nanos = i64::from(self.nanos);
        if self.seconds < 0 && self.nanos > 0 {
            let millis = multiply_exact(self.seconds + 1, 1000)?;
            add_exact(millis, nanos / NANOS_PER_MILLI - 1000)
        } else {
            add_exact(multiply_exact(self.seconds, 1000)?, nanos / NANOS_PER_MILLI)
        }
    }

    fn plus_parts(&self, seconds: i64, nanos: i64) -> JResult<Instant> {
        if seconds == 0 && nanos == 0 {
            return Ok(*self);
        }
        let total = add_exact(self.seconds, seconds)?;
        let total = add_exact(total, nanos / NANOS_PER_SECOND)?;
        Instant::of_epoch_second(total, i64::from(self.nanos) + nanos % NANOS_PER_SECOND)
    }

    /// `Instant.plus(TemporalAmount)` for a duration: seconds first, then nanos, so that the
    /// intermediate result is range checked too.
    pub fn plus(&self, duration: &Duration) -> JResult<Instant> {
        self.plus_parts(duration.seconds, 0)?.plus_parts(0, i64::from(duration.nanos))
    }

    /// `Instant.minus(TemporalAmount)` for a duration, in the two steps of [`Instant::plus`].
    pub fn minus(&self, duration: &Duration) -> JResult<Instant> {
        let seconds_done = if duration.seconds == i64::MIN {
            self.plus_parts(i64::MAX, 0)?.plus_parts(1, 0)?
        } else {
            self.plus_parts(-duration.seconds, 0)?
        };
        seconds_done.plus_parts(0, -i64::from(duration.nanos))
    }

    /// `Instant.compareTo`.
    pub fn compare(&self, other: &Instant) -> Ordering {
        self.cmp(other)
    }

    /// `Instant.parse`.
    pub fn parse(text: &[u16]) -> JResult<Instant> {
        parse_instant(text)
    }

    /// `Instant.atZone`.
    pub fn at_zone(&self, zone: &ZoneId) -> JResult<ZonedDateTime> {
        ZonedDateTime::of_instant(self, zone)
    }
}

impl fmt::Display for Instant {
    /// `Instant.toString`: fractions print in groups of three digits.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = civil_from_epoch_day(self.seconds.div_euclid(SECONDS_PER_DAY));
        let second_of_day = self.seconds.rem_euclid(SECONDS_PER_DAY);
        if year > 9999 {
            f.write_str("+")?;
        } else if year < 0 {
            f.write_str("-")?;
        }
        write!(f, "{:04}-{month:02}-{day:02}", year.abs())?;
        write!(f, "T{:02}:{:02}:{:02}", second_of_day / 3600, second_of_day / 60 % 60, second_of_day % 60)?;
        if self.nanos == 0 {
        } else if self.nanos % 1_000_000 == 0 {
            write!(f, ".{:03}", self.nanos / 1_000_000)?;
        } else if self.nanos % 1000 == 0 {
            write!(f, ".{:06}", self.nanos / 1000)?;
        } else {
            write!(f, ".{:09}", self.nanos)?;
        }
        f.write_str("Z")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::js;

    fn parse(text: &str) -> Instant {
        Instant::parse(&js(text)).unwrap()
    }

    #[test]
    fn to_string_covers_year_shapes() {
        assert_eq!(Instant::EPOCH.to_string(), "1970-01-01T00:00:00Z");
        assert_eq!(Instant::of_epoch_milli(-1).to_string(), "1969-12-31T23:59:59.999Z");
        let far = Instant { seconds: MAX_SECOND, nanos: 999_999_999 };
        assert_eq!(far.to_string(), "+1000000000-12-31T23:59:59.999999999Z");
        let early = Instant { seconds: MIN_SECOND, nanos: 0 };
        assert_eq!(early.to_string(), "-1000000000-01-01T00:00:00Z");
    }

    #[test]
    fn parse_handles_end_of_day_and_leap_second() {
        assert_eq!(parse("2020-01-01T24:00:00Z"), parse("2020-01-02T00:00:00Z"));
        assert_eq!(parse("2020-06-30T23:59:60Z"), parse("2020-06-30T23:59:59Z"));
        assert_eq!(parse("2020-01-01T01:00:00+01:00"), parse("2020-01-01T00:00:00Z"));
    }

    #[test]
    fn parse_reports_the_error_index() {
        let error = Instant::parse(&js("2020-13-01T00:00:00Z")).unwrap_err();
        assert_eq!(error.class, "java.time.format.DateTimeParseException");
        assert_eq!(error.message, Some(js("Text '2020-13-01T00:00:00Z' could not be parsed at index 0")));
    }

    #[test]
    fn millis_arithmetic() {
        let instant = Instant { seconds: -2, nanos: 500_000_000 };
        assert_eq!(instant.to_epoch_milli(), Ok(-1500));
        let far = Instant { seconds: MAX_SECOND, nanos: 0 };
        assert_eq!(far.to_epoch_milli().unwrap_err().message, Some(js("long overflow")));
        assert_eq!(Instant::EPOCH.plus(&Duration::of_millis(1500)).unwrap().seconds, 1);
        assert_eq!(Instant::EPOCH.minus(&Duration::of_millis(1)).unwrap().to_epoch_milli(), Ok(-1));
    }

    #[test]
    fn plus_past_the_end_is_rejected() {
        let last = Instant { seconds: MAX_SECOND, nanos: 0 };
        assert_eq!(
            last.plus(&Duration::of_millis(1000)).unwrap_err().message,
            Some(js("Instant exceeds minimum or maximum instant"))
        );
    }
}
