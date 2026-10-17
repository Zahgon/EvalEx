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

//! The basic functions.

use std::time::{SystemTime, UNIX_EPOCH};

use super::{Function, LAZY, NON_NEGATIVE, NON_ZERO, VALUE, VAR_ARG};
use crate::error::{Failure, Outcome};
use crate::expression::Expression;
use crate::java::bigdecimal::{BigDecimal, MathContext, RoundingMode};
use crate::java::biginteger::BigInteger;
use crate::java::chars::trim;
use crate::java::decimalformat;
use crate::java::fdlibm;
use crate::java::locale::Locale;
use crate::java::{js, JString};
use crate::operators::require_boolean;
use crate::token::Token;
use crate::value::{convert_double, Value};

fn first(parameters: &[Value]) -> Outcome<&Value> {
    parameters.first().ok_or_else(Failure::null_pointer)
}

fn abs(expression: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let number = first(parameters)?.required_number()?;
    Ok(Value::Number(number.abs_mc(expression.math_context())?))
}

/// The numbers of the parameters, descending into arrays; `None` stands for null.
fn flatten(parameters: &[Value], out: &mut Vec<Option<BigDecimal>>) {
    for parameter in parameters {
        if let Value::Array(items) = parameter {
            flatten(items, out);
        } else {
            out.push(parameter.number_value());
        }
    }
}

fn average(expression: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let mut numbers = Vec::new();
    flatten(parameters, &mut numbers);
    let mut sum = BigDecimal::zero();
    let mut count = BigDecimal::zero();
    for number in numbers {
        // SumAndCount.plus adds the null of a null parameter to the running sum.
        let number = number.ok_or_else(|| {
            Failure::null_pointer_message("Cannot read field \"intCompact\" because \"augend\" is null")
        })?;
        sum = sum.add(&number)?;
        count = count.add(&BigDecimal::one())?;
    }
    Ok(Value::Number(sum.divide_mc(&count, expression.math_context())?))
}

fn ceiling(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let number = first(parameters)?.required_number()?;
    Ok(Value::Number(number.set_scale(0, RoundingMode::Ceiling)?))
}

fn floor(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let number = first(parameters)?.required_number()?;
    Ok(Value::Number(number.set_scale(0, RoundingMode::Floor)?))
}

fn first_non_null(parameters: &[Value]) -> Option<Value> {
    parameters.iter().find_map(|parameter| match parameter {
        Value::Array(items) => items.iter().find(|item| !item.is_null()).cloned(),
        value if !value.is_null() => Some(value.clone()),
        _ => None,
    })
}

fn coalesce(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    Ok(first_non_null(parameters).unwrap_or(Value::Null))
}

fn fact(expression: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    let number = first(parameters)?.required_number()?.int_value();
    let limit = expression.configuration().max_recursion_depth;
    if number > limit {
        let message = format!("The required number exceeds the configured 'maxRecursionDepth' (value: {limit})");
        return Err(Failure::evaluation_str(token, &message));
    }
    let context = expression.math_context();
    let mut factorial = BigDecimal::one();
    for i in 1..=number {
        factorial = factorial.multiply_mc(&BigDecimal::from_i32_mc(i, context)?, context)?;
    }
    Ok(Value::Number(factorial))
}

fn if_function(expression: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let index = if first(parameters)?.boolean_value() == Some(true) { 1 } else { 2 };
    evaluate_lazy(expression, parameters.get(index).unwrap_or(&Value::Null))
}

fn evaluate_lazy(expression: &Expression, parameter: &Value) -> Outcome<Value> {
    match parameter {
        Value::ExpressionNode(node) => expression.evaluate_subtree(node, 0),
        value => Ok(value.clone()),
    }
}

fn log10(expression: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let x = first(parameters)?.required_number()?.double_value();
    convert_double(fdlibm::log10(x), expression.math_context())
}

fn log(expression: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let x = first(parameters)?.required_number()?.double_value();
    convert_double(fdlibm::log(x), expression.math_context())
}

