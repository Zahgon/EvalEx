//! The temporal fields that patterns can reach: `ChronoField`, `IsoFields`, `JulianFields` and the
//! localized `WeekFields` members, with their value ranges and accessors.

use std::fmt;

use super::local::{LocalDate, LocalTime};
use super::week::WeekDef;
use super::{date_time_error, to_int_exact};
use crate::java::JResult;

const JULIAN_EPOCH_OFFSET: i64 = 40587;
const NANOS_PER_MILLI: i64 = 1_000_000;

/// The localized fields of `WeekFields`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum WeekKind {
    DayOfWeek,
    WeekOfMonth,
    WeekOfWeekBasedYear,
    WeekBasedYear,
}

/// A `TemporalField`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Field {
    Era,
    YearOfEra,
    Year,
    DayOfYear,
    MonthOfYear,
    DayOfMonth,
    AlignedWeekOfMonth,
    DayOfWeek,
    EpochDay,
    AmPmOfDay,
    ClockHourOfAmPm,
    HourOfAmPm,
    ClockHourOfDay,
    HourOfDay,
    MinuteOfHour,
    SecondOfMinute,
    NanoOfSecond,
    MicroOfSecond,
    MilliOfSecond,
    SecondOfDay,
    MilliOfDay,
    NanoOfDay,
    OffsetSeconds,
    InstantSeconds,
    QuarterOfYear,
    ModifiedJulianDay,
    Week(WeekKind, WeekDef),
}

/// A `java.time.temporal.ValueRange`.
#[derive(Clone, Copy)]
struct ValueRange {
    min: i64,
    largest_min: i64,
    smallest_max: i64,
    max: i64,
}

impl ValueRange {
    const fn fixed(min: i64, max: i64) -> ValueRange {
        ValueRange { min, largest_min: min, smallest_max: max, max }
    }

    const fn variable(min: i64, smallest_max: i64, max: i64) -> ValueRange {
        ValueRange { min, largest_min: min, smallest_max, max }
    }
}

impl fmt::Display for ValueRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.min)?;
        if self.min != self.largest_min {
            write!(f, "/{}", self.largest_min)?;
        }
        write!(f, " - {}", self.smallest_max)?;
        if self.smallest_max != self.max {
            write!(f, "/{}", self.max)?;
        }
        Ok(())
    }
}

impl Field {
    fn range(self) -> ValueRange {
        const YEARS: ValueRange = ValueRange::fixed(-999_999_999, 999_999_999);
        match self {
            Field::Era => ValueRange::fixed(0, 1),
            Field::YearOfEra => ValueRange::variable(1, 999_999_999, 1_000_000_000),
            Field::Year | Field::Week(WeekKind::WeekBasedYear, _) => YEARS,
            Field::DayOfYear => ValueRange::variable(1, 365, 366),
            Field::MonthOfYear => ValueRange::fixed(1, 12),
            Field::DayOfMonth => ValueRange::variable(1, 28, 31),
            Field::AlignedWeekOfMonth => ValueRange::variable(1, 4, 5),
            Field::DayOfWeek | Field::Week(WeekKind::DayOfWeek, _) => ValueRange::fixed(1, 7),
            Field::EpochDay => ValueRange::fixed(-365_243_219_162, 365_241_780_471),
            Field::AmPmOfDay => ValueRange::fixed(0, 1),
            Field::ClockHourOfAmPm => ValueRange::fixed(1, 12),
            Field::HourOfAmPm => ValueRange::fixed(0, 11),
            Field::ClockHourOfDay => ValueRange::fixed(1, 24),
            Field::HourOfDay => ValueRange::fixed(0, 23),
            Field::MinuteOfHour | Field::SecondOfMinute => ValueRange::fixed(0, 59),
            Field::NanoOfSecond => ValueRange::fixed(0, 999_999_999),
            Field::MicroOfSecond => ValueRange::fixed(0, 999_999),
            Field::MilliOfSecond => ValueRange::fixed(0, 999),
            Field::SecondOfDay => ValueRange::fixed(0, 86_399),
            Field::MilliOfDay => ValueRange::fixed(0, 86_400_000 - 1),
            Field::NanoOfDay => ValueRange::fixed(0, 86_400_000_000_000 - 1),
            Field::OffsetSeconds => ValueRange::fixed(-64_800, 64_800),
            Field::InstantSeconds => ValueRange::fixed(i64::MIN, i64::MAX),
            Field::QuarterOfYear => ValueRange::fixed(1, 4),
            Field::ModifiedJulianDay => {
                ValueRange::fixed(-365_243_219_162 + JULIAN_EPOCH_OFFSET, 365_241_780_471 + JULIAN_EPOCH_OFFSET)
            }
            Field::Week(WeekKind::WeekOfMonth, _) => ValueRange { min: 0, largest_min: 1, smallest_max: 4, max: 6 },
            Field::Week(WeekKind::WeekOfWeekBasedYear, _) => ValueRange::variable(1, 52, 53),
        }
    }

