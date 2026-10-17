// Copyright 2012-2022 Udo Klimaschewski (EvalEx)
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The JSON of the command's requests and responses.

use crate::java::bigdecimal::BigDecimal;
use crate::java::{js, JResult, JString, Thrown};

/// The class name of the front end's own errors.
pub const REQUEST_ERROR: &str = "EvalExCli$RequestError";

/// A failure of the front end.
pub fn request_error(message: &str) -> Thrown {
    Thrown::new(REQUEST_ERROR, message)
}

/// A JSON value.
#[derive(Clone, Debug)]
pub enum Json {
    /// `null`
    Null,
    /// `true` or `false`.
    Boolean(bool),
    /// A number, with the scale of its text.
    Number(BigDecimal),
    /// A string.
    Text(JString),
    /// An array.
    Array(Vec<Json>),
    /// An object with its members in order; a repeated name keeps its first place and its last value.
    Object(Vec<(JString, Json)>),
}

impl Json {
    /// The member `name` of an object.
    pub fn member(&self, name: &str) -> Option<&Json> {
        let Json::Object(members) = self else {
            return None;
        };
        let name = js(name);
        members.iter().find(|(key, _)| *key == name).map(|(_, value)| value)
    }
}

/// Parses one request line.
pub fn parse(text: &[u16]) -> JResult<Json> {
    let mut parser = Parser { text, at: 0, depth: 0 };
    let value = parser.value()?;
    parser.space();
    if parser.at != text.len() {
        return Err(parser.malformed());
    }
    Ok(value)
}

/// The deepest nesting of a request; beyond it the original's JVM has overflowed its stack.
const MAX_DEPTH: usize = 2_000;

struct Parser<'a> {
    text: &'a [u16],
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn malformed(&self) -> Thrown {
        request_error(&format!("malformed JSON at index {}", self.at))
    }

    fn peek(&self) -> Option<u16> {
        self.text.get(self.at).copied()
    }

    fn peek_is(&self, ascii: u8) -> bool {
        self.peek() == Some(u16::from(ascii))
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(0x20 | 0x09 | 0x0d)) {
            self.at += 1;
        }
    }

    fn literal(&mut self, word: &str) -> bool {
        let word = js(word);
        let matched = self.text.get(self.at..self.at + word.len()) == Some(word.as_slice());
        if matched {
            self.at += word.len();
        }
        matched
    }

    fn value(&mut self) -> JResult<Json> {
        if self.depth >= MAX_DEPTH {
            return Err(Thrown::no_message("java.lang.StackOverflowError"));
        }
        self.depth += 1;
        let value = self.nested_value();
        self.depth -= 1;
        value
    }

    fn nested_value(&mut self) -> JResult<Json> {
        self.space();
        match self.peek() {
            None => Err(self.malformed()),
            Some(0x7b) => self.object(),
            Some(0x5b) => self.array(),
            Some(0x22) => Ok(Json::Text(self.string()?)),
            Some(_) if self.literal("true") => Ok(Json::Boolean(true)),
            Some(_) if self.literal("false") => Ok(Json::Boolean(false)),
            Some(_) if self.literal("null") => Ok(Json::Null),
            Some(_) => self.number(),
        }
    }

    fn object(&mut self) -> JResult<Json> {
        let mut members: Vec<(JString, Json)> = Vec::new();
        self.at += 1;
        self.space();
        if self.peek_is(b'}') {
            self.at += 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.space();
            if !self.peek_is(b'"') {
                return Err(self.malformed());
            }
            let key = self.string()?;
            self.space();
            if !self.peek_is(b':') {
                return Err(self.malformed());
            }
            self.at += 1;
            let value = self.value()?;
            match members.iter_mut().find(|(existing, _)| *existing == key) {
                Some(member) => member.1 = value,
                None => members.push((key, value)),
            }
            self.space();
            if self.peek_is(b'}') {
                self.at += 1;
                return Ok(Json::Object(members));
            }
            if !self.peek_is(b',') {
                return Err(self.malformed());
            }
            self.at += 1;
        }
    }

    fn array(&mut self) -> JResult<Json> {
        let mut items = Vec::new();
        self.at += 1;
        self.space();
        if self.peek_is(b']') {
            self.at += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.space();
            if self.peek_is(b']') {
                self.at += 1;
                return Ok(Json::Array(items));
            }
            if !self.peek_is(b',') {
                return Err(self.malformed());
            }
            self.at += 1;
        }
    }

    fn string(&mut self) -> JResult<JString> {
        let mut out = JString::new();
        self.at += 1;
        loop {
            let Some(unit) = self.peek() else {
                return Err(self.malformed());
            };
            if unit == u16::from(b'"') {
                self.at += 1;
                return Ok(out);
            }
            if unit < 0x20 {
                return Err(self.malformed());
            }
            self.at += 1;
            if unit == u16::from(b'\\') {
                out.push(self.escape()?);
            } else {
                out.push(unit);
            }
        }
    }

    fn escape(&mut self) -> JResult<u16> {
        let Some(unit) = self.peek() else {
            return Err(self.malformed());
        };
        let plain = match u8::try_from(unit) {
            Ok(b'"') => Some(0x22),
            Ok(b'\\') => Some(0x5c),
            Ok(b'/') => Some(0x2f),
            Ok(b'b') => Some(0x08),
            Ok(b'f') => Some(0x0c),
            Ok(b'n') => Some(0x0a),
            Ok(b'r') => Some(0x0d),
            Ok(b't') => Some(0x09),
            _ => None,
        };
        if let Some(plain) = plain {
            self.at += 1;
            return Ok(plain);
        }
        if unit != u16::from(b'u') {
            return Err(self.malformed());
        }
        let Some(digits) = self.text.get(self.at + 1..self.at + 5) else {
            return Err(self.malformed());
        };
        let mut value = 0u16;
        for &digit in digits {
            let digit = u8::try_from(digit).ok().and_then(|byte| char::from(byte).to_digit(16));
            match digit {
                Some(digit) => value = value * 16 + digit as u16,
                None => return Err(self.malformed()),
            }
        }
        self.at += 5;
        Ok(value)
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while self.peek().is_some_and(|unit| (0x30..=0x39).contains(&unit)) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> JResult<Json> {
        let start = self.at;
        if self.peek_is(b'-') {
            self.at += 1;
        }
        let first = self.peek();
        let count = self.digits();
        if count == 0 || (first == Some(0x30) && count > 1) {
            self.at = start;
            return Err(self.malformed());
        }
        if self.peek_is(b'.') {
            self.at += 1;
            if self.digits() == 0 {
                return Err(self.malformed());
            }
        }
        if self.peek_is(b'e') || self.peek_is(b'E') {
            self.at += 1;
            if self.peek_is(b'+') || self.peek_is(b'-') {
                self.at += 1;
            }
            if self.digits() == 0 {
                return Err(self.malformed());
            }
        }
        Ok(Json::Number(BigDecimal::parse(&self.text[start..self.at])?))
    }
}

