//! `LocalDate`, `LocalTime` and `LocalDateTime`.

use std::fmt;

use super::field::Field;
use super::zone::ZoneId;
use super::zoned::ZonedDateTime;
use super::{date_time_error, Unit};
use crate::java::JResult;

pub(crate) const DAYS_PER_WEEK: i32 = 7;
pub(crate) const SECONDS_PER_DAY: i64 = 86_400;
const SECONDS_PER_HOUR: i32 = 3600;
const SECONDS_PER_MINUTE: i32 = 60;
/// Days from 0000-03-01 to 1970-01-01.
const EPOCH_SHIFT: i64 = 719_468;
const DAYS_PER_400_YEARS: i64 = 146_097;
const MONTH_NAMES: [&str; 12] = [
    "JANUARY",
    "FEBRUARY",
    "MARCH",
    "APRIL",
    "MAY",
    "JUNE",
    "JULY",
    "AUGUST",
    "SEPTEMBER",
    "OCTOBER",
    "NOVEMBER",
    "DECEMBER",
];

/// `Year.isLeap`.
pub(crate) fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn month_length(month: i32, leap: bool) -> i32 {
    match month {
        2 => 28 + i32::from(leap),
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The civil year, month and day of an epoch day; the year is not limited to `LocalDate`.
pub(crate) fn civil_from_epoch_day(epoch_day: i64) -> (i64, i32, i32) {
    let shifted = epoch_day + EPOCH_SHIFT;
    let era = shifted.div_euclid(DAYS_PER_400_YEARS);
    let day_of_era = shifted.rem_euclid(DAYS_PER_400_YEARS);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_shifted_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_shifted_year + 2) / 153;
    let day = (day_of_shifted_year - (153 * shifted_month + 2) / 5 + 1) as i32;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 } as i32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `LocalDate.toEpochDay` for any year.
pub(crate) fn epoch_day_from_civil(year: i64, month: i32, day: i32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let shifted_month = i64::from(if month > 2 { month - 3 } else { month + 9 });
    let day_of_shifted_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_shifted_year;
    era * DAYS_PER_400_YEARS + day_of_era - EPOCH_SHIFT
}

/// A date without a zone; years are limited to ±999,999,999.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalDate {
    year: i32,
    month: u8,
    day: u8,
}

impl LocalDate {
    /// `LocalDate.of(year, month, dayOfMonth)`.
    pub fn of(year: i32, month: i32, day: i32) -> JResult<LocalDate> {
        Field::Year.check_valid(i64::from(year))?;
        Field::MonthOfYear.check_valid(i64::from(month))?;
        Field::DayOfMonth.check_valid(i64::from(day))?;
        let leap = is_leap_year(i64::from(year));
        if day > month_length(month, leap) {
            let message = if day == 29 {
                format!("Invalid date 'February 29' as '{year}' is not a leap year")
            } else {
                let name = MONTH_NAMES.get((month - 1) as usize).unwrap_or(&"");
                format!("Invalid date '{name} {day}'")
            };
            return Err(date_time_error(&message));
        }
        Ok(LocalDate::new_valid(year, month, day))
    }

    /// `LocalDate.ofYearDay(year, dayOfYear)`.
    pub fn of_year_day(year: i32, day_of_year: i32) -> JResult<LocalDate> {
        Field::Year.check_valid(i64::from(year))?;
        Field::DayOfYear.check_valid(i64::from(day_of_year))?;
        if day_of_year == 366 && !is_leap_year(i64::from(year)) {
            let message = format!("Invalid date 'DayOfYear 366' as '{year}' is not a leap year");
            return Err(date_time_error(&message));
        }
        LocalDate::of_epoch_day(epoch_day_from_civil(i64::from(year), 1, 1) + i64::from(day_of_year) - 1)
    }

    /// `LocalDate.ofEpochDay(epochDay)`.
    pub fn of_epoch_day(epoch_day: i64) -> JResult<LocalDate> {
        Field::EpochDay.check_valid(epoch_day)?;
        let (year, month, day) = civil_from_epoch_day(epoch_day);
        Ok(LocalDate::new_valid(year as i32, month, day))
    }

