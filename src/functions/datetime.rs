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

//! The date-time and duration functions.

use std::time::{SystemTime, UNIX_EPOCH};

use super::{Function, NON_NEGATIVE, VALUE, VAR_ARG};
use crate::error::{Failure, Outcome};
use crate::expression::Expression;
use crate::java::bigdecimal::BigDecimal;
use crate::java::chars::trim;
use crate::java::time::{
    parse_to_instant, time_zone_ids_contains, DateTimeFormatter, Duration, Instant, LocalDateTime, Unit, ZoneId,
    ZonedDateTime,
};
use crate::java::{js, JString};
use crate::token::Token;
use crate::value::Value;

/// `ZoneIdConverter.convert`.
fn convert_zone(token: &Token, zone: &[u16]) -> Outcome<ZoneId> {
    ZoneId::of(zone).map_err(|thrown| {
        let mut message = js("Unable to convert zone string '");
        message.extend_from_slice(&token.value);
        message.extend("' to a zone ID: ".encode_utf16());
        message.extend(thrown.message.unwrap_or_default());
        Failure::evaluation(token, &message)
    })
}

fn first(parameters: &[Value]) -> Outcome<&Value> {
    parameters.first().ok_or_else(Failure::null_pointer)
}

fn int_at(parameters: &[Value], index: usize, count: usize) -> Outcome<i32> {
    if index >= count {
        return Ok(0);
    }
    let value = parameters.get(index).ok_or_else(Failure::null_pointer)?;
    Ok(value.required_number()?.int_value())
}

fn validate_new(token: &Token, parameters: &[Value]) -> Outcome<()> {
    let mut count = parameters.len();
    if count == 0 {
        return Err(Failure::evaluation_str(token, "Not enough parameters for function"));
    }
    if count == 1 {
        return match first(parameters)? {
            Value::Number(_) => Ok(()),
            _ => Err(Failure::evaluation_str(
                token,
                "Expected a number value for the time in milliseconds since the epoch",
            )),
        };
    }
    if let Some(Value::Str(zone)) = parameters.last() {
        if !time_zone_ids_contains(zone) {
            let mut message = js("Time zone with id '");
            message.extend_from_slice(zone);
            message.extend("' not found".encode_utf16());
            return Err(Failure::evaluation(token, &message));
        }
        count -= 1;
    }
    if count < 3 {
        return Err(Failure::evaluation_str(token, "A minimum of 3 parameters (year, month, day) is required"));
    }
    if count > 7 {
        return Err(Failure::evaluation_str(token, "Too many parameters to function"));
    }
    Ok(())
}

fn date_new(expression: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    let mut count = parameters.len();
    if count == 1 {
        let millis = first(parameters)?.required_number()?;
        return Ok(Value::DateTime(Instant::of_epoch_milli(millis.long_value())));
    }
    let mut zone = expression.configuration().zone_id.clone();
    if let Some(Value::Str(name)) = parameters.last() {
        zone = convert_zone(token, name)?;
        count -= 1;
    }
    let date_time = LocalDateTime::of(
        int_at(parameters, 0, 3)?,
        int_at(parameters, 1, 3)?,
        int_at(parameters, 2, 3)?,
        int_at(parameters, 3, count)?,
        int_at(parameters, 4, count)?,
        int_at(parameters, 5, count)?,
        int_at(parameters, 6, count)?,
    )?;
    Ok(Value::DateTime(date_time.at_zone(&zone)?.to_instant()))
}

fn validate_formatted(token: &Token, parameters: &[Value]) -> Outcome<()> {
    if parameters.len() > 3 {
        return Err(Failure::evaluation_str(token, "Too many parameters"));
    }
    let value = first(parameters)?;
    if !matches!(value, Value::DateTime(_)) {
        let message = format!("Unable to format a '{}' type as a date-time", value.type_name());
        return Err(Failure::evaluation_str(token, &message));
    }
    Ok(())
}

