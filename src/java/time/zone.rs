//! `ZoneOffset`, `ZoneId` and `ZoneRules`, the latter backed by the generated [`super::tzdata`].

use std::cmp::Ordering;
use std::fmt;

use super::date_time_error;
use super::local::{LocalDate, LocalDateTime, SECONDS_PER_DAY};
use super::tzdata::{TIME_ZONE_IDS, ZONES, ZONE_IDS};
use crate::java::{js, JResult, JString, Thrown};

const SECONDS_PER_HOUR: i32 = 3600;
const SECONDS_PER_MINUTE: i32 = 60;
const MAX_OFFSET_SECONDS: i32 = 18 * SECONDS_PER_HOUR;
const ZONE_RULES_EXCEPTION: &str = "java.time.zone.ZoneRulesException";

/// How a [`TransitionRule`]'s wall-clock time is to be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TimeDefinition {
    Utc,
    Standard,
    Wall,
}

/// A `ZoneOffsetTransitionRule`: the recurring transition of one year.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct TransitionRule {
    pub(super) month: i32,
    /// Day of month; negative counts from the end of the month, -1 being the last day.
    pub(super) day_indicator: i32,
    /// ISO day of week to adjust to, 0 for none.
    pub(super) day_of_week: i32,
    /// Local second of the day; 86400 means midnight at the end of the day.
    pub(super) second_of_day: i64,
    pub(super) definition: TimeDefinition,
    pub(super) standard_offset: i32,
    pub(super) offset_before: i32,
    pub(super) offset_after: i32,
}

/// The generated rules of one region.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ZoneData {
    pub(super) id: &'static str,
    /// The offset before the first transition.
    pub(super) initial_offset: i32,
    /// Instant of each transition with the offset after it, ascending.
    pub(super) transitions: &'static [(i64, i32)],
    /// Transitions recurring each year after the last listed one.
    pub(super) rules: &'static [TransitionRule],
}

/// A single offset change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Transition {
    epoch_second: i64,
    before: i32,
    after: i32,
}

impl Transition {
    /// The local second, as UTC, where the invalid or doubled range of this transition starts.
    fn local_low(&self) -> i64 {
        self.epoch_second + i64::from(self.before.min(self.after))
    }

    /// The local second where that range ends.
    fn local_high(&self) -> i64 {
        self.epoch_second + i64::from(self.before.max(self.after))
    }

    /// How the transition treats a local time.
    fn classify(&self, local: LocalInstant) -> LocalOffsets {
        if local < (self.local_low(), 0) {
            LocalOffsets::Single(self.before)
        } else if local >= (self.local_high(), 0) {
            LocalOffsets::Single(self.after)
        } else {
            self.as_range()
        }
    }

    fn as_range(&self) -> LocalOffsets {
        if self.after > self.before {
            LocalOffsets::Gap(*self)
        } else {
            LocalOffsets::Overlap(*self)
        }
    }

    /// Length of a gap in seconds.
    pub(super) fn gap_seconds(&self) -> i64 {
        i64::from(self.after - self.before)
    }

    /// The offset after the transition.
    pub(super) fn offset_after(&self) -> i32 {
        self.after
    }

    /// The offset before the transition.
    pub(super) fn offset_before(&self) -> i32 {
        self.before
    }
}

/// The offsets a local date-time can have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LocalOffsets {
    Single(i32),
    Gap(Transition),
    Overlap(Transition),
}

/// A local date-time as UTC seconds and nanos, for comparison with transitions.
type LocalInstant = (i64, i32);

impl TransitionRule {
    fn transition_in(&self, year: i32) -> JResult<Transition> {
        let date = if self.day_indicator < 0 {
            let last = LocalDate::month_end_day(year, self.month);
            let date = LocalDate::of(year, self.month, last + 1 + self.day_indicator)?;
            if self.day_of_week == 0 {
                date
            } else {
                date.previous_or_same(self.day_of_week)?
            }
        } else {
            let date = LocalDate::of(year, self.month, self.day_indicator)?;
            if self.day_of_week == 0 {
                date
            } else {
                date.next_or_same(self.day_of_week)?
            }
        };
        let reference = match self.definition {
            TimeDefinition::Utc => 0,
            TimeDefinition::Standard => self.standard_offset,
            TimeDefinition::Wall => self.offset_before,
        };
        let local = date.to_epoch_day() * SECONDS_PER_DAY + self.second_of_day;
        Ok(Transition {
            epoch_second: local - i64::from(reference),
            before: self.offset_before,
            after: self.offset_after,
        })
    }
}

