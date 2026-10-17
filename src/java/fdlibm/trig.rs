// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
// Copyright (C) 2004 by Sun Microsystems, Inc. All rights reserved.
//
// Permission to use, copy, modify, and distribute this software is freely granted, provided that
// this notice is preserved.

//! Sine, cosine and tangent (`s_sin.c`, `s_cos.c`, `s_tan.c`, `k_sin.c`, `k_cos.c`, `k_tan.c`).

use super::rem_pio2::rem_pio2;
use super::{clear_low_word, from_high_word, high_word};

const S1: f64 = f64::from_bits(0xBFC5_5555_5555_5549);
const S2: f64 = f64::from_bits(0x3F81_1111_1110_F8A6);
const S3: f64 = f64::from_bits(0xBF2A_01A0_19C1_61D5);
const S4: f64 = f64::from_bits(0x3EC7_1DE3_57B1_FE7D);
const S5: f64 = f64::from_bits(0xBE5A_E5E6_8A2B_9CEB);
const S6: f64 = f64::from_bits(0x3DE5_D93A_5ACF_D57C);

const C1: f64 = f64::from_bits(0x3FA5_5555_5555_554C);
const C2: f64 = f64::from_bits(0xBF56_C16C_16C1_5177);
const C3: f64 = f64::from_bits(0x3EFA_01A0_19CB_1590);
const C4: f64 = f64::from_bits(0xBE92_7E4F_809C_52AD);
const C5: f64 = f64::from_bits(0x3E21_EE9E_BDB4_B1C4);
const C6: f64 = f64::from_bits(0xBDA8_FAE9_BE88_38D4);

const T: [f64; 13] = [
    f64::from_bits(0x3FD5_5555_5555_5563),
    f64::from_bits(0x3FC1_1111_1110_FE7A),
    f64::from_bits(0x3FAB_A1BA_1BB3_41FE),
    f64::from_bits(0x3F96_64F4_8406_D637),
    f64::from_bits(0x3F82_26E3_E96E_8493),
    f64::from_bits(0x3F6D_6D22_C956_0328),
    f64::from_bits(0x3F57_DBC8_FEE0_8315),
    f64::from_bits(0x3F43_44D8_F2F2_6501),
    f64::from_bits(0x3F30_26F7_1A8D_1068),
    f64::from_bits(0x3F14_7E88_A037_92A6),
    f64::from_bits(0x3F12_B80F_32F0_A7E9),
    f64::from_bits(0xBEF3_75CB_DB60_5373),
    f64::from_bits(0x3EFB_2A70_74BF_7AD4),
];
const PIO4: f64 = f64::from_bits(0x3FE9_21FB_5444_2D18);
const PIO4_LO: f64 = f64::from_bits(0x3C81_A626_3314_5C07);

/// Sine of `x + y` for `|x| <= pi/4`, where `y` is the tail of `x`, or `None` if it is known to be
/// exactly zero.
fn kernel_sin(x: f64, y: Option<f64>) -> f64 {
    let ix = high_word(x) & 0x7FFF_FFFF;
    if ix < 0x3E40_0000 {
        return x;
    }
    let z = x * x;
    let v = z * x;
    let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
    match y {
        None => x + v * (S1 + z * r),
        Some(y) => x - ((z * (0.5 * y - v * r) - y) - v * S1),
    }
}

/// Cosine of `x + y` for `|x| <= pi/4`.
fn kernel_cos(x: f64, y: f64) -> f64 {
    let ix = high_word(x) & 0x7FFF_FFFF;
    if ix < 0x3E40_0000 {
        return 1.0;
    }
    let z = x * x;
    let r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
    if ix < 0x3FD3_3333 {
        return 1.0 - (0.5 * z - (z * r - x * y));
    }
    let qx = if ix > 0x3FE9_0000 { 0.28125 } else { from_high_word(ix - 0x0020_0000) };
    let hz = 0.5 * z - qx;
    let a = 1.0 - qx;
    a - (hz - (z * r - x * y))
}

/// `-1 / (x + r)`, computed accurately given `w`, the rounded sum `x + r`.
fn accurate_negative_reciprocal(x: f64, r: f64, w: f64) -> f64 {
    let z = clear_low_word(w);
    let v = r - (z - x);
    let a = -1.0 / w;
    let t = clear_low_word(a);
    let s = 1.0 + t * z;
    t + a * (s + t * v)
}

