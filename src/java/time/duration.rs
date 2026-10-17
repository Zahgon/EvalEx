//! `java.time.Duration`.

use crate::java::integer::parse_long;
use std::cmp::Ordering;
use std::fmt;

use super::{add_exact, multiply_exact};
use crate::java::{JResult, Thrown};

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const NANOS_PER_MILLI: i64 = 1_000_000;
const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 3600;
const SECONDS_PER_DAY: i64 = 86_400;
const PARSE_EXCEPTION: &str = "java.time.format.DateTimeParseException";
const PARSE_FAILED: &str = "Text cannot be parsed to a Duration";

/// A time span with nanosecond precision; `nanos` is always in `0..1_000_000_000`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration {
    /// Whole seconds, may be negative.
    pub seconds: i64,
    /// Nanosecond adjustment, always positive.
    pub nanos: i32,
}

impl Duration {
    /// `Duration.ZERO`.
    pub const ZERO: Duration = Duration { seconds: 0, nanos: 0 };

    /// `Duration.ofDays`.
    pub fn of_days(days: i64) -> JResult<Duration> {
        Ok(Duration { seconds: multiply_exact(days, SECONDS_PER_DAY)?, nanos: 0 })
    }

    /// `Duration.ofMillis`.
    pub fn of_millis(millis: i64) -> Duration {
        Duration { seconds: millis.div_euclid(1000), nanos: (millis.rem_euclid(1000) * NANOS_PER_MILLI) as i32 }
    }

    /// `Duration.ofSeconds(seconds, nanoAdjustment)`.
    fn of_seconds(seconds: i64, nano_adjustment: i64) -> JResult<Duration> {
        Ok(Duration {
            seconds: add_exact(seconds, nano_adjustment.div_euclid(NANOS_PER_SECOND))?,
            nanos: nano_adjustment.rem_euclid(NANOS_PER_SECOND) as i32,
        })
    }

    fn plus_parts(&self, seconds: i64, nanos: i64) -> JResult<Duration> {
        if seconds == 0 && nanos == 0 {
            return Ok(*self);
        }
        let total = add_exact(self.seconds, seconds)?;
        let total = add_exact(total, nanos / NANOS_PER_SECOND)?;
        Duration::of_seconds(total, i64::from(self.nanos) + nanos % NANOS_PER_SECOND)
    }

    /// `Duration.plusHours`.
    pub fn plus_hours(&self, hours: i64) -> JResult<Duration> {
        self.plus_parts(multiply_exact(hours, SECONDS_PER_HOUR)?, 0)
    }

    /// `Duration.plusMinutes`.
    pub fn plus_minutes(&self, minutes: i64) -> JResult<Duration> {
        self.plus_parts(multiply_exact(minutes, SECONDS_PER_MINUTE)?, 0)
    }

    /// `Duration.plusSeconds`.
    pub fn plus_seconds(&self, seconds: i64) -> JResult<Duration> {
        self.plus_parts(seconds, 0)
    }

    /// `Duration.plusMillis`.
    pub fn plus_millis(&self, millis: i64) -> JResult<Duration> {
        self.plus_parts(millis / 1000, millis % 1000 * NANOS_PER_MILLI)
    }

    /// `Duration.plusNanos`.
    pub fn plus_nanos(&self, nanos: i64) -> JResult<Duration> {
        self.plus_parts(0, nanos)
    }

    /// `Duration.plus(Duration)`.
    pub fn plus(&self, other: &Duration) -> JResult<Duration> {
        self.plus_parts(other.seconds, i64::from(other.nanos))
    }

    /// `Duration.minus(Duration)`.
    pub fn minus(&self, other: &Duration) -> JResult<Duration> {
        let nanos = -i64::from(other.nanos);
        if other.seconds == i64::MIN {
            return self.plus_parts(i64::MAX, nanos)?.plus_parts(1, 0);
        }
        self.plus_parts(-other.seconds, nanos)
    }

    /// `Duration.toMillis`.
    pub fn to_millis(&self) -> JResult<i64> {
        let (seconds, nanos) = if self.seconds < 0 {
            (self.seconds + 1, i64::from(self.nanos) - NANOS_PER_SECOND)
        } else {
            (self.seconds, i64::from(self.nanos))
        };
        add_exact(multiply_exact(seconds, 1000)?, nanos / NANOS_PER_MILLI)
    }

    /// `Duration.compareTo`.
    pub fn compare(&self, other: &Duration) -> Ordering {
        self.cmp(other)
    }

    /// `Duration.parse`.
    pub fn parse(text: &[u16]) -> JResult<Duration> {
        let spec = DurationSyntax::scan(text).ok_or_else(|| parse_error(None))?;
        spec.evaluate()
    }
}