    /// `ChronoField.checkValidValue`.
    pub(crate) fn check_valid(self, value: i64) -> JResult<i64> {
        let range = self.range();
        if value < range.min || value > range.max {
            let message = format!("Invalid value for {self} (valid values {range}): {value}");
            return Err(date_time_error(&message));
        }
        Ok(value)
    }

    /// `ChronoField.checkValidIntValue`.
    pub(crate) fn check_valid_int(self, value: i64) -> JResult<i32> {
        to_int_exact(self.check_valid(value)?)
    }

    /// `ChronoField.isTimeBased`.
    pub(crate) fn is_time_based(self) -> bool {
        matches!(
            self,
            Field::AmPmOfDay
                | Field::ClockHourOfAmPm
                | Field::HourOfAmPm
                | Field::ClockHourOfDay
                | Field::HourOfDay
                | Field::MinuteOfHour
                | Field::SecondOfMinute
                | Field::NanoOfSecond
                | Field::MicroOfSecond
                | Field::MilliOfSecond
                | Field::SecondOfDay
                | Field::MilliOfDay
                | Field::NanoOfDay
        )
    }

    /// The value of a date-based field on a date, `None` for other fields.
    pub(crate) fn of_date(self, date: &LocalDate) -> Option<JResult<i64>> {
        let year = i64::from(date.year());
        let value = match self {
            Field::Era => i64::from(year >= 1),
            Field::YearOfEra => {
                if year >= 1 {
                    year
                } else {
                    1 - year
                }
            }
            Field::Year => year,
            Field::DayOfYear => i64::from(date.day_of_year()),
            Field::MonthOfYear => i64::from(date.month()),
            Field::DayOfMonth => i64::from(date.day()),
            Field::AlignedWeekOfMonth => i64::from((date.day() - 1) / 7 + 1),
            Field::DayOfWeek => i64::from(date.day_of_week()),
            Field::EpochDay => date.to_epoch_day(),
            Field::QuarterOfYear => i64::from((date.month() + 2) / 3),
            Field::ModifiedJulianDay => date.to_epoch_day() + JULIAN_EPOCH_OFFSET,
            Field::Week(kind, def) => match kind {
                WeekKind::DayOfWeek => i64::from(def.localized_day_of_week(date)),
                WeekKind::WeekOfMonth => i64::from(def.week_of_month(date)),
                WeekKind::WeekOfWeekBasedYear => return Some(def.week_of_week_based_year(date).map(i64::from)),
                WeekKind::WeekBasedYear => def.week_based_year(date),
            },
            _ => return None,
        };
        Some(Ok(value))
    }

