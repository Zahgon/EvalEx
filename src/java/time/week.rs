//! The week computations of `java.time.temporal.WeekFields`.

use std::fmt;

use super::date_time_error;
use super::field::Field;
use super::local::{LocalDate, DAYS_PER_WEEK};
use crate::java::JResult;

/// The two parameters of a `WeekFields` instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WeekDef {
    /// ISO number of the first day of the week.
    first_day: u8,
    /// Minimal number of days in the first week.
    minimal_days: u8,
}

impl WeekDef {
    /// `WeekFields[SUNDAY,1]`.
    pub(crate) const SUNDAY_1: WeekDef = WeekDef { first_day: 7, minimal_days: 1 };
    /// `WeekFields[MONDAY,4]`.
    pub(crate) const MONDAY_4: WeekDef = WeekDef { first_day: 1, minimal_days: 4 };

    /// The ISO day of week of a localized day of week.
    pub(crate) fn iso_day_of_week(self, localized: i32) -> i32 {
        (i32::from(self.first_day) - 1 + (localized - 1)).rem_euclid(7) + 1
    }

    /// The number of a date's day within the localized week, from 1.
    pub(crate) fn localized_day_of_week(self, date: &LocalDate) -> i32 {
        self.localized_from_iso(date.day_of_week())
    }

    /// The localized number of an ISO day of week.
    pub(crate) fn localized_from_iso(self, iso: i32) -> i32 {
        (iso - i32::from(self.first_day)).rem_euclid(7) + 1
    }

    /// Offset from the first of a period to the start of its first week.
    fn start_of_week_offset(self, day: i32, localized_dow: i32) -> i32 {
        let week_start = (day - localized_dow).rem_euclid(7);
        if week_start + 1 > i32::from(self.minimal_days) {
            7 - week_start
        } else {
            -week_start
        }
    }

    /// `WeekFields.weekOfMonth` of a date.
    pub(crate) fn week_of_month(self, date: &LocalDate) -> i32 {
        let dow = self.localized_day_of_week(date);
        let day = date.day();
        compute_week(self.start_of_week_offset(day, dow), day)
    }

    /// `WeekFields.weekOfWeekBasedYear` of a date.
    pub(crate) fn week_of_week_based_year(self, date: &LocalDate) -> JResult<i32> {
        let (doy, _, week) = self.week_of_calendar_year(date);
        if week == 0 {
            let last_of_previous = date.plus_days(-i64::from(doy))?;
            let (_, _, previous_week) = self.week_of_calendar_year(&last_of_previous);
            return Ok(self.fold_final_week(&last_of_previous, previous_week));
        }
        Ok(self.fold_final_week(date, week))
    }

    /// `WeekFields.weekBasedYear` of a date.
    pub(crate) fn week_based_year(self, date: &LocalDate) -> i64 {
        let (_, offset, week) = self.week_of_calendar_year(date);
        let year = i64::from(date.year());
        if week == 0 {
            year - 1
        } else if week >= self.first_week_of_next_year(date, offset) {
            year + 1
        } else {
            year
        }
    }

    /// The date of a day in a week of a week-based year, clamping the week to the year.
    pub(crate) fn date_of_week_based_year(self, year: i32, week: i32, localized_dow: i32) -> JResult<LocalDate> {
        let first = LocalDate::of(year, 1, 1)?;
        let offset = self.start_of_week_offset(1, self.localized_day_of_week(&first));
        let last_week = self.first_week_of_next_year(&first, offset) - 1;
        let days = -offset + (localized_dow - 1) + (week.min(last_week) - 1) * DAYS_PER_WEEK;
        first.plus_days(i64::from(days))
    }

    /// Validates a week against the number of weeks of its week-based year.
    pub(crate) fn check_week_of_year(self, field: Field, year: i32, week: i64, localized_dow: i32) -> JResult<i32> {
        let in_year = self.date_of_week_based_year(year, 1, localized_dow)?;
        let last_week = i64::from(self.weeks_of_year_containing(&in_year)?);
        if !(1..=last_week).contains(&week) {
            return Err(date_time_error(&format!("Invalid value for {field} (valid values 1 - {last_week}): {week}")));
        }
        Ok(week as i32)
    }

    /// The number of weeks of the week-based year of a date.
    fn weeks_of_year_containing(self, date: &LocalDate) -> JResult<i32> {
        let (doy, offset, week) = self.week_of_calendar_year(date);
        if week == 0 {
            let previous = date.plus_days(-i64::from(doy))?;
            let (_, offset, _) = self.week_of_calendar_year(&previous);
            return Ok(self.first_week_of_next_year(&previous, offset) - 1);
        }
        let next = self.first_week_of_next_year(date, offset);
        if week >= next {
            let days_left = date.length_of_year() - doy + 1 + DAYS_PER_WEEK;
            let following = date.plus_days(i64::from(days_left))?;
            let (_, offset, _) = self.week_of_calendar_year(&following);
            return Ok(self.first_week_of_next_year(&following, offset) - 1);
        }
        Ok(next - 1)
    }

    /// Day of year, week-start offset and raw week number of a date.
    fn week_of_calendar_year(self, date: &LocalDate) -> (i32, i32, i32) {
        let dow = self.localized_day_of_week(date);
        let doy = date.day_of_year();
        let offset = self.start_of_week_offset(doy, dow);
        (doy, offset, compute_week(offset, doy))
    }

    /// Maps a week number past the end of the year onto the next year's numbering.
    fn fold_final_week(self, date: &LocalDate, week: i32) -> i32 {
        if week > 50 {
            let (_, offset, _) = self.week_of_calendar_year(date);
            let next = self.first_week_of_next_year(date, offset);
            if week >= next {
                return week - next + 1;
            }
        }
        week
    }

    /// The week number that belongs to the next week-based year.
    fn first_week_of_next_year(self, date: &LocalDate, offset: i32) -> i32 {
        compute_week(offset, date.length_of_year() + i32::from(self.minimal_days))
    }
}

impl fmt::Display for WeekDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let day = if self.first_day == 7 { "SUNDAY" } else { "MONDAY" };
        write!(f, "WeekFields[{day},{}]", self.minimal_days)
    }
}

fn compute_week(offset: i32, day: i32) -> i32 {
    (DAYS_PER_WEEK + offset + (day - 1)) / DAYS_PER_WEEK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_week_rules_on_a_year_boundary() {
        let date = LocalDate::of(2021, 1, 3).unwrap();
        assert_eq!(WeekDef::MONDAY_4.week_of_week_based_year(&date), Ok(53));
        assert_eq!(WeekDef::MONDAY_4.week_based_year(&date), 2020);
        assert_eq!(WeekDef::SUNDAY_1.week_of_week_based_year(&date), Ok(2));
        assert_eq!(WeekDef::SUNDAY_1.week_based_year(&date), 2021);
    }

    #[test]
    fn week_date_round_trip() {
        let date = WeekDef::MONDAY_4.date_of_week_based_year(2020, 53, 7).unwrap();
        assert_eq!(date.to_string(), "2021-01-03");
    }

    #[test]
    fn display_names_the_first_day() {
        assert_eq!(WeekDef::SUNDAY_1.to_string(), "WeekFields[SUNDAY,1]");
    }
}
