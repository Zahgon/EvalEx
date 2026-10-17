//! Backslash escapes outside and inside bracket classes.

use super::charclass::{self, CharClass};
use super::node::{Fold, Kind};
use super::parser::{ascii, Parsed, Parser, CASE_INSENSITIVE, UNICODE_CASE, UNICODE_CHARACTER_CLASS};
use super::property;

/// What an escape sequence stands for.
pub(super) enum Escaped {
    /// A literal code point.
    Char(i32),
    /// A predefined class.
    Class(CharClass),
    /// An assertion or another construct with its own node.
    Node(Kind),
}

impl Parser<'_> {
    /// Parses the escape at the cursor, which is on the backslash.
    pub(super) fn escape(&mut self, in_class: bool, is_range: bool) -> Parsed<Escaped> {
        let ch = self.skip();
        let unicode = self.has(UNICODE_CHARACTER_CLASS);
        let literal = |value: i32| Ok(Escaped::Char(value));
        let class = |class: CharClass| Ok(Escaped::Class(class));
        let digits = || property::digit_class(unicode);
        let spaces = || property::space_class(unicode);
        let word = || property::word_class(unicode);
        let horizontal = || CharClass::test(charclass::horizontal_space);
        let vertical = || CharClass::test(charclass::vertical_space);
        match ascii(ch) {
            b'0' => return literal(self.octal()?),
            b'1'..=b'9' if !in_class => return self.back_reference(ch - i32::from(b'0')),
            b'A' if !in_class => return Ok(Escaped::Node(Kind::Begin)),
            b'B' if !in_class => {
                return Ok(Escaped::Node(Kind::WordBoundary { negated: true, unicode }));
            }
            b'D' => return class(digits().negate()),
            b'G' if !in_class => return Ok(Escaped::Node(Kind::LastMatch)),
            b'H' => return class(horizontal().negate()),
            b'N' => return literal(self.character_name()?),
            b'R' if !in_class => return Ok(Escaped::Node(Kind::LineEnding)),
            b'S' => return class(spaces().negate()),
            b'V' => return class(vertical().negate()),
            b'W' => return class(word().negate()),
            b'X' if !in_class => return Ok(Escaped::Node(Kind::Grapheme)),
            b'Z' if !in_class => return Ok(Escaped::Node(self.dollar(false))),
            b'a' => return literal(0x07),
            b'b' if !in_class => {
                if let Some(kind) = self.word_or_grapheme_boundary(unicode)? {
                    return Ok(Escaped::Node(kind));
                }
            }
            b'c' => return literal(self.control()?),
            b'd' => return class(digits()),
            b'e' => return literal(0x1B),
            b'f' => return literal(0x0C),
            b'h' => return class(horizontal()),
            b'k' if !in_class => return self.named_reference(),
            b'n' => return literal(0x0A),
            b'r' => return literal(0x0D),
            b's' => return class(spaces()),
            b't' => return literal(0x09),
            b'u' => return literal(self.unicode_escape()?),
            b'v' => {
                // Only the bound of a range reads `\v` as the old vertical tab.
                return if is_range { literal(0x0B) } else { class(vertical()) };
            }
            b'w' => return class(word()),
            b'x' => return literal(self.hexadecimal()?),
            b'z' if !in_class => return Ok(Escaped::Node(Kind::End)),
            b'A'..=b'Z' | b'a'..=b'z' | b'1'..=b'9' => {}
            _ => return literal(ch),
        }
        Err(self.error("Illegal/unsupported escape sequence"))
    }

    /// `None` when a `{` after `\b` is not the start of `{g}`: the caller reports the error.
    fn word_or_grapheme_boundary(&mut self, unicode: bool) -> Parsed<Option<Kind>> {
        if self.peek() == i32::from(b'{') {
            if self.skip() == i32::from(b'g') {
                if self.read() == i32::from(b'}') {
                    return Ok(Some(Kind::GraphemeBoundary));
                }
                return Ok(None);
            }
            self.unread();
            self.unread();
        }
        Ok(Some(Kind::WordBoundary { negated: false, unicode }))
    }

    fn fold(&self) -> Fold {
        if !self.has(CASE_INSENSITIVE) {
            Fold::None
        } else if self.has(UNICODE_CASE) {
            Fold::Unicode
        } else {
            Fold::Ascii
        }
    }

    /// `\1`: further digits extend the number only while such a group exists.
    fn back_reference(&mut self, first: i32) -> Parsed<Escaped> {
        let mut group = first as u32;
        loop {
            let ch = self.peek();
            if !charclass::ascii_digit(ch as u32) {
                break;
            }
            let extended = group.saturating_mul(10).saturating_add((ch - i32::from(b'0')) as u32);
            if self.group_count - 1 < extended {
                break;
            }
            group = extended;
            self.read();
        }
        self.has_group_ref = true;
        let fold = self.fold();
        Ok(Escaped::Node(Kind::BackRef { group, fold }))
    }

    /// `\k<name>`; the `k` is consumed.
    fn named_reference(&mut self) -> Parsed<Escaped> {
        if self.read() != i32::from(b'<') {
            return Err(self.error("\\k is not followed by '<' for named capturing group"));
        }
        let first = self.read();
        let name = self.group_name(first)?;
        let Some(group) = self.named_group(&name) else {
            let message = format!("named capturing group <{name}> does not exist");
            return Err(self.error(&message));
        };
        self.has_group_ref = true;
        let fold = self.fold();
        Ok(Escaped::Node(Kind::BackRef { group, fold }))
    }

    fn octal(&mut self) -> Parsed<i32> {
        let is_octal = |ch: i32| (i32::from(b'0')..=i32::from(b'7')).contains(&ch);
        let value = |ch: i32| ch - i32::from(b'0');
        let first = self.read();
        if !is_octal(first) {
            return Err(self.error("Illegal octal escape sequence"));
        }
        let second = self.read();
        if !is_octal(second) {
            self.unread();
            return Ok(value(first));
        }
        let third = self.read();
        if is_octal(third) && value(first) <= 3 {
            return Ok(value(first) * 64 + value(second) * 8 + value(third));
        }
        self.unread();
        Ok(value(first) * 8 + value(second))
    }

    fn hex_value(ch: i32) -> i32 {
        char::from_u32(ch as u32).and_then(|c| c.to_digit(16)).map_or(0, |digit| digit as i32)
    }

    fn hexadecimal(&mut self) -> Parsed<i32> {
        let is_hex = |ch: i32| charclass::ascii_hex(ch as u32);
        let first = self.read();
        if is_hex(first) {
            let second = self.read();
            if is_hex(second) {
                return Ok(Parser::hex_value(first) * 16 + Parser::hex_value(second));
            }
        } else if first == i32::from(b'{') && is_hex(self.peek()) {
            let mut value = 0;
            let mut ch = self.read();
            while is_hex(ch) {
                value = (value << 4) + Parser::hex_value(ch);
                if value > 0x10FFFF {
                    return Err(self.error("Hexadecimal codepoint is too big"));
                }
                ch = self.read();
            }
            if ch != i32::from(b'}') {
                return Err(self.error("Unclosed hexadecimal escape sequence"));
            }
            return Ok(value);
        }
        Err(self.error("Illegal hexadecimal escape sequence"))
    }

    fn four_hex_digits(&mut self) -> Parsed<i32> {
        let mut value = 0;
        for _ in 0..4 {
            let ch = self.read();
            if !charclass::ascii_hex(ch as u32) {
                return Err(self.error("Illegal Unicode escape sequence"));
            }
            value = value * 16 + Parser::hex_value(ch);
        }
        Ok(value)
    }

    /// `\uhhhh`; two escapes forming a surrogate pair stand for one code point.
    fn unicode_escape(&mut self) -> Parsed<i32> {
        let high = self.four_hex_digits()?;
        if (0xD800..=0xDBFF).contains(&high) {
            let after_high = self.cursor;
            if self.read() == i32::from(b'\\') && self.read() == i32::from(b'u') {
                let low = self.four_hex_digits()?;
                if (0xDC00..=0xDFFF).contains(&low) {
                    return Ok(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00));
                }
            }
            self.cursor = after_high;
        }
        Ok(high)
    }

    fn control(&mut self) -> Parsed<i32> {
        if self.cursor < self.length {
            Ok(self.read() ^ 64)
        } else {
            Err(self.error("Illegal control escape sequence"))
        }
    }

    /// `\N{name}`. Character names are not available, so every name is reported as unknown.
    fn character_name(&mut self) -> Parsed<i32> {
        if self.read() != i32::from(b'{') {
            return Err(self.error("Illegal character name escape sequence"));
        }
        let start = self.cursor;
        while self.read() != i32::from(b'}') {
            if self.cursor >= self.length {
                return Err(self.error("Unclosed character name escape sequence"));
            }
        }
        let name = self.text_between(start, self.cursor.saturating_sub(1));
        let mut message = super::js("Unknown character name [");
        message.extend(name);
        message.extend(super::js("]"));
        Err(self.error_text(message))
    }
}