fn find_extreme(parameters: &[Value], find_min: bool) -> Outcome<Value> {
    let mut numbers = Vec::new();
    flatten(parameters, &mut numbers);
    let mut current: Option<BigDecimal> = None;
    for number in numbers {
        let replace = match (&current, &number) {
            (None, _) => true,
            (Some(_), None) => {
                return Err(Failure::null_pointer_message(
                    "Cannot invoke \"java.math.BigDecimal.compareTo(java.math.BigDecimal)\" because \"newValue\" is null",
                ));
            }
            (Some(old), Some(new)) => {
                let order = new.compare_to(old);
                if find_min {
                    order.is_lt()
                } else {
                    order.is_gt()
                }
            }
        };
        if replace {
            current = number;
        }
    }
    Ok(current.map_or(Value::Null, Value::Number))
}

fn max(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    find_extreme(parameters, false)
}

fn min(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    find_extreme(parameters, true)
}

fn not(_: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    Ok(Value::Boolean(!require_boolean(first(parameters)?)?))
}

fn number_parse_locale(expression: &Expression, parameters: &[Value]) -> Locale {
    match parameters.get(2) {
        Some(value) if !value.is_null() => {
            let tag = value.string_value().unwrap_or_default();
            Locale::for_language_tag(trim(&tag))
        }
        _ => expression.configuration().locale,
    }
}

fn quoted_message(prefix: &str, text: &[u16], suffix: &str) -> JString {
    let mut message = js(prefix);
    message.extend_from_slice(text);
    message.extend(suffix.encode_utf16());
    message
}

fn number_parse(expression: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    let string = first(parameters)?;
    if string.is_null() {
        return Err(Failure::evaluation_str(token, "The input string cannot be null"));
    }
    let text = string.string_value().unwrap_or_default();
    let sanitized = trim(&text).to_vec();
    let context = expression.math_context();
    let locale = number_parse_locale(expression, parameters);
    let number = match parameters.get(1) {
        Some(value) if !value.is_null() => {
            let pattern = value.string_value().unwrap_or_default();
            parse_with_pattern(context, token, &sanitized, trim(&pattern), &locale)?
        }
        _ => BigDecimal::parse_mc(&sanitized, context).map_err(|thrown| {
            if thrown.class == "java.lang.NumberFormatException" {
                let message = quoted_message("Value '", &sanitized, "' cannot be safely parsed into a valid number.");
                Failure::evaluation(token, &message)
            } else {
                thrown.into()
            }
        })?,
    };
    Ok(Value::Number(number))
}

fn parse_with_pattern(
    context: &MathContext,
    token: &Token,
    text: &[u16],
    pattern: &[u16],
    locale: &Locale,
) -> Outcome<BigDecimal> {
    let format = decimalformat::DecimalFormat::new(pattern, locale)
        .map_err(|_| Failure::evaluation(token, &quoted_message("Invalid number format in pattern '", pattern, "'")))?;
    match format.parse(text) {
        decimalformat::Parsed::Number(number) => Ok(number.round(context)?),
        decimalformat::Parsed::NotFinite => Err(Failure::evaluation(
            token,
            &quoted_message("Value '", text, "' cannot be safely parsed into a valid number"),
        )),
        decimalformat::Parsed::NoMatch => Err(Failure::evaluation(
            token,
            &quoted_message("Value '", text, "' does not match the specified format pattern."),
        )),
        decimalformat::Parsed::Failed(thrown) if thrown.class == "java.lang.NumberFormatException" => {
            Err(Failure::evaluation(
                token,
                &quoted_message("Value '", text, "' cannot be safely parsed into a valid number."),
            ))
        }
        decimalformat::Parsed::Failed(thrown) => Err(thrown.into()),
    }
}

fn validate_number_parse(token: &Token, parameters: &[Value]) -> Outcome<()> {
    if parameters.len() > 3 {
        return Err(Failure::evaluation_str(token, "Too many parameters"));
    }
    Ok(())
}

fn random(expression: &Expression, _: &Token, _: &[Value]) -> Outcome<Value> {
    // A fraction in [0, 1) from the clock; a clock set before 1970 has no fraction to offer and gives zero.
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.subsec_nanos());
    convert_double(f64::from(nanos) / 1e9, expression.math_context())
}

