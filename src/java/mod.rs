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

//! Behaviour of the Java runtime that EvalEx inherits from the JDK.

pub mod bigdecimal;
pub mod biginteger;
pub mod chars;
pub mod decimalformat;
pub mod double;
pub mod fdlibm;
pub mod formatter;
pub mod hashmap;
pub mod integer;
pub mod locale;
pub mod regex;
pub mod time;

/// A Java string: UTF-16 code units, lone surrogates allowed.
pub type JString = Vec<u16>;

/// Encodes a Rust string as a [`JString`].
pub fn js(text: &str) -> JString {
    text.encode_utf16().collect()
}

/// A Java exception: its fully qualified class name and `getMessage()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thrown {
    /// Fully qualified class name, for example `java.lang.ArithmeticException`.
    pub class: String,
    /// The value of `getMessage()`.
    pub message: Option<JString>,
}

impl Thrown {
    /// An exception with a message.
    pub fn new(class: &str, message: &str) -> Thrown {
        Thrown::with_message(class, js(message))
    }

    /// An exception whose message is already a Java string.
    pub fn with_message(class: &str, message: JString) -> Thrown {
        Thrown { class: class.to_string(), message: Some(message) }
    }

    /// An exception without a message.
    pub fn no_message(class: &str) -> Thrown {
        Thrown { class: class.to_string(), message: None }
    }
}

/// The result of a Java operation that may throw.
pub type JResult<T> = Result<T, Thrown>;
