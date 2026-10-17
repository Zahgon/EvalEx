//! The number symbols of `java.text.DecimalFormatSymbols`, for the locales EvalEx supports.
//!
//! Every table uses ASCII digits, `-` as minus sign and three-digit grouping, and has identical
//! plain and monetary separators, so only the differing symbols are kept here.

use crate::java::locale::{Locale, TextField, TextStyle};

/// The symbols of one locale that vary between the supported languages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NumberSymbols {
    /// `getDecimalSeparator`.
    pub(crate) decimal: u16,
    /// `getGroupingSeparator`.
    pub(crate) grouping: u16,
    /// `getCurrencySymbol`.
    pub(crate) currency: &'static str,
    /// `getInternationalCurrencySymbol`.
    pub(crate) international_currency: &'static str,
}

const DOT: u16 = 0x2E;
const COMMA: u16 = 0x2C;
const NARROW_NO_BREAK_SPACE: u16 = 0x202F;

impl NumberSymbols {
    /// The symbols of a locale.
    ///
    /// `Locale` keeps only its text tables and week rules, so the language is recognised by its
    /// name of January, and the currency is the one of the supported region of that language.
    pub(crate) fn of(locale: &Locale) -> NumberSymbols {
        let months = locale.texts(TextField::Month, TextStyle::Full);
        match months.first().copied() {
            Some("January") => NumberSymbols::with(DOT, COMMA, "$", "USD"),
            Some("Januar") => NumberSymbols::with(COMMA, DOT, "\u{20AC}", "EUR"),
            Some("janvier") => NumberSymbols::with(COMMA, NARROW_NO_BREAK_SPACE, "\u{20AC}", "EUR"),
            Some("janeiro") => NumberSymbols::with(COMMA, DOT, "R$", "BRL"),
            _ => NumberSymbols::with(DOT, COMMA, "\u{A4}", "XXX"),
        }
    }

    const fn with(
        decimal: u16,
        grouping: u16,
        currency: &'static str,
        international_currency: &'static str,
    ) -> NumberSymbols {
        NumberSymbols { decimal, grouping, currency, international_currency }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::js;

    #[test]
    fn languages_select_their_separators() {
        let french = NumberSymbols::of(&Locale::for_language_tag(&js("fr-FR")));
        assert_eq!((french.decimal, french.grouping), (0x2C, 0x202F));
        let german = NumberSymbols::of(&Locale::for_language_tag(&js("de-DE")));
        assert_eq!((german.decimal, german.grouping), (0x2C, 0x2E));
        assert_eq!(NumberSymbols::of(&Locale::US).currency, "$");
    }

    #[test]
    fn unknown_languages_use_the_root_table() {
        let root = NumberSymbols::of(&Locale::for_language_tag(&js("xx")));
        assert_eq!(root.international_currency, "XXX");
        assert_eq!(root.decimal, 0x2E);
    }
}
