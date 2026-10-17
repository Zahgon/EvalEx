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

//! EvalEx: a mathematical and boolean expression evaluator.
//!
//! This crate is a Rust port of the Java library EvalEx 3.8.0. Expressions are tokenized ([`tokenizer`]), converted to a
//! syntax tree ([`parser`]) and evaluated ([`expression`]) with [`operators`] and [`functions`] on [`value`]s. The
//! behaviour of the Java runtime the library depends on lives in [`java`], and [`cli`] is the `evalex` command.

pub mod cli;
pub mod config;
pub mod error;
pub mod expression;
pub mod functions;
pub mod java;
pub mod operators;
pub mod parser;
pub mod token;
pub mod tokenizer;
pub mod value;
