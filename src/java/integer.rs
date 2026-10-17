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

//! Parsing of the integer wrapper classes.

use super::chars::decimal_digit;
use super::{js, JResult, Thrown};

const NUMBER_FORMAT: &str = "java.lang.NumberFormatException";

fn input_error(text: &[u16]) -> Thrown {
    let mut message = js("For input string: \"");
    message.extend_from_slice(text);
    message.push(u16::from(b'"'));
    Thrown::with_message(NUMBER_FORMAT, message)
}

/// `Long.parseLong` within `min..=max`, the bounds of the target type's parser.
fn parse_within(text: &[u16], min: i64, max: i64) -> JResult<i64> {
    let (negative, digits) = match text.first() {
        Some(0x2d) => (true, &text[1..]),
        Some(0x2b) => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() {
        return Err(input_error(text));
    }
    let mut value: i64 = 0;
    for &unit in digits {
        let digit = decimal_digit(unit).ok_or_else(|| input_error(text))?;
        value = value
            .checked_mul(10)
            .and_then(|scaled| scaled.checked_sub(i64::from(digit)))
            .ok_or_else(|| input_error(text))?;
    }
    let value = if negative { value } else { value.checked_neg().ok_or_else(|| input_error(text))? };
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(input_error(text))
    }
}

/// `Long.valueOf(String)`.
pub fn parse_long(text: &[u16]) -> JResult<i64> {
    parse_within(text, i64::MIN, i64::MAX)
}

/// `Integer.valueOf(String)`.
pub fn parse_int(text: &[u16]) -> JResult<i32> {
    let value = parse_within(text, i64::from(i32::MIN), i64::from(i32::MAX))?;
    i32::try_from(value).map_err(|_| input_error(text))
}

fn narrowed(text: &[u16], min: i64, max: i64) -> JResult<i64> {
    let value = i64::from(parse_int(text)?);
    if (min..=max).contains(&value) {
        return Ok(value);
    }
    let mut message = js("Value out of range. Value:\"");
    message.extend_from_slice(text);
    message.extend("\" Radix:10".encode_utf16());
    Err(Thrown::with_message(NUMBER_FORMAT, message))
}

/// `Short.valueOf(String)`.
pub fn parse_short(text: &[u16]) -> JResult<i16> {
    let value = narrowed(text, i64::from(i16::MIN), i64::from(i16::MAX))?;
    i16::try_from(value).map_err(|_| input_error(text))
}

/// `Byte.valueOf(String)`.
pub fn parse_byte(text: &[u16]) -> JResult<i8> {
    let value = narrowed(text, i64::from(i8::MIN), i64::from(i8::MAX))?;
    i8::try_from(value).map_err(|_| input_error(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_signed_values() {
        assert_eq!(parse_int(&js("-12")), Ok(-12));
        assert_eq!(parse_long(&js("+9223372036854775807")), Ok(i64::MAX));
        assert_eq!(parse_byte(&js("-128")), Ok(-128));
    }

    #[test]
    fn rejects_like_java() {
        let error = parse_int(&js("x")).err().and_then(|thrown| thrown.message);
        assert_eq!(error, Some(js("For input string: \"x\"")));
        let error = parse_byte(&js("300")).err().and_then(|thrown| thrown.message);
        assert_eq!(error, Some(js("Value out of range. Value:\"300\" Radix:10")));
    }
}