fn date_format(expression: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    let configuration = expression.configuration();
    let formatter = match parameters.get(1) {
        Some(pattern) => {
            let Some(pattern) = pattern.string_value() else {
                return Err(Failure::null_pointer_message("pattern"));
            };
            DateTimeFormatter::of_pattern(&pattern, &configuration.locale)?
        }
        None => match configuration.date_time_formatters.first() {
            Some(formatter) => formatter.clone(),
            None => return Err(index_out_of_bounds()),
        },
    };
    let zone = match parameters.get(2) {
        Some(value) => zone_parameter(token, value)?,
        None => configuration.zone_id.clone(),
    };
    let moment = first(parameters)?.date_time_value().at_zone(&zone)?;
    Ok(Value::Str(formatter.format(&moment)?))
}

fn index_out_of_bounds() -> Failure {
    crate::java::Thrown::new("java.lang.IndexOutOfBoundsException", "Index 0 out of bounds for length 0").into()
}

fn zone_parameter(token: &Token, value: &Value) -> Outcome<ZoneId> {
    match value.string_value() {
        Some(name) => convert_zone(token, &name),
        None => Err(Failure::null_pointer_message("zoneId")),
    }
}

fn date_parse(expression: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    let configuration = expression.configuration();
    let value = first(parameters)?.string_value();
    let zone = match parameters.get(1) {
        Some(zone) if !zone.is_null() => zone_parameter(token, zone)?,
        _ => configuration.zone_id.clone(),
    };
    let formatters = if parameters.len() > 2 {
        parsed_formatters(token, parameters, &configuration.locale)?
    } else {
        configuration.date_time_formatters.clone()
    };
    let Some(value) = value else {
        return Err(Failure::null_pointer_message("text"));
    };
    for formatter in &formatters {
        match parse_to_instant(formatter, &value, &zone) {
            Ok(instant) => return Ok(Value::DateTime(instant)),
            Err(thrown) if thrown.class.starts_with("java.time.") => {}
            Err(thrown) => return Err(thrown.into()),
        }
    }
    let mut message = js("Unable to parse date-time string '");
    message.extend_from_slice(&value);
    message.push(u16::from(b'\''));
    Err(Failure::evaluation(token, &message))
}

fn parsed_formatters(
    token: &Token,
    parameters: &[Value],
    _locale: &crate::java::locale::Locale,
) -> Outcome<Vec<DateTimeFormatter>> {
    let mut formatters = Vec::new();
    for (index, parameter) in parameters.iter().enumerate().skip(2) {
        let Some(pattern) = parameter.string_value() else {
            return Err(Failure::null_pointer_message("pattern"));
        };
        let locale = crate::java::locale::Locale::US;
        let formatter = DateTimeFormatter::of_pattern(&pattern, &locale).map_err(|thrown| {
            if thrown.class == "java.lang.IllegalArgumentException" {
                let mut message = js(&format!("Illegal date-time format in parameter {}: '", index + 1));
                message.extend_from_slice(&pattern);
                message.push(u16::from(b'\''));
                Failure::evaluation(token, &message)
            } else {
                thrown.into()
            }
        })?;
        formatters.push(formatter);
    }
    Ok(formatters)
}

fn date_to_epoch(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let millis = first(parameters)?.date_time_value().to_epoch_milli()?;
    Ok(Value::Number(BigDecimal::from_i64(millis)))
}

