//! Resolving parsed fields into a date, a time and an instant, as `java.time.format.Parsed` does.

use std::fmt;

use super::ResolverStyle;
use crate::java::time::field::{Field, WeekKind};
use crate::java::time::instant::Instant;
use crate::java::time::local::{LocalDate, LocalDateTime, LocalTime};
use crate::java::time::week::WeekDef;
use crate::java::time::zone::ZoneId;
use crate::java::time::{date_time_error, to_int_exact};
use crate::java::{JResult, Thrown};

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const NANOS_PER_MINUTE: i64 = 60 * NANOS_PER_SECOND;
const NANOS_PER_HOUR: i64 = 60 * NANOS_PER_MINUTE;
const NANOS_PER_MILLI: i64 = 1_000_000;
const JULIAN_EPOCH_OFFSET: i64 = 40587;

/// The parsed state: fields in parse order, and what they resolved to.
#[derive(Clone, Debug, Default)]
pub(super) struct Parsed {
    fields: Vec<(Field, i64)>,
    zone: Option<ZoneId>,
    date: Option<LocalDate>,
    time: Option<LocalTime>,
    /// Set when 24:00 was parsed and the day still has to advance.
    excess_day: bool,
}

/// The outcome of `DateTimeFormatter.parse`, queried by `parseToInstant`.
pub(super) struct Resolved(Parsed);

impl Parsed {
    /// The value of a field.
    pub(super) fn get(&self, field: Field) -> Option<i64> {
        self.fields.iter().find(|(key, _)| *key == field).map(|&(_, value)| value)
    }

    /// Stores a field and returns its previous value.
    pub(super) fn insert(&mut self, field: Field, value: i64) -> Option<i64> {
        for entry in &mut self.fields {
            if entry.0 == field {
                return Some(std::mem::replace(&mut entry.1, value));
            }
        }
        self.fields.push((field, value));
        None
    }

    fn remove(&mut self, field: Field) -> Option<i64> {
        let index = self.fields.iter().position(|(key, _)| *key == field)?;
        Some(self.fields.remove(index).1)
    }

    fn contains(&self, field: Field) -> bool {
        self.get(field).is_some()
    }

    pub(super) fn set_zone(&mut self, zone: ZoneId) {
        self.zone = Some(zone);
    }

    /// `Parsed.resolve`.
    pub(super) fn resolve(mut self, style: ResolverStyle) -> JResult<Resolved> {
        self.resolve_fields(style)?;
        self.resolve_incomplete_time(style)?;
        self.cross_check()?;
        if self.excess_day {
            if let (Some(date), Some(_)) = (self.date, self.time) {
                self.date = Some(date.plus_days(1)?);
                self.excess_day = false;
            }
        }
        self.resolve_fractional();
        self.resolve_instant()?;
        Ok(Resolved(self))
    }

    fn resolve_fields(&mut self, style: ResolverStyle) -> JResult<()> {
        self.resolve_chrono_fields(style)?;
        let mut changed = false;
        while self.resolve_one_derived_field(style)? {
            changed = true;
        }
        if changed {
            self.resolve_chrono_fields(style)?;
        }
        Ok(())
    }

    fn resolve_chrono_fields(&mut self, style: ResolverStyle) -> JResult<()> {
        let date = self.resolve_date(style)?;
        self.update_date(date)?;
        self.resolve_time_fields(style)
    }

