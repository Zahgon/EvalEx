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

//! The configuration of an expression.

use crate::java::bigdecimal::{MathContext, RoundingMode};
use crate::java::locale::Locale;
use crate::java::time::{DateTimeFormatter, ZoneId};
use crate::java::JResult;

/// The value of the decimal places settings that turns rounding off.
pub const DECIMAL_PLACES_ROUNDING_UNLIMITED: i32 = -1;
/// Default precision of the math context.
pub const DEFAULT_PRECISION: i32 = 68;
/// Default rounding mode of the math context.
pub const DEFAULT_ROUNDING: RoundingMode = RoundingMode::HalfEven;
/// Default limit of the evaluation depth.
pub const DEFAULT_MAX_RECURSION_DEPTH: i32 = 2000;
/// Precedence of `||`.
pub const PRECEDENCE_OR: i32 = 2;
/// Precedence of `&&`.
pub const PRECEDENCE_AND: i32 = 4;
/// Precedence of `=`, `==`, `!=` and `<>`.
pub const PRECEDENCE_EQUALITY: i32 = 7;
/// Precedence of `>`, `>=`, `<` and `<=`.
pub const PRECEDENCE_COMPARISON: i32 = 10;
/// Precedence of `+` and `-`.
pub const PRECEDENCE_ADDITIVE: i32 = 20;
/// Precedence of `*`, `/` and `%`.
pub const PRECEDENCE_MULTIPLICATIVE: i32 = 30;
/// Default precedence of `^`.
pub const PRECEDENCE_POWER: i32 = 40;
/// Precedence of the prefix operators.
pub const PRECEDENCE_UNARY: i32 = 60;

/// The settings of an expression. `Default` holds the library's defaults.
#[derive(Clone, Debug)]
pub struct Configuration {
    /// Precision and rounding of arithmetic.
    pub math_context: MathContext,
    /// Whether `[` and `]` are array operators.
    pub arrays_allowed: bool,
    /// Whether `.` is the structure operator.
    pub structures_allowed: bool,
    /// Whether `2x` means `2*x`.
    pub implicit_multiplication_allowed: bool,
    /// Whether `'` also delimits string literals.
    pub single_quote_string_literals_allowed: bool,
    /// Whether unknown variables and fields evaluate to the undefined value.
    pub lenient_mode: bool,
    /// Precedence of `^`.
    pub power_of_precedence: i32,
    /// Decimal places of the final result, or [`DECIMAL_PLACES_ROUNDING_UNLIMITED`].
    pub decimal_places_result: i32,
    /// Decimal places of every intermediate value, or [`DECIMAL_PLACES_ROUNDING_UNLIMITED`].
    pub decimal_places_rounding: i32,
    /// Whether the final number loses its trailing zeros.
    pub strip_trailing_zeros: bool,
    /// Whether a variable may replace a constant.
    pub allow_overwrite_constants: bool,
    /// Default time zone.
    pub zone_id: ZoneId,
    /// Default locale.
    pub locale: Locale,
    /// Limit of the evaluation depth.
    pub max_recursion_depth: i32,
    /// The formatters tried, in order, when a date-time is parsed; the first one also formats.
    pub date_time_formatters: Vec<DateTimeFormatter>,
}

impl Configuration {
    /// The library's defaults with the given time zone and locale.
    pub fn with_defaults(zone_id: ZoneId, locale: Locale) -> JResult<Configuration> {
        Ok(Configuration {
            math_context: MathContext::new(DEFAULT_PRECISION, DEFAULT_ROUNDING)?,
            arrays_allowed: true,
            structures_allowed: true,
            implicit_multiplication_allowed: true,
            single_quote_string_literals_allowed: false,
            lenient_mode: false,
            power_of_precedence: PRECEDENCE_POWER,
            decimal_places_result: DECIMAL_PLACES_ROUNDING_UNLIMITED,
            decimal_places_rounding: DECIMAL_PLACES_ROUNDING_UNLIMITED,
            strip_trailing_zeros: true,
            allow_overwrite_constants: true,
            zone_id,
            locale,
            max_recursion_depth: DEFAULT_MAX_RECURSION_DEPTH,
            date_time_formatters: vec![
                DateTimeFormatter::iso_date_time(),
                DateTimeFormatter::iso_date(),
                DateTimeFormatter::iso_local_date_time(),
                DateTimeFormatter::iso_local_date(),
                DateTimeFormatter::rfc_1123_date_time(),
            ],
        })
    }
}
