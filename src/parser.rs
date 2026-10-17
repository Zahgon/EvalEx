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

//! Converts tokens to an abstract syntax tree with the shunting yard algorithm.

use std::rc::Rc;

use crate::config::Configuration;
use crate::error::{Failure, Outcome};
use crate::java::Thrown;
use crate::operators::Operator;
use crate::token::{Node, Token, TokenType};

fn no_such_element() -> Failure {
    Thrown::no_message("java.util.NoSuchElementException").into()
}

/// The stacks of one conversion.
pub struct ShuntingYard<'a> {
    expression: &'a [u16],
    configuration: &'a Configuration,
    operators: Vec<Token>,
    operands: Vec<Rc<Node>>,
}

impl<'a> ShuntingYard<'a> {
    /// A converter for the tokens of `expression`.
    pub fn new(expression: &'a [u16], configuration: &'a Configuration) -> ShuntingYard<'a> {
        ShuntingYard { expression, configuration, operators: Vec::new(), operands: Vec::new() }
    }

    /// Builds the tree.
    pub fn to_abstract_syntax_tree(mut self, tokens: Vec<Token>) -> Outcome<Rc<Node>> {
        let mut previous: Option<TokenType> = None;
        for token in tokens {
            let kind = token.kind;
            match kind {
                TokenType::VariableOrConstant | TokenType::NumberLiteral | TokenType::StringLiteral => {
                    self.operands.push(Node::leaf(token));
                }
                TokenType::Function => self.operators.push(token),
                TokenType::Comma => self.process_until(TokenType::BraceOpen)?,
                TokenType::InfixOperator | TokenType::PrefixOperator | TokenType::PostfixOperator => {
                    self.process_operator(token)?;
                }
                TokenType::BraceOpen => self.process_brace_open(previous, token),
                TokenType::BraceClose => self.process_brace_close()?,
                TokenType::ArrayOpen => self.process_array_open(token)?,
                TokenType::ArrayClose => self.process_array_close()?,
                TokenType::StructureSeparator => self.process_structure_separator(token)?,
                TokenType::FunctionParamStart | TokenType::ArrayIndex => {
                    let message = format!("Unexpected token of type '{}'", kind.name());
                    return Err(Failure::parse_token(&token, &message));
                }
            }
            previous = Some(kind);
        }
        while let Some(token) = self.operators.pop() {
            self.create_operator_node(token)?;
        }
        if self.operands.is_empty() {
            return Err(Failure::parse_expression(self.expression, "Empty expression"));
        }
        if self.operands.len() > 1 {
            return Err(Failure::parse_expression(self.expression, "Too many operands"));
        }
        self.operands.pop().ok_or_else(no_such_element)
    }

    fn peek_kind(&self) -> Option<TokenType> {
        self.operators.last().map(|token| token.kind)
    }

    fn pop_operator(&mut self) -> Outcome<Token> {
        self.operators.pop().ok_or_else(no_such_element)
    }

    fn pop_operand(&mut self) -> Outcome<Rc<Node>> {
        self.operands.pop().ok_or_else(no_such_element)
    }

    fn process_structure_separator(&mut self, current: Token) -> Outcome<()> {
        self.pop_structure_separators()?;
        self.operators.push(current);
        Ok(())
    }

    fn pop_structure_separators(&mut self) -> Outcome<()> {
        while self.peek_kind() == Some(TokenType::StructureSeparator) {
            let token = self.pop_operator()?;
            self.create_operator_node(token)?;
        }
        Ok(())
    }

    fn process_brace_open(&mut self, previous: Option<TokenType>, current: Token) {
        if previous == Some(TokenType::Function) {
            let start = Token::new(current.start, current.value.clone(), TokenType::FunctionParamStart);
            self.operands.push(Node::leaf(start));
        }
        self.operators.push(current);
    }

    fn process_brace_close(&mut self) -> Outcome<()> {
        self.process_until(TokenType::BraceOpen)?;
        self.pop_operator()?;
        if self.peek_kind() == Some(TokenType::Function) {
            let function = self.pop_operator()?;
            let mut parameters = Vec::new();
            loop {
                let node = self.pop_operand()?;
                if node.token.kind == TokenType::FunctionParamStart {
                    break;
                }
                parameters.push(node);
            }
            parameters.reverse();
            Self::validate_function_parameters(&function, parameters.len())?;
            self.operands.push(Node::with(function, parameters));
        }
        Ok(())
    }

    fn validate_function_parameters(token: &Token, count: usize) -> Outcome<()> {
        let Some(function) = token.function else {
            return Ok(());
        };
        if count < function.required_parameters() {
            return Err(Failure::parse_token(token, "Not enough parameters for function"));
        }
        if !function.has_var_args() && count > function.parameters.len() {
            return Err(Failure::parse_token(token, "Too many parameters for function"));
        }
        Ok(())
    }

    fn process_array_open(&mut self, current: Token) -> Outcome<()> {
        self.pop_structure_separators()?;
        let index = Token::new(current.start, current.value.clone(), TokenType::ArrayIndex);
        self.operators.push(index);
        self.operators.push(current);
        Ok(())
    }

    fn process_array_close(&mut self) -> Outcome<()> {
        self.process_until(TokenType::ArrayOpen)?;
        self.pop_operator()?;
        let array_token = self.pop_operator()?;
        let index = self.pop_operand()?;
        let array = self.pop_operand()?;
        self.operands.push(Node::with(array_token, vec![array, index]));
        Ok(())
    }

    fn process_until(&mut self, until: TokenType) -> Outcome<()> {
        while self.peek_kind().is_some_and(|kind| kind != until) {
            let token = self.pop_operator()?;
            self.create_operator_node(token)?;
        }
        Ok(())
    }

    fn create_operator_node(&mut self, token: Token) -> Outcome<()> {
        let Some(operand1) = self.operands.pop() else {
            return Err(Failure::parse_token(&token, "Missing operand for operator"));
        };
        if matches!(token.kind, TokenType::PrefixOperator | TokenType::PostfixOperator) {
            self.operands.push(Node::with(token, vec![operand1]));
        } else {
            let Some(operand2) = self.operands.pop() else {
                return Err(Failure::parse_token(&token, "Missing second operand for operator"));
            };
            self.operands.push(Node::with(token, vec![operand2, operand1]));
        }
        Ok(())
    }

    fn process_operator(&mut self, current: Token) -> Outcome<()> {
        while let Some(next) = self.operators.last() {
            let is_operator = matches!(
                next.kind,
                TokenType::InfixOperator
                    | TokenType::PrefixOperator
                    | TokenType::PostfixOperator
                    | TokenType::StructureSeparator
            );
            if !is_operator || !self.next_has_higher_precedence(current.operator, next.operator) {
                break;
            }
            let token = self.pop_operator()?;
            self.create_operator_node(token)?;
        }
        self.operators.push(current);
        Ok(())
    }

    fn next_has_higher_precedence(&self, current: Option<&Operator>, next: Option<&Operator>) -> bool {
        // A structure separator has no operator definition and binds tighter than every operator.
        let (Some(current), Some(next)) = (current, next) else {
            return true;
        };
        let current_precedence = current.precedence(self.configuration);
        let next_precedence = next.precedence(self.configuration);
        if current.left_associative {
            current_precedence <= next_precedence
        } else {
            current_precedence < next_precedence
        }
    }
}
