//! Character predicates: what one position of the input is tested against.

use super::unicode;

/// Pure ASCII test applied to a code point.
fn is_ascii_where(cp: u32, test: fn(&u8) -> bool) -> bool {
    u8::try_from(cp).is_ok_and(|byte| byte.is_ascii() && test(&byte))
}

/// `[a-z]`.
pub(super) fn ascii_lower(cp: u32) -> bool {
    is_ascii_where(cp, u8::is_ascii_lowercase)
}

/// `[A-Z]`.
pub(super) fn ascii_upper(cp: u32) -> bool {
    is_ascii_where(cp, u8::is_ascii_uppercase)
}

/// `[a-zA-Z]`.
pub(super) fn ascii_alpha(cp: u32) -> bool {
    is_ascii_where(cp, u8::is_ascii_alphabetic)
}

/// `[0-9]`.
pub(super) fn ascii_digit(cp: u32) -> bool {
    is_ascii_where(cp, u8::is_ascii_digit)
}

/// `[a-zA-Z0-9]`.
pub(super) fn ascii_alnum(cp: u32) -> bool {
    is_ascii_where(cp, u8::is_ascii_alphanumeric)
}

/// `[0-9a-fA-F]`.
pub(super) fn ascii_hex(cp: u32) -> bool {
    is_ascii_where(cp, u8::is_ascii_hexdigit)
}

/// `[ \t\n\x0B\f\r]`.
pub(super) fn ascii_space(cp: u32) -> bool {
    matches!(cp, 0x09..=0x0D | 0x20)
}

/// `[a-zA-Z0-9_]`.
pub(super) fn ascii_word(cp: u32) -> bool {
    cp == 0x5F || ascii_alnum(cp)
}

/// ASCII punctuation.
pub(super) fn ascii_punct(cp: u32) -> bool {
    is_ascii_where(cp, u8::is_ascii_punctuation)
}

/// ASCII visible characters.
pub(super) fn ascii_graph(cp: u32) -> bool {
    is_ascii_where(cp, u8::is_ascii_graphic)
}

/// `[ \t]`.
pub(super) fn ascii_blank(cp: u32) -> bool {
    matches!(cp, 0x09 | 0x20)
}

/// ASCII control characters.
pub(super) fn ascii_control(cp: u32) -> bool {
    is_ascii_where(cp, u8::is_ascii_control)
}

/// The ASCII-only lower-casing of `Pattern`: anything else is unchanged.
pub(super) fn ascii_to_lower(cp: u32) -> u32 {
    if ascii_upper(cp) {
        cp + 0x20
    } else {
        cp
    }
}

/// The ASCII-only upper-casing of `Pattern`: anything else is unchanged.
pub(super) fn ascii_to_upper(cp: u32) -> u32 {
    if ascii_lower(cp) {
        cp - 0x20
    } else {
        cp
    }
}

/// The folding `Pattern` applies under `UNICODE_CASE`.
pub(super) fn unicode_fold(cp: u32) -> u32 {
    unicode::to_lower(unicode::to_upper(cp))
}

/// `\h`.
pub(super) fn horizontal_space(cp: u32) -> bool {
    matches!(cp, 0x09 | 0x20 | 0xA0 | 0x1680 | 0x180E | 0x2000..=0x200A | 0x202F | 0x205F | 0x3000)
}

/// `\v`, also the line breaks of `\R`.
pub(super) fn vertical_space(cp: u32) -> bool {
    matches!(cp, 0x0A..=0x0D | 0x85 | 0x2028 | 0x2029)
}

/// `true` for the line terminators of `.`, `^` and `$` outside `UNIX_LINES`.
pub(super) fn line_terminator(cp: u32) -> bool {
    matches!(cp, 0x0A | 0x0D | 0x85 | 0x2028 | 0x2029)
}

/// The set of code points below 256, as written directly in a bracket class.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Latin1Set([u64; 4]);

impl Latin1Set {
    fn insert(&mut self, cp: u32) {
        if let Some(word) = self.0.get_mut((cp >> 6) as usize) {
            *word |= 1 << (cp & 63);
        }
    }

    fn contains(&self, cp: u32) -> bool {
        self.0.get((cp >> 6) as usize).is_some_and(|word| word >> (cp & 63) & 1 == 1)
    }

    /// Adds `cp` and, when requested, its case variants.
    pub(super) fn add(&mut self, cp: u32, case_insensitive: bool, unicode_case: bool) {
        if case_insensitive {
            if cp < 0x80 {
                self.insert(ascii_to_upper(cp));
                self.insert(ascii_to_lower(cp));
            } else if unicode_case {
                self.insert(unicode::to_lower(cp));
                self.insert(unicode::to_upper(cp));
            }
        }
        self.insert(cp);
    }
}