    /// The value of a time-based field on a time, `None` for other fields.
    pub(crate) fn of_time(self, time: &LocalTime) -> Option<i64> {
        let hour = i64::from(time.hour());
        let nano = i64::from(time.nano());
        let value = match self {
            Field::AmPmOfDay => hour / 12,
            Field::ClockHourOfAmPm => {
                let hour = hour % 12;
                if hour == 0 {
                    12
                } else {
                    hour
                }
            }
            Field::HourOfAmPm => hour % 12,
            Field::ClockHourOfDay => {
                if hour == 0 {
                    24
                } else {
                    hour
                }
            }
            Field::HourOfDay => hour,
            Field::MinuteOfHour => i64::from(time.minute()),
            Field::SecondOfMinute => i64::from(time.second()),
            Field::NanoOfSecond => nano,
            Field::MicroOfSecond => nano / 1000,
            Field::MilliOfSecond => nano / NANOS_PER_MILLI,
            Field::SecondOfDay => i64::from(time.second_of_day()),
            Field::MilliOfDay => i64::from(time.second_of_day()) * 1000 + nano / NANOS_PER_MILLI,
            Field::NanoOfDay => i64::from(time.second_of_day()) * 1_000_000_000 + nano,
            _ => return None,
        };
        Some(value)
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Field::Era => "Era",
            Field::YearOfEra => "YearOfEra",
            Field::Year => "Year",
            Field::DayOfYear => "DayOfYear",
            Field::MonthOfYear => "MonthOfYear",
            Field::DayOfMonth => "DayOfMonth",
            Field::AlignedWeekOfMonth => "AlignedWeekOfMonth",
            Field::DayOfWeek => "DayOfWeek",
            Field::EpochDay => "EpochDay",
            Field::AmPmOfDay => "AmPmOfDay",
            Field::ClockHourOfAmPm => "ClockHourOfAmPm",
            Field::HourOfAmPm => "HourOfAmPm",
            Field::ClockHourOfDay => "ClockHourOfDay",
            Field::HourOfDay => "HourOfDay",
            Field::MinuteOfHour => "MinuteOfHour",
            Field::SecondOfMinute => "SecondOfMinute",
            Field::NanoOfSecond => "NanoOfSecond",
            Field::MicroOfSecond => "MicroOfSecond",
            Field::MilliOfSecond => "MilliOfSecond",
            Field::SecondOfDay => "SecondOfDay",
            Field::MilliOfDay => "MilliOfDay",
            Field::NanoOfDay => "NanoOfDay",
            Field::OffsetSeconds => "OffsetSeconds",
            Field::InstantSeconds => "InstantSeconds",
            Field::QuarterOfYear => "QuarterOfYear",
            Field::ModifiedJulianDay => "ModifiedJulianDay",
            Field::Week(kind, def) => {
                let name = match kind {
                    WeekKind::DayOfWeek => "DayOfWeek",
                    WeekKind::WeekOfMonth => "WeekOfMonth",
                    WeekKind::WeekOfWeekBasedYear => "WeekOfWeekBasedYear",
                    WeekKind::WeekBasedYear => "WeekBasedYear",
                };
                return write!(f, "{name}[{def}]");
            }
        };
        f.write_str(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::js;

    #[test]
    fn range_messages_show_variable_bounds() {
        let error = Field::DayOfMonth.check_valid(32).unwrap_err();
        assert_eq!(error.message, Some(js("Invalid value for DayOfMonth (valid values 1 - 28/31): 32")));
        let error = Field::Week(WeekKind::WeekOfMonth, WeekDef::SUNDAY_1).check_valid(7).unwrap_err();
        assert_eq!(
            error.message,
            Some(js("Invalid value for WeekOfMonth[WeekFields[SUNDAY,1]] (valid values 0/1 - 4/6): 7"))
        );
    }

    #[test]
    fn accessors_read_clock_hours() {
        let time = LocalTime::of(0, 30, 0, 0).unwrap();
        assert_eq!(Field::ClockHourOfDay.of_time(&time), Some(24));
        assert_eq!(Field::ClockHourOfAmPm.of_time(&time), Some(12));
        assert_eq!(Field::Year.of_time(&time), None);
    }
}
