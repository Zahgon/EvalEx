//! Unicode character data as Java 21 (Unicode 15.0) reports it.
//!
//! General categories come from a table generated out of `Character.getType`. Everything else is
//! derived from them or from Rust's `char` methods, filtered by "assigned in Unicode 15" because
//! Rust ships a newer Unicode version.

use super::unicode_tables::CATEGORY_RUNS;

/// Bit masks over `Character.getType` values.
pub(super) mod category {
    pub(in super::super) const UNASSIGNED: u32 = 1 << 0;
    pub(in super::super) const UPPERCASE: u32 = 1 << 1;
    pub(in super::super) const LOWERCASE: u32 = 1 << 2;
    pub(in super::super) const TITLECASE: u32 = 1 << 3;
    pub(in super::super) const MODIFIER_LETTER: u32 = 1 << 4;
    pub(in super::super) const OTHER_LETTER: u32 = 1 << 5;
    pub(in super::super) const NON_SPACING_MARK: u32 = 1 << 6;
    pub(in super::super) const ENCLOSING_MARK: u32 = 1 << 7;
    pub(in super::super) const SPACING_MARK: u32 = 1 << 8;
    pub(in super::super) const DECIMAL_DIGIT: u32 = 1 << 9;
    pub(in super::super) const LETTER_NUMBER: u32 = 1 << 10;
    pub(in super::super) const OTHER_NUMBER: u32 = 1 << 11;
    pub(in super::super) const SPACE_SEPARATOR: u32 = 1 << 12;
    pub(in super::super) const LINE_SEPARATOR: u32 = 1 << 13;
    pub(in super::super) const PARAGRAPH_SEPARATOR: u32 = 1 << 14;
    pub(in super::super) const CONTROL: u32 = 1 << 15;
    pub(in super::super) const FORMAT: u32 = 1 << 16;
    pub(in super::super) const PRIVATE_USE: u32 = 1 << 18;
    pub(in super::super) const SURROGATE: u32 = 1 << 19;
    pub(in super::super) const DASH_PUNCTUATION: u32 = 1 << 20;
    pub(in super::super) const START_PUNCTUATION: u32 = 1 << 21;
    pub(in super::super) const END_PUNCTUATION: u32 = 1 << 22;
    pub(in super::super) const CONNECTOR_PUNCTUATION: u32 = 1 << 23;
    pub(in super::super) const OTHER_PUNCTUATION: u32 = 1 << 24;
    pub(in super::super) const MATH_SYMBOL: u32 = 1 << 25;
    pub(in super::super) const CURRENCY_SYMBOL: u32 = 1 << 26;
    pub(in super::super) const MODIFIER_SYMBOL: u32 = 1 << 27;
    pub(in super::super) const OTHER_SYMBOL: u32 = 1 << 28;
    pub(in super::super) const INITIAL_PUNCTUATION: u32 = 1 << 29;
    pub(in super::super) const FINAL_PUNCTUATION: u32 = 1 << 30;

    /// `L`: every letter category.
    pub(in super::super) const LETTER: u32 = UPPERCASE | LOWERCASE | TITLECASE | MODIFIER_LETTER | OTHER_LETTER;
    /// `LC`: the cased letters.
    pub(in super::super) const CASED_LETTER: u32 = UPPERCASE | LOWERCASE | TITLECASE;
    /// `M`: every mark category.
    pub(in super::super) const MARK: u32 = NON_SPACING_MARK | ENCLOSING_MARK | SPACING_MARK;
    /// `N`: every number category.
    pub(in super::super) const NUMBER: u32 = DECIMAL_DIGIT | LETTER_NUMBER | OTHER_NUMBER;
    /// `Z`: every separator category.
    pub(in super::super) const SEPARATOR: u32 = SPACE_SEPARATOR | LINE_SEPARATOR | PARAGRAPH_SEPARATOR;
    /// `P`: every punctuation category.
    pub(in super::super) const PUNCTUATION: u32 = DASH_PUNCTUATION
        | START_PUNCTUATION
        | END_PUNCTUATION
        | CONNECTOR_PUNCTUATION
        | OTHER_PUNCTUATION
        | INITIAL_PUNCTUATION
        | FINAL_PUNCTUATION;
    /// `S`: every symbol category.
    pub(in super::super) const SYMBOL: u32 = MATH_SYMBOL | CURRENCY_SYMBOL | MODIFIER_SYMBOL | OTHER_SYMBOL;
    /// `C`: the "other" categories.
    pub(in super::super) const OTHER: u32 = CONTROL | FORMAT | PRIVATE_USE | SURROGATE | UNASSIGNED;
}