fn round(expression: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let number = first(parameters)?.required_number()?;
    let scale = parameters.get(1).ok_or_else(Failure::null_pointer)?.required_number()?;
    let mode = expression.math_context().rounding;
    Ok(Value::Number(number.set_scale(scale.int_value(), mode)?))
}

fn sqrt(expression: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let x = first(parameters)?.required_number()?;
    let context = expression.math_context();
    if x.signum() == 0 {
        return Ok(Value::Number(BigDecimal::zero()));
    }
    let n = x.move_point_right(context.precision.wrapping_shl(1))?.to_big_integer()?;
    let bits = (n.bit_length() + 1) >> 1;
    let mut ix = n.shift_right(bits)?;
    let one = BigInteger::from_i64(1);
    loop {
        let previous = ix.clone();
        let (quotient, _) = n.divide_and_remainder(&ix)?;
        ix = ix.add(&quotient)?.shift_right(1)?;
        let test = ix.subtract(&previous)?.abs();
        if test.is_zero() || test.compare_to(&one).is_eq() {
            break;
        }
    }
    Ok(Value::Number(BigDecimal::from_big_integer(&ix, context.precision)))
}

fn sum(expression: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let context = expression.math_context();
    let mut total = BigDecimal::zero();
    for parameter in parameters {
        total = total.add_mc(&recursive_sum(parameter, context)?, context)?;
    }
    Ok(Value::Number(total))
}

fn recursive_sum(parameter: &Value, context: &MathContext) -> Outcome<BigDecimal> {
    let mut total = BigDecimal::zero();
    if let Value::Array(items) = parameter {
        for item in items.iter() {
            total = total.add_mc(&recursive_sum(item, context)?, context)?;
        }
    } else {
        let number = parameter.number_value().ok_or_else(|| {
            Failure::null_pointer_message("Cannot invoke \"java.math.BigDecimal.signum()\" because \"augend\" is null")
        })?;
        total = total.add_mc(&number, context)?;
    }
    Ok(total)
}

fn switch(expression: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let value = first(parameters)?;
    let mut result = Value::Null;
    let mut index = 1;
    while let Some(candidate) = parameters.get(index) {
        match parameters.get(index + 1) {
            Some(next) => {
                let other = evaluate_lazy(expression, candidate)?;
                index += 2;
                if (value.is_null() && other.is_null())
                    || (value.same_type(&other) && value.compare_to(&other)?.is_eq())
                {
                    result = next.clone();
                    break;
                }
            }
            None => {
                result = candidate.clone();
                index += 1;
            }
        }
    }
    evaluate_lazy(expression, &result)
}

/// The functions of this module.
pub const FUNCTIONS: &[Function] = &[
    Function::new("ABS", &[VALUE], abs),
    Function::new("AVERAGE", &[VALUE, VAR_ARG], average),
    Function::new("CEILING", &[VALUE], ceiling),
    Function::new("COALESCE", &[VAR_ARG], coalesce),
    Function::new("FACT", &[VALUE], fact),
    Function::new("FLOOR", &[VALUE], floor),
    Function::new("IF", &[VALUE, LAZY, LAZY], if_function),
    Function::new("LOG", &[NON_ZERO_NON_NEGATIVE], log),
    Function::new("LOG10", &[NON_ZERO_NON_NEGATIVE], log10),
    Function::new("MAX", &[VALUE, VAR_ARG], max),
    Function::new("MIN", &[VALUE, VAR_ARG], min),
    Function::new("NOT", &[VALUE], not),
    Function::validated("NUMBER_PARSE", &[VALUE, VAR_ARG], validate_number_parse, number_parse),
    Function::new("RANDOM", &[], random),
    Function::new("ROUND", &[VALUE, VALUE], round),
    Function::new("SQRT", &[NON_NEGATIVE], sqrt),
    Function::new("SUM", &[VAR_ARG], sum),
    Function::new("SWITCH", &[VALUE, VALUE, LAZY, super::Parameter { var_arg: true, lazy: true, ..VALUE }], switch),
];

const NON_ZERO_NON_NEGATIVE: super::Parameter = super::Parameter { non_negative: true, ..NON_ZERO };
