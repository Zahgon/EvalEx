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

//! The `evalex` command end to end.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_evalex"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the command starts");
    child.stdin.take().expect("standard input is piped").write_all(input).expect("the requests are written");
    child.wait_with_output().expect("the command finishes")
}

fn answer(request: &str) -> String {
    let output = run(&[], format!("{request}\n").as_bytes());
    String::from_utf8(output.stdout).expect("responses are UTF-8")
}

#[test]
fn evaluates_a_number_with_all_printed_forms() {
    assert_eq!(
        answer(r#"{"expression":"1.50*2"}"#),
        "{\"type\":\"NUMBER\",\"text\":\"3\",\"string\":\"3\",\"plain\":\"3\",\"engineering\":\"3\",\"scale\":0,\"precision\":1}\n"
    );
}

#[test]
fn reports_failures_with_positions_and_exit_status() {
    let output = run(&[], b"{\"expression\":\"1 +\"}\n");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "{\"error\":{\"class\":\"com.ezylang.evalex.parser.ParseException\",\"message\":\"Missing second operand for operator\",\"start\":3,\"end\":3,\"token\":\"+\"}}\n"
    );
}

#[test]
fn rejects_arguments_with_the_usage_text() {
    let output = run(&["--help"], b"");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("usage: evalex"));
}

#[test]
fn skips_empty_lines_and_keeps_order() {
    let output =
        run(&[], b"\n{\"expression\":\"1\",\"op\":\"validate\"}\n\n{\"expression\":\"a+b\",\"op\":\"variables\"}\n");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "{\"valid\":true}\n{\"used\":[\"a\",\"b\"],\"undefined\":[\"a\",\"b\"]}\n"
    );
}

#[test]
fn prints_the_syntax_tree() {
    assert_eq!(
        answer(r#"{"expression":"-a","op":"ast"}"#),
        "{\"ast\":\"{\\\"type\\\":\\\"PREFIX_OPERATOR\\\",\\\"value\\\":\\\"-\\\",\\\"children\\\":[{\\\"type\\\":\\\"VARIABLE_OR_CONSTANT\\\",\\\"value\\\":\\\"a\\\"}]}\"}\n"
    );
}

#[test]
fn prints_structures_in_hash_map_order() {
    let response = answer(r#"{"expression":"s","variables":{"s":{"x":1,"y":2,"zz":3}}}"#);
    let zz = response.find("[\"zz\"").expect("zz is listed");
    let x = response.find("[\"x\"").expect("x is listed");
    assert!(zz < x, "{response}");
}

#[test]
fn rejects_malformed_json() {
    assert_eq!(
        answer("{"),
        "{\"error\":{\"class\":\"EvalExCli$RequestError\",\"message\":\"malformed JSON at index 1\"}}\n"
    );
}
