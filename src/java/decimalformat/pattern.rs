//! `DecimalFormat.applyPattern`, reduced to what parsing depends on.

use crate::java::{JResult, Thrown};

const ILLEGAL_ARGUMENT: &str = "java.lang.IllegalArgumentException";

const QUOTE: u16 = 0x27;
const SEPARATOR: u16 = 0x3B;
const PERCENT: u16 = 0x25;
const PER_MILLE: u16 = 0x2030;
const MINUS: u16 = 0x2D;
const CURRENCY: u16 = 0xA4;
const OPTIONAL_DIGIT: u16 = 0x23;
const ZERO_DIGIT: u16 = 0x30;
const GROUPING: u16 = 0x2C;
const DECIMAL: u16 = 0x2E;
const EXPONENT: u16 = 0x45;

/// One element of an affix; symbols stay abstract until the locale expands them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AffixItem {
    Literal(u16),
    Minus,
    Percent,
    PerMille,
    Currency,
    InternationalCurrency,
}

/// The parts of a pattern that influence parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pattern {
    pub(super) positive_prefix: Vec<AffixItem>,
    pub(super) positive_suffix: Vec<AffixItem>,
    pub(super) negative_prefix: Vec<AffixItem>,
    pub(super) negative_suffix: Vec<AffixItem>,
    pub(super) grouping_used: bool,
    pub(super) multiplier: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prefix,
    Number,
    Suffix,
}

/// The digit layout of a positive subpattern.
#[derive(Default)]
struct DigitCounts {
    optional_left: u32,
    zeros: u32,
    optional_right: u32,
    decimal_position: Option<u32>,
    /// Digits since the last grouping separator; `None` before the first one.
    grouping: Option<u32>,
    exponent_seen: bool,
}

/// The outcome of scanning one subpattern.
struct Subpattern {
    prefix: Vec<AffixItem>,
    suffix: Vec<AffixItem>,
    multiplier: i32,
    grouping: Option<u32>,
    /// Where the negative subpattern starts, after an unquoted separator.
    next_start: Option<usize>,
}

/// Parses a pattern. Without a separator the same text is scanned a second time as the negative
/// subpattern, whose digits are skipped; only if that yields different affixes does it count.
pub(super) fn parse(pattern: &[u16]) -> JResult<Pattern> {
    if pattern.is_empty() {
        return Ok(Pattern {
            positive_prefix: Vec::new(),
            positive_suffix: Vec::new(),
            negative_prefix: vec![AffixItem::Minus],
            negative_suffix: Vec::new(),
            grouping_used: true,
            multiplier: 1,
        });
    }
    let scanner = Scanner { pattern };
    let positive = scanner.scan(0, false)?;
    let negative_start = positive.next_start.unwrap_or(0);
    let negative = if negative_start < pattern.len() { Some(scanner.scan(negative_start, true)?) } else { None };
    let (negative_prefix, negative_suffix) = match negative {
        Some(negative) if negative.prefix != positive.prefix || negative.suffix != positive.suffix => {
            (negative.prefix, negative.suffix)
        }
        _ => {
            let mut prefix = vec![AffixItem::Minus];
            prefix.extend_from_slice(&positive.prefix);
            (prefix, positive.suffix.clone())
        }
    };
    Ok(Pattern {
        positive_prefix: positive.prefix,
        positive_suffix: positive.suffix,
        negative_prefix,
        negative_suffix,
        grouping_used: positive.grouping.is_some_and(|count| count > 0),
        multiplier: positive.multiplier,
    })
}

struct Scanner<'a> {
    pattern: &'a [u16],
}

