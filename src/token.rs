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

//! The tokens of an expression and the nodes of its syntax tree.

use std::rc::Rc;

use crate::functions::Function;
use crate::java::JString;
use crate::operators::Operator;

/// The kind of a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenType {
    /// `(`
    BraceOpen,
    /// `)`
    BraceClose,
    /// `,`
    Comma,
    /// A quoted string.
    StringLiteral,
    /// A number.
    NumberLiteral,
    /// A name that is not followed by `(`.
    VariableOrConstant,
    /// An operator between two operands.
    InfixOperator,
    /// An operator before its operand.
    PrefixOperator,
    /// An operator after its operand.
    PostfixOperator,
    /// A function name.
    Function,
    /// The start of a function's parameter list, on the operand stack.
    FunctionParamStart,
    /// `[`
    ArrayOpen,
    /// `]`
    ArrayClose,
    /// An array index, created while parsing.
    ArrayIndex,
    /// `.`
    StructureSeparator,
}

impl TokenType {
    /// The name of the Java enum constant.
    pub fn name(self) -> &'static str {
        match self {
            TokenType::BraceOpen => "BRACE_OPEN",
            TokenType::BraceClose => "BRACE_CLOSE",
            TokenType::Comma => "COMMA",
            TokenType::StringLiteral => "STRING_LITERAL",
            TokenType::NumberLiteral => "NUMBER_LITERAL",
            TokenType::VariableOrConstant => "VARIABLE_OR_CONSTANT",
            TokenType::InfixOperator => "INFIX_OPERATOR",
            TokenType::PrefixOperator => "PREFIX_OPERATOR",
            TokenType::PostfixOperator => "POSTFIX_OPERATOR",
            TokenType::Function => "FUNCTION",
            TokenType::FunctionParamStart => "FUNCTION_PARAM_START",
            TokenType::ArrayOpen => "ARRAY_OPEN",
            TokenType::ArrayClose => "ARRAY_CLOSE",
            TokenType::ArrayIndex => "ARRAY_INDEX",
            TokenType::StructureSeparator => "STRUCTURE_SEPARATOR",
        }
    }
}

/// One token of an expression.
#[derive(Clone, Debug)]
pub struct Token {
    /// One-based position in UTF-16 code units.
    pub start: i32,
    /// The token's text, unescaped for string literals.
    pub value: JString,
    /// The kind of token.
    pub kind: TokenType,
    /// The function a [`TokenType::Function`] token calls.
    pub function: Option<&'static Function>,
    /// The operator of an operator token.
    pub operator: Option<&'static Operator>,
}

impl Token {
    /// A token without a function or operator definition.
    pub fn new(start: i32, value: JString, kind: TokenType) -> Token {
        Token { start, value, kind, function: None, operator: None }
    }

    /// `Token.toString()`.
    pub fn to_java_string(&self) -> JString {
        let mut out = crate::java::js(&format!("Token(startPosition={}, value=", self.start));
        out.extend_from_slice(&self.value);
        out.extend(format!(", type={})", self.kind.name()).encode_utf16());
        out
    }
}

/// A node of the abstract syntax tree.
#[derive(Debug)]
pub struct Node {
    /// The token this node stands for.
    pub token: Token,
    /// Its operands or parameters.
    pub parameters: Vec<Rc<Node>>,
}

impl Node {
    /// A node without parameters.
    pub fn leaf(token: Token) -> Rc<Node> {
        Rc::new(Node { token, parameters: Vec::new() })
    }

    /// A node with parameters.
    pub fn with(token: Token, parameters: Vec<Rc<Node>>) -> Rc<Node> {
        Rc::new(Node { token, parameters })
    }

    /// `ASTNode.toJSON()`: nothing in the token text is escaped.
    pub fn to_json(&self) -> JString {
        let mut out = crate::java::js(&format!("{{\"type\":\"{}\",\"value\":\"", self.token.kind.name()));
        out.extend_from_slice(&self.token.value);
        out.push(u16::from(b'"'));
        if !self.parameters.is_empty() {
            out.extend(",\"children\":[".encode_utf16());
            for (index, child) in self.parameters.iter().enumerate() {
                if index > 0 {
                    out.push(u16::from(b','));
                }
                out.extend(child.to_json());
            }
            out.push(u16::from(b']'));
        }
        out.push(u16::from(b'}'));
        out
    }

    /// `ASTNode.toString()`.
    pub fn to_java_string(&self) -> JString {
        let mut out = crate::java::js("ASTNode(parameters=[");
        for (index, child) in self.parameters.iter().enumerate() {
            if index > 0 {
                out.extend(", ".encode_utf16());
            }
            out.extend(child.to_java_string());
        }
        out.extend("], token=".encode_utf16());
        out.extend(self.token.to_java_string());
        out.push(u16::from(b')'));
        out
    }

    /// This node and everything below it, parents first.
    pub fn all_nodes(self: &Rc<Node>) -> Vec<Rc<Node>> {
        let mut nodes = Vec::new();
        let mut pending = vec![Rc::clone(self)];
        while let Some(node) = pending.pop() {
            pending.extend(node.parameters.iter().rev().cloned());
            nodes.push(node);
        }
        nodes
    }
}
