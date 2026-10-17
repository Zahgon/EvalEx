//! Resolution of `\p{...}` names and the predefined classes that depend on `(?U)`.

use super::charclass::{self, CharClass};
use super::unicode::{self, category};

fn category_class(mask: u32) -> Option<CharClass> {
    Some(CharClass::category(mask))
}

fn test(test: fn(u32) -> bool) -> Option<CharClass> {
    Some(CharClass::test(test))
}

fn white_space(cp: u32) -> bool {
    matches!(cp, 0x09..=0x0D | 0x85) || unicode::is_space_char(cp)
}

fn punctuation(cp: u32) -> bool {
    unicode::has_category(cp, category::PUNCTUATION)
}

fn hex_digit(cp: u32) -> bool {
    unicode::is_digit(cp)
        || matches!(
            cp,
            0x30..=0x39
                | 0x41..=0x46
                | 0x61..=0x66
                | 0xFF10..=0xFF19
                | 0xFF21..=0xFF26
                | 0xFF41..=0xFF46
        )
}

fn join_control(cp: u32) -> bool {
    matches!(cp, 0x200C | 0x200D)
}

fn noncharacter(cp: u32) -> bool {
    cp & 0xFFFE == 0xFFFE || (0xFDD0..=0xFDEF).contains(&cp)
}

fn word(cp: u32) -> bool {
    unicode::is_alphabetic(cp)
        || join_control(cp)
        || unicode::has_category(cp, category::MARK | category::DECIMAL_DIGIT | category::CONNECTOR_PUNCTUATION)
}

fn alnum(cp: u32) -> bool {
    unicode::is_alphabetic(cp) || unicode::is_digit(cp)
}

fn blank(cp: u32) -> bool {
    cp == 0x09 || unicode::has_category(cp, category::SPACE_SEPARATOR)
}

fn graph(cp: u32) -> bool {
    !white_space(cp) && !unicode::has_category(cp, category::CONTROL | category::SURROGATE | category::UNASSIGNED)
}

fn print(cp: u32) -> bool {
    (graph(cp) || blank(cp)) && !unicode::has_category(cp, category::CONTROL)
}

fn cased(cp: u32) -> bool {
    unicode::is_lower(cp) || unicode::is_upper(cp) || unicode::is_title(cp)
}

fn emoji(cp: u32) -> bool {
    matches!(
        cp,
        0x23 | 0x2A | 0x30..=0x39 | 0xA9 | 0xAE | 0x203C | 0x2049 | 0x2122 | 0x2139
            | 0x2194..=0x21AA | 0x231A..=0x23FF | 0x25AA..=0x27BF | 0x1F000..=0x1FAFF
    )
}

fn pictographic(cp: u32) -> bool {
    matches!(
        cp,
        0xA9 | 0xAE | 0x203C | 0x2049 | 0x2122 | 0x2139 | 0x2194..=0x21AA | 0x231A..=0x23FF
            | 0x25AA..=0x27BF | 0x1F000..=0x1FAFF
    )
}

fn emoji_modifier(cp: u32) -> bool {
    matches!(cp, 0x1F3FB..=0x1F3FF)
}

/// `\d`.
pub(super) fn digit_class(unicode_classes: bool) -> CharClass {
    if unicode_classes {
        CharClass::test(unicode::is_digit)
    } else {
        CharClass::test(charclass::ascii_digit)
    }
}

/// `\s`.
pub(super) fn space_class(unicode_classes: bool) -> CharClass {
    if unicode_classes {
        CharClass::test(white_space)
    } else {
        CharClass::test(charclass::ascii_space)
    }
}

/// `\w`.
pub(super) fn word_class(unicode_classes: bool) -> CharClass {
    if unicode_classes {
        CharClass::test(word)
    } else {
        CharClass::test(charclass::ascii_word)
    }
}

/// `true` when `\b` counts `cp` as a word character.
pub(super) fn is_word(cp: u32, unicode_classes: bool) -> bool {
    if unicode_classes {
        word(cp)
    } else {
        charclass::ascii_word(cp)
    }
}

