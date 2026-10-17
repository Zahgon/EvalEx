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

//! The `evalex` command: JSON-lines requests on standard input, one response line each.

pub mod json;

use std::rc::Rc;

use crate::config::{Configuration, DEFAULT_PRECISION, DEFAULT_ROUNDING};
use crate::error::{Failure, Outcome};
use crate::expression::Expression;
use crate::java::bigdecimal::{BigDecimal, MathContext, RoundingMode};
use crate::java::biginteger::BigInteger;
use crate::java::double::{self, parse_double, parse_float};
use crate::java::hashmap::HashMap;
use crate::java::integer;
use crate::java::locale::Locale;
use crate::java::time::{DateTimeFormatter, Duration, Instant, ZoneId};
use crate::java::{js, JString, Thrown};
use crate::value::Value;
use json::{request_error, write_string, Json};

/// The usage text.
pub const USAGE: &str = "usage: evalex < requests.jsonl\n\nReads one JSON request per line on standard input and writes one JSON response per line.\nRequest: {\"expression\":S,\"op\":\"evaluate|validate|variables|ast\",\"variables\":{...},\"config\":{...}}\n";

fn int_option(value: &Json, name: &str) -> Outcome<i32> {
    match value {
        Json::Number(number) => Ok(number.int_value_exact()?),
        _ => Err(request_error(&format!("{name} must be a number")).into()),
    }
}

fn boolean_option(value: &Json, name: &str) -> Outcome<bool> {
    match value {
        Json::Boolean(flag) => Ok(*flag),
        _ => Err(request_error(&format!("{name} must be a boolean")).into()),
    }
}

fn string_option<'a>(value: &'a Json, name: &str) -> Outcome<&'a JString> {
    match value {
        Json::Text(text) => Ok(text),
        _ => Err(request_error(&format!("{name} must be a string")).into()),
    }
}

fn rounding_mode(name: &[u16]) -> Outcome<RoundingMode> {
    RoundingMode::from_name(name).ok_or_else(|| {
        let mut message = js("No enum constant java.math.RoundingMode.");
        message.extend_from_slice(name);
        Thrown::with_message("java.lang.IllegalArgumentException", message).into()
    })
}

fn formatter(name: &[u16], locale: &Locale) -> Outcome<DateTimeFormatter> {
    Ok(match String::from_utf16_lossy(name).as_str() {
        "ISO_DATE_TIME" => DateTimeFormatter::iso_date_time(),
        "ISO_DATE" => DateTimeFormatter::iso_date(),
        "ISO_LOCAL_DATE_TIME" => DateTimeFormatter::iso_local_date_time(),
        "ISO_LOCAL_DATE" => DateTimeFormatter::iso_local_date(),
        "RFC_1123_DATE_TIME" => DateTimeFormatter::rfc_1123_date_time(),
        _ => DateTimeFormatter::of_pattern(name, locale)?,
    })
}