/// The system clock; a clock set before 1970 reads as the epoch.
fn current_instant() -> Instant {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    Instant::of_epoch_milli(i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

fn now(_: &Expression, _: &Token, _: &[Value]) -> Outcome<Value> {
    Ok(Value::DateTime(current_instant()))
}

fn today(expression: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    let zone = match parameters.first() {
        Some(value) if !value.is_null() => zone_parameter(token, value)?,
        _ => expression.configuration().zone_id.clone(),
    };
    let moment = current_instant().at_zone(&zone)?.truncated_to(Unit::Days)?;
    Ok(Value::DateTime(moment.to_instant()))
}

fn truncated(moment: ZonedDateTime, unit: &[u16]) -> Option<crate::java::JResult<ZonedDateTime>> {
    Some(match String::from_utf16(unit).ok()?.as_str() {
        "y" => moment.truncated_to(Unit::Days).and_then(|day| day.with_first_day_of_year()),
        "M" => moment.truncated_to(Unit::Days).and_then(|day| day.with_first_day_of_month()),
        "d" => moment.truncated_to(Unit::Days),
        "H" => moment.truncated_to(Unit::Hours),
        "m" => moment.truncated_to(Unit::Minutes),
        "s" => moment.truncated_to(Unit::Seconds),
        _ => return None,
    })
}

fn truncate(expression: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    let instant = first(parameters)?.date_time_value();
    let unit: JString = match parameters.get(1) {
        Some(value) if !value.is_null() => trim(&value.string_value().unwrap_or_default()).to_vec(),
        _ => js("d"),
    };
    if !["y", "M", "d", "H", "m", "s"].iter().any(|known| js(known) == unit) {
        let mut message = js("Invalid time unit '");
        message.extend_from_slice(&unit);
        message.extend("'. Use standard Java format tokens: 'y', 'M', 'd', 'H', 'm', 's'.".encode_utf16());
        return Err(Failure::evaluation(token, &message));
    }
    let zone = match parameters.get(2) {
        Some(value) if !value.is_null() => {
            let name = trim(&value.string_value().unwrap_or_default()).to_vec();
            ZoneId::of(&name).map_err(|_| {
                let mut message = js("Time zone with id '");
                message.extend_from_slice(&name);
                message.extend("' not found".encode_utf16());
                Failure::evaluation(token, &message)
            })?
        }
        _ => expression.configuration().zone_id.clone(),
    };
    let moment = instant.at_zone(&zone)?;
    match truncated(moment, &unit) {
        Some(result) => Ok(Value::DateTime(result?.to_instant())),
        None => Err(Failure::unsupported_types(token)),
    }
}

fn duration_from_millis(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let millis = first(parameters)?.required_number()?.long_value();
    Ok(Value::Duration(Duration::of_millis(millis)))
}

fn duration_new(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let count = parameters.len();
    let value = |index| int_at(parameters, index, count).map(i64::from);
    let duration = Duration::of_days(value(0)?)?
        .plus_hours(value(1)?)?
        .plus_minutes(value(2)?)?
        .plus_seconds(value(3)?)?
        .plus_millis(value(4)?)?
        .plus_nanos(if count == 6 { value(5)? } else { 0 })?;
    Ok(Value::Duration(duration))
}

fn duration_parse(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let Some(text) = first(parameters)?.string_value() else {
        return Err(Failure::null_pointer_message("text"));
    };
    Ok(Value::Duration(Duration::parse(&text)?))
}

fn duration_to_millis(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let millis = first(parameters)?.duration_value().to_millis()?;
    Ok(Value::Number(BigDecimal::from_i64(millis)))
}

const NON_NEGATIVE_VAR_ARG: super::Parameter = super::Parameter { var_arg: true, ..NON_NEGATIVE };

/// The functions of this module.
pub const FUNCTIONS: &[Function] = &[
    Function::validated("DT_DATE_NEW", &[NON_NEGATIVE_VAR_ARG], validate_new, date_new),
    Function::new("DT_DATE_PARSE", &[VALUE, VAR_ARG], date_parse),
    Function::validated("DT_DATE_FORMAT", &[VALUE, VAR_ARG], validate_formatted, date_format),
    Function::new("DT_DATE_TO_EPOCH", &[VALUE], date_to_epoch),
    Function::new("DT_DURATION_NEW", &[VALUE, VAR_ARG], duration_new),
    Function::new("DT_DURATION_FROM_MILLIS", &[VALUE], duration_from_millis),
    Function::new("DT_DURATION_TO_MILLIS", &[VALUE], duration_to_millis),
    Function::new("DT_DURATION_PARSE", &[VALUE], duration_parse),
    Function::new("DT_NOW", &[], now),
    Function::new("DT_TODAY", &[VAR_ARG], today),
    Function::validated("DT_TRUNCATE", &[VALUE, VAR_ARG], validate_formatted, truncate),
];