/// A predicate over code points.
#[derive(Clone, Debug)]
enum Pred {
    All,
    Single(u32),
    /// Equal to one of two values.
    Either(u32, u32),
    /// Equal after Unicode case folding; holds the folded value.
    Folded(u32),
    Range(u32, u32),
    /// Range test on the character and its ASCII case variants.
    AsciiFoldedRange(u32, u32),
    /// Range test on the character and its Unicode case variants.
    UnicodeFoldedRange(u32, u32),
    Latin1(Box<Latin1Set>),
    Test(fn(u32) -> bool),
    Category(u32),
    Not(Box<Pred>),
    And(Box<Pred>, Box<Pred>),
    Or(Vec<Pred>),
}

impl Pred {
    fn test(&self, cp: u32) -> bool {
        match self {
            Pred::All => true,
            Pred::Single(value) => cp == *value,
            Pred::Either(first, second) => cp == *first || cp == *second,
            Pred::Folded(folded) => cp == *folded || unicode_fold(cp) == *folded,
            Pred::Range(first, last) => (*first..=*last).contains(&cp),
            Pred::AsciiFoldedRange(first, last) => {
                let range = *first..=*last;
                range.contains(&cp)
                    || (cp < 0x80 && (range.contains(&ascii_to_upper(cp)) || range.contains(&ascii_to_lower(cp))))
            }
            Pred::UnicodeFoldedRange(first, last) => {
                let range = *first..=*last;
                let upper = unicode::to_upper(cp);
                range.contains(&cp) || range.contains(&upper) || range.contains(&unicode::to_lower(upper))
            }
            Pred::Latin1(set) => set.contains(cp),
            Pred::Test(test) => test(cp),
            Pred::Category(mask) => unicode::has_category(cp, *mask),
            Pred::Not(inner) => !inner.test(cp),
            Pred::And(left, right) => left.test(cp) && right.test(cp),
            Pred::Or(parts) => parts.iter().any(|part| part.test(cp)),
        }
    }
}

/// A character test.
#[derive(Clone, Debug)]
pub(super) struct CharClass(Pred);

impl CharClass {
    /// Every code point (`.` under `DOTALL`, `\p{all}`).
    pub(super) fn all() -> CharClass {
        CharClass(Pred::All)
    }

    /// Exactly `cp`.
    pub(super) fn single(cp: u32) -> CharClass {
        CharClass(Pred::Single(cp))
    }

    /// `first` or `second`.
    pub(super) fn either(first: u32, second: u32) -> CharClass {
        CharClass(Pred::Either(first, second))
    }

    /// Anything folding to `folded` under `UNICODE_CASE`.
    pub(super) fn folded(folded: u32) -> CharClass {
        CharClass(Pred::Folded(folded))
    }

    /// The inclusive range.
    pub(super) fn range(first: u32, last: u32) -> CharClass {
        CharClass(Pred::Range(first, last))
    }

    /// The inclusive range under `CASE_INSENSITIVE`.
    pub(super) fn ascii_folded_range(first: u32, last: u32) -> CharClass {
        CharClass(Pred::AsciiFoldedRange(first, last))
    }

    /// The inclusive range under `CASE_INSENSITIVE | UNICODE_CASE`.
    pub(super) fn unicode_folded_range(first: u32, last: u32) -> CharClass {
        CharClass(Pred::UnicodeFoldedRange(first, last))
    }

    /// A set of Latin-1 characters.
    pub(super) fn latin1(set: Latin1Set) -> CharClass {
        CharClass(Pred::Latin1(Box::new(set)))
    }

    /// A test function over code points.
    pub(super) fn test(test: fn(u32) -> bool) -> CharClass {
        CharClass(Pred::Test(test))
    }

    /// Membership in a mask of general categories.
    pub(super) fn category(mask: u32) -> CharClass {
        CharClass(Pred::Category(mask))
    }

    /// The complement.
    pub(super) fn negate(self) -> CharClass {
        CharClass(Pred::Not(Box::new(self.0)))
    }

    /// The intersection.
    pub(super) fn and(self, other: CharClass) -> CharClass {
        CharClass(Pred::And(Box::new(self.0), Box::new(other.0)))
    }

    /// The union.
    pub(super) fn union(self, other: CharClass) -> CharClass {
        CharClass(match self.0 {
            Pred::Or(mut parts) => {
                parts.push(other.0);
                Pred::Or(parts)
            }
            pred => Pred::Or(vec![pred, other.0]),
        })
    }

    /// `true` when `cp` belongs to the class.
    pub(super) fn contains(&self, cp: u32) -> bool {
        self.0.test(cp)
    }
}
