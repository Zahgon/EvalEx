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

//! Parsing and evaluating one expression.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::config::{Configuration, DECIMAL_PLACES_ROUNDING_UNLIMITED};
use crate::error::{Failure, Outcome};
use crate::java::bigdecimal::{BigDecimal, MathContext};
use crate::java::chars::case_insensitive_key;
use crate::java::{js, JResult, JString, Thrown};
use crate::operators::{require_boolean, OperatorKind};
use crate::parser::ShuntingYard;
use crate::token::{Node, Token, TokenType};
use crate::tokenizer::Tokenizer;
use crate::value::Value;

const PI: &str =
    "3.1415926535897932384626433832795028841971693993751058209749445923078164062862089986280348253421170679";
/// The stack evaluation needs: callers on the default thread stack of 2 MiB run it on a thread of this size, as the
/// command does.
pub const REQUIRED_STACK_BYTES: usize = 256 << 20;
/// The deepest evaluation that fits [`REQUIRED_STACK_BYTES`].
const MAX_NESTING: u32 = 10_000;
/// The longest expression, which bounds the depth of its tree and of everything that walks it.
const MAX_TOKENS: usize = 10_000;
const E: &str = "2.71828182845904523536028747135266249775724709369995957496696762772407663";

/// A map with the keys of a `TreeMap` ordered by `String.CASE_INSENSITIVE_ORDER`.
#[derive(Clone, Debug)]
pub struct CaseInsensitiveMap<V> {
    entries: BTreeMap<Vec<u32>, (JString, V)>,
}

impl<V> Default for CaseInsensitiveMap<V> {
    fn default() -> CaseInsensitiveMap<V> {
        CaseInsensitiveMap { entries: BTreeMap::new() }
    }
}

impl<V> CaseInsensitiveMap<V> {
    /// The value of `key`.
    pub fn get(&self, key: &[u16]) -> Option<&V> {
        self.entries.get(&case_insensitive_key(key)).map(|(_, value)| value)
    }

    /// Sets the value of `key`; a key that exists keeps its original spelling.
    pub fn put(&mut self, key: &[u16], value: V) {
        match self.entries.get_mut(&case_insensitive_key(key)) {
            Some(entry) => entry.1 = value,
            None => {
                self.entries.insert(case_insensitive_key(key), (key.to_vec(), value));
            }
        }
    }

    /// Removes `key`.
    pub fn remove(&mut self, key: &[u16]) {
        self.entries.remove(&case_insensitive_key(key));
    }

    /// The keys in order, with their original spelling.
    pub fn keys(&self) -> impl Iterator<Item = &JString> {
        self.entries.values().map(|(key, _)| key)
    }
}

/// An expression with its configuration, constants and variables.
#[derive(Debug)]
pub struct Expression {
    configuration: Configuration,
    text: JString,
    constants: CaseInsensitiveMap<Value>,
    variables: CaseInsensitiveMap<Value>,
    nesting: Cell<u32>,
}

/// The `StackOverflowError` of an expression nested beyond what the evaluator's stack holds.
fn stack_overflow() -> Failure {
    Thrown::no_message("java.lang.StackOverflowError").into()
}

fn standard_constants() -> JResult<CaseInsensitiveMap<Value>> {
    let mut constants = CaseInsensitiveMap::default();
    constants.put(&js("TRUE"), Value::Boolean(true));
    constants.put(&js("FALSE"), Value::Boolean(false));
    constants.put(&js("PI"), Value::Number(BigDecimal::parse(&js(PI))?));
    constants.put(&js("E"), Value::Number(BigDecimal::parse(&js(E))?));
    constants.put(&js("NULL"), Value::Null);
    constants.put(&js("DT_FORMAT_ISO_DATE_TIME"), Value::string("yyyy-MM-dd'T'HH:mm:ss[.SSS][XXX]['['VV']']"));
    constants.put(&js("DT_FORMAT_LOCAL_DATE_TIME"), Value::string("yyyy-MM-dd'T'HH:mm:ss[.SSS]"));
    constants.put(&js("DT_FORMAT_LOCAL_DATE"), Value::string("yyyy-MM-dd"));
    Ok(constants)
}

impl Expression {
    /// An expression with the standard constants and no variables.
    pub fn new(text: JString, configuration: Configuration) -> JResult<Expression> {
        Ok(Expression {
            configuration,
            text,
            constants: standard_constants()?,
            variables: CaseInsensitiveMap::default(),
            nesting: Cell::new(0),
        })
    }

    /// The configuration.
    pub fn configuration(&self) -> &Configuration {
        &self.configuration
    }

    /// The math context of the configuration.
    pub fn math_context(&self) -> &MathContext {
        &self.configuration.math_context
    }

