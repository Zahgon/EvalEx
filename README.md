EvalEx - Expression Evaluator for Rust
======================================

EvalEx is an expression evaluator that parses and evaluates expression strings. This crate is a Rust port of the Java
library [EvalEx](https://github.com/ezylang/EvalEx) 3.8.0. It has no dependencies and reproduces the Java library's
results exactly, including the behaviour of the Java runtime under it.

## Key features

- Numbers are arbitrary-precision decimals with a configurable math context and rounding mode.
- Mathematical and boolean operators, with implicit multiplication.
- Basic, trigonometric, string and date-time functions.
- Variables, constants, arrays, structures and lazily evaluated parameters.
- Dates, times and durations with time zone rules for the common regions.
- The pages under `docs` describe the expression language, its functions and its configuration.

## Build and test

```
cargo build --release
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

## The evalex command

`evalex` reads one JSON request per line on standard input and writes one JSON response per line:

```
$ echo '{"expression":"a*(2+b)","variables":{"a":1.5,"b":2}}' | evalex
{"type":"NUMBER","text":"6","string":"6","plain":"6","engineering":"6","scale":0,"precision":1}
```

A request has the members `expression`, `op` (`evaluate`, `validate`, `variables` or `ast`), `variables` and `config`.
A failed request answers with `{"error":{"class":...,"message":...}}` and the exit status is 1. Any command-line
argument prints the usage text.

## Library use

```rust
use evalex::config::Configuration;
use evalex::expression::Expression;
use evalex::java::js;
use evalex::java::locale::Locale;
use evalex::java::time::ZoneId;

let zone = ZoneId::of(&js("UTC"))?;
let configuration = Configuration::with_defaults(zone, Locale::US)?;
let mut expression = Expression::new(js("a + 1"), configuration)?;
expression.with(&js("a"), evalex::value::Value::string("x"))?;
let result = expression.evaluate()?;
```

## Layout

| Path | Contents |
| :-- | :-- |
| `src/tokenizer.rs`, `src/parser.rs` | Tokens and the shunting yard parser |
| `src/expression.rs`, `src/value.rs` | Evaluation and values |
| `src/operators.rs`, `src/functions/` | The operators and the function dictionary |
| `src/java/` | Java runtime behaviour: `BigDecimal`, `StrictMath`, `Double`, `Formatter`, `DecimalFormat`, `java.util.regex`, `java.time`, `HashMap` |
| `src/cli/` | The `evalex` command |
| `tests/` | Tests of the library and the command |

## License

Apache License 2.0, see `LICENSE`. `src/java/fdlibm` is derived from fdlibm and keeps its notice.