    fn new_valid(year: i32, month: i32, day: i32) -> LocalDate {
        LocalDate { year, month: month as u8, day: day as u8 }
    }

    /// The proleptic ISO year.
    pub fn year(&self) -> i32 {
        self.year
    }

    /// The month, from 1 to 12.
    pub fn month(&self) -> i32 {
        i32::from(self.month)
    }

    /// The day of the month, from 1.
    pub fn day(&self) -> i32 {
        i32::from(self.day)
    }

    /// The ISO day of week, from 1 (Monday) to 7 (Sunday).
    pub fn day_of_week(&self) -> i32 {
        ((self.to_epoch_day() + 3).rem_euclid(7) + 1) as i32
    }

    /// The day of the year, from 1.
    pub fn day_of_year(&self) -> i32 {
        let first = epoch_day_from_civil(i64::from(self.year), 1, 1);
        (self.to_epoch_day() - first + 1) as i32
    }

    /// `LocalDate.lengthOfYear`.
    pub fn length_of_year(&self) -> i32 {
        365 + i32::from(is_leap_year(i64::from(self.year)))
    }

    /// `LocalDate.toEpochDay`.
    pub fn to_epoch_day(&self) -> i64 {
        epoch_day_from_civil(i64::from(self.year), self.month(), self.day())
    }

    /// `LocalDate.plusDays`.
    pub fn plus_days(&self, days: i64) -> JResult<LocalDate> {
        // The epoch-day range check cannot see years just past the limit, so the year is checked.
        let epoch_day = self.to_epoch_day().saturating_add(days);
        let (year, month, day) = civil_from_epoch_day(epoch_day.clamp(i64::MIN / 2, i64::MAX / 2));
        let year = Field::Year.check_valid_int(year)?;
        Ok(LocalDate::new_valid(year, month, day))
    }

    /// `LocalDate.with(TemporalAdjusters.nextOrSame(dayOfWeek))`.
    pub(crate) fn next_or_same(&self, day_of_week: i32) -> JResult<LocalDate> {
        let days = (day_of_week - self.day_of_week()).rem_euclid(7);
        self.plus_days(i64::from(days))
    }

    /// `LocalDate.with(TemporalAdjusters.previousOrSame(dayOfWeek))`.
    pub(crate) fn previous_or_same(&self, day_of_week: i32) -> JResult<LocalDate> {
        let days = (self.day_of_week() - day_of_week).rem_euclid(7);
        self.plus_days(-i64::from(days))
    }

    /// The last day of a month of a year, which must be valid.
    pub(crate) fn month_end_day(year: i32, month: i32) -> i32 {
        month_length(month, is_leap_year(i64::from(year)))
    }

    /// `LocalDate.withDayOfMonth(1)`; never invalid.
    fn first_of_month(&self) -> LocalDate {
        LocalDate::new_valid(self.year, self.month(), 1)
    }

    /// `LocalDate.withDayOfYear(1)`; never invalid.
    fn first_of_year(&self) -> LocalDate {
        LocalDate::new_valid(self.year, 1, 1)
    }
}

impl fmt::Display for LocalDate {
    /// `LocalDate.toString`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let year = self.year;
        if year.abs() < 1000 {
            let sign = if year < 0 { "-" } else { "" };
            write!(f, "{sign}{:04}", year.abs())?;
        } else {
            if year > 9999 {
                f.write_str("+")?;
            }
            write!(f, "{year}")?;
        }
        write!(f, "-{:02}-{:02}", self.month, self.day)
    }
}

/// A time of day with nanosecond precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalTime {
    hour: u8,
    minute: u8,
    second: u8,
    nano: i32,
}

impl LocalTime {
    /// `LocalTime.MIDNIGHT`.
    pub const MIDNIGHT: LocalTime = LocalTime { hour: 0, minute: 0, second: 0, nano: 0 };

    /// `LocalTime.of(hour, minute, second, nanoOfSecond)`.
    pub fn of(hour: i32, minute: i32, second: i32, nano: i32) -> JResult<LocalTime> {
        Field::HourOfDay.check_valid(i64::from(hour))?;
        Field::MinuteOfHour.check_valid(i64::from(minute))?;
        Field::SecondOfMinute.check_valid(i64::from(second))?;
        Field::NanoOfSecond.check_valid(i64::from(nano))?;
        Ok(LocalTime { hour: hour as u8, minute: minute as u8, second: second as u8, nano })
    }

