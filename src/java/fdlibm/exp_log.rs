// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Permission to use, copy, modify, and distribute this software is freely granted, provided that
// this notice is preserved.

//! Exponential and logarithms (`e_exp.c`, `e_log.c`, `e_log10.c`).

use super::{add_to_exponent, high_word, with_high_word};

/// `ln(2)` split so that multiples of the high part are exact.
pub(super) const LN2_HI: f64 = f64::from_bits(0x3FE6_2E42_FEE0_0000);
pub(super) const LN2_LO: f64 = f64::from_bits(0x3DEA_39EF_3579_3C76);
/// `1 / ln(2)`.
pub(super) const INV_LN2: f64 = f64::from_bits(0x3FF7_1547_652B_82FE);
/// Largest argument of `exp` with a finite result.
pub(super) const O_THRESHOLD: f64 = f64::from_bits(0x4086_2E42_FEFA_39EF);
const U_THRESHOLD: f64 = f64::from_bits(0xC087_4910_D52D_3051);
/// 2^-1000.
const TWO_M1000: f64 = f64::from_bits(0x0170_0000_0000_0000);

pub(super) const P1: f64 = f64::from_bits(0x3FC5_5555_5555_553E);
pub(super) const P2: f64 = f64::from_bits(0xBF66_C16C_16BE_BD93);
pub(super) const P3: f64 = f64::from_bits(0x3F11_566A_AF25_DE2C);
pub(super) const P4: f64 = f64::from_bits(0xBEBB_BD41_C5D2_6BF1);
pub(super) const P5: f64 = f64::from_bits(0x3E66_3769_72BE_A4D0);

const TWO54: f64 = 18_014_398_509_481_984.0;
const LG1: f64 = f64::from_bits(0x3FE5_5555_5555_5593);
const LG2: f64 = f64::from_bits(0x3FD9_9999_9997_FA04);
const LG3: f64 = f64::from_bits(0x3FD2_4924_9422_9359);
const LG4: f64 = f64::from_bits(0x3FCC_71C5_1D8E_78AF);
const LG5: f64 = f64::from_bits(0x3FC7_4664_96CB_03DE);
const LG6: f64 = f64::from_bits(0x3FC3_9A09_D078_C69F);
const LG7: f64 = f64::from_bits(0x3FC2_F112_DF3E_5244);

const IVLN10: f64 = f64::from_bits(0x3FDB_CB7B_1526_E50E);
const LOG10_2HI: f64 = f64::from_bits(0x3FD3_4413_509F_6000);
const LOG10_2LO: f64 = f64::from_bits(0x3D59_FEF3_11F1_2B36);

/// `StrictMath.exp`.
pub fn exp(x: f64) -> f64 {
    let negative = high_word(x) < 0;
    let ix = high_word(x) & 0x7FFF_FFFF;

    if ix >= 0x4086_2E42 {
        if x.is_nan() {
            return x + x;
        }
        if ix >= 0x7FF0_0000 {
            return if negative { 0.0 } else { x };
        }
        if x > O_THRESHOLD {
            return f64::INFINITY;
        }
        if x < U_THRESHOLD {
            return 0.0;
        }
    }

    // Reduce to x = k * ln2 + r with |r| <= 0.5 * ln2.
    let (r, hi, lo, k) = if ix > 0x3FD6_2E42 {
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
        (hi - lo, hi, lo, k)
    } else if ix < 0x3E30_0000 {
        return 1.0 + x;
    } else {
        (x, 0.0, 0.0, 0)
    };

    let t = r * r;
    let c = r - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    if k == 0 {
        return 1.0 - ((r * c) / (c - 2.0) - r);
    }
    let y = 1.0 - ((lo - (r * c) / (2.0 - c)) - hi);
    if k >= -1021 {
        add_to_exponent(y, k)
    } else {
        add_to_exponent(y, k + 1000) * TWO_M1000
    }
}

