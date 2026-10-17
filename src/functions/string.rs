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

//! The string functions.

use super::{require_string, Function, NON_NEGATIVE, VALUE, VAR_ARG};
use crate::error::{Failure, Outcome};
use crate::expression::Expression;
use crate::java::chars::{to_lower_case, to_upper_case, trim};
use crate::java::formatter::{self, FormatArgument};
use crate::java::regex::Pattern;
use crate::java::time::Duration;
use crate::java::{js, JString, Thrown};
use crate::token::Token;
use crate::value::Value;
use std::rc::Rc;

fn parameter(parameters: &[Value], index: usize) -> Outcome<&Value> {
    parameters.get(index).ok_or_else(Failure::null_pointer)
}

fn length_argument(parameters: &[Value], index: usize) -> Outcome<i32> {
    Ok(parameter(parameters, index)?.required_number()?.int_value())
}

fn starts_at(text: &[u16], needle: &[u16], at: usize) -> bool {
    text.get(at..at + needle.len()) == Some(needle)
}

fn contains(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let string = parameter(parameters, 0)?.string_value();
    let substring = parameter(parameters, 1)?.string_value();
    let result = match (string, substring) {
        (Some(string), Some(substring)) => {
            let string = to_upper_case(&string);
            let substring = to_upper_case(&substring);
            (0..=string.len()).any(|at| starts_at(&string, &substring, at))
        }
        _ => false,
    };
    Ok(Value::Boolean(result))
}

fn ends_with(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let string = require_string(parameter(parameters, 0)?)?;
    let substring = require_string(parameter(parameters, 1)?)?;
    Ok(Value::Boolean(string.ends_with(&substring)))
}

fn starts_with(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let string = require_string(parameter(parameters, 0)?)?;
    let substring = require_string(parameter(parameters, 1)?)?;
    Ok(Value::Boolean(string.starts_with(&substring)))
}

/// The argument `String.format` receives for a value.
fn format_argument(expression: &Expression, value: &Value) -> Outcome<FormatArgument> {
    Ok(match value {
        Value::Str(text) => FormatArgument::Text(text.clone()),
        Value::Number(number) => FormatArgument::Decimal(number.clone()),
        Value::Boolean(flag) => FormatArgument::Boolean(*flag),
        Value::DateTime(instant) => FormatArgument::DateTime(instant.at_zone(&expression.configuration().zone_id)?),
        Value::Duration(duration) => FormatArgument::Other {
            class: "java.time.Duration",
            text: js(&duration.to_string()),
            hash: duration_hash(duration),
        },
        Value::Null | Value::Undefined => FormatArgument::Null,
        Value::Array(_) | Value::Structure(_) | Value::ExpressionNode(_) => FormatArgument::Other {
            class: collection_class(value),
            text: value.string_value().unwrap_or_default(),
            hash: 0,
        },
    })
}

/// `Duration.hashCode()`.
fn duration_hash(duration: &Duration) -> i32 {
    let seconds = duration.seconds;
    ((seconds ^ (seconds >> 32 & 0xffff_ffff)) as i32).wrapping_add(duration.nanos.wrapping_mul(51))
}

fn collection_class(value: &Value) -> &'static str {
    match value {
        Value::Array(_) => "java.util.ArrayList",
        Value::Structure(_) => "java.util.HashMap",
        _ => "com.ezylang.evalex.parser.ASTNode",
    }
}

fn format(expression: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let template = parameter(parameters, 0)?.string_value();
    let Some(template) = template else {
        return Err(Failure::null_pointer_message("Cannot invoke \"String.length()\" because \"s\" is null"));
    };
    let arguments = parameters
        .get(1..)
        .unwrap_or_default()
        .iter()
        .map(|value| format_argument(expression, value))
        .collect::<Outcome<Vec<_>>>()?;
    let text = formatter::format(&expression.configuration().locale, &template, &arguments)?;
    Ok(Value::Str(text))
}

fn left(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let string = require_string(parameter(parameters, 0)?)?;
    let length = kept_length(&string, length_argument(parameters, 1)?);
    Ok(Value::Str(string[..length].to_vec()))
}

fn right(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let string = require_string(parameter(parameters, 0)?)?;
    let length = kept_length(&string, length_argument(parameters, 1)?);
    Ok(Value::Str(string[string.len() - length..].to_vec()))
}

/// `Math.max(0, Math.min(length, string.length()))`.
fn kept_length(string: &[u16], length: i32) -> usize {
    usize::try_from(length).unwrap_or(0).min(string.len())
}

fn length(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let string = require_string(parameter(parameters, 0)?)?;
    Ok(Value::Number(crate::java::bigdecimal::BigDecimal::from_i64(string.len() as i64)))
}

fn lower(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    Ok(Value::Str(to_lower_case(&require_string(parameter(parameters, 0)?)?)))
}

fn upper(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    Ok(Value::Str(to_upper_case(&require_string(parameter(parameters, 0)?)?)))
}

