//! `java.util.Locale`, reduced to the data `java.time.format` reads from it.
//!
//! Real CLDR tables exist for the languages en, de, fr and pt; every other language uses the
//! root table, which is what the JDK shows for unknown locales. Week rules follow the region.

mod data;

use crate::java::time::week::WeekDef;

/// The kinds of `java.time.format.TextStyle`, in the order of the generated tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextStyle {
    Full,
    FullStandalone,
    Short,
    ShortStandalone,
    Narrow,
    NarrowStandalone,
}

/// The temporal fields that have locale-specific text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextField {
    Era,
    Month,
    DayOfWeek,
    AmPm,
    Quarter,
}

type StyleTable = [&'static [&'static str]; 6];

/// The generated text tables of one language.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LocaleData {
    era: StyleTable,
    month: StyleTable,
    day_of_week: StyleTable,
    am_pm: StyleTable,
    quarter: StyleTable,
    gmt: &'static str,
}

/// A locale: the language tables and the week rules of the region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locale {
    data: &'static LocaleData,
    week: WeekDef,
}

impl Locale {
    /// `Locale.US`.
    pub const US: Locale = Locale { data: &data::EN, week: WeekDef::SUNDAY_1 };

    /// `Locale.forLanguageTag`: ill-formed tags give the root locale.
    pub fn for_language_tag(tag: &[u16]) -> Locale {
        let text = String::from_utf16_lossy(tag);
        let mut subtags = text.split('-');
        let language = subtags.next().unwrap_or("");
        if !is_alpha(language, 2, 8) {
            return Locale::of_parts("", "");
        }
        let region = subtags
            .take_while(|part| part.len() >= 2)
            .find(|part| is_alpha(part, 2, 2) || (part.len() == 3 && is_digits(part)))
            .unwrap_or("");
        Locale::of_parts(language, region)
    }

    fn of_parts(language: &str, region: &str) -> Locale {
        let data = match language.to_ascii_lowercase().as_str() {
            "en" => &data::EN,
            "de" => &data::DE,
            "fr" => &data::FR,
            "pt" => &data::PT,
            _ => &data::ROOT,
        };
        let week = match region.to_ascii_uppercase().as_str() {
            "DE" | "FR" => WeekDef::MONDAY_4,
            _ => WeekDef::SUNDAY_1,
        };
        Locale { data, week }
    }

    /// The week rules, as `WeekFields.of(locale)`.
    pub(crate) fn week_def(&self) -> WeekDef {
        self.week
    }

    /// The texts of a field in a style, in value order; empty when the JDK has none.
    pub(crate) fn texts(&self, field: TextField, style: TextStyle) -> &'static [&'static str] {
        let table = match field {
            TextField::Era => &self.data.era,
            TextField::Month => &self.data.month,
            TextField::DayOfWeek => &self.data.day_of_week,
            TextField::AmPm => &self.data.am_pm,
            TextField::Quarter => &self.data.quarter,
        };
        table[style as usize]
    }

    /// The prefix of localized offsets, `GMT` in most locales.
    pub(crate) fn gmt_text(&self) -> &'static str {
        self.data.gmt
    }
}

fn is_alpha(part: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphabetic())
}

fn is_digits(part: &str) -> bool {
    part.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::js;

    #[test]
    fn known_tags_select_language_and_week_rules() {
        let german = Locale::for_language_tag(&js("de-DE"));
        assert_eq!(german.texts(TextField::Era, TextStyle::Short)[1], "n. Chr.");
        assert_eq!(german.week_def(), WeekDef::MONDAY_4);
        let bare = Locale::for_language_tag(&js("DE"));
        assert_eq!(bare.week_def(), WeekDef::SUNDAY_1);
        assert_eq!(Locale::for_language_tag(&js("fr-FR")).gmt_text(), "UTC");
    }

    #[test]
    fn unknown_and_malformed_tags_use_root() {
        let root = Locale::for_language_tag(&js("en_US"));
        assert_eq!(root.texts(TextField::Era, TextStyle::Full)[1], "CE");
        assert_eq!(Locale::for_language_tag(&js("xx")), root);
    }
}