    /// The time at a second of the day in `0..86400`.
    fn of_valid_second(second_of_day: i32, nano: i32) -> LocalTime {
        LocalTime {
            hour: (second_of_day / SECONDS_PER_HOUR) as u8,
            minute: (second_of_day / SECONDS_PER_MINUTE % SECONDS_PER_MINUTE) as u8,
            second: (second_of_day % SECONDS_PER_MINUTE) as u8,
            nano,
        }
    }

    /// The hour of the day.
    pub fn hour(&self) -> i32 {
        i32::from(self.hour)
    }

    /// The minute of the hour.
    pub fn minute(&self) -> i32 {
        i32::from(self.minute)
    }

    /// The second of the minute.
    pub fn second(&self) -> i32 {
        i32::from(self.second)
    }

    /// The nanosecond of the second.
    pub fn nano(&self) -> i32 {
        self.nano
    }

    /// `LocalTime.toSecondOfDay`.
    pub fn second_of_day(&self) -> i32 {
        self.hour() * SECONDS_PER_HOUR + self.minute() * SECONDS_PER_MINUTE + self.second()
    }

    fn truncated_to(&self, unit: Unit) -> LocalTime {
        match unit {
            Unit::Days => LocalTime::MIDNIGHT,
            Unit::Hours => LocalTime::of_valid_second(self.hour() * SECONDS_PER_HOUR, 0),
            Unit::Minutes => LocalTime::of_valid_second(self.second_of_day() - self.second(), 0),
            Unit::Seconds => LocalTime { nano: 0, ..*self },
        }
    }
}

impl fmt::Display for LocalTime {
    /// `LocalTime.toString`: seconds and fraction appear only when needed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)?;
        if self.second == 0 && self.nano == 0 {
            return Ok(());
        }
        write!(f, ":{:02}", self.second)?;
        if self.nano == 0 {
            Ok(())
        } else if self.nano % 1_000_000 == 0 {
            write!(f, ".{:03}", self.nano / 1_000_000)
        } else if self.nano % 1000 == 0 {
            write!(f, ".{:06}", self.nano / 1000)
        } else {
            write!(f, ".{:09}", self.nano)
        }
    }
}

/// A date-time without a zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalDateTime {
    date: LocalDate,
    time: LocalTime,
}

impl LocalDateTime {
    /// `LocalDateTime.of(year, month, day, hour, minute, second, nanoOfSecond)`.
    pub fn of(
        year: i32,
        month: i32,
        day: i32,
        hour: i32,
        minute: i32,
        second: i32,
        nano: i32,
    ) -> JResult<LocalDateTime> {
        let date = LocalDate::of(year, month, day)?;
        let time = LocalTime::of(hour, minute, second, nano)?;
        Ok(LocalDateTime { date, time })
    }

    /// Combines a date and a time.
    pub fn new(date: LocalDate, time: LocalTime) -> LocalDateTime {
        LocalDateTime { date, time }
    }

    /// `LocalDateTime.ofEpochSecond(epochSecond, nano, offset)` for an in-range nano.
    pub(crate) fn of_epoch_second(epoch: i64, nano: i32, offset: i32) -> JResult<LocalDateTime> {
        let local = epoch.saturating_add(i64::from(offset));
        let date = LocalDate::of_epoch_day(local.div_euclid(SECONDS_PER_DAY))?;
        let second_of_day = local.rem_euclid(SECONDS_PER_DAY) as i32;
        let time = LocalTime::of_valid_second(second_of_day, nano);
        Ok(LocalDateTime { date, time })
    }

    /// The date part.
    pub fn date(&self) -> LocalDate {
        self.date
    }

    /// The time part.
    pub fn time(&self) -> LocalTime {
        self.time
    }

    /// Seconds since the epoch of this date-time read as UTC.
    pub(crate) fn local_epoch_second(&self) -> i64 {
        self.date.to_epoch_day() * SECONDS_PER_DAY + i64::from(self.time.second_of_day())
    }

