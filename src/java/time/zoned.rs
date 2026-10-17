//! `ZonedDateTime`.

use super::instant::Instant;
use super::local::{LocalDate, LocalDateTime, LocalTime};
use super::zone::{LocalOffsets, ZoneId};
use super::Unit;
use crate::java::JResult;

/// A date-time with its zone and the offset in force.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZonedDateTime {
    date_time: LocalDateTime,
    offset_seconds: i32,
    zone: ZoneId,
}

impl ZonedDateTime {
    /// `ZonedDateTime.ofLocal`: a gap moves the time forward, an overlap keeps the preferred offset
    /// when valid and otherwise takes the earlier one.
    pub(crate) fn of_local(
        date_time: LocalDateTime,
        zone: &ZoneId,
        preferred_offset: Option<i32>,
    ) -> JResult<ZonedDateTime> {
        let (date_time, offset_seconds) = match zone.offsets_at_local(&date_time)? {
            LocalOffsets::Single(offset) => (date_time, offset),
            LocalOffsets::Gap(transition) => {
                (date_time.plus_seconds(transition.gap_seconds())?, transition.offset_after())
            }
            LocalOffsets::Overlap(transition) => {
                let offset = preferred_offset
                    .filter(|&offset| offset == transition.offset_after())
                    .unwrap_or(transition.offset_before());
                (date_time, offset)
            }
        };
        Ok(ZonedDateTime { date_time, offset_seconds, zone: zone.clone() })
    }

    /// `ZonedDateTime.ofInstant`.
    pub(crate) fn of_instant(instant: &Instant, zone: &ZoneId) -> JResult<ZonedDateTime> {
        let offset_seconds = zone.offset_at(instant.seconds)?;
        let date_time = LocalDateTime::of_epoch_second(instant.seconds, instant.nanos, offset_seconds)?;
        Ok(ZonedDateTime { date_time, offset_seconds, zone: zone.clone() })
    }

    /// `ZonedDateTime.toInstant`.
    pub fn to_instant(&self) -> Instant {
        Instant { seconds: self.epoch_second(), nanos: self.nano() }
    }

    /// `ZonedDateTime.truncatedTo`, for the units up to days.
    pub fn truncated_to(&self, unit: Unit) -> JResult<ZonedDateTime> {
        self.resolve_local(self.date_time.truncated_to(unit))
    }

    /// `with(TemporalAdjusters.firstDayOfMonth())`.
    pub fn with_first_day_of_month(&self) -> JResult<ZonedDateTime> {
        self.resolve_local(self.date_time.with_first_day_of_month())
    }

    /// `with(TemporalAdjusters.firstDayOfYear())`.
    pub fn with_first_day_of_year(&self) -> JResult<ZonedDateTime> {
        self.resolve_local(self.date_time.with_first_day_of_year())
    }

    fn resolve_local(&self, date_time: LocalDateTime) -> JResult<ZonedDateTime> {
        ZonedDateTime::of_local(date_time, &self.zone, Some(self.offset_seconds))
    }

    /// The local date.
    pub fn date(&self) -> LocalDate {
        self.date_time.date()
    }

    /// The local time.
    pub fn time(&self) -> LocalTime {
        self.date_time.time()
    }

    /// The proleptic ISO year.
    pub fn year(&self) -> i32 {
        self.date().year()
    }

    /// The month, from 1 to 12.
    pub fn month(&self) -> i32 {
        self.date().month()
    }

    /// The day of the month.
    pub fn day(&self) -> i32 {
        self.date().day()
    }

    /// The hour of the day.
    pub fn hour(&self) -> i32 {
        self.time().hour()
    }

    /// The minute of the hour.
    pub fn minute(&self) -> i32 {
        self.time().minute()
    }

    /// The second of the minute.
    pub fn second(&self) -> i32 {
        self.time().second()
    }

    /// The nanosecond of the second.
    pub fn nano(&self) -> i32 {
        self.time().nano()
    }

    /// The offset from UTC in seconds.
    pub fn offset_seconds(&self) -> i32 {
        self.offset_seconds
    }

    /// The zone.
    pub fn zone(&self) -> &ZoneId {
        &self.zone
    }

    /// The ISO day of week, from 1 (Monday) to 7 (Sunday).
    pub fn day_of_week(&self) -> i32 {
        self.date().day_of_week()
    }

    /// The day of the year, from 1.
    pub fn day_of_year(&self) -> i32 {
        self.date().day_of_year()
    }

    /// `ZonedDateTime.toEpochSecond`.
    pub fn epoch_second(&self) -> i64 {
        self.date_time.local_epoch_second() - i64::from(self.offset_seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::js;

    fn paris() -> ZoneId {
        ZoneId::of(&js("Europe/Paris")).unwrap()
    }

    #[test]
    fn gap_moves_forward_by_its_length() {
        let local = LocalDateTime::of(2024, 3, 31, 2, 30, 0, 0).unwrap();
        let zoned = local.at_zone(&paris()).unwrap();
        assert_eq!(zoned.hour(), 3);
        assert_eq!(zoned.minute(), 30);
        assert_eq!(zoned.offset_seconds(), 7200);
    }

    #[test]
    fn overlap_defaults_to_the_earlier_offset() {
        let local = LocalDateTime::of(2024, 10, 27, 2, 30, 0, 0).unwrap();
        assert_eq!(local.at_zone(&paris()).unwrap().offset_seconds(), 7200);
    }

    #[test]
    fn truncation_keeps_the_later_offset_of_an_overlap() {
        let instant = Instant { seconds: 1_729_992_600, nanos: 5 };
        let later = ZonedDateTime::of_instant(&instant, &paris()).unwrap();
        assert_eq!(later.offset_seconds(), 3600);
        let truncated = later.truncated_to(Unit::Hours).unwrap();
        assert_eq!(truncated.hour(), 2);
        assert_eq!(truncated.minute(), 0);
        assert_eq!(truncated.offset_seconds(), 3600);
    }

    #[test]
    fn first_days_keep_the_time() {
        let local = LocalDateTime::of(2024, 7, 15, 12, 0, 0, 0).unwrap();
        let zoned = local.at_zone(&paris()).unwrap();
        let month = zoned.with_first_day_of_month().unwrap();
        assert_eq!((month.month(), month.day()), (7, 1));
        let year = zoned.with_first_day_of_year().unwrap();
        assert_eq!((year.month(), year.day()), (1, 1));
        assert_eq!(year.offset_seconds(), 3600);
    }
}