fn matches_regex(_: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    let string = parameter(parameters, 0)?.string_value();
    let Some(regex) = parameter(parameters, 1)?.string_value() else {
        return Err(Failure::null_pointer_message(
            "Cannot invoke \"String.isEmpty()\" because \"this.pattern\" is null",
        ));
    };
    let pattern = Pattern::compile(&regex).map_err(|error| {
        let mut message = js("java.util.regex.PatternSyntaxException: ");
        message.extend(error.message());
        Failure::evaluation(token, &message)
    })?;
    let Some(string) = string else {
        return Err(Failure::null_pointer_message(
            "Cannot invoke \"java.lang.CharSequence.length()\" because \"this.text\" is null",
        ));
    };
    Ok(Value::Boolean(pattern.matches(&string)?))
}

/// `String.split(Pattern.quote(separator))`.
fn split_text(string: &[u16], separator: &[u16]) -> Vec<JString> {
    let mut parts: Vec<JString> = Vec::new();
    let mut index = 0;
    let mut at = 0;
    while at <= string.len() {
        if !starts_at(string, separator, at) {
            at += 1;
            continue;
        }
        let end = at + separator.len();
        if !(index == 0 && at == 0 && separator.is_empty()) {
            parts.push(string[index..at].to_vec());
            index = end;
        }
        at = if separator.is_empty() { at + 1 } else { end };
    }
    if index == 0 && parts.is_empty() {
        return vec![string.to_vec()];
    }
    parts.push(string[index..].to_vec());
    while parts.last().is_some_and(Vec::is_empty) {
        parts.pop();
    }
    parts
}

fn split(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let string = require_string(parameter(parameters, 0)?)?;
    let separator = require_string(parameter(parameters, 1)?)?;
    let parts = split_text(&string, &separator).into_iter().map(Value::Str).collect();
    Ok(Value::Array(Rc::new(parts)))
}

fn validate_substring(token: &Token, parameters: &[Value]) -> Outcome<()> {
    if parameters.len() > 2 && length_argument(parameters, 2)? < length_argument(parameters, 1)? {
        return Err(Failure::evaluation_str(token, "End index must be greater than or equal to start index"));
    }
    Ok(())
}

fn out_of_bounds(begin: i32, end: i32, length: usize) -> Failure {
    let message = format!("Range [{begin}, {end}) out of bounds for length {length}");
    Thrown::new("java.lang.StringIndexOutOfBoundsException", &message).into()
}

fn substring(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let string = require_string(parameter(parameters, 0)?)?;
    let start = length_argument(parameters, 1)?;
    let length = i32::try_from(string.len()).unwrap_or(i32::MAX);
    let end = if parameters.len() > 2 { length_argument(parameters, 2)?.min(length) } else { length };
    if start < 0 || start > end || end > length {
        return Err(out_of_bounds(start, end, string.len()));
    }
    Ok(Value::Str(string[start as usize..end as usize].to_vec()))
}

fn trim_function(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    Ok(Value::Str(trim(&require_string(parameter(parameters, 0)?)?).to_vec()))
}

/// The functions of this module.
pub const FUNCTIONS: &[Function] = &[
    Function::new("STR_CONTAINS", &[VALUE, VALUE], contains),
    Function::new("STR_ENDS_WITH", &[VALUE, VALUE], ends_with),
    Function::new("STR_FORMAT", &[VALUE, VAR_ARG], format),
    Function::new("STR_LEFT", &[VALUE, VALUE], left),
    Function::new("STR_LENGTH", &[VALUE], length),
    Function::new("STR_LOWER", &[VALUE], lower),
    Function::new("STR_MATCHES", &[VALUE, VALUE], matches_regex),
    Function::new("STR_RIGHT", &[VALUE, VALUE], right),
    Function::new("STR_SPLIT", &[VALUE, VALUE], split),
    Function::new("STR_STARTS_WITH", &[VALUE, VALUE], starts_with),
    Function::validated(
        "STR_SUBSTRING",
        &[VALUE, NON_NEGATIVE, super::Parameter { var_arg: true, ..NON_NEGATIVE }],
        validate_substring,
        substring,
    ),
    Function::new("STR_TRIM", &[VALUE], trim_function),
    Function::new("STR_UPPER", &[VALUE], upper),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn split_of(text: &str, separator: &str) -> Vec<JString> {
        split_text(&js(text), &js(separator))
    }

    #[test]
    fn split_drops_trailing_empty_strings() {
        assert_eq!(split_of("a,b,,", ","), vec![js("a"), js("b")]);
        assert_eq!(split_of(",a", ","), vec![js(""), js("a")]);
        assert_eq!(split_of(",,", ","), Vec::<JString>::new());
        assert_eq!(split_of("", ","), vec![js("")]);
    }

    #[test]
    fn split_on_nothing_gives_the_code_units() {
        assert_eq!(split_of("abc", ""), vec![js("a"), js("b"), js("c")]);
    }
}
