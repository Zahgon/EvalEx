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

//! The operators of the expression language.

use std::cmp::Ordering;

use crate::config::{
    Configuration, PRECEDENCE_ADDITIVE, PRECEDENCE_AND, PRECEDENCE_COMPARISON, PRECEDENCE_EQUALITY,
    PRECEDENCE_MULTIPLICATIVE, PRECEDENCE_OR, PRECEDENCE_POWER, PRECEDENCE_UNARY,
};
use crate::error::{Failure, Outcome};
use crate::expression::Expression;
use crate::java::bigdecimal::{BigDecimal, RoundingMode};
use crate::java::fdlibm;
use crate::java::time::Duration;
use crate::token::Token;
use crate::value::Value;

/// What an operator computes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperatorKind {
    /// Binary `+`.
    Plus,
    /// Binary `-`.
    Minus,
    /// `*`
    Multiply,
    /// `/`
    Divide,
    /// `%`
    Modulo,
    /// `^`
    Power,
    /// `=` and `==`.
    Equals,
    /// `!=` and `<>`.
    NotEquals,
    /// `>`
    Greater,
    /// `>=`
    GreaterEquals,
    /// `<`
    Less,
    /// `<=`
    LessEquals,
    /// `&&`
    And,
    /// `||`
    Or,
    /// Prefix `-`.
    Negate,
    /// Prefix `+`.
    Identity,
    /// Prefix `!`.
    Not,
}

/// The definition of an operator.
#[derive(Debug)]
pub struct Operator {
    /// The operation.
    pub kind: OperatorKind,
    /// Binding strength.
    pub precedence: i32,
    /// Whether operators of equal precedence group to the left.
    pub left_associative: bool,
    /// Whether the operands reach the operator unevaluated.
    pub operands_lazy: bool,
}

const fn operator(kind: OperatorKind, precedence: i32) -> Operator {
    Operator { kind, precedence, left_associative: true, operands_lazy: false }
}

const fn lazy(kind: OperatorKind, precedence: i32) -> Operator {
    Operator { kind, precedence, left_associative: true, operands_lazy: true }
}

const fn right_associative(kind: OperatorKind, precedence: i32) -> Operator {
    Operator { kind, precedence, left_associative: false, operands_lazy: false }
}

static PLUS: Operator = operator(OperatorKind::Plus, PRECEDENCE_ADDITIVE);
static MINUS: Operator = operator(OperatorKind::Minus, PRECEDENCE_ADDITIVE);
static MULTIPLY: Operator = operator(OperatorKind::Multiply, PRECEDENCE_MULTIPLICATIVE);
static DIVIDE: Operator = operator(OperatorKind::Divide, PRECEDENCE_MULTIPLICATIVE);
static MODULO: Operator = operator(OperatorKind::Modulo, PRECEDENCE_MULTIPLICATIVE);
static POWER: Operator = right_associative(OperatorKind::Power, PRECEDENCE_POWER);
static EQUALS: Operator = operator(OperatorKind::Equals, PRECEDENCE_EQUALITY);
static NOT_EQUALS: Operator = operator(OperatorKind::NotEquals, PRECEDENCE_EQUALITY);
static GREATER: Operator = operator(OperatorKind::Greater, PRECEDENCE_COMPARISON);
static GREATER_EQUALS: Operator = operator(OperatorKind::GreaterEquals, PRECEDENCE_COMPARISON);
static LESS: Operator = operator(OperatorKind::Less, PRECEDENCE_COMPARISON);
static LESS_EQUALS: Operator = operator(OperatorKind::LessEquals, PRECEDENCE_COMPARISON);
static AND: Operator = lazy(OperatorKind::And, PRECEDENCE_AND);
static OR: Operator = lazy(OperatorKind::Or, PRECEDENCE_OR);
static NEGATE: Operator = right_associative(OperatorKind::Negate, PRECEDENCE_UNARY);
static IDENTITY: Operator = right_associative(OperatorKind::Identity, PRECEDENCE_UNARY);
static NOT: Operator = operator(OperatorKind::Not, PRECEDENCE_UNARY);

fn ascii(text: &[u16]) -> Option<String> {
    String::from_utf16(text).ok()
}

