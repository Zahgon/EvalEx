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

//! The function dictionary.

mod basic;
mod datetime;
mod string;
mod trigonometric;

use crate::error::{Failure, Outcome};
use crate::expression::Expression;
use crate::java::bigdecimal::BigDecimal;
use crate::java::chars::case_insensitive_key;
use crate::java::js;
use crate::token::Token;
use crate::value::Value;

/// The definition of one parameter.
#[derive(Clone, Copy, Debug)]
pub struct Parameter {
    /// The parameter takes all remaining arguments.
    pub var_arg: bool,
    /// The parameter reaches the function unevaluated.
    pub lazy: bool,
    /// The value must not equal zero.
    pub non_zero: bool,
    /// The value must not be negative.
    pub non_negative: bool,
}

/// A required, evaluated parameter without restrictions.
pub const VALUE: Parameter = Parameter { var_arg: false, lazy: false, non_zero: false, non_negative: false };
/// A required parameter that is not evaluated before the call.
pub const LAZY: Parameter = Parameter { lazy: true, ..VALUE };
/// A parameter that takes all remaining arguments.
pub const VAR_ARG: Parameter = Parameter { var_arg: true, ..VALUE };
/// A required parameter that must not be zero.
pub const NON_ZERO: Parameter = Parameter { non_zero: true, ..VALUE };
/// A required parameter that must not be negative.
pub const NON_NEGATIVE: Parameter = Parameter { non_negative: true, ..VALUE };

/// What a function computes from its parameters.
pub type Evaluate = fn(&Expression, &Token, &[Value]) -> Outcome<Value>;
/// The checks of a function that run before it is evaluated.
pub type Validate = fn(&Token, &[Value]) -> Outcome<()>;

/// A function of the expression language.
#[derive(Debug)]
pub struct Function {
    /// The function's name.
    pub name: &'static str,
    /// The parameter definitions.
    pub parameters: &'static [Parameter],
    /// The checks beyond those of the parameter definitions.
    pub validate: Option<Validate>,
    /// The implementation.
    pub evaluate: Evaluate,
}

impl Function {
    /// A function with no checks beyond its parameter definitions.
    pub const fn new(name: &'static str, parameters: &'static [Parameter], evaluate: Evaluate) -> Function {
        Function { name, parameters, validate: None, evaluate }
    }

    /// A function with its own checks.
    pub const fn validated(
        name: &'static str,
        parameters: &'static [Parameter],
        validate: Validate,
        evaluate: Evaluate,
    ) -> Function {
        Function { name, parameters, validate: Some(validate), evaluate }
    }

    /// Whether the last parameter takes all remaining arguments.
    pub fn has_var_args(&self) -> bool {
        self.parameters.iter().any(|parameter| parameter.var_arg)
    }

    /// The number of parameters a call has to pass.
    pub fn required_parameters(&self) -> usize {
        self.parameters.len() - usize::from(self.has_var_args())
    }

    fn definition(&self, index: usize) -> Option<&Parameter> {
        self.parameters.get(index).or_else(|| self.parameters.last())
    }

    /// Whether the parameter at `index` is not evaluated before the call.
    pub fn is_parameter_lazy(&self, index: usize) -> bool {
        self.definition(index).is_some_and(|parameter| parameter.lazy)
    }

    /// The checks that run before the function is evaluated.
    pub fn validate(&self, token: &Token, parameters: &[Value]) -> Outcome<()> {
        self.validate_definitions(token, parameters)?;
        match self.validate {
            Some(validate) => validate(token, parameters),
            None => Ok(()),
        }
    }

    /// The checks of the parameter definitions.
    fn validate_definitions(&self, token: &Token, parameters: &[Value]) -> Outcome<()> {
        for (index, value) in parameters.iter().enumerate() {
            let Some(definition) = self.definition(index) else {
                continue;
            };
            if definition.non_zero {
                let number = value.number_value().ok_or_else(|| {
                    Failure::null_pointer_message(
                        "Cannot invoke \"java.math.BigDecimal.equals(Object)\" because the return value of \
                         \"com.ezylang.evalex.data.EvaluationValue.getNumberValue()\" is null",
                    )
                })?;
                if number.java_equals(&BigDecimal::zero()) {
                    return Err(Failure::evaluation_str(token, "Parameter must not be zero"));
                }
            }
            if definition.non_negative {
                let number = value.number_value().ok_or_else(|| {
                    Failure::null_pointer_message(
                        "Cannot invoke \"java.math.BigDecimal.signum()\" because the return value of \
                         \"com.ezylang.evalex.data.EvaluationValue.getNumberValue()\" is null",
                    )
                })?;
                if number.signum() < 0 {
                    return Err(Failure::evaluation_str(token, "Parameter must not be negative"));
                }
            }
        }
        Ok(())
    }
}

/// The function named `name`, in any case.
pub fn find(name: &[u16]) -> Option<&'static Function> {
    let key = case_insensitive_key(name);
    [basic::FUNCTIONS, trigonometric::FUNCTIONS, string::FUNCTIONS, datetime::FUNCTIONS]
        .into_iter()
        .flatten()
        .find(|function| case_insensitive_key(&js(function.name)) == key)
}

/// The JVM's message for a null operand of a function, unboxed or dereferenced at the call.
pub(crate) fn require_string(value: &Value) -> Outcome<crate::java::JString> {
    value.required_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_functions_in_any_case() {
        assert_eq!(find(&js("sqrt")).map(|function| function.name), Some("SQRT"));
        assert!(find(&js("NO_SUCH")).is_none());
    }

    #[test]
    fn counts_required_parameters() {
        let max = find(&js("MAX")).map(Function::required_parameters);
        assert_eq!(max, Some(1));
        let round = find(&js("ROUND")).map(Function::required_parameters);
        assert_eq!(round, Some(2));
    }
}