/// The general categories, POSIX classes and `java*` methods, by their case-sensitive names.
pub(super) fn general(name: &str, case_insensitive: bool) -> Option<CharClass> {
    let cased_or = |mask| category_class(if case_insensitive { category::CASED_LETTER } else { mask });
    let cased_or_test = |exact: fn(u32) -> bool| test(if case_insensitive { cased } else { exact });
    let alpha_or = |exact: fn(u32) -> bool| test(if case_insensitive { charclass::ascii_alpha } else { exact });
    match name {
        "Cn" => category_class(category::UNASSIGNED),
        "Lu" => cased_or(category::UPPERCASE),
        "Ll" => cased_or(category::LOWERCASE),
        "Lt" => cased_or(category::TITLECASE),
        "Lm" => category_class(category::MODIFIER_LETTER),
        "Lo" => category_class(category::OTHER_LETTER),
        "Mn" => category_class(category::NON_SPACING_MARK),
        "Me" => category_class(category::ENCLOSING_MARK),
        "Mc" => category_class(category::SPACING_MARK),
        "Nd" => category_class(category::DECIMAL_DIGIT),
        "Nl" => category_class(category::LETTER_NUMBER),
        "No" => category_class(category::OTHER_NUMBER),
        "Zs" => category_class(category::SPACE_SEPARATOR),
        "Zl" => category_class(category::LINE_SEPARATOR),
        "Zp" => category_class(category::PARAGRAPH_SEPARATOR),
        "Cc" => category_class(category::CONTROL),
        "Cf" => category_class(category::FORMAT),
        "Co" => category_class(category::PRIVATE_USE),
        "Cs" => category_class(category::SURROGATE),
        "Pd" => category_class(category::DASH_PUNCTUATION),
        "Ps" => category_class(category::START_PUNCTUATION),
        "Pe" => category_class(category::END_PUNCTUATION),
        "Pc" => category_class(category::CONNECTOR_PUNCTUATION),
        "Po" => category_class(category::OTHER_PUNCTUATION),
        "Sm" => category_class(category::MATH_SYMBOL),
        "Sc" => category_class(category::CURRENCY_SYMBOL),
        "Sk" => category_class(category::MODIFIER_SYMBOL),
        "So" => category_class(category::OTHER_SYMBOL),
        "Pi" => category_class(category::INITIAL_PUNCTUATION),
        "Pf" => category_class(category::FINAL_PUNCTUATION),
        "L" => category_class(category::LETTER),
        "M" => category_class(category::MARK),
        "N" => category_class(category::NUMBER),
        "Z" => category_class(category::SEPARATOR),
        "C" => category_class(category::OTHER),
        "P" => category_class(category::PUNCTUATION),
        "S" => category_class(category::SYMBOL),
        "LC" => category_class(category::CASED_LETTER),
        "LD" => category_class(category::LETTER | category::DECIMAL_DIGIT),
        "L1" => Some(CharClass::range(0, 0xFF)),
        "all" => Some(CharClass::all()),
        "ASCII" => Some(CharClass::range(0, 0x7F)),
        "Alnum" => test(charclass::ascii_alnum),
        "Alpha" => test(charclass::ascii_alpha),
        "Blank" => test(charclass::ascii_blank),
        "Cntrl" => test(charclass::ascii_control),
        "Digit" => test(charclass::ascii_digit),
        "Graph" => test(charclass::ascii_graph),
        "Lower" => alpha_or(charclass::ascii_lower),
        "Print" => Some(CharClass::range(0x20, 0x7E)),
        "Punct" => test(charclass::ascii_punct),
        "Space" => test(charclass::ascii_space),
        "Upper" => alpha_or(charclass::ascii_upper),
        "XDigit" => test(charclass::ascii_hex),
        "javaLowerCase" => cased_or_test(unicode::is_lower),
        "javaUpperCase" => cased_or_test(unicode::is_upper),
        "javaTitleCase" => cased_or_test(unicode::is_title),
        "javaAlphabetic" => test(unicode::is_alphabetic),
        "javaIdeographic" => test(unicode::is_ideographic),
        "javaDigit" => test(unicode::is_digit),
        "javaDefined" => test(unicode::is_defined),
        "javaLetter" => test(unicode::is_letter),
        "javaLetterOrDigit" => test(unicode::is_letter_or_digit),
        "javaJavaIdentifierStart" => test(unicode::is_java_identifier_start),
        "javaJavaIdentifierPart" => test(unicode::is_java_identifier_part),
        "javaUnicodeIdentifierStart" => test(unicode::is_unicode_identifier_start),
        "javaUnicodeIdentifierPart" => test(unicode::is_unicode_identifier_part),
        "javaIdentifierIgnorable" => test(unicode::is_identifier_ignorable),
        "javaSpaceChar" => test(unicode::is_space_char),
        "javaWhitespace" => test(unicode::is_whitespace),
        "javaISOControl" => test(unicode::is_iso_control),
        "javaMirrored" => test(unicode::is_mirrored),
        _ => None,
    }
}