impl ZoneData {
    fn offset_before_transition(&self, index: usize) -> i32 {
        match index.checked_sub(1).and_then(|i| self.transitions.get(i)) {
            Some(&(_, offset)) => offset,
            None => self.initial_offset,
        }
    }

    fn transition(&self, index: usize) -> Option<Transition> {
        let &(epoch_second, after) = self.transitions.get(index)?;
        Some(Transition { epoch_second, before: self.offset_before_transition(index), after })
    }

    fn year_transitions(&self, year: i32) -> JResult<Vec<Transition>> {
        self.rules.iter().map(|rule| rule.transition_in(year)).collect()
    }

    fn offset_at(&self, epoch_second: i64) -> JResult<i32> {
        let last_listed = self.transitions.last();
        if let Some(&(last_epoch, last_offset)) = last_listed {
            if !self.rules.is_empty() && epoch_second > last_epoch {
                let local = epoch_second.saturating_add(i64::from(last_offset));
                let year = LocalDate::of_epoch_day(local.div_euclid(SECONDS_PER_DAY))?.year();
                let transitions = self.year_transitions(year)?;
                for transition in &transitions {
                    if epoch_second < transition.epoch_second {
                        return Ok(transition.before);
                    }
                }
                let last = transitions.last().map(|t| t.after);
                return Ok(last.unwrap_or(last_offset));
            }
        }
        let count = self.transitions.partition_point(|&(t, _)| t <= epoch_second);
        Ok(self.offset_before_transition(count))
    }

    /// Bound number `index` of the sorted low and high local bounds of all transitions.
    fn local_bound(&self, index: usize) -> Option<i64> {
        let transition = self.transition(index / 2)?;
        Some(if index.is_multiple_of(2) { transition.local_low() } else { transition.local_high() })
    }

    fn offsets_at_local(&self, dt: &LocalDateTime) -> JResult<LocalOffsets> {
        let local = (dt.local_epoch_second(), dt.time().nano());
        let bounds = self.transitions.len() * 2;
        let after_listed = bounds > 0 && self.local_bound(bounds - 1).is_some_and(|last| local > (last, 0));
        if !self.rules.is_empty() && after_listed {
            return self.offsets_by_rule(dt.date().year(), local);
        }
        let below = partition_bounds(bounds, |index| self.local_bound(index).is_some_and(|bound| (bound, 0) <= local));
        let Some(index) = below.checked_sub(1) else {
            return Ok(LocalOffsets::Single(self.initial_offset));
        };
        let number = index / 2;
        if index % 2 == 1 {
            let (_, after) = self.transitions.get(number).copied().unwrap_or((0, 0));
            return Ok(LocalOffsets::Single(after));
        }
        Ok(self.transition(number).map_or(LocalOffsets::Single(self.initial_offset), |t| t.as_range()))
    }

    fn offsets_by_rule(&self, year: i32, local: LocalInstant) -> JResult<LocalOffsets> {
        let mut result = LocalOffsets::Single(self.initial_offset);
        for transition in self.year_transitions(year)? {
            result = transition.classify(local);
            if result != LocalOffsets::Single(transition.after) {
                break;
            }
        }
        Ok(result)
    }
}

/// Number of leading indices in `0..count` for which the monotonic predicate holds.
fn partition_bounds(count: usize, below: impl Fn(usize) -> bool) -> usize {
    let (mut low, mut high) = (0, count);
    while low < high {
        let middle = low + (high - low) / 2;
        if below(middle) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low
}

/// The rules of a [`ZoneId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rules {
    Fixed(i32),
    Region(&'static ZoneData),
}

/// A `java.time.ZoneId`: an offset, a prefixed offset or a region.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZoneId {
    id: JString,
    rules: Rules,
    is_offset: bool,
}

impl ZoneId {
    /// `ZoneId.of(zoneId)`.
    pub fn of(id: &[u16]) -> JResult<ZoneId> {
        let first = id.first().copied();
        if id.len() <= 1 || first == Some(u16::from(b'+')) || first == Some(u16::from(b'-')) {
            return ZoneId::of_offset_id(id);
        }
        if starts_with(id, "UTC") || starts_with(id, "GMT") {
            ZoneId::of_with_prefix(id, 3)
        } else if starts_with(id, "UT") {
            ZoneId::of_with_prefix(id, 2)
        } else {
            ZoneId::of_region(id)
        }
    }