/// Tangent of `x + y` for `|x| <= pi/4` if `odd_quadrant` is false, otherwise `-1 / tan(x + y)`.
fn kernel_tan(mut x: f64, mut y: f64, odd_quadrant: bool) -> f64 {
    let hx = high_word(x);
    let ix = hx & 0x7FFF_FFFF;
    if ix < 0x3E30_0000 {
        if !odd_quadrant {
            return x;
        }
        if x == 0.0 {
            return 1.0 / x.abs();
        }
        return accurate_negative_reciprocal(x, y, x + y);
    }
    let large = ix >= 0x3FE5_9428;
    if large {
        if hx < 0 {
            x = -x;
            y = -y;
        }
        let z = PIO4 - x;
        let w = PIO4_LO - y;
        x = z + w;
        y = 0.0;
    }
    let z = x * x;
    let w = z * z;
    // x^5 * (T[1] + x^2 * T[2] + ...) is split into its even and odd powers of x^2.
    let r = T[1] + w * (T[3] + w * (T[5] + w * (T[7] + w * (T[9] + w * T[11]))));
    let v = z * (T[2] + w * (T[4] + w * (T[6] + w * (T[8] + w * (T[10] + w * T[12])))));
    let s = z * x;
    let mut r = y + z * (s * (r + v) + y);
    r += T[0] * s;
    let w = x + r;
    if large {
        let v = if odd_quadrant { -1.0 } else { 1.0 };
        let sign = f64::from(1 - ((hx >> 30) & 2));
        return sign * (v - 2.0 * (x - (w * w / (w + v) - r)));
    }
    if odd_quadrant {
        accurate_negative_reciprocal(x, r, w)
    } else {
        w
    }
}

/// `StrictMath.sin`.
pub fn sin(x: f64) -> f64 {
    let ix = high_word(x) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        return kernel_sin(x, None);
    }
    if ix >= 0x7FF0_0000 {
        return f64::NAN;
    }
    let (n, y0, y1) = rem_pio2(x);
    match n & 3 {
        0 => kernel_sin(y0, Some(y1)),
        1 => kernel_cos(y0, y1),
        2 => -kernel_sin(y0, Some(y1)),
        _ => -kernel_cos(y0, y1),
    }
}

/// `StrictMath.cos`.
pub fn cos(x: f64) -> f64 {
    let ix = high_word(x) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        return kernel_cos(x, 0.0);
    }
    if ix >= 0x7FF0_0000 {
        return f64::NAN;
    }
    let (n, y0, y1) = rem_pio2(x);
    match n & 3 {
        0 => kernel_cos(y0, y1),
        1 => -kernel_sin(y0, Some(y1)),
        2 => -kernel_cos(y0, y1),
        _ => kernel_sin(y0, Some(y1)),
    }
}

/// `StrictMath.tan`.
pub fn tan(x: f64) -> f64 {
    let ix = high_word(x) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        return kernel_tan(x, 0.0, false);
    }
    if ix >= 0x7FF0_0000 {
        return f64::NAN;
    }
    let (n, y0, y1) = rem_pio2(x);
    kernel_tan(y0, y1, n & 1 == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_values_match_strict_math() {
        assert_eq!(sin(1.0).to_bits(), 0x3FEA_ED54_8F09_0CEE);
        assert_eq!(sin(-0.0).to_bits(), 0x8000_0000_0000_0000);
        assert_eq!(sin(1e22).to_bits(), 0xBFEB_453A_B76B_F397);
        assert_eq!(sin(f64::MAX).to_bits(), 0x3F74_52FC_98B3_4E97);
        assert!(sin(f64::INFINITY).is_nan());
    }

    #[test]
    fn cos_values_match_strict_math() {
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(cos(1.0).to_bits(), 0x3FE1_4A28_0FB5_068C);
        assert_eq!(cos(1e22).to_bits(), 0x3FE0_BE2C_EF01_C8F4);
        assert_eq!(cos(f64::MAX).to_bits(), 0xBFEF_FFE6_2ECF_AB75);
        assert!(cos(f64::NAN).is_nan());
    }

    #[test]
    fn the_double_closest_to_a_multiple_of_pi_over_two_survives_reduction() {
        let hardest = f64::from_bits(0x7506_AC5B_262C_A1FF);
        assert_eq!(sin(hardest), 1.0);
        assert_eq!(cos(hardest).to_bits(), 0xBC21_4AE7_2E6B_A22F);
    }

    #[test]
    fn tan_values_match_strict_math() {
        assert_eq!(tan(1.0).to_bits(), 0x3FF8_EB24_5CBE_E3A6);
        assert_eq!(tan(-0.0).to_bits(), 0x8000_0000_0000_0000);
        assert_eq!(tan(std::f64::consts::FRAC_PI_2).to_bits(), 0x434D_0296_7C31_CDB5);
        assert!(tan(f64::NEG_INFINITY).is_nan());
    }
}