/// The binary properties of `\p{IsName}`, by case-insensitive name.
pub(super) fn binary(name: &str, case_insensitive: bool) -> Option<CharClass> {
    let cased_or = |exact: fn(u32) -> bool| test(if case_insensitive { cased } else { exact });
    match name.to_uppercase().as_str() {
        "ALPHABETIC" => test(unicode::is_alphabetic),
        "ASSIGNED" => test(unicode::is_defined),
        "CONTROL" => category_class(category::CONTROL),
        "EMOJI" => test(emoji),
        "EMOJI_COMPONENT" => test(|cp| matches!(cp, 0x23 | 0x2A | 0x30..=0x39 | 0x200D | 0xFE0F)),
        "EMOJI_MODIFIER" => test(emoji_modifier),
        "EMOJI_MODIFIER_BASE" => test(pictographic),
        "EMOJI_PRESENTATION" => test(pictographic),
        "EXTENDED_PICTOGRAPHIC" => test(pictographic),
        "HEXDIGIT" | "HEX_DIGIT" => test(hex_digit),
        "IDEOGRAPHIC" => test(unicode::is_ideographic),
        "JOINCONTROL" | "JOIN_CONTROL" => test(join_control),
        "LETTER" => test(unicode::is_letter),
        "LOWERCASE" => cased_or(unicode::is_lower),
        "NONCHARACTERCODEPOINT" | "NONCHARACTER_CODE_POINT" => test(noncharacter),
        "PUNCTUATION" => test(punctuation),
        "TITLECASE" => cased_or(unicode::is_title),
        "UPPERCASE" => cased_or(unicode::is_upper),
        "WHITESPACE" | "WHITE_SPACE" => test(white_space),
        "WORD" => test(word),
        _ => posix_unicode(name, case_insensitive),
    }
}

/// The POSIX names under `(?U)`, by case-insensitive name.
pub(super) fn posix_unicode(name: &str, case_insensitive: bool) -> Option<CharClass> {
    let cased_or = |exact: fn(u32) -> bool| test(if case_insensitive { cased } else { exact });
    match name.to_uppercase().as_str() {
        "ALNUM" => test(alnum),
        "ALPHA" => test(unicode::is_alphabetic),
        "BLANK" => test(blank),
        "CNTRL" => category_class(category::CONTROL),
        "DIGIT" => test(unicode::is_digit),
        "GRAPH" => test(graph),
        "LOWER" => cased_or(unicode::is_lower),
        "PRINT" => test(print),
        "PUNCT" => test(punctuation),
        "SPACE" => test(white_space),
        "UPPER" => cased_or(unicode::is_upper),
        "XDIGIT" => test(hex_digit),
        _ => None,
    }
}