/// Code points of the ideographic blocks, as inclusive ranges.
const IDEOGRAPHIC: &[(u32, u32)] = &[
    (0x3006, 0x3007),
    (0x3021, 0x3029),
    (0x3038, 0x303A),
    (0x3400, 0x4DBF),
    (0x4E00, 0x9FFF),
    (0xF900, 0xFA6D),
    (0xFA70, 0xFAD9),
    (0x16FE4, 0x16FE4),
    (0x17000, 0x187F7),
    (0x18800, 0x18CD5),
    (0x18D00, 0x18D08),
    (0x1B170, 0x1B2FB),
    (0x20000, 0x2A6DF),
    (0x2A700, 0x2B739),
    (0x2B740, 0x2B81D),
    (0x2B820, 0x2CEA1),
    (0x2CEB0, 0x2EBE0),
    (0x2F800, 0x2FA1D),
    (0x30000, 0x3134A),
    (0x31350, 0x323AF),
];

/// The mask bit of the general category of `cp`; zero bits are impossible for code points.
pub(super) fn category_bit(cp: u32) -> u32 {
    let run = CATEGORY_RUNS.partition_point(|&entry| entry >> 5 <= cp);
    let entry = CATEGORY_RUNS.get(run.wrapping_sub(1)).copied().unwrap_or(0);
    1 << (entry & 31)
}

/// `true` when the general category of `cp` is in `mask`.
pub(super) fn has_category(cp: u32, mask: u32) -> bool {
    category_bit(cp) & mask != 0
}

fn assigned_char(cp: u32) -> Option<char> {
    char::from_u32(cp).filter(|_| !has_category(cp, category::UNASSIGNED))
}

/// `Character.toUpperCase(int)`.
pub(super) fn to_upper(cp: u32) -> u32 {
    let Some(c) = assigned_char(cp) else {
        return cp;
    };
    match cp {
        // Case pairs added after Unicode 15.
        0x19B | 0x264 | 0xA7D3 | 0xA7D5 => return cp,
        // Full mappings expand these to two characters, the simple mapping is the titlecase form.
        0x1F80..=0x1F87 | 0x1F90..=0x1F97 | 0x1FA0..=0x1FA7 => return cp + 8,
        0x1FB3 | 0x1FC3 | 0x1FF3 => return cp + 9,
        _ => {}
    }
    let mut mapped = c.to_uppercase();
    match (mapped.next(), mapped.next()) {
        (Some(single), None) => u32::from(single),
        _ => cp,
    }
}

/// `Character.toLowerCase(int)`.
pub(super) fn to_lower(cp: u32) -> u32 {
    let Some(c) = assigned_char(cp) else {
        return cp;
    };
    if cp == 0x130 {
        return 0x69;
    }
    let mut mapped = c.to_lowercase();
    match (mapped.next(), mapped.next()) {
        (Some(single), None) => u32::from(single),
        _ => cp,
    }
}

/// `Character.isLowerCase(int)`.
pub(super) fn is_lower(cp: u32) -> bool {
    cp == 0x295 || assigned_char(cp).is_some_and(char::is_lowercase)
}

/// `Character.isUpperCase(int)`.
pub(super) fn is_upper(cp: u32) -> bool {
    assigned_char(cp).is_some_and(char::is_uppercase)
}

/// `Character.isTitleCase(int)`.
pub(super) fn is_title(cp: u32) -> bool {
    has_category(cp, category::TITLECASE)
}