fn configuration(spec: Option<&Json>) -> Outcome<Configuration> {
    let mut config = Configuration::with_defaults(ZoneId::of(&js("UTC"))?, Locale::US)?;
    let spec = match spec {
        None | Some(Json::Null) => return Ok(config),
        Some(spec @ Json::Object(_)) => spec,
        Some(_) => return Err(request_error("config must be an object").into()),
    };
    let precision = spec.member("precision");
    let mode = spec.member("roundingMode");
    if precision.is_some() || mode.is_some() {
        let digits = precision.map_or(Ok(DEFAULT_PRECISION), |value| int_option(value, "precision"))?;
        let rounding = match mode {
            Some(value) => rounding_mode(string_option(value, "roundingMode")?)?,
            None => DEFAULT_ROUNDING,
        };
        config.math_context = MathContext::new(digits, rounding)?;
    }
    if let Some(value) = spec.member("decimalPlacesRounding") {
        config.decimal_places_rounding = int_option(value, "decimalPlacesRounding")?;
    }
    if let Some(value) = spec.member("decimalPlacesResult") {
        config.decimal_places_result = int_option(value, "decimalPlacesResult")?;
    }
    if let Some(value) = spec.member("powerOfPrecedence") {
        config.power_of_precedence = int_option(value, "powerOfPrecedence")?;
    }
    if let Some(value) = spec.member("maxRecursionDepth") {
        config.max_recursion_depth = int_option(value, "maxRecursionDepth")?;
    }
    if let Some(value) = spec.member("stripTrailingZeros") {
        config.strip_trailing_zeros = boolean_option(value, "stripTrailingZeros")?;
    }
    if let Some(value) = spec.member("implicitMultiplication") {
        config.implicit_multiplication_allowed = boolean_option(value, "implicitMultiplication")?;
    }
    if let Some(value) = spec.member("arrays") {
        config.arrays_allowed = boolean_option(value, "arrays")?;
    }
    if let Some(value) = spec.member("structures") {
        config.structures_allowed = boolean_option(value, "structures")?;
    }
    if let Some(value) = spec.member("singleQuoteStrings") {
        config.single_quote_string_literals_allowed = boolean_option(value, "singleQuoteStrings")?;
    }
    if let Some(value) = spec.member("lenient") {
        config.lenient_mode = boolean_option(value, "lenient")?;
    }
    if let Some(value) = spec.member("allowOverwriteConstants") {
        config.allow_overwrite_constants = boolean_option(value, "allowOverwriteConstants")?;
    }
    if let Some(value) = spec.member("locale") {
        config.locale = Locale::for_language_tag(string_option(value, "locale")?);
    }
    if let Some(value) = spec.member("zone") {
        config.zone_id = ZoneId::of(string_option(value, "zone")?)?;
    }
    if let Some(value) = spec.member("dateTimeFormats") {
        let Json::Array(formats) = value else {
            return Err(request_error("dateTimeFormats must be an array").into());
        };
        let mut formatters = Vec::with_capacity(formats.len());
        for format in formats {
            let Json::Text(name) = format else {
                return Err(request_error("dateTimeFormats must hold strings").into());
            };
            formatters.push(formatter(name, &config.locale)?);
        }
        config.date_time_formatters = formatters;
    }
    Ok(config)
}

/// The value of one of the typed forms, or `None` for any other member name.
fn typed_form(expression: &Expression, kind: &str, text: &[u16]) -> Outcome<Option<Value>> {
    let context = expression.math_context();
    let int = |value: i64| Value::Number(BigDecimal::from_i64(value));
    Ok(Some(match kind {
        "$double" => crate::value::convert_double(parse_double(text)?, context)?,
        "$float" => {
            let widened = f64::from(parse_float(text)?);
            Value::Number(BigDecimal::parse(&js(&double::to_string(widened)))?)
        }
        "$int" => int(integer::parse_int(text)?.into()),
        "$long" => int(integer::parse_long(text)?),
        "$short" => int(integer::parse_short(text)?.into()),
        "$byte" => int(integer::parse_byte(text)?.into()),
        "$bigint" => Value::Number(BigDecimal::from_big_integer_mc(&BigInteger::parse(text, 10)?, context)?),
        "$char" => match text.first() {
            Some(&unit) => Value::Str(vec![unit]),
            None => {
                return Err(Thrown::new(
                    "java.lang.StringIndexOutOfBoundsException",
                    "Index 0 out of bounds for length 0",
                )
                .into())
            }
        },
        "$datetime" => Value::DateTime(Instant::parse(text)?),
        "$duration" => Value::Duration(Duration::parse(text)?),
        "$expression" => Value::ExpressionNode(expression.create_expression_node(text)?),
        _ => return Ok(None),
    }))
}

/// Converts a request value to the expression value `Expression.with` makes of it.
fn value_of(expression: &Expression, json: &Json) -> Outcome<Value> {
    Ok(match json {
        Json::Null => Value::Null,
        Json::Boolean(flag) => Value::Boolean(*flag),
        Json::Number(number) => Value::Number(number.clone()),
        Json::Text(text) => Value::Str(text.clone()),
        Json::Array(items) => {
            let items = items.iter().map(|item| value_of(expression, item)).collect::<Outcome<Vec<_>>>()?;
            Value::Array(Rc::new(items))
        }
        Json::Object(members) => {
            if let [(kind, Json::Text(text))] = members.as_slice() {
                if kind.first() == Some(&u16::from(b'$')) {
                    let kind = String::from_utf16_lossy(kind);
                    if let Some(value) = typed_form(expression, &kind, text)? {
                        return Ok(value);
                    }
                }
            }
            let mut map = HashMap::new();
            for (name, member) in members {
                map.put(name.clone(), value_of(expression, member)?);
            }
            Value::Structure(Rc::new(map))
        }
    })
}