impl fmt::Display for Duration {
    /// `Duration.toString`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Duration::ZERO {
            return f.write_str("PT0S");
        }
        let negative_with_nanos = self.seconds < 0 && self.nanos > 0;
        let total = self.seconds + i64::from(negative_with_nanos);
        let hours = total / SECONDS_PER_HOUR;
        let minutes = total % SECONDS_PER_HOUR / SECONDS_PER_MINUTE;
        let seconds = total % SECONDS_PER_MINUTE;
        f.write_str("PT")?;
        if hours != 0 {
            write!(f, "{hours}H")?;
        }
        if minutes != 0 {
            write!(f, "{minutes}M")?;
        }
        if seconds == 0 && self.nanos == 0 {
            return Ok(());
        }
        if negative_with_nanos && seconds == 0 {
            f.write_str("-0")?;
        } else {
            write!(f, "{seconds}")?;
        }
        if self.nanos > 0 {
            let fraction =
                if self.seconds < 0 { NANOS_PER_SECOND - i64::from(self.nanos) } else { i64::from(self.nanos) };
            let digits = format!("{fraction:09}");
            write!(f, ".{}", digits.trim_end_matches('0'))?;
        }
        f.write_str("S")
    }
}

fn parse_error(detail: Option<&str>) -> Thrown {
    match detail {
        Some(detail) => Thrown::new(PARSE_EXCEPTION, &format!("{PARSE_FAILED}: {detail}")),
        None => Thrown::new(PARSE_EXCEPTION, PARSE_FAILED),
    }
}

/// The groups of the ISO-8601 duration syntax `[-+]P[nD][T[nH][nM][n[.f]S]]`, with the case-free
/// designators and either `.` or `,` before the fraction.
struct DurationSyntax<'a> {
    negate: bool,
    days: Option<&'a [u16]>,
    hours: Option<&'a [u16]>,
    minutes: Option<&'a [u16]>,
    seconds: Option<&'a [u16]>,
    fraction: Option<&'a [u16]>,
}

impl<'a> DurationSyntax<'a> {
    fn scan(text: &'a [u16]) -> Option<DurationSyntax<'a>> {
        let mut rest = text;
        let negate = eat_sign(&mut rest) == Some(b'-');
        if !eat_letter(&mut rest, b'p') {
            return None;
        }
        let days = eat_number_before(&mut rest, b'd');
        let upper_t = rest.first() == Some(&0x54);
        let has_time = eat_letter(&mut rest, b't');
        let (mut hours, mut minutes, mut seconds, mut fraction) = (None, None, None, None);
        if has_time {
            hours = eat_number_before(&mut rest, b'h');
            minutes = eat_number_before(&mut rest, b'm');
            if let Some((whole, digits)) = eat_seconds(&mut rest) {
                seconds = Some(whole);
                fraction = digits;
            }
        }
        let time_empty = hours.is_none() && minutes.is_none() && seconds.is_none();
        // The JDK rejects a time part without fields only when its `T` is upper case.
        if !rest.is_empty() || (upper_t && time_empty) || (days.is_none() && time_empty) {
            return None;
        }
        Some(DurationSyntax { negate, days, hours, minutes, seconds, fraction })
    }

    fn evaluate(&self) -> JResult<Duration> {
        let days = parse_scaled(self.days, SECONDS_PER_DAY, "days")?;
        let hours = parse_scaled(self.hours, SECONDS_PER_HOUR, "hours")?;
        let minutes = parse_scaled(self.minutes, SECONDS_PER_MINUTE, "minutes")?;
        let seconds = parse_scaled(self.seconds, 1, "seconds")?;
        let negative_seconds = self.seconds.is_some_and(|s| s.first() == Some(&u16::from(b'-')));
        let nanos = parse_fraction(self.fraction, negative_seconds);
        let overflow = |_| parse_error(Some("overflow"));
        let total = add_exact(minutes, seconds)
            .and_then(|sum| add_exact(hours, sum))
            .and_then(|sum| add_exact(days, sum))
            .map_err(overflow)?;
        let duration = Duration::of_seconds(total, nanos).map_err(overflow)?;
        if self.negate {
            duration.negated().map_err(overflow)
        } else {
            Ok(duration)
        }
    }
}

impl Duration {
    /// `Duration.negated`.
    fn negated(&self) -> JResult<Duration> {
        let negated_seconds = multiply_exact(self.seconds, -1)?;
        Duration::of_seconds(negated_seconds, -i64::from(self.nanos))
    }
}

fn is_letter(unit: u16, lower: u8) -> bool {
    unit == u16::from(lower) || unit == u16::from(lower.to_ascii_uppercase())
}

fn eat_letter(rest: &mut &[u16], lower: u8) -> bool {
    match rest.split_first() {
        Some((&first, tail)) if is_letter(first, lower) => {
            *rest = tail;
            true
        }
        _ => false,
    }
}

fn eat_sign(rest: &mut &[u16]) -> Option<u8> {
    let (&first, tail) = rest.split_first()?;
    let sign = u8::try_from(first).ok().filter(|b| matches!(b, b'+' | b'-'))?;
    *rest = tail;
    Some(sign)
}

/// Length of the `[-+]?[0-9]+` number at the start of `text`, if there is one.
fn number_length(text: &[u16]) -> Option<usize> {
    let sign = usize::from(matches!(text.first().copied(), Some(43 | 45)));
    let digits = digit_run(text.get(sign..).unwrap_or(&[]));
    (digits > 0).then_some(sign + digits)
}

fn digit_run(text: &[u16]) -> usize {
    text.iter().take_while(|&&c| (48..=57).contains(&c)).count()
}

fn eat_number_before<'a>(rest: &mut &'a [u16], lower: u8) -> Option<&'a [u16]> {
    let length = number_length(rest)?;
    let (number, tail) = rest.split_at(length);
    let (&designator, tail) = tail.split_first()?;
    if !is_letter(designator, lower) {
        return None;
    }
    *rest = tail;
    Some(number)
}

