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

//! The `evalex` command.

use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::thread;

fn answer(input: Vec<u8>) -> (String, bool) {
    let text = String::from_utf8_lossy(&input);
    evalex::cli::run(&text)
}

fn main() -> ExitCode {
    if std::env::args_os().len() > 1 {
        eprint!("{}", evalex::cli::USAGE);
        return ExitCode::from(2);
    }
    let mut input = Vec::new();
    if let Err(error) = io::stdin().read_to_end(&mut input) {
        eprintln!("evalex: cannot read standard input: {error}");
        return ExitCode::FAILURE;
    }
    let worker =
        thread::Builder::new().stack_size(evalex::expression::REQUIRED_STACK_BYTES).spawn(move || answer(input));
    let answered = worker.map(thread::JoinHandle::join);
    let (output, failed) = match answered {
        Ok(Ok(answer)) => answer,
        _ => {
            eprintln!("evalex: the evaluation thread failed");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = io::stdout().write_all(output.as_bytes()) {
        eprintln!("evalex: cannot write standard output: {error}");
        return ExitCode::FAILURE;
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
