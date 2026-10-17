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

//! Trigonometric and hyperbolic functions, computed in `double` as the library does.

use std::cmp::Ordering;

use super::{Function, NON_ZERO, VALUE};
use crate::error::{Failure, Outcome};
use crate::expression::Expression;
use crate::java::bigdecimal::BigDecimal;
use crate::java::double::{to_degrees, to_radians};
use crate::java::fdlibm::{acos, asin, atan, atan2, cos, cosh, log, pow, sin, sinh, tan, tanh};
use crate::java::{js, JString};
use crate::token::Token;
use crate::value::{convert_double, Value};

const FRAC_PI_2: f64 = std::f64::consts::PI / 2.0;

fn argument(parameters: &[Value], index: usize) -> Outcome<f64> {
    match parameters.get(index) {
        Some(value) => Ok(value.required_number()?.double_value()),
        None => Err(Failure::null_pointer()),
    }
}

fn convert(expression: &Expression, value: f64) -> Outcome<Value> {
    convert_double(value, expression.math_context())
}

macro_rules! unary {
    ($name:ident, |$x:ident| $body:expr) => {
        fn $name(expression: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
            let $x = argument(parameters, 0)?;
            convert(expression, $body)
        }
    };
}

unary!(sin_degrees, |x| sin(to_radians(x)));
unary!(cos_degrees, |x| cos(to_radians(x)));
unary!(tan_degrees, |x| tan(to_radians(x)));
unary!(cot_degrees, |x| 1.0 / tan(to_radians(x)));
unary!(sec_degrees, |x| 1.0 / cos(to_radians(x)));
unary!(csc_degrees, |x| 1.0 / sin(to_radians(x)));
unary!(sin_radians, |x| sin(x));
unary!(cos_radians, |x| cos(x));
unary!(tan_radians, |x| tan(x));
unary!(cot_radians, |x| 1.0 / tan(x));
unary!(sec_radians, |x| 1.0 / cos(x));
unary!(csc_radians, |x| 1.0 / sin(x));
unary!(atan_degrees, |x| to_degrees(atan(x)));
unary!(atan_radians, |x| atan(x));
unary!(acot_degrees, |x| to_degrees(FRAC_PI_2 - atan(x)));
unary!(acot_radians, |x| FRAC_PI_2 - atan(x));
unary!(sinh_value, |x| sinh(x));
unary!(cosh_value, |x| cosh(x));
unary!(tanh_value, |x| tanh(x));
unary!(coth_value, |x| 1.0 / tanh(x));
unary!(sech_value, |x| 1.0 / cosh(x));
unary!(csch_value, |x| 1.0 / sinh(x));
unary!(asinh_value, |x| log(x + (pow(x, 2.0) + 1.0).sqrt()));
unary!(acoth_value, |x| log((x + 1.0) / (x - 1.0)) * 0.5);
unary!(degrees, |x| to_degrees(x));
unary!(radians, |x| to_radians(x));

fn acosh_value(expression: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    let x = argument(parameters, 0)?;
    if x.total_cmp(&1.0) == Ordering::Less {
        return Err(Failure::evaluation_str(token, "Value must be greater or equal to one"));
    }
    convert(expression, log(x + (pow(x, 2.0) - 1.0).sqrt()))
}

fn atanh_value(expression: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    let x = argument(parameters, 0)?;
    if x.abs() >= 1.0 {
        return Err(Failure::evaluation_str(token, "Absolute value must be less than 1"));
    }
    convert(expression, 0.5 * log((1.0 + x) / (1.0 - x)))
}

/// The argument of an inverse sine or cosine, checked for the range -1 to 1.
fn unit_argument(token: &Token, parameters: &[Value], name: &str) -> Outcome<f64> {
    let Some(value) = parameters.first() else {
        return Err(Failure::null_pointer());
    };
    let number = value.required_number()?;
    let illegal = |bound: &str| -> Failure {
        let mut message: JString = js(&format!("Illegal {name}(x) for x {bound}: x = "));
        message.extend(number.to_string().encode_utf16());
        Failure::evaluation(token, &message)
    };
    if number.compare_to(&BigDecimal::one()).is_gt() {
        return Err(illegal("> 1"));
    }
    if number.compare_to(&BigDecimal::from_i64(-1)).is_lt() {
        return Err(illegal("< -1"));
    }
    Ok(number.double_value())
}

fn asin_degrees(expression: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    convert(expression, to_degrees(asin(unit_argument(token, parameters, "asin")?)))
}

fn asin_radians(expression: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    convert(expression, asin(unit_argument(token, parameters, "asinr")?))
}

fn acos_degrees(expression: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    convert(expression, to_degrees(acos(unit_argument(token, parameters, "acos")?)))
}

fn acos_radians(expression: &Expression, token: &Token, parameters: &[Value]) -> Outcome<Value> {
    convert(expression, acos(unit_argument(token, parameters, "acosr")?))
}

fn atan2_degrees(expression: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let y = argument(parameters, 0)?;
    let x = argument(parameters, 1)?;
    convert(expression, to_degrees(atan2(y, x)))
}

fn atan2_radians(expression: &Expression, _: &Token, parameters: &[Value]) -> Outcome<Value> {
    let y = argument(parameters, 0)?;
    let x = argument(parameters, 1)?;
    convert(expression, atan2(y, x))
}

/// The functions of this module.
pub const FUNCTIONS: &[Function] = &[
    Function::new("ACOS", &[VALUE], acos_degrees),
    Function::new("ACOSH", &[VALUE], acosh_value),
    Function::new("ACOSR", &[VALUE], acos_radians),
    Function::new("ACOT", &[NON_ZERO], acot_degrees),
    Function::new("ACOTH", &[VALUE], acoth_value),
    Function::new("ACOTR", &[NON_ZERO], acot_radians),
    Function::new("ASIN", &[VALUE], asin_degrees),
    Function::new("ASINH", &[VALUE], asinh_value),
    Function::new("ASINR", &[VALUE], asin_radians),
    Function::new("ATAN", &[VALUE], atan_degrees),
    Function::new("ATAN2", &[VALUE, VALUE], atan2_degrees),
    Function::new("ATAN2R", &[VALUE, VALUE], atan2_radians),
    Function::new("ATANH", &[VALUE], atanh_value),
    Function::new("ATANR", &[VALUE], atan_radians),
    Function::new("COS", &[VALUE], cos_degrees),
    Function::new("COSH", &[VALUE], cosh_value),
    Function::new("COSR", &[VALUE], cos_radians),
    Function::new("COT", &[NON_ZERO], cot_degrees),
    Function::new("COTH", &[NON_ZERO], coth_value),
    Function::new("COTR", &[NON_ZERO], cot_radians),
    Function::new("CSC", &[NON_ZERO], csc_degrees),
    Function::new("CSCH", &[NON_ZERO], csch_value),
    Function::new("CSCR", &[NON_ZERO], csc_radians),
    Function::new("DEG", &[VALUE], degrees),
    Function::new("RAD", &[VALUE], radians),
    Function::new("SEC", &[NON_ZERO], sec_degrees),
    Function::new("SECH", &[NON_ZERO], sech_value),
    Function::new("SECR", &[NON_ZERO], sec_radians),
    Function::new("SIN", &[VALUE], sin_degrees),
    Function::new("SINH", &[VALUE], sinh_value),
    Function::new("SINR", &[VALUE], sin_radians),
    Function::new("TAN", &[VALUE], tan_degrees),
    Function::new("TANH", &[VALUE], tanh_value),
    Function::new("TANR", &[VALUE], tan_radians),
];