/// The prefix operator spelled `text`.
pub fn prefix(text: &[u16]) -> Option<&'static Operator> {
    match ascii(text)?.as_str() {
        "+" => Some(&IDENTITY),
        "-" => Some(&NEGATE),
        "!" => Some(&NOT),
        _ => None,
    }
}

/// The postfix operator spelled `text`; the language has none.
pub fn postfix(_text: &[u16]) -> Option<&'static Operator> {
    None
}

/// The infix operator spelled `text`.
pub fn infix(text: &[u16]) -> Option<&'static Operator> {
    match ascii(text)?.as_str() {
        "+" => Some(&PLUS),
        "-" => Some(&MINUS),
        "*" => Some(&MULTIPLY),
        "/" => Some(&DIVIDE),
        "^" => Some(&POWER),
        "%" => Some(&MODULO),
        "=" | "==" => Some(&EQUALS),
        "!=" | "<>" => Some(&NOT_EQUALS),
        ">" => Some(&GREATER),
        ">=" => Some(&GREATER_EQUALS),
        "<" => Some(&LESS),
        "<=" => Some(&LESS_EQUALS),
        "&&" => Some(&AND),
        "||" => Some(&OR),
        _ => None,
    }
}

impl Operator {
    /// The precedence under `configuration`.
    pub fn precedence(&self, configuration: &Configuration) -> i32 {
        if self.kind == OperatorKind::Power {
            configuration.power_of_precedence
        } else {
            self.precedence
        }
    }

    /// Applies a prefix operator.
    pub fn evaluate_prefix(&self, expression: &Expression, token: &Token, operand: &Value) -> Outcome<Value> {
        let context = expression.math_context();
        match (self.kind, operand) {
            (OperatorKind::Not, _) => Ok(Value::Boolean(!require_boolean(operand)?)),
            (OperatorKind::Negate, Value::Number(number)) => Ok(Value::Number(number.negate_mc(context)?)),
            (OperatorKind::Identity, Value::Number(number)) => Ok(Value::Number(number.plus_mc(context)?)),
            _ => Err(Failure::unsupported_types(token)),
        }
    }

    /// Applies an infix operator to evaluated operands.
    pub fn evaluate_infix(
        &self,
        expression: &Expression,
        token: &Token,
        left: &Value,
        right: &Value,
    ) -> Outcome<Value> {
        match self.kind {
            OperatorKind::Plus => plus(expression, left, right),
            OperatorKind::Minus => minus(expression, token, left, right),
            OperatorKind::Multiply => multiply(expression, token, left, right),
            OperatorKind::Divide => divide(expression, token, left, right),
            OperatorKind::Modulo => modulo(expression, token, left, right),
            OperatorKind::Power => power(expression, token, left, right),
            OperatorKind::Equals => equals(left, right),
            OperatorKind::NotEquals => not_equals(left, right),
            OperatorKind::Greater => compare(left, right, Ordering::is_gt),
            OperatorKind::GreaterEquals => compare(left, right, Ordering::is_ge),
            OperatorKind::Less => compare(left, right, Ordering::is_lt),
            OperatorKind::LessEquals => compare(left, right, Ordering::is_le),
            OperatorKind::And
            | OperatorKind::Or
            | OperatorKind::Negate
            | OperatorKind::Identity
            | OperatorKind::Not => Err(Failure::unsupported_types(token)),
        }
    }
}

/// Unboxes a boolean; a null value is the JVM's `NullPointerException`.
pub fn require_boolean(value: &Value) -> Outcome<bool> {
    value.boolean_value().ok_or_else(|| {
        Failure::null_pointer_message(
            "Cannot invoke \"java.lang.Boolean.booleanValue()\" because the return value of \
             \"com.ezylang.evalex.data.EvaluationValue.getBooleanValue()\" is null",
        )
    })
}

fn plus(expression: &Expression, left: &Value, right: &Value) -> Outcome<Value> {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a.add_mc(b, expression.math_context())?)),
        (Value::DateTime(a), Value::Duration(b)) => Ok(Value::DateTime(a.plus(b)?)),
        (Value::Duration(a), Value::Duration(b)) => Ok(Value::Duration(a.plus(b)?)),
        (Value::DateTime(a), Value::Number(b)) => Ok(Value::DateTime(a.plus(&Duration::of_millis(b.long_value()))?)),
        _ => {
            let mut text = string_or_null(left);
            text.extend(string_or_null(right));
            Ok(Value::Str(text))
        }
    }
}