    /// `ZoneOffset.UTC`.
    pub fn utc() -> ZoneId {
        ZoneId::of_total_seconds(0).unwrap_or_else(|_| ZoneId::fixed(js("Z"), 0, true))
    }

    /// `ZoneOffset.ofTotalSeconds`.
    pub(crate) fn of_total_seconds(total: i32) -> JResult<ZoneId> {
        if !(-MAX_OFFSET_SECONDS..=MAX_OFFSET_SECONDS).contains(&total) {
            return Err(date_time_error("Zone offset not in valid range: -18:00 to +18:00"));
        }
        Ok(ZoneId::fixed(js(&offset_id(total)), total, true))
    }

    /// `ZoneOffset.of(offsetId)`.
    pub(crate) fn of_offset_id(id: &[u16]) -> JResult<ZoneId> {
        ZoneId::of_total_seconds(parse_offset_seconds(id)?)
    }

    /// `ZoneId.ofOffset(prefix, offset)`, for a prefix of UTC, GMT or UT.
    pub(crate) fn of_prefixed(prefix: &[u16], total_seconds: i32) -> ZoneId {
        let mut id = prefix.to_vec();
        if total_seconds != 0 {
            id.extend(js(&offset_id(total_seconds)));
        }
        ZoneId::fixed(id, total_seconds, false)
    }

    fn fixed(id: JString, total_seconds: i32, is_offset: bool) -> ZoneId {
        ZoneId { id, rules: Rules::Fixed(total_seconds), is_offset }
    }

    fn of_with_prefix(id: &[u16], prefix_length: usize) -> JResult<ZoneId> {
        let (prefix, rest) = id.split_at(prefix_length.min(id.len()));
        let Some(&sign) = rest.first() else {
            return Ok(ZoneId::of_prefixed(prefix, 0));
        };
        if sign != u16::from(b'+') && sign != u16::from(b'-') {
            return ZoneId::of_region(id);
        }
        match parse_offset_seconds(rest).and_then(ZoneId::of_total_seconds) {
            Ok(offset) => Ok(ZoneId::of_prefixed(prefix, offset.fixed_offset_or_zero())),
            Err(_) => {
                let mut message = js("Invalid ID for offset-based ZoneId: ");
                message.extend_from_slice(id);
                Err(Thrown::with_message(super::DATE_TIME_EXCEPTION, message))
            }
        }
    }

    fn of_region(id: &[u16]) -> JResult<ZoneId> {
        if !is_region_name(id) {
            let mut message = js("Invalid ID for region-based ZoneId, invalid format: ");
            message.extend_from_slice(id);
            return Err(Thrown::with_message(super::DATE_TIME_EXCEPTION, message));
        }
        let text = String::from_utf16_lossy(id);
        if ZONE_IDS.binary_search(&text.as_str()).is_err() {
            let mut message = js("Unknown time-zone ID: ");
            message.extend_from_slice(id);
            return Err(Thrown::with_message(ZONE_RULES_EXCEPTION, message));
        }
        let rules = ZONES
            .binary_search_by(|zone| zone.id.cmp(text.as_str()))
            .ok()
            .and_then(|index| ZONES.get(index))
            .map_or(Rules::Fixed(0), Rules::Region);
        Ok(ZoneId { id: id.to_vec(), rules, is_offset: false })
    }

    /// `ZoneId.getId()`.
    pub fn id(&self) -> &[u16] {
        &self.id
    }

    /// Whether this is a `ZoneOffset` rather than a `ZoneRegion`.
    pub(crate) fn is_offset(&self) -> bool {
        self.is_offset
    }

    fn fixed_offset_or_zero(&self) -> i32 {
        match self.rules {
            Rules::Fixed(total) => total,
            Rules::Region(_) => 0,
        }
    }

    /// `ZoneRules.getOffset(Instant)`.
    pub(crate) fn offset_at(&self, epoch_second: i64) -> JResult<i32> {
        match self.rules {
            Rules::Fixed(total) => Ok(total),
            Rules::Region(zone) => zone.offset_at(epoch_second),
        }
    }

