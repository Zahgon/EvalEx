//! Bracket classes and `\p` properties.

use super::charclass::{CharClass, Latin1Set};
use super::escape::Escaped;
use super::parser::UNICODE_CHARACTER_CLASS;
use super::parser::{ascii, Parsed, Parser, CASE_INSENSITIVE, UNICODE_CASE};
use super::{js, property, JString};

impl Parser<'_> {
    /// Parses a class body. The cursor is on `[`, or just before the first character of an
    /// operand of `&&` when `consume` is false; then the closing `]` is left for the caller.
    pub(super) fn bracket_class(&mut self, consume: bool) -> Parsed<CharClass> {
        self.enter_nested()?;
        let class = self.bracket_body(consume)?;
        self.nesting -= 1;
        Ok(class)
    }

    fn bracket_body(&mut self, consume: bool) -> Parsed<CharClass> {
        let mut previous: Option<CharClass> = None;
        let mut current: Option<CharClass> = None;
        let mut bits = Latin1Set::default();
        let mut has_bits = false;
        let mut negated = false;
        let mut ch = self.next();
        loop {
            match ascii(ch) {
                b'^' if previous.is_none()
                    && !has_bits
                    && self.at(self.cursor.saturating_sub(1)) == i32::from(b'[') =>
                {
                    ch = self.next();
                    negated = true;
                    continue;
                }
                b'[' => {
                    let nested = self.bracket_class(true)?;
                    current = Some(nested.clone());
                    previous = Some(match previous {
                        Some(class) => class.union(nested),
                        None => nested,
                    });
                    ch = self.peek();
                    continue;
                }
                b'&' => {
                    ch = self.next();
                    if ch == i32::from(b'&') {
                        ch = self.next();
                        let mut right = None;
                        while ch != i32::from(b']') && ch != i32::from(b'&') {
                            let operand = if ch == i32::from(b'[') {
                                self.bracket_class(true)?
                            } else {
                                self.unread();
                                self.bracket_class(false)?
                            };
                            right = Some(match right {
                                Some(class) => CharClass::union(class, operand),
                                None => operand,
                            });
                            ch = self.peek();
                        }
                        if has_bits {
                            let set = CharClass::latin1(bits);
                            match previous.take() {
                                Some(class) => previous = Some(class.union(set)),
                                None => {
                                    previous = Some(set.clone());
                                    current = Some(set);
                                }
                            }
                            has_bits = false;
                        }
                        if right.is_some() {
                            current = right.clone();
                        }
                        previous = Some(match (previous.take(), current.clone()) {
                            (None, _) => match right {
                                Some(class) => class,
                                None => return Err(self.error("Bad class syntax")),
                            },
                            (Some(class), Some(operand)) => class.and(operand),
                            (Some(_), None) => {
                                return Err(self.error("Bad intersection syntax"));
                            }
                        });
                        continue;
                    }
                    // A single `&` is an ordinary character.
                    self.unread();
                }
                0 if self.cursor >= self.length => {
                    return Err(self.error("Unclosed character class"));
                }
                b']' if previous.is_some() || has_bits => {
                    if consume {
                        self.next();
                    }
                    let set = has_bits.then(|| CharClass::latin1(bits));
                    let class = match (previous, set) {
                        (Some(class), Some(set)) => class.union(set),
                        (Some(class), None) => class,
                        (None, Some(set)) => set,
                        (None, None) => return Err(self.error("Bad class syntax")),
                    };
                    return Ok(if negated { class.negate() } else { class });
                }
                _ => {}
            }
            match self.range(&mut bits)? {
                None => {
                    has_bits = true;
                    current = None;
                }
                Some(class) => {
                    current = Some(class.clone());
                    previous = Some(match previous {
                        Some(known) => known.union(class),
                        None => class,
                    });
                }
            }
            ch = self.peek();
        }
    }

    /// One class member. `None` means it went into `bits`.
    fn range(&mut self, bits: &mut Latin1Set) -> Parsed<Option<CharClass>> {
        let first = self.peek();
        let first = if first == i32::from(b'\\') {
            let escaped = self.next_escaped();
            if ascii(escaped) == b'p' || ascii(escaped) == b'P' {
                return self.property(ascii(escaped) == b'P').map(Some);
            }
            let is_range = self.at(self.cursor + 1) == i32::from(b'-');
            self.unread();
            match self.escape(true, is_range)? {
                Escaped::Char(value) => value,
                Escaped::Class(class) => return Ok(Some(class)),
                Escaped::Node(_) => return Err(self.error("Illegal/unsupported escape sequence")),
            }
        } else {
            self.next();
            first
        };

        if self.peek() == i32::from(b'-') {
            let end = self.at(self.cursor + 1);
            if end == i32::from(b'[') {
                return Ok(self.bits_or_single(bits, first));
            }
            if end != i32::from(b']') {
                self.next();
                let mut last = self.peek();
                if last == i32::from(b'\\') {
                    last = match self.escape(true, true)? {
                        Escaped::Char(value) => value,
                        Escaped::Class(_) | Escaped::Node(_) => -1,
                    };
                } else {
                    self.next();
                }
                if last < first {
                    return Err(self.error("Illegal character range"));
                }
                let (first, last) = (first as u32, last as u32);
                return Ok(Some(if !self.has(CASE_INSENSITIVE) {
                    CharClass::range(first, last)
                } else if self.has(UNICODE_CASE) {
                    CharClass::unicode_folded_range(first, last)
                } else {
                    CharClass::ascii_folded_range(first, last)
                }));
            }
        }
        Ok(self.bits_or_single(bits, first))
    }

    /// Latin-1 literals go into `bits`, except those whose case variants lie outside Latin-1.
    fn bits_or_single(&self, bits: &mut Latin1Set, ch: i32) -> Option<CharClass> {
        let cp = ch as u32;
        let case_insensitive = self.has(CASE_INSENSITIVE);
        let unicode_case = self.has(UNICODE_CASE);
        let folds_beyond_latin1 = case_insensitive
            && unicode_case
            && matches!(cp, 0xFF | 0xB5 | 0x49 | 0x69 | 0x53 | 0x73 | 0x4B | 0x6B | 0xC5 | 0xE5);
        if cp < 256 && !folds_beyond_latin1 {
            bits.add(cp, case_insensitive, unicode_case);
            None
        } else {
            Some(self.single(cp))
        }
    }

    pub(super) fn text_between(&self, start: usize, end: usize) -> JString {
        let code_points = self.text.get(start..end).unwrap_or(&[]);
        let mut units = Vec::with_capacity(code_points.len());
        for &ch in code_points {
            push_code_point(&mut units, ch as u32);
        }
        units
    }

    /// `\p` and `\P`; the cursor is on the letter.
    pub(super) fn property(&mut self, complement: bool) -> Parsed<CharClass> {
        let single_letter = self.next() != i32::from(b'{');
        if single_letter {
            self.unread();
        }
        self.next();
        let name = if single_letter {
            let start = self.cursor;
            self.read();
            self.text_between(start, start + 1)
        } else {
            let start = self.cursor;
            self.set_end_marker(i32::from(b'}'));
            while self.read() != i32::from(b'}') {}
            self.set_end_marker(0);
            let end = self.cursor;
            if end > self.length {
                return Err(self.error("Unclosed character family"));
            }
            if start + 1 >= end {
                return Err(self.error("Empty character family"));
            }
            self.text_between(start, end - 1)
        };

        let class = match name.iter().position(|&unit| unit == u16::from(b'=')) {
            Some(equals) => {
                let key = name.get(..equals).unwrap_or(&[]);
                let value = name.get(equals + 1..).unwrap_or(&[]);
                self.keyed_property(key, value)?
            }
            None => match self.named_property(&name) {
                Some(class) => class,
                None => {
                    let mut message = js("Unknown character property name {");
                    message.extend_from_slice(&name);
                    message.extend(js("}"));
                    return Err(self.error_text(message));
                }
            },
        };
        Ok(if complement { class.negate() } else { class })
    }

    fn set_end_marker(&mut self, marker: i32) {
        if let Some(slot) = self.text.get_mut(self.length) {
            *slot = marker;
        }
    }

    /// `\p{key=value}`.
    fn keyed_property(&self, key: &[u16], value: &[u16]) -> Parsed<CharClass> {
        let key = String::from_utf16_lossy(key).to_lowercase();
        let text = String::from_utf16_lossy(value);
        let case_insensitive = self.has(CASE_INSENSITIVE);
        let class = match key.as_str() {
            "sc" | "script" => property::script(&text),
            "blk" | "block" => property::block(&text),
            "gc" | "general_category" => property::general(&text, case_insensitive),
            _ => None,
        };
        class.ok_or_else(|| {
            let mut message = js("Unknown Unicode property {name=<");
            message.extend(js(&key));
            message.extend(js(">, value=<"));
            message.extend_from_slice(value);
            message.extend(js(">}"));
            self.error_text(message)
        })
    }

    /// `\p{name}`.
    fn named_property(&self, name: &[u16]) -> Option<CharClass> {
        let text = String::from_utf16(name).ok()?;
        let case_insensitive = self.has(CASE_INSENSITIVE);
        if let Some(block) = text.strip_prefix("In") {
            property::block(block)
        } else if let Some(short) = text.strip_prefix("Is") {
            property::binary(short, case_insensitive)
                .or_else(|| property::general(short, case_insensitive))
                .or_else(|| property::script(short))
        } else {
            self.has(UNICODE_CHARACTER_CLASS)
                .then(|| property::posix_unicode(&text, case_insensitive))
                .flatten()
                .or_else(|| property::general(&text, case_insensitive))
        }
    }
}

fn push_code_point(units: &mut JString, cp: u32) {
    if let Some(c) = char::from_u32(cp) {
        let mut buffer = [0; 2];
        units.extend_from_slice(c.encode_utf16(&mut buffer));
    } else if let Ok(unit) = u16::try_from(cp) {
        units.push(unit);
    }
}