impl Scanner<'_> {
    fn failure(&self, description: &str) -> Thrown {
        let mut message: Vec<u16> = description.encode_utf16().collect();
        message.extend(" \"".encode_utf16());
        message.extend_from_slice(self.pattern);
        message.push(0x22);
        Thrown::with_message(ILLEGAL_ARGUMENT, message)
    }

    fn scan(&self, start: usize, negative: bool) -> JResult<Subpattern> {
        let mut phase = Phase::Prefix;
        let mut pos = start;
        let mut in_quote = false;
        let mut prefix = Vec::new();
        let mut suffix = Vec::new();
        let mut multiplier = 1;
        let mut counts = DigitCounts::default();
        let mut next_start = None;
        while let Some(&unit) = self.pattern.get(pos) {
            if phase == Phase::Number {
                if negative {
                    if is_number_unit(unit) || unit == EXPONENT {
                        pos += 1;
                    } else {
                        phase = Phase::Suffix;
                    }
                } else if self.scan_number_unit(unit, &mut pos, &mut counts)? {
                    phase = Phase::Suffix;
                }
                continue;
            }
            let affix = if phase == Phase::Prefix { &mut prefix } else { &mut suffix };
            let doubled = self.pattern.get(pos + 1) == Some(&unit);
            if in_quote {
                if unit != QUOTE {
                    affix.push(AffixItem::Literal(unit));
                } else if doubled {
                    affix.push(AffixItem::Literal(QUOTE));
                    pos += 1;
                } else {
                    in_quote = false;
                }
                pos += 1;
                continue;
            }
            match unit {
                OPTIONAL_DIGIT | ZERO_DIGIT | GROUPING | DECIMAL => {
                    phase = Phase::Number;
                    continue;
                }
                CURRENCY if doubled => {
                    affix.push(AffixItem::InternationalCurrency);
                    pos += 1;
                }
                CURRENCY => affix.push(AffixItem::Currency),
                QUOTE if doubled => {
                    affix.push(AffixItem::Literal(QUOTE));
                    pos += 1;
                }
                QUOTE => in_quote = true,
                SEPARATOR => {
                    if phase == Phase::Prefix || negative {
                        let text = format!("Unquoted special character '{}' in pattern", ';');
                        return Err(self.failure(&text));
                    }
                    next_start = Some(pos + 1);
                    break;
                }
                PERCENT | PER_MILLE => {
                    if multiplier != 1 {
                        return Err(self.failure("Too many percent/per mille characters in pattern"));
                    }
                    multiplier = if unit == PERCENT { 100 } else { 1000 };
                    affix.push(if unit == PERCENT { AffixItem::Percent } else { AffixItem::PerMille });
                }
                MINUS => affix.push(AffixItem::Minus),
                _ => affix.push(AffixItem::Literal(unit)),
            }
            pos += 1;
        }
        self.check_digits(&mut counts, in_quote)?;
        Ok(Subpattern { prefix, suffix, multiplier, grouping: counts.grouping, next_start })
    }

    /// Handles one unit of the digit part; returns whether the digit part has ended.
    fn scan_number_unit(&self, unit: u16, pos: &mut usize, counts: &mut DigitCounts) -> JResult<bool> {
        let in_integer = counts.decimal_position.is_none();
        match unit {
            OPTIONAL_DIGIT => {
                if counts.zeros > 0 {
                    counts.optional_right += 1;
                } else {
                    counts.optional_left += 1;
                }
            }
            ZERO_DIGIT => {
                if counts.optional_right > 0 {
                    return Err(self.failure("Unexpected '0' in pattern"));
                }
                counts.zeros += 1;
            }
            GROUPING => {
                counts.grouping = Some(0);
                *pos += 1;
                return Ok(false);
            }
            DECIMAL => {
                if !in_integer {
                    return Err(self.failure("Multiple decimal separators in pattern"));
                }
                counts.decimal_position = Some(counts.optional_left + counts.zeros + counts.optional_right);
                *pos += 1;
                return Ok(false);
            }
            EXPONENT => {
                if counts.exponent_seen {
                    return Err(self.failure("Multiple exponential symbols in pattern"));
                }
                counts.exponent_seen = true;
                *pos += 1;
                let tail = self.pattern.get(*pos..).unwrap_or_default();
                let exponent_digits = tail.iter().take_while(|u| **u == ZERO_DIGIT).count();
                *pos += exponent_digits;
                if counts.optional_left + counts.zeros < 1 || exponent_digits < 1 {
                    return Err(self.failure("Malformed exponential pattern"));
                }
                return Ok(true);
            }
            _ => return Ok(true),
        }
        if in_integer {
            if let Some(grouping) = counts.grouping.as_mut() {
                *grouping += 1;
            }
        }
        *pos += 1;
        Ok(false)
    }

    fn check_digits(&self, counts: &mut DigitCounts, in_quote: bool) -> JResult<()> {
        if counts.zeros == 0 && counts.optional_left > 0 {
            if let Some(decimal) = counts.decimal_position {
                let integer_digits = decimal.max(1);
                counts.optional_right = counts.optional_left.saturating_sub(integer_digits);
                counts.optional_left = integer_digits - 1;
                counts.zeros = 1;
            }
        }
        let integer_digits = counts.optional_left + counts.zeros;
        let decimal_misplaced = match counts.decimal_position {
            Some(decimal) => decimal < counts.optional_left || decimal > integer_digits,
            None => counts.optional_right > 0,
        };
        if decimal_misplaced || counts.grouping == Some(0) || in_quote {
            return Err(self.failure("Malformed pattern"));
        }
        Ok(())
    }
}

fn is_number_unit(unit: u16) -> bool {
    matches!(unit, OPTIONAL_DIGIT | ZERO_DIGIT | GROUPING | DECIMAL)
}