    /// The offsets valid at a local date-time, from `ZoneRules.getValidOffsets`.
    pub(super) fn offsets_at_local(&self, dt: &LocalDateTime) -> JResult<LocalOffsets> {
        match self.rules {
            Rules::Fixed(total) => Ok(LocalOffsets::Single(total)),
            Rules::Region(zone) => zone.offsets_at_local(dt),
        }
    }
}

impl fmt::Display for ZoneId {
    /// `ZoneId.toString`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf16_lossy(&self.id))
    }
}

/// Whether `java.util.TimeZone.getAvailableIDs()` contains an id.
pub fn time_zone_ids_contains(id: &[u16]) -> bool {
    TIME_ZONE_IDS.binary_search_by(|known| compare_units(known, id)).is_ok()
}

/// All region ids, for the parser's longest-match lookup.
pub(crate) fn region_ids() -> &'static [&'static str] {
    &ZONE_IDS
}

fn compare_units(known: &str, id: &[u16]) -> Ordering {
    known.encode_utf16().cmp(id.iter().copied())
}

fn starts_with(id: &[u16], prefix: &str) -> bool {
    id.len() >= prefix.len() && prefix.encode_utf16().eq(id.iter().copied().take(prefix.len()))
}

/// `ZoneRegion.checkName`.
fn is_region_name(id: &[u16]) -> bool {
    let is_letter = |c: u16| u8::try_from(c).is_ok_and(|b| b.is_ascii_alphabetic());
    let is_rest = |c: u16| {
        u8::try_from(c).is_ok_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'~' | b'+' | b'-'))
    };
    id.len() >= 2 && id.iter().copied().take(1).all(is_letter) && id.iter().copied().skip(1).all(is_rest)
}

/// `ZoneOffset.getId()`.
pub(crate) fn offset_id(total: i32) -> String {
    if total == 0 {
        return "Z".to_string();
    }
    let abs = total.abs();
    let sign = if total < 0 { '-' } else { '+' };
    let hours = abs / SECONDS_PER_HOUR;
    let minutes = abs / SECONDS_PER_MINUTE % SECONDS_PER_MINUTE;
    let seconds = abs % SECONDS_PER_MINUTE;
    if seconds == 0 {
        format!("{sign}{hours:02}:{minutes:02}")
    } else {
        format!("{sign}{hours:02}:{minutes:02}:{seconds:02}")
    }
}

fn offset_format_error(detail: &str, id: &[u16]) -> Thrown {
    let mut message = js("Invalid ID for ZoneOffset, ");
    message.extend(js(detail));
    message.extend_from_slice(id);
    Thrown::with_message(super::DATE_TIME_EXCEPTION, message)
}

/// Two digits of `id` at `pos`; a colon is required first when `colon` is set.
fn offset_number(id: &[u16], pos: usize, colon: bool) -> JResult<i32> {
    if colon && id.get(pos - 1).copied() != Some(u16::from(b':')) {
        return Err(offset_format_error("colon not found when expected: ", id));
    }
    let digit = |index: usize| {
        id.get(index)
            .and_then(|&c| char::from_u32(u32::from(c)))
            .and_then(|c| c.to_digit(10))
            .filter(|_| id.get(index).is_some_and(|&c| c < 128))
    };
    match (digit(pos), digit(pos + 1)) {
        (Some(tens), Some(units)) => Ok((tens * 10 + units) as i32),
        _ => Err(offset_format_error("non numeric characters found: ", id)),
    }
}

/// `ZoneOffset.of`: the total seconds of an offset id.
fn parse_offset_seconds(id: &[u16]) -> JResult<i32> {
    if id == js("Z") {
        return Ok(0);
    }
    // `+h` and `+h:mm` style ids are normalized by zero-padding the hour.
    let normalized: JString = match id.len() {
        2 => id.first().copied().into_iter().chain([u16::from(b'0')]).chain(id.get(1).copied()).collect(),
        _ => id.to_vec(),
    };
    let id = normalized.as_slice();
    let (hours, minutes, seconds) = match id.len() {
        3 => (offset_number(id, 1, false)?, 0, 0),
        5 => (offset_number(id, 1, false)?, offset_number(id, 3, false)?, 0),
        6 => (offset_number(id, 1, false)?, offset_number(id, 4, true)?, 0),
        7 => (offset_number(id, 1, false)?, offset_number(id, 3, false)?, offset_number(id, 5, false)?),
        9 => (offset_number(id, 1, false)?, offset_number(id, 4, true)?, offset_number(id, 7, true)?),
        _ => return Err(offset_format_error("invalid format: ", id)),
    };
    let sign = match id.first().copied().and_then(|c| u8::try_from(c).ok()) {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => return Err(offset_format_error("plus/minus not found when expected: ", id)),
    };
    check_offset_parts(sign * hours, sign * minutes, sign * seconds)?;
    Ok(sign * (hours * SECONDS_PER_HOUR + minutes * SECONDS_PER_MINUTE + seconds))
}

