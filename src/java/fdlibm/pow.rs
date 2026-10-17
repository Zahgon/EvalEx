// Copyright (C) 2004 by Sun Microsystems, Inc. All rights reserved.
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Permission to use, copy, modify, and distribute this software is freely granted, provided that
// this notice is preserved.

//! Power function (`e_pow.c`).

use super::exp_log::{P1, P2, P3, P4, P5};
use super::{add_to_exponent, clear_low_word, from_high_word, high_word, low_word, scalbn, with_high_word};

const BP: [f64; 2] = [1.0, 1.5];
const DP_H: [f64; 2] = [0.0, f64::from_bits(0x3FE2_B803_4000_0000)];
const DP_L: [f64; 2] = [0.0, f64::from_bits(0x3E4C_FDEB_43CF_D006)];
const TWO53: f64 = 9_007_199_254_740_992.0;
const HUGE: f64 = 1.0e300;
const TINY: f64 = 1.0e-300;
const L1: f64 = f64::from_bits(0x3FE3_3333_3333_3303);
const L2: f64 = f64::from_bits(0x3FDB_6DB6_DB6F_ABFF);
const L3: f64 = f64::from_bits(0x3FD5_5555_518F_264D);
const L4: f64 = f64::from_bits(0x3FD1_7460_A91D_4101);
const L5: f64 = f64::from_bits(0x3FCD_864A_93C9_DB65);
const L6: f64 = f64::from_bits(0x3FCA_7E28_4A45_4EEF);
const LG2: f64 = f64::from_bits(0x3FE6_2E42_FEFA_39EF);
const LG2_H: f64 = f64::from_bits(0x3FE6_2E43_0000_0000);
const LG2_L: f64 = f64::from_bits(0xBE20_5C61_0CA8_6C39);
/// `-(1024 - log2(overflow threshold + 0.5 ulp))`.
const OVT: f64 = 8.008_566_259_537_294e-17;
/// `2 / (3 * ln(2))`.
const CP: f64 = f64::from_bits(0x3FEE_C709_DC3A_03FD);
const CP_H: f64 = f64::from_bits(0x3FEE_C709_E000_0000);
const CP_L: f64 = f64::from_bits(0xBE3E_2FE0_145B_01F5);
/// `1 / ln(2)`.
const IVLN2: f64 = f64::from_bits(0x3FF7_1547_652B_82FE);
const IVLN2_H: f64 = f64::from_bits(0x3FF7_1547_6000_0000);
const IVLN2_L: f64 = f64::from_bits(0x3E54_AE0B_F85D_DF44);

/// How `y` behaves as an exponent of a negative base.
#[derive(Clone, Copy, PartialEq)]
enum Parity {
    NotInteger,
    Odd,
    Even,
}

/// Classifies `y`, given its words with the sign removed from `iy`.
fn parity(iy: i32, ly: u32) -> Parity {
    let of_bit = |low_bit: u32| {
        if low_bit & 1 == 1 {
            Parity::Odd
        } else {
            Parity::Even
        }
    };
    if iy >= 0x4340_0000 {
        return Parity::Even;
    }
    if iy < 0x3FF0_0000 {
        return Parity::NotInteger;
    }
    let k = (iy >> 20) - 0x3FF;
    if k > 20 {
        let j = ly >> (52 - k);
        if j << (52 - k) == ly {
            return of_bit(j);
        }
    } else if ly == 0 {
        let j = iy >> (20 - k);
        if j << (20 - k) == iy {
            return of_bit(j as u32);
        }
    }
    Parity::NotInteger
}

/// `log2(|x|)` in two pieces for `x` so close to one that `|y|` above 2^31 can still give a finite
/// result; a truncated series suffices there.
fn log2_near_one(ax: f64) -> (f64, f64) {
    let t = ax - 1.0;
    let w = (t * t) * (0.5 - t * (0.333_333_333_333_333_3 - t * 0.25));
    let u = IVLN2_H * t;
    let v = t * IVLN2_L - w * IVLN2;
    let t1 = clear_low_word(u + v);
    (t1, v - (t1 - u))
}

