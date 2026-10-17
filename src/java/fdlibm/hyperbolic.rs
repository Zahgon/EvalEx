// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Permission to use, copy, modify, and distribute this software is freely granted, provided that
// this notice is preserved.

//! Hyperbolic functions (`e_sinh.c`, `e_cosh.c`, `s_tanh.c`) and their helper `expm1` (`s_expm1.c`).

use super::exp_log::{exp, INV_LN2, LN2_HI, LN2_LO, O_THRESHOLD};
use super::{add_to_exponent, from_high_word, high_word, low_word};

const Q1: f64 = f64::from_bits(0xBFA1_1111_1111_10F4);
const Q2: f64 = f64::from_bits(0x3F5A_01A0_19FE_5585);
const Q3: f64 = f64::from_bits(0xBF14_CE19_9EAA_DBB7);
const Q4: f64 = f64::from_bits(0x3ED0_CFCA_86E6_5239);
const Q5: f64 = f64::from_bits(0xBE8A_FDB7_6E09_C32D);

const SHUGE: f64 = 1.0e307;
const TINY: f64 = 1.0e-300;

/// `exp(x) - 1`, accurate near zero.
fn expm1(x: f64) -> f64 {
    let negative = high_word(x) < 0;
    let ix = high_word(x) & 0x7FFF_FFFF;

    if ix >= 0x4043_687A {
        if ix >= 0x4086_2E42 {
            if x.is_nan() {
                return x + x;
            }
            if ix >= 0x7FF0_0000 {
                return if negative { -1.0 } else { x };
            }
            if x > O_THRESHOLD {
                return f64::INFINITY;
            }
        }
        if negative {
            return -1.0;
        }
    }

    // Reduce to x = k * ln2 + r, keeping the rounding error of r in c.
    let (r, c, k) = if ix > 0x3FD6_2E42 {
        let (hi, lo, k) = if ix < 0x3FF0_A2B2 {
            if negative {
                (x + LN2_HI, -LN2_LO, -1)
            } else {
                (x - LN2_HI, LN2_LO, 1)
            }
        } else {
            let half = if negative { -0.5 } else { 0.5 };
            let k = (INV_LN2 * x + half) as i32;
            let t = f64::from(k);
            (x - t * LN2_HI, t * LN2_LO, k)
        };
        let r = hi - lo;
        (r, (hi - r) - lo, k)
    } else if ix < 0x3C90_0000 {
        return x;
    } else {
        (x, 0.0, 0)
    };

    let hfx = 0.5 * r;
    let hxs = r * hfx;
    let r1 = 1.0 + hxs * (Q1 + hxs * (Q2 + hxs * (Q3 + hxs * (Q4 + hxs * Q5))));
    let t = 3.0 - r1 * hfx;
    let e = hxs * ((r1 - t) / (6.0 - r * t));
    if k == 0 {
        return r - (r * e - hxs);
    }
    let e = (r * (e - c) - c) - hxs;
    match k {
        -1 => 0.5 * (r - e) - 0.5,
        1 if r < -0.25 => -2.0 * (e - (r + 0.5)),
        1 => 1.0 + 2.0 * (r - e),
        _ if k <= -2 || k > 56 => add_to_exponent(1.0 - (e - r), k) - 1.0,
        _ if k < 20 => add_to_exponent(from_high_word(0x3FF0_0000 - (0x20_0000 >> k)) - (e - r), k),
        _ => add_to_exponent(r - (e + from_high_word((0x3FF - k) << 20)) + 1.0, k),
    }
}

/// True if `|x|`, given by its high word `ix`, does not exceed the argument at which `sinh` and
/// `cosh` overflow.
fn below_overflow_threshold(x: f64, ix: i32) -> bool {
    ix < 0x4086_33CE || (ix == 0x4086_33CE && low_word(x) <= 0x8FB9_F87D)
}