    /// Sets a variable; a constant of that name is replaced when the configuration allows it.
    pub fn with(&mut self, name: &[u16], value: Value) -> JResult<()> {
        if self.constants.get(name).is_some() {
            if !self.configuration.allow_overwrite_constants {
                let mut message = js("Can't set value for constant '");
                message.extend_from_slice(name);
                message.push(u16::from(b'\''));
                return Err(Thrown::with_message("java.lang.UnsupportedOperationException", message));
            }
            self.constants.remove(name);
        }
        self.variables.put(name, value);
        Ok(())
    }

    /// The syntax tree of the expression.
    pub fn abstract_syntax_tree(&self) -> Outcome<Rc<Node>> {
        self.create_expression_node(&self.text)
    }

    /// The syntax tree of another expression under this configuration.
    pub fn create_expression_node(&self, text: &[u16]) -> Outcome<Rc<Node>> {
        let tokens = Tokenizer::new(text, &self.configuration).parse()?;
        if tokens.len() > MAX_TOKENS {
            return Err(stack_overflow());
        }
        ShuntingYard::new(text, &self.configuration).to_abstract_syntax_tree(tokens)
    }

    /// Evaluates the expression and applies the settings for the final result.
    pub fn evaluate(&self) -> Outcome<Value> {
        let tree = self.abstract_syntax_tree()?;
        let mut result = self.evaluate_subtree(&tree, 0)?;
        if let Value::Number(number) = &result {
            let mut number = number.clone();
            if self.configuration.decimal_places_result != DECIMAL_PLACES_ROUNDING_UNLIMITED {
                number = self.round(&number, self.configuration.decimal_places_result)?;
            }
            if self.configuration.strip_trailing_zeros {
                number = number.strip_trailing_zeros()?;
            }
            result = Value::Number(number);
        }
        Ok(result)
    }

    fn round(&self, number: &BigDecimal, places: i32) -> Outcome<BigDecimal> {
        Ok(number.set_scale(places, self.math_context().rounding)?)
    }

    /// Evaluates a node at a recursion depth; lazy operands and parameters start again at depth 0.
    ///
    /// Depth resets let expression variables recurse without end, so the real nesting has its own limit, where the
    /// original's JVM overflows its stack.
    pub fn evaluate_subtree(&self, node: &Rc<Node>, depth: i32) -> Outcome<Value> {
        if self.nesting.get() >= MAX_NESTING {
            return Err(stack_overflow());
        }
        self.nesting.set(self.nesting.get() + 1);
        let result = self.evaluate_node(node, depth);
        self.nesting.set(self.nesting.get() - 1);
        result
    }

    fn evaluate_node(&self, node: &Rc<Node>, depth: i32) -> Outcome<Value> {
        let token = &node.token;
        if depth > self.configuration.max_recursion_depth {
            return Err(Failure::evaluation_str(token, "Max recursion depth exceeded"));
        }
        let next = depth + 1;
        let result = match token.kind {
            TokenType::NumberLiteral => self.number_literal(&token.value)?,
            TokenType::StringLiteral => Value::Str(token.value.clone()),
            TokenType::VariableOrConstant => match self.variable_or_constant(token)? {
                Value::ExpressionNode(inner) => self.evaluate_subtree(&inner, next)?,
                value => value,
            },
            TokenType::PrefixOperator | TokenType::PostfixOperator => self.evaluate_prefix(node, next)?,
            TokenType::InfixOperator => self.evaluate_infix(node, next)?,
            TokenType::ArrayIndex => self.evaluate_array_index(node, next)?,
            TokenType::StructureSeparator => self.evaluate_structure_separator(node, next)?,
            TokenType::Function => self.evaluate_function(node, next)?,
            _ => {
                let mut message = js("Unexpected evaluation token: ");
                message.extend(token.to_java_string());
                return Err(Failure::evaluation(token, &message));
            }
        };
        if let Value::Number(number) = &result {
            if self.configuration.decimal_places_rounding != DECIMAL_PLACES_ROUNDING_UNLIMITED {
                return Ok(Value::Number(self.round(number, self.configuration.decimal_places_rounding)?));
            }
        }
        Ok(result)
    }

    fn number_literal(&self, text: &[u16]) -> Outcome<Value> {
        let context = self.math_context();
        let number = match text {
            [zero, x, digits @ ..] if *zero == u16::from(b'0') && (*x | 0x20) == u16::from(b'x') => {
                let integer = crate::java::biginteger::BigInteger::parse(digits, 16)?;
                BigDecimal::from_big_integer_mc(&integer, context)?
            }
            _ => BigDecimal::parse_mc(text, context)?,
        };
        Ok(Value::Number(number))
    }

    fn variable_or_constant(&self, token: &Token) -> Outcome<Value> {
        if let Some(value) = self.constants.get(&token.value).or_else(|| self.variables.get(&token.value)) {
            return Ok(value.clone());
        }
        if self.configuration.lenient_mode {
            return Ok(Value::Undefined);
        }
        let mut message = js("Variable or constant value for '");
        message.extend_from_slice(&token.value);
        message.extend("' not found".encode_utf16());
        Err(Failure::evaluation(token, &message))
    }