/// `ZoneOffset.validate` for parts of equal sign.
fn check_offset_parts(hours: i32, minutes: i32, seconds: i32) -> JResult<()> {
    let out_of_range = |part: &str, value: i32, limit: i32| {
        date_time_error(&format!(
            "Zone offset {part} not in valid range: value {value} is not in the range \
             -{limit} to {limit}"
        ))
    };
    if hours.abs() > 18 {
        return Err(out_of_range("hours", hours, 18));
    }
    if minutes.abs() > 59 {
        return Err(out_of_range("minutes", minutes, 59));
    }
    if seconds.abs() > 59 {
        return Err(out_of_range("seconds", seconds, 59));
    }
    if hours.abs() == 18 && (minutes != 0 || seconds != 0) {
        return Err(date_time_error("Zone offset not in valid range: -18:00 to +18:00"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(id: &str) -> (String, String) {
        let error = ZoneId::of(&js(id)).unwrap_err();
        let message = String::from_utf16_lossy(&error.message.unwrap_or_default());
        (error.class, message)
    }

    #[test]
    fn offset_ids_normalize() {
        assert_eq!(ZoneId::of(&js("+1")).unwrap().to_string(), "+01:00");
        assert_eq!(ZoneId::of(&js("-00:00")).unwrap().to_string(), "Z");
        assert_eq!(ZoneId::of(&js("+010203")).unwrap().to_string(), "+01:02:03");
    }

    #[test]
    fn prefixed_ids_are_regions() {
        let zone = ZoneId::of(&js("UTC+1")).unwrap();
        assert_eq!(zone.to_string(), "UTC+01:00");
        assert!(!zone.is_offset());
        assert_eq!(zone.offset_at(0), Ok(3600));
        assert_eq!(ZoneId::of(&js("GMT+0")).unwrap().to_string(), "GMT");
    }

    #[test]
    fn invalid_ids_report_java_messages() {
        assert_eq!(
            failure("Europe/Nowhere"),
            ("java.time.zone.ZoneRulesException".to_string(), "Unknown time-zone ID: Europe/Nowhere".to_string())
        );
        assert_eq!(failure("1a").1, "Invalid ID for region-based ZoneId, invalid format: 1a");
        assert_eq!(failure("+01:").1, "Invalid ID for ZoneOffset, invalid format: +01:");
        assert_eq!(failure("UTC+25").1, "Invalid ID for offset-based ZoneId: UTC+25");
        assert_eq!(failure("+19").1, "Zone offset hours not in valid range: value 19 is not in the range -18 to 18");
    }

    #[test]
    fn region_offsets_follow_transitions_and_rules() {
        let paris = ZoneId::of(&js("Europe/Paris")).unwrap();
        assert_eq!(paris.offset_at(1_711_846_799), Ok(3600));
        assert_eq!(paris.offset_at(1_711_846_800), Ok(7200));
        assert_eq!(paris.offset_at(32_503_680_000), Ok(3600));
    }

    #[test]
    fn local_lookup_detects_gaps_and_overlaps() {
        let paris = ZoneId::of(&js("Europe/Paris")).unwrap();
        let gap = LocalDateTime::of(2024, 3, 31, 2, 30, 0, 0).unwrap();
        assert!(matches!(paris.offsets_at_local(&gap), Ok(LocalOffsets::Gap(_))));
        let overlap = LocalDateTime::of(2100, 10, 31, 2, 30, 0, 0).unwrap();
        assert!(matches!(paris.offsets_at_local(&overlap), Ok(LocalOffsets::Overlap(_))));
    }

    #[test]
    fn time_zone_id_list_is_exact() {
        assert!(time_zone_ids_contains(&js("America/New_York")));
        assert!(time_zone_ids_contains(&js("EST")));
        assert!(!time_zone_ids_contains(&js("Mars/Olympus")));
    }
}
