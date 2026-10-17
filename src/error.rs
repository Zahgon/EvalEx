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

//! The failures of parsing and evaluating an expression.

use crate::java::{js, JString, Thrown};
use crate::token::Token;

/// Class name of a parse failure.
pub const PARSE_EXCEPTION: &str = "com.ezylang.evalex.parser.ParseException";
/// Class name of an evaluation failure.
pub const EVALUATION_EXCEPTION: &str = "com.ezylang.evalex.EvaluationException";

/// Where in the expression a failure was detected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Start position, one-based in UTF-16 code units.
    pub start: i32,
    /// End position.
    pub end: i32,
    /// The text of the token concerned.
    pub token: JString,
}

/// A failure: the exception thrown, with its position when the library reports one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// Exception class and message.
    pub thrown: Thrown,
    /// Position in the expression.
    pub span: Option<Span>,
}

impl Failure {
    /// A parse failure at explicit positions.
    pub fn parse_at(start: i32, end: i32, token: JString, message: &[u16]) -> Failure {
        Failure {
            thrown: Thrown::with_message(PARSE_EXCEPTION, message.to_vec()),
            span: Some(Span { start, end, token }),
        }
    }

    /// A parse failure at a token; the end position is that of the token's last character.
    pub fn parse_token(token: &Token, message: &str) -> Failure {
        let end = token.start + java_length(&token.value) - 1;
        Failure::parse_at(token.start, end, token.value.clone(), &js(message))
    }

    /// A parse failure about the whole expression.
    pub fn parse_expression(expression: &[u16], message: &str) -> Failure {
        Failure::parse_at(1, java_length(expression), expression.to_vec(), &js(message))
    }

    /// An evaluation failure at a token; the end position is one past the token's last character.
    pub fn evaluation(token: &Token, message: &[u16]) -> Failure {
        let end = token.start + java_length(&token.value);
        Failure {
            thrown: Thrown::with_message(EVALUATION_EXCEPTION, message.to_vec()),
            span: Some(Span { start: token.start, end, token: token.value.clone() }),
        }
    }

    /// An evaluation failure with a plain message.
    pub fn evaluation_str(token: &Token, message: &str) -> Failure {
        Failure::evaluation(token, &js(message))
    }

    /// The failure of an operation on values of types it does not support.
    pub fn unsupported_types(token: &Token) -> Failure {
        Failure::evaluation_str(token, "Unsupported data types in operation")
    }

    /// `Objects.requireNonNull` on a null value.
    pub fn null_pointer() -> Failure {
        Thrown::no_message("java.lang.NullPointerException").into()
    }

    /// A `NullPointerException` with the JVM's helpful message.
    pub fn null_pointer_message(message: &str) -> Failure {
        Thrown::new("java.lang.NullPointerException", message).into()
    }
}

impl From<Thrown> for Failure {
    fn from(thrown: Thrown) -> Failure {
        Failure { thrown, span: None }
    }
}

/// `String.length()` as the `int` the library computes positions with.
pub(crate) fn java_length(text: &[u16]) -> i32 {
    i32::try_from(text.len()).unwrap_or(i32::MAX)
}

/// The result of parsing or evaluating.
pub type Outcome<T> = Result<T, Failure>;