type SecondsParts<'a> = (&'a [u16], Option<&'a [u16]>);

fn eat_seconds<'a>(rest: &mut &'a [u16]) -> Option<SecondsParts<'a>> {
    let length = number_length(rest)?;
    let (whole, tail) = rest.split_at(length);
    let (fraction, tail) = match tail.split_first() {
        Some((&separator, after)) if separator == 46 || separator == 44 => {
            let digits = digit_run(after);
            if digits > 9 {
                return None;
            }
            let (fraction, tail) = after.split_at(digits);
            (Some(fraction), tail)
        }
        _ => (None, tail),
    };
    let (&designator, tail) = tail.split_first()?;
    if !is_letter(designator, b's') {
        return None;
    }
    *rest = tail;
    Some((whole, fraction))
}

fn parse_scaled(number: Option<&[u16]>, multiplier: i64, name: &str) -> JResult<i64> {
    let Some(number) = number else {
        return Ok(0);
    };
    parse_long(number).ok().and_then(|value| value.checked_mul(multiplier)).ok_or_else(|| parse_error(Some(name)))
}

fn parse_fraction(fraction: Option<&[u16]>, negative: bool) -> i64 {
    let digits = fraction.unwrap_or(&[]);
    let mut value = digits.iter().fold(0i64, |total, &digit| total * 10 + i64::from(digit) - 48);
    for _ in digits.len()..9 {
        value *= 10;
    }
    if negative {
        -value
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::js;

    fn parse(text: &str) -> JResult<Duration> {
        Duration::parse(&js(text))
    }

    #[test]
    fn parse_accepts_iso_forms() {
        assert_eq!(parse("PT1H30M").unwrap().seconds, 5400);
        assert_eq!(parse("p1dt1s").unwrap().seconds, 86_401);
        assert_eq!(parse("-PT0.5S").unwrap().to_string(), "PT-0.5S");
        assert_eq!(parse("PT-1,25S").unwrap().to_string(), "PT-1.25S");
        assert_eq!(parse("+P0D"), Ok(Duration::ZERO));
        assert_eq!(parse("P1Dt").unwrap().seconds, 86_400);
    }

    #[test]
    fn parse_reports_java_messages() {
        for text in ["", "P", "PT", "P1DT", "PT1.1234567890S", "PT1H1H"] {
            assert_eq!(parse(text).unwrap_err().message, Some(js("Text cannot be parsed to a Duration")));
        }
        assert_eq!(
            parse("P9223372036854775807D").unwrap_err().message,
            Some(js("Text cannot be parsed to a Duration: days"))
        );
        assert_eq!(parse("PT9223372036854775807S").unwrap().seconds, i64::MAX);
        assert_eq!(parse("PT9223372036854775807S1").unwrap_err().class, "java.time.format.DateTimeParseException");
    }

    #[test]
    fn to_string_splits_units() {
        assert_eq!(Duration::ZERO.to_string(), "PT0S");
        assert_eq!(Duration::of_millis(-1).to_string(), "PT-0.001S");
        assert_eq!(Duration::of_days(1).unwrap().to_string(), "PT24H");
        let mixed = Duration::of_millis(3_661_500);
        assert_eq!(mixed.to_string(), "PT1H1M1.5S");
    }

    #[test]
    fn arithmetic_reports_overflow() {
        let error = Duration::of_days(i64::MAX).unwrap_err();
        assert_eq!(error.class, "java.lang.ArithmeticException");
        assert_eq!(error.message, Some(js("long overflow")));
        let max = Duration { seconds: i64::MAX, nanos: 0 };
        assert!(max.plus_seconds(1).is_err());
        assert_eq!(Duration::of_millis(1500).to_millis(), Ok(1500));
        assert_eq!(Duration::of_millis(-1500).to_millis(), Ok(-1500));
    }

    #[test]
    fn minus_handles_the_smallest_duration() {
        let lowest = Duration { seconds: i64::MIN, nanos: 0 };
        assert_eq!(Duration::ZERO.minus(&lowest).unwrap_err().message, Some(js("long overflow")));
        assert_eq!(Duration::of_millis(1).minus(&Duration::of_millis(3)), Ok(Duration::of_millis(-2)));
    }
}