/// The argument preparation of both logarithms: `Ok((y, k))` with a normal `y = x * 2^-k`, where `k`
/// is the scaling that lifted a subnormal `x`, or `Err(result)` if `x` needs no computation.
fn scale_log_argument(x: f64) -> Result<(f64, i32), f64> {
    if x == 0.0 {
        return Err(f64::NEG_INFINITY);
    }
    if x < 0.0 {
        return Err(f64::NAN);
    }
    if !x.is_finite() {
        return Err(x + x);
    }
    if high_word(x) < 0x0010_0000 {
        Ok((x * TWO54, -54))
    } else {
        Ok((x, 0))
    }
}

/// `StrictMath.log`.
pub fn log(x: f64) -> f64 {
    let (x, mut k) = match scale_log_argument(x) {
        Ok(scaled) => scaled,
        Err(result) => return result,
    };
    let hx = high_word(x);
    k += (hx >> 20) - 1023;
    let hx = hx & 0x000F_FFFF;
    let i = (hx + 0x95F64) & 0x10_0000;
    // Normalize x or x/2 into [sqrt(2)/2, sqrt(2)).
    let x = with_high_word(x, hx | (i ^ 0x3FF0_0000));
    k += i >> 20;
    let f = x - 1.0;
    let dk = f64::from(k);

    if (0x000F_FFFF & (2 + hx)) < 3 {
        // |f| < 2^-20
        if f == 0.0 {
            return if k == 0 { 0.0 } else { dk * LN2_HI + dk * LN2_LO };
        }
        let r = f * f * (0.5 - 0.333_333_333_333_333_3 * f);
        return if k == 0 { f - r } else { dk * LN2_HI - ((r - dk * LN2_LO) - f) };
    }

    let s = f / (2.0 + f);
    let z = s * s;
    let w = z * z;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    let r = t2 + t1;
    if ((hx - 0x6147A) | (0x6B851 - hx)) > 0 {
        let hfsq = 0.5 * f * f;
        if k == 0 {
            f - (hfsq - s * (hfsq + r))
        } else {
            dk * LN2_HI - ((hfsq - (s * (hfsq + r) + dk * LN2_LO)) - f)
        }
    } else if k == 0 {
        f - s * (f - r)
    } else {
        dk * LN2_HI - ((s * (f - r) - dk * LN2_LO) - f)
    }
}

/// `StrictMath.log10`.
pub fn log10(x: f64) -> f64 {
    let (x, mut k) = match scale_log_argument(x) {
        Ok(scaled) => scaled,
        Err(result) => return result,
    };
    let hx = high_word(x);
    k += (hx >> 20) - 1023;
    let i = i32::from(k < 0);
    let y = f64::from(k + i);
    let x = with_high_word(x, (hx & 0x000F_FFFF) | ((0x3FF - i) << 20));
    let z = y * LOG10_2LO + IVLN10 * log(x);
    z + y * LOG10_2HI
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp_values_match_strict_math() {
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(exp(1.0).to_bits(), 0x4005_BF0A_8B14_576A);
        assert_eq!(exp(f64::NEG_INFINITY), 0.0);
        assert_eq!(exp(f64::INFINITY), f64::INFINITY);
        assert_eq!(exp(710.0), f64::INFINITY);
        assert_eq!(exp(-746.0), 0.0);
        assert!(exp(f64::NAN).is_nan());
    }

    #[test]
    fn log_values_match_strict_math() {
        assert_eq!(log(1.0).to_bits(), 0);
        assert_eq!(log(2.0).to_bits(), 0x3FE6_2E42_FEFA_39EF);
        assert_eq!(log(0.0), f64::NEG_INFINITY);
        assert_eq!(log(-0.0), f64::NEG_INFINITY);
        assert_eq!(log(f64::INFINITY), f64::INFINITY);
        assert!(log(-1.0).is_nan());
        assert!(log(f64::from_bits(0xFFF8_0000_0000_0001)).is_nan());
        assert!(log(f64::NEG_INFINITY).is_nan());
        assert_eq!(log(f64::MIN_POSITIVE).to_bits(), 0xC086_232B_DD7A_BCD2);
    }

    #[test]
    fn log10_values_match_strict_math() {
        assert_eq!(log10(1000.0), 3.0);
        assert_eq!(log10(1.0).to_bits(), 0);
        assert_eq!(log10(0.0), f64::NEG_INFINITY);
        assert!(log10(-1.0).is_nan());
    }
}