/// `Character.isAlphabetic(int)`.
pub(super) fn is_alphabetic(cp: u32) -> bool {
    // Rust also counts the marks that became Other_Alphabetic after Unicode 15.
    !matches!(cp, 0x363..=0x36F | 0x1DD3..=0x1DE6) && assigned_char(cp).is_some_and(char::is_alphabetic)
}

/// `Character.isDigit(int)`.
pub(super) fn is_digit(cp: u32) -> bool {
    has_category(cp, category::DECIMAL_DIGIT)
}

/// `Character.isLetter(int)`.
pub(super) fn is_letter(cp: u32) -> bool {
    has_category(cp, category::LETTER)
}

/// `Character.isLetterOrDigit(int)`.
pub(super) fn is_letter_or_digit(cp: u32) -> bool {
    has_category(cp, category::LETTER | category::DECIMAL_DIGIT)
}

/// `Character.isDefined(int)`.
pub(super) fn is_defined(cp: u32) -> bool {
    !has_category(cp, category::UNASSIGNED)
}

/// `Character.isISOControl(int)`.
pub(super) fn is_iso_control(cp: u32) -> bool {
    cp <= 0x1F || (0x7F..=0x9F).contains(&cp)
}

/// `Character.isSpaceChar(int)`.
pub(super) fn is_space_char(cp: u32) -> bool {
    has_category(cp, category::SEPARATOR)
}

/// `Character.isWhitespace(int)`.
pub(super) fn is_whitespace(cp: u32) -> bool {
    matches!(cp, 0x09..=0x0D | 0x1C..=0x1F) || (is_space_char(cp) && !matches!(cp, 0xA0 | 0x2007 | 0x202F))
}

/// `Character.isIdeographic(int)`.
pub(super) fn is_ideographic(cp: u32) -> bool {
    IDEOGRAPHIC.iter().any(|&(first, last)| (first..=last).contains(&cp))
}

/// `Character.isIdentifierIgnorable(int)`.
pub(super) fn is_identifier_ignorable(cp: u32) -> bool {
    matches!(cp, 0..=8 | 0x0E..=0x1B | 0x7F..=0x9F) || has_category(cp, category::FORMAT)
}

/// `Character.isJavaIdentifierStart(int)`.
pub(super) fn is_java_identifier_start(cp: u32) -> bool {
    has_category(
        cp,
        category::LETTER | category::LETTER_NUMBER | category::CURRENCY_SYMBOL | category::CONNECTOR_PUNCTUATION,
    )
}

/// `Character.isJavaIdentifierPart(int)`.
pub(super) fn is_java_identifier_part(cp: u32) -> bool {
    is_java_identifier_start(cp)
        || is_identifier_ignorable(cp)
        || has_category(cp, category::DECIMAL_DIGIT | category::SPACING_MARK | category::NON_SPACING_MARK)
}

/// `Character.isUnicodeIdentifierStart(int)`.
pub(super) fn is_unicode_identifier_start(cp: u32) -> bool {
    has_category(cp, category::LETTER | category::LETTER_NUMBER)
        || matches!(cp, 0x1885 | 0x1886 | 0x2118 | 0x212E | 0x309B | 0x309C)
}

/// `Character.isUnicodeIdentifierPart(int)`.
pub(super) fn is_unicode_identifier_part(cp: u32) -> bool {
    is_unicode_identifier_start(cp)
        || is_identifier_ignorable(cp)
        || matches!(cp, 0xB7 | 0x387 | 0x1369..=0x1371 | 0x19DA)
        || has_category(
            cp,
            category::CONNECTOR_PUNCTUATION
                | category::DECIMAL_DIGIT
                | category::SPACING_MARK
                | category::NON_SPACING_MARK,
        )
}

/// Best approximation of `Character.isMirrored(int)`: paired punctuation plus the ASCII and
/// Latin-1 mathematical brackets.
pub(super) fn is_mirrored(cp: u32) -> bool {
    matches!(cp, 0x3C | 0x3E | 0xAB | 0xBB) || has_category(cp, category::START_PUNCTUATION | category::END_PUNCTUATION)
}