/// `log2(ax)` in two pieces, for any finite positive `ax`.
fn log2_general(mut ax: f64) -> (f64, f64) {
    let mut ix = high_word(ax);
    let mut n = 0;
    if ix < 0x0010_0000 {
        ax *= TWO53;
        n -= 53;
        ix = high_word(ax);
    }
    n += (ix >> 20) - 0x3FF;
    let j = ix & 0x000F_FFFF;

    // Normalize into [1, sqrt(3/2)) or [sqrt(3/2), sqrt(3)) around BP[k].
    ix = j | 0x3FF0_0000;
    let k = if j <= 0x3988E {
        0
    } else if j < 0xBB67A {
        1
    } else {
        n += 1;
        ix -= 0x0010_0000;
        0
    };
    let ax = with_high_word(ax, ix);

    // ss = s_h + s_l = (ax - bp) / (ax + bp)
    let u = ax - BP[k];
    let v = 1.0 / (ax + BP[k]);
    let ss = u * v;
    let s_h = clear_low_word(ss);
    let t_h = from_high_word(((ix >> 1) | 0x2000_0000) + 0x0008_0000 + ((k as i32) << 18));
    let t_l = ax - (t_h - BP[k]);
    let s_l = v * ((u - s_h * t_h) - s_h * t_l);

    // log(ax)
    let s2 = ss * ss;
    let mut r = s2 * s2 * (L1 + s2 * (L2 + s2 * (L3 + s2 * (L4 + s2 * (L5 + s2 * L6)))));
    r += s_l * (s_h + ss);
    let s2 = s_h * s_h;
    let t_h = clear_low_word(3.0 + s2 + r);
    let t_l = r - ((t_h - 3.0) - s2);

    // u + v = ss * (1 + ...)
    let u = s_h * t_h;
    let v = s_l * t_h + t_l * ss;

    // 2 / (3 * log2) * (ss + ...)
    let p_h = clear_low_word(u + v);
    let p_l = v - (p_h - u);
    let z_h = CP_H * p_h;
    let z_l = CP_L * p_h + p_l * CP + DP_L[k];

    // log2(ax) = (ss + ...) * 2 / (3 * log2) = n + dp_h + z_h + z_l
    let t = f64::from(n);
    let t1 = clear_low_word(((z_h + z_l) + DP_H[k]) + t);
    (t1, z_l - (((t1 - t) - DP_H[k]) - z_h))
}