    /// `LocalDateTime.plusSeconds`.
    pub(crate) fn plus_seconds(&self, seconds: i64) -> JResult<LocalDateTime> {
        let total = i64::from(self.time.second_of_day()) + seconds;
        let date = self.date.plus_days(total.div_euclid(SECONDS_PER_DAY))?;
        let second_of_day = total.rem_euclid(SECONDS_PER_DAY) as i32;
        let time = LocalTime::of_valid_second(second_of_day, self.time.nano);
        Ok(LocalDateTime { date, time })
    }

    /// `LocalDateTime.atZone`: gaps move forward, overlaps take the earlier offset.
    pub fn at_zone(&self, zone: &ZoneId) -> JResult<ZonedDateTime> {
        ZonedDateTime::of_local(*self, zone, None)
    }

    pub(crate) fn truncated_to(&self, unit: Unit) -> LocalDateTime {
        LocalDateTime { date: self.date, time: self.time.truncated_to(unit) }
    }

    pub(crate) fn with_first_day_of_month(&self) -> LocalDateTime {
        LocalDateTime { date: self.date.first_of_month(), time: self.time }
    }

    pub(crate) fn with_first_day_of_year(&self) -> LocalDateTime {
        LocalDateTime { date: self.date.first_of_year(), time: self.time }
    }
}

impl fmt::Display for LocalDateTime {
    /// `LocalDateTime.toString`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}T{}", self.date, self.time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::js;

    fn message(result: JResult<LocalDate>) -> Option<Vec<u16>> {
        result.unwrap_err().message
    }

    #[test]
    fn invalid_dates_report_java_messages() {
        assert_eq!(
            message(LocalDate::of(2023, 13, 1)),
            Some(js("Invalid value for MonthOfYear (valid values 1 - 12): 13"))
        );
        assert_eq!(message(LocalDate::of(2023, 2, 30)), Some(js("Invalid date 'FEBRUARY 30'")));
        assert_eq!(
            message(LocalDate::of(2023, 2, 29)),
            Some(js("Invalid date 'February 29' as '2023' is not a leap year"))
        );
        assert_eq!(
            message(LocalDate::of_year_day(2023, 366)),
            Some(js("Invalid date 'DayOfYear 366' as '2023' is not a leap year"))
        );
    }

    #[test]
    fn epoch_day_round_trips_across_eras() {
        for (year, month, day, epoch_day) in [
            (1970, 1, 1, 0),
            (2000, 2, 29, 11_016),
            (0, 3, 1, -719_468),
            (-1, 12, 31, -719_529),
            (-999_999_999, 1, 1, -365_243_219_162),
            (999_999_999, 12, 31, 365_241_780_471),
        ] {
            let date = LocalDate::of(year, month, day).unwrap();
            assert_eq!(date.to_epoch_day(), epoch_day);
            assert_eq!(LocalDate::of_epoch_day(epoch_day), Ok(date));
        }
    }

    #[test]
    fn calendar_accessors() {
        let date = LocalDate::of(2024, 3, 1).unwrap();
        assert_eq!(date.day_of_week(), 5);
        assert_eq!(date.day_of_year(), 61);
        assert_eq!(date.length_of_year(), 366);
        assert_eq!(date.next_or_same(1).unwrap().day(), 4);
        assert_eq!(date.previous_or_same(1).unwrap().day(), 26);
    }

    #[test]
    fn text_forms_match_to_string() {
        assert_eq!(LocalDate::of(-5, 1, 2).unwrap().to_string(), "-0005-01-02");
        assert_eq!(LocalDate::of(12345, 1, 2).unwrap().to_string(), "+12345-01-02");
        let time = LocalTime::of(1, 2, 0, 5_000).unwrap();
        assert_eq!(time.to_string(), "01:02:00.000005");
        assert_eq!(LocalTime::MIDNIGHT.to_string(), "00:00");
    }

    #[test]
    fn plus_days_stops_at_the_year_limit() {
        let last = LocalDate::of(999_999_999, 12, 31).unwrap();
        let error = last.plus_days(1).unwrap_err();
        assert_eq!(error.message, Some(js("Invalid value for Year (valid values -999999999 - 999999999): 1000000000")));
    }
}
