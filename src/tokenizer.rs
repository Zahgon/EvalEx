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

//! Splits an expression into tokens.

use crate::config::Configuration;
use crate::error::{java_length, Failure, Outcome};
use crate::functions;
use crate::java::chars;
use crate::java::{js, JString};
use crate::operators;
use crate::token::{Token, TokenType};

const NOT_STARTED: i32 = -2;
const END: i32 = -1;

fn is_char(value: i32, ascii: u8) -> bool {
    value == i32::from(ascii)
}

fn is_digit(value: i32) -> bool {
    u16::try_from(value).is_ok_and(chars::is_digit)
}

fn is_letter(value: i32) -> bool {
    u16::try_from(value).is_ok_and(chars::is_letter)
}

/// The tokenizer of one expression.
pub struct Tokenizer<'a> {
    expression: &'a [u16],
    configuration: &'a Configuration,
    tokens: Vec<Token>,
    column: usize,
    current: i32,
    brace_balance: i32,
    array_balance: i32,
}

impl<'a> Tokenizer<'a> {
    /// A tokenizer positioned before the first character.
    pub fn new(expression: &'a [u16], configuration: &'a Configuration) -> Tokenizer<'a> {
        Tokenizer {
            expression,
            configuration,
            tokens: Vec::new(),
            column: 0,
            current: NOT_STARTED,
            brace_balance: 0,
            array_balance: 0,
        }
    }

    /// Reads all tokens, adding the implicit multiplications.
    pub fn parse(mut self) -> Outcome<Vec<Token>> {
        while let Some(token) = self.next_token()? {
            if self.implicit_multiplication_possible(&token) {
                if self.configuration.implicit_multiplication_allowed {
                    let mut multiplication = Token::new(token.start, js("*"), TokenType::InfixOperator);
                    multiplication.operator = operators::infix(&js("*"));
                    self.tokens.push(multiplication);
                } else {
                    return Err(Failure::parse_token(&token, "Missing operator"));
                }
            }
            self.validate_token(&token)?;
            self.tokens.push(token);
        }
        if self.brace_balance > 0 {
            return Err(Failure::parse_expression(self.expression, "Closing brace not found"));
        }
        if self.array_balance > 0 {
            return Err(Failure::parse_expression(self.expression, "Closing array not found"));
        }
        Ok(self.tokens)
    }

    fn previous_kind(&self) -> Option<TokenType> {
        self.tokens.last().map(|token| token.kind)
    }

    fn implicit_multiplication_possible(&self, current: &Token) -> bool {
        use TokenType::{BraceClose, BraceOpen, Function, NumberLiteral, VariableOrConstant};
        matches!(
            (self.previous_kind(), current.kind),
            (Some(BraceClose), BraceOpen) | (Some(NumberLiteral), VariableOrConstant | Function | BraceOpen)
        )
    }

    fn validate_token(&self, current: &Token) -> Outcome<()> {
        let previous = self.previous_kind();
        if current.kind == TokenType::StructureSeparator && previous.is_none() {
            return Err(Failure::parse_token(current, "Misplaced structure operator"));
        }
        if previous == Some(TokenType::InfixOperator) && Self::invalid_after_infix(current.kind) {
            return Err(Failure::parse_token(current, "Unexpected token after infix operator"));
        }
        Ok(())
    }

    fn invalid_after_infix(kind: TokenType) -> bool {
        matches!(kind, TokenType::InfixOperator | TokenType::BraceClose | TokenType::Comma)
    }

    fn next_token(&mut self) -> Outcome<Option<Token>> {
        self.skip_blanks();
        if self.current == END {
            return Ok(None);
        }
        let token = if self.at_string_literal_start() {
            self.parse_string_literal()?
        } else if is_char(self.current, b'(') {
            self.parse_brace_open()
        } else if is_char(self.current, b')') {
            self.parse_brace_close()?
        } else if is_char(self.current, b'[') && self.configuration.arrays_allowed {
            self.parse_array_open()?
        } else if is_char(self.current, b']') && self.configuration.arrays_allowed {
            self.parse_array_close()?
        } else if is_char(self.current, b'.')
            && !self.next_char_is_number_char()
            && self.configuration.structures_allowed
        {
            self.parse_structure_separator()?
        } else if is_char(self.current, b',') {
            let token = Token::new(self.position(), js(","), TokenType::Comma);
            self.consume();
            token
        } else if self.at_identifier_start() {
            self.parse_identifier()?
        } else if self.at_number_start() {
            self.parse_number_literal()?
        } else {
            self.parse_operator()?
        };
        Ok(Some(token))
    }

    fn position(&self) -> i32 {
        i32::try_from(self.column).unwrap_or(i32::MAX)
    }

    fn parse_structure_separator(&mut self) -> Outcome<Token> {
        let token = Token::new(self.position(), js("."), TokenType::StructureSeparator);
        if self.array_open_or_separator_not_allowed() {
            return Err(Failure::parse_token(&token, "Structure separator not allowed here"));
        }
        self.consume();
        Ok(token)
    }

    fn parse_array_close(&mut self) -> Outcome<Token> {
        let token = Token::new(self.position(), js("]"), TokenType::ArrayClose);
        if !self.array_close_allowed() {
            return Err(Failure::parse_token(&token, "Array close not allowed here"));
        }
        self.consume();
        self.array_balance -= 1;
        if self.array_balance < 0 {
            return Err(Failure::parse_token(&token, "Unexpected closing array"));
        }
        Ok(token)
    }

    fn parse_array_open(&mut self) -> Outcome<Token> {
        let token = Token::new(self.position(), js("["), TokenType::ArrayOpen);
        if self.array_open_or_separator_not_allowed() {
            return Err(Failure::parse_token(&token, "Array open not allowed here"));
        }
        self.consume();
        self.array_balance += 1;
        Ok(token)
    }

    fn parse_brace_close(&mut self) -> Outcome<Token> {
        let token = Token::new(self.position(), js(")"), TokenType::BraceClose);
        self.consume();
        self.brace_balance -= 1;
        if self.brace_balance < 0 {
            return Err(Failure::parse_token(&token, "Unexpected closing brace"));
        }
        Ok(token)
    }

    fn parse_brace_open(&mut self) -> Token {
        let token = Token::new(self.position(), js("("), TokenType::BraceOpen);
        self.consume();
        self.brace_balance += 1;
        token
    }

    fn parse_operator(&mut self) -> Outcome<Token> {
        let start = self.position();
        let mut value = JString::new();
        loop {
            value.push(self.current_unit());
            let mut possible = value.clone();
            possible.push(self.peek_next_unit());
            let found = (self.prefix_allowed() && operators::prefix(&possible).is_some())
                || (self.postfix_allowed() && operators::postfix(&possible).is_some())
                || (self.infix_allowed() && operators::infix(&possible).is_some());
            self.consume();
            if !found {
                break;
            }
        }
        if let Some(token) = self.create_operator_token(start, &value) {
            return Ok(token);
        }
        if value == [u16::from(b'.')] && self.configuration.structures_allowed {
            return Ok(Token::new(start, value, TokenType::StructureSeparator));
        }
        let end = start + java_length(&value) - 1;
        let mut message = js("Undefined operator '");
        message.extend_from_slice(&value);
        message.push(u16::from(b'\''));
        Err(Failure::parse_at(start, end, value, &message))
    }

    /// The code unit Java's `(char) currentChar` cast gives.
    fn current_unit(&self) -> u16 {
        self.current as u16
    }

    /// `(char) peekNextChar()`: the end of input casts to U+FFFF.
    fn peek_next_unit(&self) -> u16 {
        self.peek_next() as u16
    }

    fn create_operator_token(&self, start: i32, text: &[u16]) -> Option<Token> {
        let token = |operator, kind| {
            let mut token = Token::new(start, text.to_vec(), kind);
            token.operator = Some(operator);
            token
        };
        if self.prefix_allowed() {
            if let Some(operator) = operators::prefix(text) {
                return Some(token(operator, TokenType::PrefixOperator));
            }
        }
        if self.postfix_allowed() {
            if let Some(operator) = operators::postfix(text) {
                return Some(token(operator, TokenType::PostfixOperator));
            }
        }
        operators::infix(text).map(|operator| token(operator, TokenType::InfixOperator))
    }

    fn array_open_or_separator_not_allowed(&self) -> bool {
        !matches!(
            self.previous_kind(),
            Some(
                TokenType::BraceClose
                    | TokenType::VariableOrConstant
                    | TokenType::ArrayClose
                    | TokenType::StringLiteral
            )
        )
    }

    fn array_close_allowed(&self) -> bool {
        !matches!(
            self.previous_kind(),
            None | Some(
                TokenType::BraceOpen
                    | TokenType::InfixOperator
                    | TokenType::PrefixOperator
                    | TokenType::Function
                    | TokenType::Comma
                    | TokenType::ArrayOpen
            )
        )
    }

    fn prefix_allowed(&self) -> bool {
        matches!(
            self.previous_kind(),
            None | Some(
                TokenType::BraceOpen
                    | TokenType::InfixOperator
                    | TokenType::Comma
                    | TokenType::PrefixOperator
                    | TokenType::ArrayOpen
            )
        )
    }

    fn postfix_allowed(&self) -> bool {
        matches!(
            self.previous_kind(),
            Some(
                TokenType::BraceClose
                    | TokenType::NumberLiteral
                    | TokenType::VariableOrConstant
                    | TokenType::StringLiteral
            )
        )
    }

    fn infix_allowed(&self) -> bool {
        matches!(
            self.previous_kind(),
            Some(
                TokenType::BraceClose
                    | TokenType::VariableOrConstant
                    | TokenType::StringLiteral
                    | TokenType::PostfixOperator
                    | TokenType::NumberLiteral
                    | TokenType::ArrayClose
            )
        )
    }

    fn parse_number_literal(&mut self) -> Outcome<Token> {
        let next = self.peek_next();
        if is_char(self.current, b'0') && (is_char(next, b'x') || is_char(next, b'X')) {
            Ok(self.parse_hex_number_literal())
        } else {
            self.parse_decimal_number_literal()
        }
    }

    fn parse_decimal_number_literal(&mut self) -> Outcome<Token> {
        let start = self.position();
        let mut value = JString::new();
        let mut last = END;
        let mut scientific = false;
        let mut dot = false;
        while self.current != END && self.at_number_char() {
            if is_char(self.current, b'.') && dot {
                value.push(self.current_unit());
                let token = Token::new(start, value, TokenType::NumberLiteral);
                return Err(Failure::parse_token(&token, "Number contains more than one decimal point"));
            }
            dot |= is_char(self.current, b'.');
            scientific |= is_char(self.current, b'e') || is_char(self.current, b'E');
            value.push(self.current_unit());
            last = self.current;
            self.consume();
        }
        let token = Token::new(start, value, TokenType::NumberLiteral);
        if scientific && [b'e', b'E', b'+', b'-', b'.'].iter().any(|&c| is_char(last, c)) {
            return Err(Failure::parse_token(&token, "Illegal scientific format"));
        }
        Ok(token)
    }

    fn parse_hex_number_literal(&mut self) -> Token {
        let start = self.position();
        let mut value = vec![self.current_unit()];
        self.consume();
        loop {
            value.push(self.current_unit());
            self.consume();
            if self.current == END || !self.at_hex_char() {
                break;
            }
        }
        Token::new(start, value, TokenType::NumberLiteral)
    }

    fn parse_identifier(&mut self) -> Outcome<Token> {
        let start = self.position();
        let mut name = JString::new();
        while self.current != END && self.at_identifier_char() {
            name.push(self.current_unit());
            self.consume();
        }
        if let Some(token) = self.create_operator_token(start, &name) {
            return Ok(token);
        }
        self.skip_blanks();
        if is_char(self.current, b'(') {
            let Some(function) = functions::find(&name) else {
                let mut message = js("Undefined function '");
                message.extend_from_slice(&name);
                message.push(u16::from(b'\''));
                return Err(Failure::parse_at(start, self.position(), name, &message));
            };
            let mut token = Token::new(start, name, TokenType::Function);
            token.function = Some(function);
            Ok(token)
        } else {
            Ok(Token::new(start, name, TokenType::VariableOrConstant))
        }
    }

    fn parse_string_literal(&mut self) -> Outcome<Token> {
        let quote = self.current;
        let start = self.position();
        let mut value = JString::new();
        self.consume();
        let mut in_quote = true;
        while in_quote && self.current != END {
            if is_char(self.current, b'\\') {
                self.consume();
                value.push(self.escape_character()?);
            } else if self.current == quote {
                in_quote = false;
            } else {
                value.push(self.current_unit());
            }
            self.consume();
        }
        if in_quote {
            return Err(Failure::parse_at(start, self.position(), value, &js("Closing quote not found")));
        }
        Ok(Token::new(start, value, TokenType::StringLiteral))
    }

    fn escape_character(&self) -> Outcome<u16> {
        match u8::try_from(self.current) {
            Ok(b'\'') => Ok(u16::from(b'\'')),
            Ok(b'"') => Ok(u16::from(b'"')),
            Ok(b'\\') => Ok(u16::from(b'\\')),
            Ok(b'n') => Ok(0x0a),
            Ok(b'r') => Ok(0x0d),
            Ok(b't') => Ok(0x09),
            Ok(b'b') => Ok(0x08),
            Ok(b'f') => Ok(0x0c),
            _ => {
                let token = vec![u16::from(b'\\'), self.current_unit()];
                Err(Failure::parse_at(self.position(), 1, token, &js("Unknown escape character")))
            }
        }
    }

    fn at_number_start(&self) -> bool {
        is_digit(self.current) || (is_char(self.current, b'.') && is_digit(self.peek_next()))
    }

    fn at_number_char(&self) -> bool {
        let previous = self.peek_previous();
        let current = self.current;
        let exponent = is_char(current, b'e') || is_char(current, b'E');
        if (is_char(previous, b'e') || is_char(previous, b'E')) && !is_char(current, b'.') {
            return is_digit(current) || is_char(current, b'+') || is_char(current, b'-');
        }
        if is_char(previous, b'.') && !is_char(current, b'.') {
            return is_digit(current) || exponent;
        }
        is_digit(current) || is_char(current, b'.') || exponent
    }

    fn next_char_is_number_char(&mut self) -> bool {
        if self.peek_next() == END {
            return false;
        }
        self.consume();
        let at_number = self.at_number_char();
        self.column -= 1;
        self.current = self.char_at(self.column.wrapping_sub(1));
        at_number
    }

    fn at_hex_char(&self) -> bool {
        u8::try_from(self.current).is_ok_and(|byte| byte.is_ascii_hexdigit())
    }

    fn at_identifier_start(&self) -> bool {
        is_letter(self.current) || is_char(self.current, b'_')
    }

    fn at_identifier_char(&self) -> bool {
        is_letter(self.current) || is_digit(self.current) || is_char(self.current, b'_')
    }

    fn at_string_literal_start(&self) -> bool {
        is_char(self.current, b'"')
            || (is_char(self.current, b'\'') && self.configuration.single_quote_string_literals_allowed)
    }

    fn skip_blanks(&mut self) {
        if self.current == NOT_STARTED {
            self.consume();
        }
        while self.current != END && u16::try_from(self.current).is_ok_and(chars::is_whitespace) {
            self.consume();
        }
    }

    fn char_at(&self, index: usize) -> i32 {
        self.expression.get(index).map_or(END, |&unit| i32::from(unit))
    }

    fn peek_next(&self) -> i32 {
        self.char_at(self.column)
    }

    fn peek_previous(&self) -> i32 {
        if self.column == 1 {
            END
        } else {
            self.char_at(self.column.wrapping_sub(2))
        }
    }

    fn consume(&mut self) {
        if self.column == self.expression.len() {
            self.current = END;
        } else {
            self.current = self.char_at(self.column);
            self.column += 1;
        }
    }
}
