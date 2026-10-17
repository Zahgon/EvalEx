//! The `%t` date-time conversions for `ZonedDateTime`.

use crate::java::locale::{Locale, TextField, TextStyle};
use crate::java::time::zone::offset_id;
use crate::java::time::{LocalDateTime, ZonedDateTime};

/// Appends the text of a validated date-time suffix.
pub(super) fn write(out: &mut String, value: &ZonedDateTime, suffix: char, locale: &Locale) {
    let year = i64::from(value.year());
    let year_of_era = if year > 0 { year } else { 1 - year };
    match suffix {
        'H' => pad(out, value.hour(), 2),
        'I' => pad(out, clock_hour(value), 2),
        'k' => pad(out, value.hour(), 1),
        'l' => pad(out, clock_hour(value), 1),
        'M' => pad(out, value.minute(), 2),
        'S' => pad(out, value.second(), 2),
        'L' => pad(out, value.nano() / 1_000_000, 3),
        'N' => pad(out, value.nano(), 9),
        'p' => out.push_str(&am_pm(value, locale).to_lowercase()),
        'z' => write_offset(out, value.offset_seconds()),
        'Z' => out.push_str(&zone_name(value, locale)),
        's' => pad(out, value.epoch_second(), 1),
        'Q' => {
            let milli = i64::from(value.nano() / 1_000_000);
            pad(out, value.epoch_second().wrapping_mul(1000).wrapping_add(milli), 1);
        }
        'B' => out.push_str(month(value, locale, TextStyle::Full)),
        'b' | 'h' => out.push_str(month(value, locale, TextStyle::Short)),
        'A' => out.push_str(weekday(value, locale, TextStyle::Full)),
        'a' => out.push_str(weekday(value, locale, TextStyle::Short)),
        'C' => pad(out, year_of_era / 100, 2),
        'Y' => pad(out, year_of_era, 4),
        'y' => pad(out, year_of_era % 100, 2),
        'j' => pad(out, value.day_of_year(), 3),
        'm' => pad(out, value.month(), 2),
        'd' => pad(out, value.day(), 2),
        'e' => pad(out, value.day(), 1),
        'R' => write_joined(out, value, locale, "H:M"),
        'T' => write_joined(out, value, locale, "H:M:S"),
        'r' => {
            write_joined(out, value, locale, "I:M:S");
            out.push(' ');
            out.push_str(am_pm(value, locale));
        }
        'D' => write_joined(out, value, locale, "m/d/y"),
        'F' => write_joined(out, value, locale, "Y-m-d"),
        _ => write_joined(out, value, locale, "a b d T Z Y"),
    }
}

/// Writes a composite: letters are suffixes, everything else is copied.
fn write_joined(out: &mut String, value: &ZonedDateTime, locale: &Locale, layout: &str) {
    for symbol in layout.chars() {
        if symbol.is_ascii_alphabetic() {
            write(out, value, symbol, locale);
        } else {
            out.push(symbol);
        }
    }
}

fn pad(out: &mut String, value: impl Into<i64>, width: usize) {
    out.push_str(&format!("{:0width$}", value.into()));
}

fn clock_hour(value: &ZonedDateTime) -> i32 {
    match value.hour() % 12 {
        0 => 12,
        hour => hour,
    }
}

fn indexed(texts: &'static [&'static str], index: i32) -> &'static str {
    usize::try_from(index).ok().and_then(|index| texts.get(index)).copied().unwrap_or_default()
}

fn month(value: &ZonedDateTime, locale: &Locale, style: TextStyle) -> &'static str {
    indexed(locale.texts(TextField::Month, style), value.month() - 1)
}

fn weekday(value: &ZonedDateTime, locale: &Locale, style: TextStyle) -> &'static str {
    indexed(locale.texts(TextField::DayOfWeek, style), value.day_of_week() - 1)
}

fn am_pm(value: &ZonedDateTime, locale: &Locale) -> &'static str {
    let texts = locale.texts(TextField::AmPm, TextStyle::Full);
    indexed(texts, i32::from(value.hour() >= 12))
}

fn write_offset(out: &mut String, offset_seconds: i32) {
    let minutes = offset_seconds.abs() / 60;
    out.push(if offset_seconds < 0 { '-' } else { '+' });
    pad(out, minutes / 60 * 100 + minutes % 60, 4);
}

/// `%tZ`. Offsets print their id. Regions go through `TimeZone`, whose short names are only
/// reproduced for the fixed ids: an id `TimeZone` cannot read falls back to `GMT`, and the other
/// regions, whose names are out of contract, print their id.
fn zone_name(value: &ZonedDateTime, locale: &Locale) -> String {
    let zone = value.zone();
    let id = zone.to_string();
    if zone.is_offset() {
        return id;
    }
    match id.as_str() {
        "Etc/UTC" => "UTC".to_string(),
        "Etc/GMT" => locale.gmt_text().to_string(),
        "UT" => "GMT".to_string(),
        _ if id.starts_with("UTC+") || id.starts_with("UTC-") => "GMT".to_string(),
        _ if id.starts_with("UT+") || id.starts_with("UT-") => "GMT".to_string(),
        _ if id.starts_with("GMT") && id.len() > 9 => "GMT".to_string(),
        _ => id,
    }
}

/// `ZonedDateTime.toString`.
pub(super) fn to_iso_string(value: &ZonedDateTime) -> String {
    let local = LocalDateTime::new(value.date(), value.time());
    let offset = offset_id(value.offset_seconds());
    let zone = value.zone();
    if zone.is_offset() {
        format!("{local}{offset}")
    } else {
        format!("{local}{offset}[{zone}]")
    }
}

/// `ZonedDateTime.hashCode`.
pub(super) fn hash_code(value: &ZonedDateTime) -> i32 {
    let date = value.date();
    let (year, month, day) = (date.year(), date.month(), date.day());
    let date_hash = (year & !0x7FF) ^ year.wrapping_shl(11).wrapping_add((month << 6) + day);
    let time = value.time();
    let nano_of_day = i64::from(time.second_of_day()) * 1_000_000_000 + i64::from(time.nano());
    let time_hash = (nano_of_day ^ (nano_of_day >> 32)) as i32;
    let zone_id_hash = crate::java::chars::hash_code;
    let zone = value.zone();
    let zone_hash = if zone.is_offset() { value.offset_seconds() } else { zone_id_hash(zone.id()) };
    date_hash ^ time_hash ^ value.offset_seconds() ^ zone_hash.rotate_left(3)
}
