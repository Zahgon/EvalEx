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

//! The values expressions compute with.

use std::cmp::Ordering;
use std::rc::Rc;

use crate::error::{Failure, Outcome};
use crate::java::bigdecimal::BigDecimal;
use crate::java::chars;
use crate::java::hashmap::HashMap;
use crate::java::time::{Duration, Instant};
use crate::java::{js, JString};
use crate::token::Node;

/// The value of an evaluation.
#[derive(Clone, Debug)]
pub enum Value {
    /// A string.
    Str(JString),
    /// A decimal number.
    Number(BigDecimal),
    /// A boolean.
    Boolean(bool),
    /// A point in time.
    DateTime(Instant),
    /// An amount of time.
    Duration(Duration),
    /// An array of values.
    Array(Rc<Vec<Value>>),
    /// A structure of named values.
    Structure(Rc<HashMap<Value>>),
    /// An unevaluated sub-expression.
    ExpressionNode(Rc<Node>),
    /// The null value.
    Null,
    /// The value of an unknown name in lenient mode.
    Undefined,
}

impl Value {
    /// A string value.
    pub fn string(text: &str) -> Value {
        Value::Str(js(text))
    }

    /// The name of the value's data type.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Str(_) => "STRING",
            Value::Number(_) => "NUMBER",
            Value::Boolean(_) => "BOOLEAN",
            Value::DateTime(_) => "DATE_TIME",
            Value::Duration(_) => "DURATION",
            Value::Array(_) => "ARRAY",
            Value::Structure(_) => "STRUCTURE",
            Value::ExpressionNode(_) => "EXPRESSION_NODE",
            Value::Null => "NULL",
            Value::Undefined => "UNDEFINED",
        }
    }

    /// Whether the value is null or undefined.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null | Value::Undefined)
    }

    /// `getNumberValue()`: `None` for null.
    pub fn number_value(&self) -> Option<BigDecimal> {
        match self {
            Value::Number(number) => Some(number.clone()),
            Value::Boolean(true) => Some(BigDecimal::one()),
            Value::Str(text) if chars::parse_boolean(text) => Some(BigDecimal::one()),
            Value::Null => None,
            _ => Some(BigDecimal::zero()),
        }
    }

    /// `requireNonNull(getNumberValue())`.
    pub fn required_number(&self) -> Outcome<BigDecimal> {
        self.number_value().ok_or_else(Failure::null_pointer)
    }

    /// `getStringValue()`: `None` for null.
    pub fn string_value(&self) -> Option<JString> {
        match self {
            // A plain form longer than a Java string can hold is not representable: it reads as null.
            Value::Number(number) => number.to_plain_string().ok().map(|text| js(&text)),
            Value::Null => None,
            Value::Undefined => Some(JString::new()),
            other => Some(other.inner_to_string()),
        }
    }

    /// `requireNonNull(getStringValue())`.
    pub fn required_string(&self) -> Outcome<JString> {
        self.string_value().ok_or_else(Failure::null_pointer)
    }

    /// `getBooleanValue()`: `None` for null.
    pub fn boolean_value(&self) -> Option<bool> {
        match self {
            Value::Number(number) => Some(number.signum() != 0),
            Value::Boolean(value) => Some(*value),
            Value::Str(text) => Some(chars::parse_boolean(text)),
            Value::Null => None,
            _ => Some(false),
        }
    }

    /// `getDateTimeValue()`: anything that is not a date-time gives the epoch.
    pub fn date_time_value(&self) -> Instant {
        match self {
            Value::Number(number) => Instant::of_epoch_milli(number.long_value()),
            Value::DateTime(instant) => *instant,
            Value::Str(text) => Instant::parse(text).unwrap_or(Instant::EPOCH),
            _ => Instant::EPOCH,
        }
    }

    /// `getDurationValue()`: anything that is not a duration gives zero.
    pub fn duration_value(&self) -> Duration {
        match self {
            Value::Number(number) => Duration::of_millis(number.long_value()),
            Value::Duration(duration) => *duration,
            Value::Str(text) => Duration::parse(text).unwrap_or(Duration::ZERO),
            _ => Duration::ZERO,
        }
    }

    /// `Object.toString()` of the Java object the value holds.
    fn inner_to_string(&self) -> JString {
        match self {
            Value::Str(text) => text.clone(),
            Value::Number(number) => js(&number.to_string()),
            Value::Boolean(value) => js(if *value { "true" } else { "false" }),
            Value::DateTime(instant) => js(&instant.to_string()),
            Value::Duration(duration) => js(&duration.to_string()),
            Value::Array(items) => {
                let mut out = js("[");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.extend(", ".encode_utf16());
                    }
                    out.extend(item.to_java_string());
                }
                out.push(u16::from(b']'));
                out
            }
            Value::Structure(members) => {
                let mut out = js("{");
                for (index, (name, member)) in members.iter().into_iter().enumerate() {
                    if index > 0 {
                        out.extend(", ".encode_utf16());
                    }
                    out.extend_from_slice(name);
                    out.push(u16::from(b'='));
                    out.extend(member.to_java_string());
                }
                out.push(u16::from(b'}'));
                out
            }
            Value::ExpressionNode(node) => node.to_java_string(),
            Value::Null | Value::Undefined => js("null"),
        }
    }

    /// `EvaluationValue.toString()`.
    pub fn to_java_string(&self) -> JString {
        let mut out = js("EvaluationValue(value=");
        out.extend(self.inner_to_string());
        out.extend(format!(", dataType={})", self.type_name()).encode_utf16());
        out
    }

    /// `compareTo`.
    pub fn compare_to(&self, other: &Value) -> Outcome<Ordering> {
        match self {
            Value::Number(number) => Ok(number.compare_to(&other.required_number()?)),
            Value::Boolean(value) => match other.boolean_value() {
                Some(right) => Ok(value.cmp(&right)),
                None => Err(Failure::null_pointer_message("Cannot read field \"value\" because \"b\" is null")),
            },
            Value::Null => Err(Failure::null_pointer_message("Can not compare a null value")),
            Value::Undefined => Err(Failure::null_pointer_message("Can not compare an undefined value")),
            Value::DateTime(instant) => Ok(instant.compare(&other.date_time_value())),
            Value::Duration(duration) => Ok(duration.compare(&other.duration_value())),
            _ => {
                let left = self.required_string()?;
                let right = other.required_string()?;
                Ok(left.cmp(&right))
            }
        }
    }

    /// Whether two values have the same data type.
    pub fn same_type(&self, other: &Value) -> bool {
        self.type_name() == other.type_name()
    }
}

/// The double conversion of `Expression.convertDoubleValue`: `new BigDecimal(Double.toString(value), context)`.
pub fn convert_double(value: f64, context: &crate::java::bigdecimal::MathContext) -> Outcome<Value> {
    let text = js(&crate::java::double::to_string(value));
    Ok(Value::Number(BigDecimal::parse_mc(&text, context)?))
}
