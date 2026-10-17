# EvalEx agent guide

EvalEx is a Rust crate that parses and evaluates expressions. It has no dependencies. The behaviour of the Java library
it was ported from, including that of the Java runtime, is its specification.

- Build with `cargo build --release`, and run `cargo test`, `cargo fmt --check` and
  `cargo clippy --all-targets -- -D warnings` before finishing a change.
- Keep the language core in `src/` and Java runtime behaviour in `src/java/`.
- Errors are `Result` values. There is no `unsafe`, and no input may panic.
- Add a test for every behaviour you change.
- Keep the licence header on source files.
