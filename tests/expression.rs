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

//! Evaluation through the library: the behaviour of the original's test suite.

use evalex::config::Configuration;
use evalex::expression::Expression;
use evalex::java::js;
use evalex::java::locale::Locale;
use evalex::java::time::ZoneId;
use evalex::value::Value;

fn configuration() -> Configuration {
    let zone = ZoneId::of(&js("UTC")).expect("UTC is a zone");
    Configuration::with_defaults(zone, Locale::US).expect("the defaults are valid")
}

fn evaluate(text: &str) -> Value {
    let expression = Expression::new(js(text), configuration()).expect("the constants parse");
    expression.evaluate().expect("the expression evaluates")
}

fn failure(text: &str) -> String {
    let expression = Expression::new(js(text), configuration()).expect("the constants parse");
    let failure = expression.evaluate().expect_err("the expression fails");
    String::from_utf16_lossy(&failure.thrown.message.unwrap_or_default())
}

fn number(text: &str) -> String {
    match evaluate(text) {
        Value::Number(number) => number.to_string(),
        other => panic!("{text} gave {}", other.type_name()),
    }
}

fn string(text: &str) -> String {
    match evaluate(text) {
        Value::Str(value) => String::from_utf16_lossy(&value),
        other => panic!("{text} gave {}", other.type_name()),
    }
}

fn boolean(text: &str) -> bool {
    match evaluate(text) {
        Value::Boolean(value) => value,
        other => panic!("{text} gave {}", other.type_name()),
    }
}

#[test]
fn respects_precedence_and_associativity() {
    assert_eq!(number("2+3*4"), "14");
    assert_eq!(number("2^3^2"), "512");
    assert_eq!(number("-2^2"), "4");
    assert_eq!(number("2(3+4)"), "14");
}

#[test]
fn divides_with_the_math_context() {
    assert_eq!(number("1/4"), "0.25");
    assert_eq!(number("2/3").len(), 70);
    assert_eq!(failure("1/0"), "Division by zero");
}

#[test]
fn prints_doubles_as_java_does() {
    assert_eq!(number("SIN(30)"), "0.49999999999999994");
    assert_eq!(number("LOG10(1000)"), "3");
    assert_eq!(number("DEG(PI)"), "1.8E+2");
}

#[test]
fn compares_across_types_like_the_library() {
    assert!(boolean("1.0 = 1.00"));
    assert!(!boolean("1 = \"1\""));
    assert!(boolean("NULL = NULL"));
}

#[test]
fn control_functions_are_lazy() {
    assert_eq!(number("IF(TRUE,1,1/0)"), "1");
    assert_eq!(number("SWITCH(2,1,1/0,2,5)"), "5");
    assert!(boolean("TRUE || 1/0"));
}

#[test]
fn string_functions_count_code_units() {
    assert_eq!(number("STR_LENGTH(\"😀\")"), "2");
    assert_eq!(string("STR_UPPER(\"straße\")"), "STRASSE");
    assert_eq!(string("STR_SUBSTRING(\"abcdef\",1,3)"), "bc");
}

#[test]
fn date_times_use_zone_rules() {
    assert_eq!(
        string("DT_DATE_FORMAT(DT_DATE_NEW(2024,1,15,12,0,0,\"Europe/Paris\"),\"HH:mm\",\"Asia/Kolkata\")"),
        "16:30"
    );
    assert_eq!(number("DT_DATE_TO_EPOCH(DT_DATE_NEW(1970,1,2))"), "8.64E+7");
}

#[test]
fn reports_unknown_names() {
    assert_eq!(failure("x"), "Variable or constant value for 'x' not found");
    assert_eq!(failure("FOO(1)"), "Undefined function 'FOO'");
}