/// Upper-case script name, ISO code and inclusive ranges.
type Script = (&'static str, &'static str, &'static [(u32, u32)]);

/// Common scripts by upper-case name and ISO code, with their main ranges. Scripts are outside
/// the contract: this approximates a script by those blocks.
const SCRIPTS: &[Script] = &[
    ("ARABIC", "ARAB", &[(0x600, 0x6FF), (0x750, 0x77F)]),
    ("ARMENIAN", "ARMN", &[(0x531, 0x58F)]),
    ("BENGALI", "BENG", &[(0x980, 0x9FF)]),
    ("COMMON", "ZYYY", &[(0, 0x40), (0x5B, 0x60), (0x7B, 0xA9), (0x2000, 0x206F)]),
    ("CYRILLIC", "CYRL", &[(0x400, 0x52F)]),
    ("DEVANAGARI", "DEVA", &[(0x900, 0x97F)]),
    ("GEORGIAN", "GEOR", &[(0x10A0, 0x10FF)]),
    ("GREEK", "GREK", &[(0x370, 0x3FF), (0x1F00, 0x1FFF)]),
    ("HAN", "HANI", &[(0x2E80, 0x2FD5), (0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0xF900, 0xFAD9)]),
    ("HANGUL", "HANG", &[(0x1100, 0x11FF), (0xAC00, 0xD7A3)]),
    ("HEBREW", "HEBR", &[(0x591, 0x5F4)]),
    ("HIRAGANA", "HIRA", &[(0x3041, 0x309F)]),
    ("KATAKANA", "KANA", &[(0x30A1, 0x30FF)]),
    ("LATIN", "LATN", &[(0x41, 0x5A), (0x61, 0x7A), (0xC0, 0x24F), (0x1E00, 0x1EFF)]),
    ("TAMIL", "TAML", &[(0xB80, 0xBFF)]),
    ("THAI", "THAI", &[(0xE01, 0xE5B)]),
];

/// Common blocks: upper-case names without separators and their ranges.
const BLOCKS: &[(&str, u32, u32)] = &[
    ("ARABIC", 0x600, 0x6FF),
    ("ARMENIAN", 0x530, 0x58F),
    ("ARROWS", 0x2190, 0x21FF),
    ("BASICLATIN", 0, 0x7F),
    ("BOXDRAWING", 0x2500, 0x257F),
    ("CJKUNIFIEDIDEOGRAPHS", 0x4E00, 0x9FFF),
    ("COMBININGDIACRITICALMARKS", 0x300, 0x36F),
    ("CURRENCYSYMBOLS", 0x20A0, 0x20CF),
    ("CYRILLIC", 0x400, 0x4FF),
    ("DEVANAGARI", 0x900, 0x97F),
    ("GENERALPUNCTUATION", 0x2000, 0x206F),
    ("GREEK", 0x370, 0x3FF),
    ("GREEKANDCOPTIC", 0x370, 0x3FF),
    ("HANGULSYLLABLES", 0xAC00, 0xD7AF),
    ("HEBREW", 0x590, 0x5FF),
    ("HIRAGANA", 0x3040, 0x309F),
    ("KATAKANA", 0x30A0, 0x30FF),
    ("LATIN-1SUPPLEMENT", 0x80, 0xFF),
    ("LATINEXTENDED-A", 0x100, 0x17F),
    ("LATINEXTENDED-B", 0x180, 0x24F),
    ("MATHEMATICALOPERATORS", 0x2200, 0x22FF),
    ("THAI", 0xE00, 0xE7F),
];

/// `Character.UnicodeScript.forName` over the [`SCRIPTS`] table.
pub(super) fn script(name: &str) -> Option<CharClass> {
    let name = name.to_uppercase();
    let (_, _, ranges) = SCRIPTS.iter().find(|(full, code, _)| *full == name || *code == name)?;
    let mut parts = ranges.iter().map(|&(first, last)| CharClass::range(first, last));
    let first = parts.next()?;
    Some(parts.fold(first, CharClass::union))
}

/// `Character.UnicodeBlock.forName` over the [`BLOCKS`] table.
pub(super) fn block(name: &str) -> Option<CharClass> {
    let name: String = name.to_uppercase().chars().filter(|c| !matches!(c, ' ' | '_')).collect();
    let (_, first, last) = BLOCKS.iter().find(|(block, _, _)| *block == name)?;
    Some(CharClass::range(*first, *last))
}