/// Writes a Java string as a JSON string; `None` is `null`.
pub fn write_string(text: Option<&[u16]>, out: &mut String) {
    let Some(text) = text else {
        out.push_str("null");
        return;
    };
    out.push('"');
    for decoded in char::decode_utf16(text.iter().copied()) {
        match decoded {
            Ok('"') => out.push_str("\\\""),
            Ok('\\') => out.push_str("\\\\"),
            Ok('\u{8}') => out.push_str("\\b"),
            Ok('\t') => out.push_str("\\t"),
            Ok('\n') => out.push_str("\\n"),
            Ok('\u{c}') => out.push_str("\\f"),
            Ok('\r') => out.push_str("\\r"),
            Ok(c) if c < '\u{20}' => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            Ok(c) => out.push(c),
            Err(error) => out.push_str(&format!("\\u{:04x}", error.unpaired_surrogate())),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_objects_in_member_order() {
        let value = parse(&js(r#"{"b":1.50,"a":[true,null],"b":"x"}"#)).ok();
        let Some(Json::Object(members)) = value else {
            panic!("not an object");
        };
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].0, js("b"));
        assert!(matches!(&members[0].1, Json::Text(text) if *text == js("x")));
    }

    #[test]
    fn reports_the_index_of_malformed_input() {
        let error = parse(&js("[1,")).map(|_| ()).err();
        assert_eq!(error, Some(request_error("malformed JSON at index 3")));
        let error = parse(&js("01")).map(|_| ()).err();
        assert_eq!(error, Some(request_error("malformed JSON at index 0")));
    }

    #[test]
    fn writes_escapes_and_lone_surrogates() {
        let mut out = String::new();
        write_string(Some(&[0x22, 0x0a, 0x01, 0xd83d, 0x61]), &mut out);
        assert_eq!(out, "\"\\\"\\n\\u0001\\ud83da\"");
        out.clear();
        write_string(None, &mut out);
        assert_eq!(out, "null");
    }
}