    fn parameter(node: &Node, index: usize) -> Outcome<&Rc<Node>> {
        node.parameters.get(index).ok_or_else(|| Failure::evaluation_str(&node.token, "Missing operand"))
    }

    fn evaluate_prefix(&self, node: &Rc<Node>, depth: i32) -> Outcome<Value> {
        let operand = self.evaluate_subtree(Self::parameter(node, 0)?, depth)?;
        match node.token.operator {
            Some(operator) => operator.evaluate_prefix(self, &node.token, &operand),
            None => Err(Failure::unsupported_types(&node.token)),
        }
    }

    fn evaluate_infix(&self, node: &Rc<Node>, depth: i32) -> Outcome<Value> {
        let depth = depth + 1;
        let Some(operator) = node.token.operator else {
            return Err(Failure::unsupported_types(&node.token));
        };
        let left = Self::parameter(node, 0)?;
        let right = Self::parameter(node, 1)?;
        match operator.kind {
            OperatorKind::And => {
                let result = require_boolean(&self.evaluate_subtree(left, 0)?)?
                    && require_boolean(&self.evaluate_subtree(right, 0)?)?;
                Ok(Value::Boolean(result))
            }
            OperatorKind::Or => {
                let result = require_boolean(&self.evaluate_subtree(left, 0)?)?
                    || require_boolean(&self.evaluate_subtree(right, 0)?)?;
                Ok(Value::Boolean(result))
            }
            _ => {
                let left = self.evaluate_subtree(left, depth)?;
                let right = self.evaluate_subtree(right, depth)?;
                operator.evaluate_infix(self, &node.token, &left, &right)
            }
        }
    }

    fn evaluate_array_index(&self, node: &Rc<Node>, depth: i32) -> Outcome<Value> {
        let depth = depth + 1;
        let array = self.evaluate_subtree(Self::parameter(node, 0)?, depth)?;
        let index = self.evaluate_subtree(Self::parameter(node, 1)?, depth)?;
        let (Value::Array(items), Value::Number(position)) = (&array, &index) else {
            return Err(Failure::unsupported_types(&node.token));
        };
        let position = position.int_value();
        match usize::try_from(position).ok().and_then(|at| items.get(at)) {
            Some(item) => Ok(item.clone()),
            None => {
                let message = format!("Index {position} out of bounds for array of length {}", items.len());
                Err(Failure::evaluation_str(&node.token, &message))
            }
        }
    }

    fn evaluate_structure_separator(&self, node: &Rc<Node>, depth: i32) -> Outcome<Value> {
        let depth = depth + 1;
        let structure = self.evaluate_subtree(Self::parameter(node, 0)?, depth)?;
        let name_token = &Self::parameter(node, 1)?.token;
        match &structure {
            Value::Structure(members) => match members.get(&name_token.value) {
                Some(member) => Ok(member.clone()),
                None if self.configuration.lenient_mode => Ok(Value::Undefined),
                None => {
                    let mut message = js("Field '");
                    message.extend_from_slice(&name_token.value);
                    message.extend("' not found in structure".encode_utf16());
                    Err(Failure::evaluation(name_token, &message))
                }
            },
            value if value.is_null() && self.configuration.lenient_mode => Ok(Value::Undefined),
            _ => Err(Failure::unsupported_types(&node.token)),
        }
    }

    fn evaluate_function(&self, node: &Rc<Node>, depth: i32) -> Outcome<Value> {
        let depth = depth + 1;
        let token = &node.token;
        let Some(function) = token.function else {
            return Err(Failure::unsupported_types(token));
        };
        let mut parameters = Vec::with_capacity(node.parameters.len());
        for (index, parameter) in node.parameters.iter().enumerate() {
            parameters.push(if function.is_parameter_lazy(index) {
                Value::ExpressionNode(Rc::clone(parameter))
            } else {
                self.evaluate_subtree(parameter, depth)?
            });
        }
        function.validate(token, &parameters)?;
        (function.evaluate)(self, token, &parameters)
    }

    /// The names of the expression that are not constants, in case-insensitive order.
    pub fn used_variables(&self) -> Outcome<Vec<JString>> {
        let mut used = CaseInsensitiveMap::default();
        for node in self.abstract_syntax_tree()?.all_nodes() {
            let name = &node.token.value;
            if node.token.kind == TokenType::VariableOrConstant
                && self.constants.get(name).is_none()
                && used.get(name).is_none()
            {
                used.put(name, ());
            }
        }
        Ok(used.keys().cloned().collect())
    }

    /// The used variables without a value.
    pub fn undefined_variables(&self) -> Outcome<Vec<JString>> {
        let mut names = self.used_variables()?;
        names.retain(|name| self.variables.get(name).is_none());
        Ok(names)
    }
}