/// String concatenation prints a null reference as `null`.
fn string_or_null(value: &Value) -> Vec<u16> {
    value.string_value().unwrap_or_else(|| crate::java::js("null"))
}

fn minus(expression: &Expression, token: &Token, left: &Value, right: &Value) -> Outcome<Value> {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a.subtract_mc(b, expression.math_context())?)),
        (Value::DateTime(a), Value::DateTime(b)) => {
            let difference = a.to_epoch_milli()?.wrapping_sub(b.to_epoch_milli()?);
            Ok(Value::Duration(Duration::of_millis(difference)))
        }
        (Value::DateTime(a), Value::Duration(b)) => Ok(Value::DateTime(a.minus(b)?)),
        (Value::Duration(a), Value::Duration(b)) => Ok(Value::Duration(a.minus(b)?)),
        (Value::DateTime(a), Value::Number(b)) => Ok(Value::DateTime(a.minus(&Duration::of_millis(b.long_value()))?)),
        _ => Err(Failure::unsupported_types(token)),
    }
}

fn numbers<'a>(token: &Token, left: &'a Value, right: &'a Value) -> Outcome<(&'a BigDecimal, &'a BigDecimal)> {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => Ok((a, b)),
        _ => Err(Failure::unsupported_types(token)),
    }
}

fn multiply(expression: &Expression, token: &Token, left: &Value, right: &Value) -> Outcome<Value> {
    let (a, b) = numbers(token, left, right)?;
    Ok(Value::Number(a.multiply_mc(b, expression.math_context())?))
}

fn divide(expression: &Expression, token: &Token, left: &Value, right: &Value) -> Outcome<Value> {
    let (a, b) = numbers(token, left, right)?;
    if b.java_equals(&BigDecimal::zero()) {
        return Err(Failure::evaluation_str(token, "Division by zero"));
    }
    Ok(Value::Number(a.divide_mc(b, expression.math_context())?))
}

fn modulo(expression: &Expression, token: &Token, left: &Value, right: &Value) -> Outcome<Value> {
    let (a, b) = numbers(token, left, right)?;
    if b.java_equals(&BigDecimal::zero()) {
        return Err(Failure::evaluation_str(token, "Division by zero"));
    }
    Ok(Value::Number(a.remainder_mc(b, expression.math_context())?))
}

fn power(expression: &Expression, token: &Token, left: &Value, right: &Value) -> Outcome<Value> {
    let (v1, v2) = numbers(token, left, right)?;
    let context = expression.math_context();
    let sign = v2.signum();
    let base = v1.double_value();
    let v2 = v2.multiply(&BigDecimal::from_i64(i64::from(sign)))?;
    let fraction = v2.remainder(&BigDecimal::one())?;
    let integer_part = v2.subtract(&fraction)?;
    let integer_power = v1.pow_mc(integer_part.int_value_exact()?, context)?;
    let fraction_power = BigDecimal::value_of_double(fdlibm::pow(base, fraction.double_value()))?;
    let mut result = integer_power.multiply_mc(&fraction_power, context)?;
    if sign == -1 {
        result = BigDecimal::one().divide_scale(&result, context.precision, RoundingMode::HalfUp)?;
    }
    Ok(Value::Number(result))
}

fn equals(left: &Value, right: &Value) -> Outcome<Value> {
    if left.is_null() && right.is_null() {
        return Ok(Value::Boolean(true));
    }
    if !left.same_type(right) {
        return Ok(Value::Boolean(false));
    }
    Ok(Value::Boolean(left.compare_to(right)?.is_eq()))
}

fn not_equals(left: &Value, right: &Value) -> Outcome<Value> {
    if left.is_null() && right.is_null() {
        return Ok(Value::Boolean(false));
    }
    if !left.same_type(right) {
        return Ok(Value::Boolean(true));
    }
    Ok(Value::Boolean(left.compare_to(right)?.is_ne()))
}

fn compare(left: &Value, right: &Value, test: fn(Ordering) -> bool) -> Outcome<Value> {
    Ok(Value::Boolean(test(left.compare_to(right)?)))
}