    fn update_date(&mut self, date: Option<LocalDate>) -> JResult<()> {
        match (self.date, date) {
            (Some(current), Some(new)) if current != new => Err(date_time_error(&format!(
                "Conflict found: Fields resolved to two different dates: {current} {new}"
            ))),
            (None, Some(new)) => {
                self.date = Some(new);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn add_field_value(&mut self, field: Field, value: i64) -> JResult<()> {
        match self.get(field) {
            Some(old) if old != value => {
                Err(date_time_error(&format!("Conflict found: {field} {old} differs from {field} {value}")))
            }
            _ => {
                self.insert(field, value);
                Ok(())
            }
        }
    }

    fn resolve_year_of_era(&mut self, style: ResolverStyle) -> JResult<()> {
        let Some(year_of_era) = self.remove(Field::YearOfEra) else {
            if let Some(era) = self.get(Field::Era) {
                Field::Era.check_valid(era)?;
            }
            return Ok(());
        };
        Field::YearOfEra.check_valid(year_of_era)?;
        match self.remove(Field::Era) {
            None => {
                let year = self.get(Field::Year);
                let proleptic = if year.is_none_or(|year| year > 0) { year_of_era } else { 1 - year_of_era };
                if style == ResolverStyle::Strict && year.is_none() {
                    self.insert(Field::YearOfEra, year_of_era);
                    Ok(())
                } else {
                    self.add_field_value(Field::Year, proleptic)
                }
            }
            Some(1) => self.add_field_value(Field::Year, year_of_era),
            Some(0) => self.add_field_value(Field::Year, 1 - year_of_era),
            Some(era) => Err(date_time_error(&format!("Invalid value for era: {era}"))),
        }
    }

    /// `IsoChronology.resolveDate`.
    fn resolve_date(&mut self, style: ResolverStyle) -> JResult<Option<LocalDate>> {
        self.resolve_year_of_era(style)?;
        if !self.contains(Field::Year) {
            return Ok(None);
        }
        if self.contains(Field::MonthOfYear) {
            if self.contains(Field::DayOfMonth) {
                return self.resolve_year_month_day(style).map(Some);
            }
            if self.contains(Field::AlignedWeekOfMonth) && self.contains(Field::DayOfWeek) {
                return self.resolve_aligned_week(style).map(Some);
            }
        }
        if self.contains(Field::DayOfYear) {
            let year = self.take_int(Field::Year)?;
            let day_of_year = self.take_int(Field::DayOfYear)?;
            return LocalDate::of_year_day(year, day_of_year).map(Some);
        }
        Ok(None)
    }

    /// Removes a field and checks its value; the field must be present.
    fn take_int(&mut self, field: Field) -> JResult<i32> {
        field.check_valid_int(self.remove(field).unwrap_or(0))
    }

    fn resolve_year_month_day(&mut self, style: ResolverStyle) -> JResult<LocalDate> {
        let year = self.take_int(Field::Year)?;
        let month = self.take_int(Field::MonthOfYear)?;
        let mut day = self.take_int(Field::DayOfMonth)?;
        if style == ResolverStyle::Smart {
            day = day.min(LocalDate::month_end_day(year, month));
        }
        LocalDate::of(year, month, day)
    }

    fn resolve_aligned_week(&mut self, style: ResolverStyle) -> JResult<LocalDate> {
        let year = self.take_int(Field::Year)?;
        let month = self.take_int(Field::MonthOfYear)?;
        let week = self.take_int(Field::AlignedWeekOfMonth)?;
        let day_of_week = self.take_int(Field::DayOfWeek)?;
        let date = LocalDate::of(year, month, 1)?.plus_days(i64::from(week - 1) * 7)?.next_or_same(day_of_week)?;
        if style == ResolverStyle::Strict && date.month() != month {
            return Err(date_time_error("Strict mode rejected resolved date as it is in a different month"));
        }
        Ok(date)
    }

    /// Runs `resolve` of the first non-chrono field that makes progress.
    fn resolve_one_derived_field(&mut self, style: ResolverStyle) -> JResult<bool> {
        for index in 0..self.fields.len() {
            let Some(&(field, value)) = self.fields.get(index) else {
                break;
            };
            let date = match field {
                Field::ModifiedJulianDay => {
                    self.remove(field);
                    field.check_valid(value)?;
                    Some(LocalDate::of_epoch_day(value - JULIAN_EPOCH_OFFSET)?)
                }
                Field::Week(WeekKind::DayOfWeek, def) => {
                    let localized = field.check_valid_int(value)?;
                    self.remove(field);
                    self.insert(Field::DayOfWeek, i64::from(def.iso_day_of_week(localized)));
                    return Ok(true);
                }
                Field::Week(kind, def) => self.resolve_week_field(field, kind, def, value, style)?,
                _ => None,
            };
            if date.is_some() {
                self.update_date(date)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn resolve_week_field(
        &mut self,
        field: Field,
        kind: WeekKind,
        def: WeekDef,
        value: i64,
        style: ResolverStyle,
    ) -> JResult<Option<LocalDate>> {
        let value = to_int_exact(value)?;
        let Some(day_of_week) = self.get(Field::DayOfWeek) else {
            return Ok(None);
        };
        let day_of_week = Field::DayOfWeek.check_valid_int(day_of_week)?;
        let localized_dow = def.localized_from_iso(day_of_week);
        if let Some(year) = self.get(Field::Year) {
            let year = Field::Year.check_valid_int(year)?;
            if kind == WeekKind::WeekOfMonth && self.contains(Field::MonthOfYear) {
                let month = self.get(Field::MonthOfYear).unwrap_or(0);
                let date = resolve_week_of_month(field, def, year, month, value, localized_dow, style)?;
                self.remove(field);
                self.remove(Field::Year);
                self.remove(Field::MonthOfYear);
                self.remove(Field::DayOfWeek);
                return Ok(Some(date));
            }
            return Ok(None);
        }
        let year_field = Field::Week(WeekKind::WeekBasedYear, def);
        let week_field = Field::Week(WeekKind::WeekOfWeekBasedYear, def);
        let is_week_based = matches!(kind, WeekKind::WeekOfWeekBasedYear | WeekKind::WeekBasedYear);
        let (Some(week_year), Some(week), true) = (self.get(year_field), self.get(week_field), is_week_based) else {
            return Ok(None);
        };
        let week_year = year_field.check_valid_int(week_year)?;
        let week = def.check_week_of_year(week_field, week_year, week, localized_dow)?;
        let date = def.date_of_week_based_year(week_year, week, localized_dow)?;
        if style == ResolverStyle::Strict && def.week_based_year(&date) != i64::from(week_year) {
            return Err(date_time_error("Strict mode rejected resolved date as it is in a different week-based-year"));
        }
        self.remove(year_field);
        self.remove(week_field);
        self.remove(Field::DayOfWeek);
        Ok(Some(date))
    }

    fn update_field(&mut self, source: Field, target: Field, value: i64) -> JResult<()> {
        match self.insert(target, value) {
            Some(old) if old != value => Err(date_time_error(&format!(
                "Conflict found: {target} {old} differs from {target} {value} while resolving  \
                 {source}"
            ))),
            _ => Ok(()),
        }
    }

    fn resolve_clock_hours(&mut self, style: ResolverStyle) -> JResult<()> {
        let smart_zero_allowed = |hour: i64| style == ResolverStyle::Strict || hour != 0;
        if let Some(hour) = self.remove(Field::ClockHourOfDay) {
            if smart_zero_allowed(hour) {
                Field::ClockHourOfDay.check_valid(hour)?;
            }
            let value = if hour == 24 { 0 } else { hour };
            self.update_field(Field::ClockHourOfDay, Field::HourOfDay, value)?;
        }
        if let Some(hour) = self.remove(Field::ClockHourOfAmPm) {
            if smart_zero_allowed(hour) {
                Field::ClockHourOfAmPm.check_valid(hour)?;
            }
            let value = if hour == 12 { 0 } else { hour };
            self.update_field(Field::ClockHourOfAmPm, Field::HourOfAmPm, value)?;
        }
        if self.contains(Field::AmPmOfDay) && self.contains(Field::HourOfAmPm) {
            let am_pm = self.remove(Field::AmPmOfDay).unwrap_or(0);
            let hour = self.remove(Field::HourOfAmPm).unwrap_or(0);
            Field::AmPmOfDay.check_valid(am_pm)?;
            Field::HourOfAmPm.check_valid(hour)?;
            self.update_field(Field::AmPmOfDay, Field::HourOfDay, am_pm * 12 + hour)?;
        }
        Ok(())
    }

    fn resolve_time_fields(&mut self, style: ResolverStyle) -> JResult<()> {
        self.resolve_clock_hours(style)?;
        if let Some(nano_of_day) = self.remove(Field::NanoOfDay) {
            Field::NanoOfDay.check_valid(nano_of_day)?;
            let source = Field::NanoOfDay;
            self.update_field(source, Field::HourOfDay, nano_of_day / NANOS_PER_HOUR)?;
            let minute = nano_of_day / NANOS_PER_MINUTE % 60;
            self.update_field(source, Field::MinuteOfHour, minute)?;
            let second = nano_of_day / NANOS_PER_SECOND % 60;
            self.update_field(source, Field::SecondOfMinute, second)?;
            self.update_field(source, Field::NanoOfSecond, nano_of_day % NANOS_PER_SECOND)?;
        }
        if let Some(milli_of_day) = self.remove(Field::MilliOfDay) {
            Field::MilliOfDay.check_valid(milli_of_day)?;
            let source = Field::MilliOfDay;
            self.update_field(source, Field::SecondOfDay, milli_of_day / 1000)?;
            self.update_field(source, Field::MilliOfSecond, milli_of_day % 1000)?;
        }
        if let Some(second_of_day) = self.remove(Field::SecondOfDay) {
            Field::SecondOfDay.check_valid(second_of_day)?;
            let source = Field::SecondOfDay;
            self.update_field(source, Field::HourOfDay, second_of_day / 3600)?;
            self.update_field(source, Field::MinuteOfHour, second_of_day / 60 % 60)?;
            self.update_field(source, Field::SecondOfMinute, second_of_day % 60)?;
        }
        if let Some(nano) = self.get(Field::NanoOfSecond) {
            Field::NanoOfSecond.check_valid(nano)?;
            if let Some(milli) = self.remove(Field::MilliOfSecond) {
                Field::MilliOfSecond.check_valid(milli)?;
                let merged = milli * NANOS_PER_MILLI + nano % NANOS_PER_MILLI;
                self.update_field(Field::MilliOfSecond, Field::NanoOfSecond, merged)?;
            }
        }
        self.resolve_lone_am_pm(style)?;
        let parts = (
            self.get(Field::HourOfDay),
            self.get(Field::MinuteOfHour),
            self.get(Field::SecondOfMinute),
            self.get(Field::NanoOfSecond),
        );
        if let (Some(hour), Some(minute), Some(second), Some(nano)) = parts {
            self.resolve_time(hour, minute, second, nano, style)?;
        }
        Ok(())
    }

    /// `resolveTimeLenient`: defaults the lower parts of an incomplete time.
    fn resolve_incomplete_time(&mut self, style: ResolverStyle) -> JResult<()> {
        if self.time.is_none() {
            if let Some(milli) = self.remove(Field::MilliOfSecond) {
                self.insert(Field::NanoOfSecond, milli * NANOS_PER_MILLI);
            }
            if let Some(hour) = self.get(Field::HourOfDay) {
                let minute = self.get(Field::MinuteOfHour);
                let second = self.get(Field::SecondOfMinute);
                let nano = self.get(Field::NanoOfSecond);
                let undefaultable = (minute.is_none() && (second.is_some() || nano.is_some()))
                    || (minute.is_some() && second.is_none() && nano.is_some());
                if undefaultable {
                    return Ok(());
                }
                self.resolve_time(hour, minute.unwrap_or(0), second.unwrap_or(0), nano.unwrap_or(0), style)?;
            }
        }
        for &(field, value) in &self.fields {
            if field.is_time_based() {
                field.check_valid(value)?;
            }
        }
        Ok(())
    }

    /// A lone AM/PM stands for the middle of its half of the day.
    fn resolve_lone_am_pm(&mut self, style: ResolverStyle) -> JResult<()> {
        let hour_or_lower = [Field::HourOfDay, Field::MinuteOfHour, Field::SecondOfMinute, Field::NanoOfSecond];
        if let Some(am_pm) = self.get(Field::AmPmOfDay) {
            if self.time.is_none() && !hour_or_lower.iter().any(|&field| self.contains(field)) {
                Field::AmPmOfDay.check_valid(am_pm)?;
                self.resolve_time(am_pm * 12 + 6, 0, 0, 0, style)?;
            }
        }
        Ok(())
    }

    fn resolve_time(&mut self, hour: i64, minute: i64, second: i64, nano: i64, style: ResolverStyle) -> JResult<()> {
        let minute_value = Field::MinuteOfHour.check_valid_int(minute)?;
        let nano_value = Field::NanoOfSecond.check_valid_int(nano)?;
        let end_of_day =
            style == ResolverStyle::Smart && hour == 24 && minute_value == 0 && second == 0 && nano_value == 0;
        let time = if end_of_day {
            LocalTime::MIDNIGHT
        } else {
            let hour = Field::HourOfDay.check_valid_int(hour)?;
            let second = Field::SecondOfMinute.check_valid_int(second)?;
            LocalTime::of(hour, minute_value, second, nano_value)?
        };
        if let Some(current) = self.time {
            if current != time {
                return Err(date_time_error(&format!(
                    "Conflict found: Fields resolved to different times: {current} {time}"
                )));
            }
        }
        self.time = Some(time);
        self.excess_day = end_of_day;
        self.remove(Field::HourOfDay);
        self.remove(Field::MinuteOfHour);
        self.remove(Field::SecondOfMinute);
        self.remove(Field::NanoOfSecond);
        Ok(())
    }

    /// Fields a resolved date or time can answer must agree with it and are then dropped.
    fn cross_check(&mut self) -> JResult<()> {
        if let Some(date) = self.date {
            self.cross_check_against(&date, |field| field.of_date(&date))?;
        }
        if let Some(time) = self.time {
            self.cross_check_against(&time, |field| field.of_time(&time).map(Ok))?;
        }
        Ok(())
    }

    fn cross_check_against<T: fmt::Display>(
        &mut self,
        source: &T,
        value_of: impl Fn(Field) -> Option<JResult<i64>>,
    ) -> JResult<()> {
        let mut kept = Vec::with_capacity(self.fields.len());
        for &(field, value) in &self.fields {
            match value_of(field) {
                Some(Ok(derived)) if derived != value => {
                    return Err(date_time_error(&format!(
                        "Conflict found: Field {field} {derived} differs from {field} {value} \
                         derived from {source}"
                    )))
                }
                Some(Ok(_)) => {}
                // A field the temporal cannot report has nothing to conflict with, so it stays to be resolved later.
                Some(Err(_)) | None => kept.push((field, value)),
            }
        }
        self.fields = kept;
        Ok(())
    }

    /// `resolveFractional`: seconds without a time get their sub-second fields.
    fn resolve_fractional(&mut self) {
        let has_seconds = [Field::InstantSeconds, Field::SecondOfDay, Field::SecondOfMinute]
            .iter()
            .any(|&field| self.contains(field));
        if self.time.is_some() || !has_seconds {
            return;
        }
        let nano = self.get(Field::NanoOfSecond);
        let nanos = nano.unwrap_or(0);
        self.insert(Field::NanoOfSecond, nanos);
        self.insert(Field::MicroOfSecond, nanos / 1000);
        self.insert(Field::MilliOfSecond, nanos / NANOS_PER_MILLI);
    }

    /// The state `Instant.parse` leaves after its own field handling.
    pub(super) fn of_instant_fields(seconds: i64, nano: i64) -> Parsed {
        let mut parsed = Parsed::default();
        parsed.insert(Field::InstantSeconds, seconds);
        parsed.insert(Field::NanoOfSecond, nano);
        parsed.resolve_fractional();
        parsed
    }

    /// The failure of `Instant.from` on this state.
    pub(super) fn instant_error(&self) -> Thrown {
        date_time_error(&format!(
            "Unable to obtain Instant from TemporalAccessor: {self} of type \
             java.time.format.Parsed"
        ))
    }

    /// Derives the instant from date, time and offset, the offset taking priority over the zone.
    fn resolve_instant(&mut self) -> JResult<()> {
        let (Some(date), Some(time)) = (self.date, self.time) else {
            return Ok(());
        };
        let local = LocalDateTime::new(date, time);
        let zone = match self.get(Field::OffsetSeconds) {
            Some(total) => ZoneId::of_total_seconds(total as i32)?,
            None => match &self.zone {
                Some(zone) => zone.clone(),
                None => return Ok(()),
            },
        };
        let seconds = local.at_zone(&zone)?.epoch_second();
        self.insert(Field::InstantSeconds, seconds);
        Ok(())
    }
}

fn resolve_week_of_month(
    field: Field,
    def: WeekDef,
    year: i32,
    month: i64,
    week: i32,
    localized_dow: i32,
    style: ResolverStyle,
) -> JResult<LocalDate> {
    let month_value = Field::MonthOfYear.check_valid_int(month)?;
    let first = LocalDate::of(year, month_value, 1)?;
    let week = field.check_valid_int(i64::from(week))?;
    let weeks = week - def.week_of_month(&first);
    let days = weeks * 7 + (localized_dow - def.localized_day_of_week(&first));
    let date = first.plus_days(i64::from(days))?;
    if style == ResolverStyle::Strict && date.month() != month_value {
        return Err(date_time_error("Strict mode rejected resolved date as it is in a different month"));
    }
    Ok(date)
}

impl Resolved {
    /// The query `TemporalQueries.zone()`: whether a zone or an offset was parsed. An offset field
    /// out of the valid range fails as `ZoneOffset.ofTotalSeconds` does.
    pub(super) fn has_zone(&self) -> JResult<bool> {
        if self.0.zone.is_some() {
            return Ok(true);
        }
        match self.0.get(Field::OffsetSeconds) {
            Some(total) => ZoneId::of_total_seconds(total as i32).map(|_| true),
            None => Ok(false),
        }
    }

    /// The query `TemporalQueries.localDate()`.
    pub(super) fn date(&self) -> Option<LocalDate> {
        self.0.date
    }

    /// The query `TemporalQueries.localTime()`.
    pub(super) fn time(&self) -> Option<LocalTime> {
        self.0.time
    }

    /// `Instant.from(parsed)`.
    pub(super) fn to_instant(&self) -> JResult<Instant> {
        let (Some(seconds), Some(time)) = (self.0.get(Field::InstantSeconds), self.0.time) else {
            return Err(self.0.instant_error());
        };
        Instant::of_epoch_second(seconds, i64::from(time.nano()))
    }
}

impl fmt::Display for Parsed {
    /// `Parsed.toString`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parsed = self;
        f.write_str("{")?;
        for (index, (field, value)) in parsed.fields.iter().enumerate() {
            let separator = if index == 0 { "" } else { ", " };
            write!(f, "{separator}{field}={value}")?;
        }
        f.write_str("},ISO")?;
        if let Some(zone) = &parsed.zone {
            write!(f, ",{zone},-1")?;
        }
        match (parsed.date, parsed.time) {
            (Some(date), Some(time)) => write!(f, " resolved to {date}T{time}"),
            (Some(date), None) => write!(f, " resolved to {date}"),
            (None, Some(time)) => write!(f, " resolved to {time}"),
            (None, None) => Ok(()),
        }
    }
}
