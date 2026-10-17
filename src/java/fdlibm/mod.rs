// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Permission to use, copy, modify, and distribute this software is freely granted, provided that
// this notice is preserved.

//! Rust port of Sun's fdlibm 5.3, the algorithms `java.lang.StrictMath` is specified by.
//!
//! Every function returns the same bits as its `StrictMath` counterpart for every input, apart from
//! the payload and sign of NaN results. The code therefore follows the C sources operation by
//! operation: reordering or "simplifying" a floating-point expression changes results in the last
//! bit. Statements that exist only to raise IEEE flags in C are dropped, since Rust cannot observe
//! them.

mod exp_log;
mod hyperbolic;
mod inverse_trig;
mod pow;
mod rem_pio2;
mod trig;

pub use exp_log::{exp, log, log10};
pub use hyperbolic::{cosh, sinh, tanh};
pub use inverse_trig::{acos, asin, atan, atan2};
pub use pow::pow;
pub use trig::{cos, sin, tan};

/// 2^-54, the last factor of [`scalbn`] for subnormal results.
const TWO_M54: f64 = f64::from_bits(0x3C90_0000_0000_0000);

/// The high 32 bits of `x`: sign, exponent and top of the mantissa.
fn high_word(x: f64) -> i32 {
    (x.to_bits() >> 32) as i32
}

/// The low 32 bits of the mantissa of `x`.
fn low_word(x: f64) -> u32 {
    x.to_bits() as u32
}

/// `x` with its high word replaced by `high`.
fn with_high_word(x: f64, high: i32) -> f64 {
    f64::from_bits((x.to_bits() & 0xFFFF_FFFF) | (u64::from(high as u32) << 32))
}

/// The double with high word `high` and a zero low word.
fn from_high_word(high: i32) -> f64 {
    with_high_word(0.0, high)
}

/// `x` with its low word cleared.
fn clear_low_word(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & 0xFFFF_FFFF_0000_0000)
}

/// Adds `k` to the binary exponent field of `x`, which wraps exactly as the C `__HI(x) += k << 20`.
fn add_to_exponent(x: f64, k: i32) -> f64 {
    with_high_word(x, high_word(x).wrapping_add(k << 20))
}

/// `x * 2^n` for a zero or normal `x` whose result does not overflow or fall below 2^-1075.
///
/// A subnormal result is rounded only once, by the final multiplication.
fn scalbn(x: f64, n: i32) -> f64 {
    if x == 0.0 {
        return x;
    }
    let exponent = ((high_word(x) >> 20) & 0x7FF) + n;
    if exponent > 0 {
        add_to_exponent(x, n)
    } else {
        add_to_exponent(x, n + 54) * TWO_M54
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_accessors_split_and_rebuild_a_double() {
        assert_eq!(high_word(1.5), 0x3FF8_0000);
        assert_eq!(low_word(f64::from_bits(0x0000_0001_DEAD_BEEF)), 0xDEAD_BEEF);
        assert_eq!(from_high_word(0x4000_0000), 2.0);
        assert_eq!(high_word(-2.0), 0xC000_0000_u32 as i32);
        assert_eq!(clear_low_word(f64::from_bits(0x3FF0_0000_FFFF_FFFF)), 1.0);
    }

    #[test]
    fn scalbn_rounds_subnormal_results_once() {
        assert_eq!(scalbn(0.0, 10), 0.0);
        assert_eq!(scalbn(1.5, 3), 12.0);
        assert_eq!(scalbn(1.0, -1074).to_bits(), 1);
        assert_eq!(scalbn(1.5, -1074).to_bits(), 2);
        assert_eq!(scalbn(1.0, -1075).to_bits(), 0);
    }
}