/// `StrictMath.pow`.
pub fn pow(x: f64, y: f64) -> f64 {
    let hx = high_word(x);
    let lx = low_word(x);
    let hy = high_word(y);
    let ly = low_word(y);
    let ix = hx & 0x7FFF_FFFF;
    let iy = hy & 0x7FFF_FFFF;

    if y == 0.0 {
        return 1.0;
    }
    if x.is_nan() || y.is_nan() {
        return x + y;
    }

    let y_kind = if hx < 0 { parity(iy, ly) } else { Parity::NotInteger };

    if ly == 0 {
        if iy == 0x7FF0_0000 {
            let magnitude = x.abs();
            return if magnitude == 1.0 {
                f64::NAN
            } else if magnitude > 1.0 {
                if hy >= 0 {
                    y
                } else {
                    0.0
                }
            } else if hy < 0 {
                -y
            } else {
                0.0
            };
        }
        if iy == 0x3FF0_0000 {
            return if hy < 0 { 1.0 / x } else { x };
        }
        if hy == 0x4000_0000 {
            return x * x;
        }
        if hy == 0x3FE0_0000 && hx >= 0 {
            return x.sqrt();
        }
    }

    let ax = x.abs();
    if lx == 0 && (ix == 0x7FF0_0000 || ix == 0 || ix == 0x3FF0_0000) {
        // x is +-0, +-inf or +-1.
        let mut z = ax;
        if hy < 0 {
            z = 1.0 / z;
        }
        if hx < 0 {
            if ix == 0x3FF0_0000 && y_kind == Parity::NotInteger {
                return f64::NAN;
            }
            if y_kind == Parity::Odd {
                z = -z;
            }
        }
        return z;
    }

    if hx < 0 && y_kind == Parity::NotInteger {
        return f64::NAN;
    }
    let s = if hx < 0 && y_kind == Parity::Odd { -1.0 } else { 1.0 };

    let (t1, t2) = if iy > 0x41E0_0000 {
        if iy > 0x43F0_0000 {
            // |y| > 2^64: certain overflow or underflow.
            if ix <= 0x3FEF_FFFF {
                return if hy < 0 { HUGE * HUGE } else { TINY * TINY };
            }
            if ix >= 0x3FF0_0000 {
                return if hy > 0 { HUGE * HUGE } else { TINY * TINY };
            }
        }
        if ix < 0x3FEF_FFFF {
            return if hy < 0 { s * HUGE * HUGE } else { s * TINY * TINY };
        }
        if ix > 0x3FF0_0000 {
            return if hy > 0 { s * HUGE * HUGE } else { s * TINY * TINY };
        }
        log2_near_one(ax)
    } else {
        log2_general(ax)
    };

    // Split y into y1 + y2 and compute (y1 + y2) * (t1 + t2).
    let y1 = clear_low_word(y);
    let p_l = (y - y1) * t1 + y * t2;
    let mut p_h = y1 * t1;
    let z = p_l + p_h;
    let j = high_word(z);
    let i = low_word(z) as i32;
    if j >= 0x4090_0000 {
        if (j - 0x4090_0000) | i != 0 || p_l + OVT > z - p_h {
            return s * HUGE * HUGE;
        }
    } else if (j & 0x7FFF_FFFF) >= 0x4090_CC00 && (j.wrapping_sub(0xC090_CC00_u32 as i32) | i != 0 || p_l <= z - p_h) {
        return s * TINY * TINY;
    }

    // Compute 2^(p_h + p_l), with n the nearest integer to z.
    let i = j & 0x7FFF_FFFF;
    let mut k = (i >> 20) - 0x3FF;
    let mut n = 0;
    if i > 0x3FE0_0000 {
        n = j.wrapping_add(0x0010_0000 >> (k + 1));
        k = ((n & 0x7FFF_FFFF) >> 20) - 0x3FF;
        let t = from_high_word(n & !(0x000F_FFFF >> k));
        n = ((n & 0x000F_FFFF) | 0x0010_0000) >> (20 - k);
        if j < 0 {
            n = -n;
        }
        p_h -= t;
    }
    let t = clear_low_word(p_l + p_h);
    let u = t * LG2_H;
    let v = (p_l - (t - p_h)) * LG2 + t * LG2_L;
    let z = u + v;
    let w = v - (z - u);
    let t = z * z;
    let t1 = z - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    let r = (z * t1) / (t1 - 2.0) - (w + z * w);
    let z = 1.0 - (r - z);
    let scaled = high_word(z).wrapping_add(n << 20);
    if (scaled >> 20) <= 0 {
        s * scalbn(z, n)
    } else {
        s * add_to_exponent(z, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_cases_follow_the_java_specification() {
        assert_eq!(pow(f64::NAN, 0.0), 1.0);
        assert!(pow(1.0, f64::NAN).is_nan());
        assert!(pow(1.0, f64::INFINITY).is_nan());
        assert!(pow(-1.0, f64::NEG_INFINITY).is_nan());
        assert!(pow(-8.0, 0.5).is_nan());
        assert_eq!(pow(0.5, f64::INFINITY), 0.0);
        assert_eq!(pow(2.0, f64::NEG_INFINITY), 0.0);
        assert_eq!(pow(-0.0, 3.0).to_bits(), 0x8000_0000_0000_0000);
        assert_eq!(pow(-0.0, -3.0), f64::NEG_INFINITY);
        assert_eq!(pow(f64::NEG_INFINITY, 3.0), f64::NEG_INFINITY);
    }

    #[test]
    fn integers_give_exact_results() {
        assert_eq!(pow(2.0, 10.0), 1024.0);
        assert_eq!(pow(10.0, 15.0), 1e15);
        assert_eq!(pow(-3.0, 3.0), -27.0);
        assert_eq!(pow(4.0, 0.5), 2.0);
        assert_eq!(pow(1.1, 2.5).to_bits(), 0x3FF4_4E10_8083_3B25);
    }

    #[test]
    fn extreme_results_overflow_or_round_to_subnormals() {
        assert_eq!(pow(10.0, 400.0), f64::INFINITY);
        assert_eq!(pow(-10.0, 401.0), f64::NEG_INFINITY);
        assert_eq!(pow(10.0, -400.0), 0.0);
        assert_eq!(pow(2.0, -1074.0).to_bits(), 1);
        assert_eq!(pow(2.0, -1075.0).to_bits(), 0);
    }
}