/// `StrictMath.sinh`.
pub fn sinh(x: f64) -> f64 {
    let jx = high_word(x);
    let ix = jx & 0x7FFF_FFFF;

    if ix >= 0x7FF0_0000 {
        return x + x;
    }
    let h = if jx < 0 { -0.5 } else { 0.5 };

    if ix < 0x4036_0000 {
        if ix < 0x3E30_0000 {
            return x;
        }
        let t = expm1(x.abs());
        if ix < 0x3FF0_0000 {
            return h * (2.0 * t - t * t / (t + 1.0));
        }
        return h * (t + t / (t + 1.0));
    }
    if ix < 0x4086_2E42 {
        return h * exp(x.abs());
    }
    if below_overflow_threshold(x, ix) {
        let w = exp(0.5 * x.abs());
        let t = h * w;
        return t * w;
    }
    x * SHUGE
}

/// `StrictMath.cosh`.
pub fn cosh(x: f64) -> f64 {
    let ix = high_word(x) & 0x7FFF_FFFF;

    if ix >= 0x7FF0_0000 {
        return x * x;
    }
    if ix < 0x3FD6_2E43 {
        let t = expm1(x.abs());
        let w = 1.0 + t;
        if ix < 0x3C80_0000 {
            return w;
        }
        return 1.0 + (t * t) / (w + w);
    }
    if ix < 0x4036_0000 {
        let t = exp(x.abs());
        return 0.5 * t + 0.5 / t;
    }
    if ix < 0x4086_2E42 {
        return 0.5 * exp(x.abs());
    }
    if below_overflow_threshold(x, ix) {
        let w = exp(0.5 * x.abs());
        let t = 0.5 * w;
        return t * w;
    }
    f64::INFINITY
}

/// `StrictMath.tanh`.
pub fn tanh(x: f64) -> f64 {
    let jx = high_word(x);
    let ix = jx & 0x7FFF_FFFF;

    if ix >= 0x7FF0_0000 {
        return if jx >= 0 { 1.0 / x + 1.0 } else { 1.0 / x - 1.0 };
    }
    let z = if ix < 0x4036_0000 {
        if ix < 0x3C80_0000 {
            return x * (1.0 + x);
        }
        if ix >= 0x3FF0_0000 {
            let t = expm1(2.0 * x.abs());
            1.0 - 2.0 / (t + 2.0)
        } else {
            let t = expm1(-2.0 * x.abs());
            -t / (t + 2.0)
        }
    } else {
        1.0 - TINY
    };
    if jx >= 0 {
        z
    } else {
        -z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expm1_keeps_tiny_arguments() {
        assert_eq!(expm1(1e-300), 1e-300);
        assert_eq!(expm1(f64::NEG_INFINITY), -1.0);
        assert_eq!(expm1(-50.0), -1.0);
    }

    #[test]
    fn sinh_handles_special_values() {
        assert_eq!(sinh(1.0).to_bits(), 0x3FF2_CD9F_C44E_B982);
        assert_eq!(sinh(-0.0).to_bits(), 0x8000_0000_0000_0000);
        assert_eq!(sinh(1000.0), f64::INFINITY);
        assert_eq!(sinh(-1000.0), f64::NEG_INFINITY);
        assert!(sinh(f64::NAN).is_nan());
    }

    #[test]
    fn cosh_handles_special_values() {
        assert_eq!(cosh(1.0).to_bits(), 0x3FF8_B075_51D9_F551);
        assert_eq!(cosh(0.0), 1.0);
        assert_eq!(cosh(-1000.0), f64::INFINITY);
        assert_eq!(cosh(f64::NEG_INFINITY), f64::INFINITY);
        assert!(cosh(f64::NAN).is_nan());
    }

    #[test]
    fn tanh_handles_special_values() {
        assert_eq!(tanh(1.0).to_bits(), 0x3FE8_5EFA_B514_F394);
        assert_eq!(tanh(-0.0).to_bits(), 0x8000_0000_0000_0000);
        assert_eq!(tanh(30.0), 1.0);
        assert_eq!(tanh(f64::NEG_INFINITY), -1.0);
        assert!(tanh(f64::NAN).is_nan());
    }
}