/// Writes the RESULT object of a value.
fn write_result(value: &Value, out: &mut String) -> Outcome<()> {
    out.push_str("{\"type\":\"");
    out.push_str(value.type_name());
    out.push_str("\",\"text\":");
    if let Value::Number(number) = value {
        number.to_plain_string()?;
    }
    write_string(value.string_value().as_deref(), out);
    match value {
        Value::Number(number) => {
            write_field(out, "string", &number.to_string());
            write_field(out, "plain", &number.to_plain_string()?);
            write_field(out, "engineering", &number.to_engineering_string());
            out.push_str(&format!(",\"scale\":{},\"precision\":{}", number.scale(), number.precision()));
        }
        Value::Str(text) => {
            out.push_str(",\"value\":");
            write_string(Some(text), out);
        }
        Value::DateTime(instant) => write_field(out, "value", &instant.to_string()),
        Value::Duration(duration) => write_field(out, "value", &duration.to_string()),
        Value::Boolean(flag) => out.push_str(&format!(",\"value\":{flag}")),
        Value::Array(items) => {
            out.push_str(",\"items\":[");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_result(item, out)?;
            }
            out.push(']');
        }
        Value::Structure(members) => {
            out.push_str(",\"members\":[");
            for (index, (name, member)) in members.iter().into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push('[');
                write_string(Some(name), out);
                out.push(',');
                write_result(member, out)?;
                out.push(']');
            }
            out.push(']');
        }
        Value::ExpressionNode(_) | Value::Null | Value::Undefined => {}
    }
    out.push('}');
    Ok(())
}

fn write_field(out: &mut String, name: &str, text: &str) {
    out.push_str(&format!(",\"{name}\":"));
    write_string(Some(&js(text)), out);
}

fn write_names(names: &[JString], out: &mut String) {
    out.push('[');
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_string(Some(name), out);
    }
    out.push(']');
}

fn handle(request: &Json, out: &mut String) -> Outcome<()> {
    if !matches!(request, Json::Object(_)) {
        return Err(request_error("a request must be a JSON object").into());
    }
    let Some(Json::Text(text)) = request.member("expression") else {
        return Err(request_error("expression must be a string").into());
    };
    let mut expression = Expression::new(text.clone(), configuration(request.member("config"))?)?;
    match request.member("variables") {
        None | Some(Json::Null) => {}
        Some(Json::Object(variables)) => {
            for (name, value) in variables {
                let value = value_of(&expression, value)?;
                expression.with(name, value)?;
            }
        }
        Some(_) => return Err(request_error("variables must be an object").into()),
    }
    let op = match request.member("op") {
        None => Some("evaluate".to_string()),
        Some(Json::Text(op)) => Some(String::from_utf16_lossy(op)),
        Some(_) => None,
    };
    match op.as_deref() {
        Some("evaluate") => write_result(&expression.evaluate()?, out)?,
        Some("validate") => {
            expression.abstract_syntax_tree()?;
            out.push_str("{\"valid\":true}");
        }
        Some("variables") => {
            out.push_str("{\"used\":");
            write_names(&expression.used_variables()?, out);
            out.push_str(",\"undefined\":");
            write_names(&expression.undefined_variables()?, out);
            out.push('}');
        }
        Some("ast") => {
            out.push_str("{\"ast\":");
            write_string(Some(&expression.abstract_syntax_tree()?.to_json()), out);
            out.push('}');
        }
        _ => return Err(request_error("unknown op").into()),
    }
    Ok(())
}

fn write_error(failure: &Failure, out: &mut String) {
    out.clear();
    out.push_str("{\"error\":{\"class\":");
    write_string(Some(&js(&failure.thrown.class)), out);
    out.push_str(",\"message\":");
    write_string(failure.thrown.message.as_deref(), out);
    if let Some(span) = &failure.span {
        out.push_str(&format!(",\"start\":{},\"end\":{},\"token\":", span.start, span.end));
        write_string(Some(&span.token), out);
    }
    out.push_str("}}");
}

/// Answers all requests of `input`; returns the response text and whether any request failed.
pub fn run(input: &str) -> (String, bool) {
    let mut output = String::new();
    let mut failed = false;
    for line in input.split('\n').filter(|line| !line.is_empty()) {
        let units = js(line);
        let mut out = String::new();
        let outcome = json::parse(&units).map_err(Failure::from).and_then(|request| handle(&request, &mut out));
        if let Err(failure) = outcome {
            failed = true;
            write_error(&failure, &mut out);
        }
        output.push_str(&out);
        output.push('\n');
    }
    (output, failed)
}
