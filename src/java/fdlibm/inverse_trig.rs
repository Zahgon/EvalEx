// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Permission to use, copy, modify, and distribute this software is freely granted, provided that
// this notice is preserved.

//! Inverse trigonometric functions (`e_asin.c`, `e_acos.c`, `s_atan.c`, `e_atan2.c`).

use super::{clear_low_word, high_word};

const PI: f64 = f64::from_bits(0x4009_21FB_5444_2D18);
const PI_LO: f64 = f64::from_bits(0x3CA1_A626_3314_5C07);
const PIO2_HI: f64 = f64::from_bits(0x3FF9_21FB_5444_2D18);
const PIO2_LO: f64 = f64::from_bits(0x3C91_A626_3314_5C07);
const PIO4_HI: f64 = f64::from_bits(0x3FE9_21FB_5444_2D18);
const TINY: f64 = 1.0e-300;

const PS0: f64 = f64::from_bits(0x3FC5_5555_5555_5555);
const PS1: f64 = f64::from_bits(0xBFD4_D612_03EB_6F7D);
const PS2: f64 = f64::from_bits(0x3FC9_C155_0E88_4455);
const PS3: f64 = f64::from_bits(0xBFA4_8228_B568_8F3B);
const PS4: f64 = f64::from_bits(0x3F49_EFE0_7501_B288);
const PS5: f64 = f64::from_bits(0x3F02_3DE1_0DFD_F709);
const QS1: f64 = f64::from_bits(0xC003_3A27_1C8A_2D4B);
const QS2: f64 = f64::from_bits(0x4000_2AE5_9C59_8AC8);
const QS3: f64 = f64::from_bits(0xBFE6_066C_1B8D_0159);
const QS4: f64 = f64::from_bits(0x3FB3_B8C5_B12E_9282);

const ATAN_HI: [f64; 4] = [
    f64::from_bits(0x3FDD_AC67_0561_BB4F),
    f64::from_bits(0x3FE9_21FB_5444_2D18),
    f64::from_bits(0x3FEF_730B_D281_F69B),
    f64::from_bits(0x3FF9_21FB_5444_2D18),
];
const ATAN_LO: [f64; 4] = [
    f64::from_bits(0x3C7A_2B7F_222F_65E2),
    f64::from_bits(0x3C81_A626_3314_5C07),
    f64::from_bits(0x3C70_0788_7AF0_CBBD),
    f64::from_bits(0x3C91_A626_3314_5C07),
];
const AT: [f64; 11] = [
    f64::from_bits(0x3FD5_5555_5555_550D),
    f64::from_bits(0xBFC9_9999_9998_EBC4),
    f64::from_bits(0x3FC2_4924_9200_83FF),
    f64::from_bits(0xBFBC_71C6_FE23_1671),
    f64::from_bits(0x3FB7_45CD_C54C_206E),
    f64::from_bits(0xBFB3_B0F2_AF74_9A6D),
    f64::from_bits(0x3FB1_0D66_A0D0_3D51),
    f64::from_bits(0xBFAD_DE2D_52DE_FD9A),
    f64::from_bits(0x3FA9_7B4B_2476_0DEB),
    f64::from_bits(0xBFA2_B444_2C6A_6C2F),
    f64::from_bits(0x3F90_AD3A_E322_DA11),
];

/// The rational approximation `P(t) / Q(t)` shared by `asin` and `acos`, as its two polynomials.
fn asin_polynomials(t: f64) -> (f64, f64) {
    let p = t * (PS0 + t * (PS1 + t * (PS2 + t * (PS3 + t * (PS4 + t * PS5)))));
    let q = 1.0 + t * (QS1 + t * (QS2 + t * (QS3 + t * QS4)));
    (p, q)
}

/// `StrictMath.asin`.
pub fn asin(x: f64) -> f64 {
    let hx = high_word(x);
    let ix = hx & 0x7FFF_FFFF;

    if ix >= 0x3FF0_0000 {
        if x.abs() == 1.0 {
            return x * PIO2_HI + x * PIO2_LO;
        }
        return f64::NAN;
    }
    if ix < 0x3FE0_0000 {
        if ix < 0x3E40_0000 {
            return x;
        }
        let t = x * x;
        let (p, q) = asin_polynomials(t);
        let w = p / q;
        return x + x * w;
    }

    let w = 1.0 - x.abs();
    let t = w * 0.5;
    let (p, q) = asin_polynomials(t);
    let s = t.sqrt();
    let result = if ix >= 0x3FEF_3333 {
        let w = p / q;
        PIO2_HI - (2.0 * (s + s * w) - PIO2_LO)
    } else {
        let w = clear_low_word(s);
        let c = (t - w * w) / (s + w);
        let r = p / q;
        let p = 2.0 * s * r - (PIO2_LO - 2.0 * c);
        let q = PIO4_HI - 2.0 * w;
        PIO4_HI - (p - q)
    };
    if hx > 0 {
        result
    } else {
        -result
    }
}

/// `StrictMath.acos`.
pub fn acos(x: f64) -> f64 {
    let hx = high_word(x);
    let ix = hx & 0x7FFF_FFFF;

    if ix >= 0x3FF0_0000 {
        if x.abs() == 1.0 {
            return if hx > 0 { 0.0 } else { PI + 2.0 * PIO2_LO };
        }
        return f64::NAN;
    }
    if ix < 0x3FE0_0000 {
        if ix <= 0x3C60_0000 {
            return PIO2_HI + PIO2_LO;
        }
        let z = x * x;
        let (p, q) = asin_polynomials(z);
        let r = p / q;
        return PIO2_HI - (x - (PIO2_LO - x * r));
    }
    if hx < 0 {
        let z = (1.0 + x) * 0.5;
        let (p, q) = asin_polynomials(z);
        let s = z.sqrt();
        let r = p / q;
        let w = r * s - PIO2_LO;
        return PI - 2.0 * (s + w);
    }
    let z = (1.0 - x) * 0.5;
    let s = z.sqrt();
    let df = clear_low_word(s);
    let c = (z - df * df) / (s + df);
    let (p, q) = asin_polynomials(z);
    let r = p / q;
    let w = r * s + c;
    2.0 * (df + w)
}

/// `StrictMath.atan`.
pub fn atan(x: f64) -> f64 {
    let hx = high_word(x);
    let ix = hx & 0x7FFF_FFFF;

    if ix >= 0x4410_0000 {
        if x.is_nan() {
            return x + x;
        }
        return if hx > 0 { ATAN_HI[3] + ATAN_LO[3] } else { -ATAN_HI[3] - ATAN_LO[3] };
    }

    let (id, x) = if ix < 0x3FDC_0000 {
        if ix < 0x3E20_0000 {
            return x;
        }
        (None, x)
    } else {
        let x = x.abs();
        if ix < 0x3FE6_0000 {
            (Some(0), (2.0 * x - 1.0) / (2.0 + x))
        } else if ix < 0x3FF3_0000 {
            (Some(1), (x - 1.0) / (x + 1.0))
        } else if ix < 0x4003_8000 {
            (Some(2), (x - 1.5) / (1.0 + 1.5 * x))
        } else {
            (Some(3), -1.0 / x)
        }
    };

    let z = x * x;
    let w = z * z;
    // The sum of AT[i] * z^(i + 1) is split into its odd and even terms.
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    match id {
        None => x - x * (s1 + s2),
        Some(id) => {
            let z = ATAN_HI[id] - ((x * (s1 + s2) - ATAN_LO[id]) - x);
            if hx < 0 {
                -z
            } else {
                z
            }
        }
    }
}

/// `StrictMath.atan2`.
pub fn atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return x + y;
    }
    if x == 1.0 {
        return atan(y);
    }
    let hx = high_word(x);
    let ix = hx & 0x7FFF_FFFF;
    let hy = high_word(y);
    let iy = hy & 0x7FFF_FFFF;
    let y_negative = hy < 0;
    let x_negative = hx < 0;
    let signed_like_y = |z: f64| if y_negative { -z - TINY } else { z + TINY };

    if y == 0.0 {
        return if x_negative { signed_like_y(PI) } else { y };
    }
    if x == 0.0 {
        return signed_like_y(PIO2_HI);
    }
    if x.is_infinite() {
        if y.is_infinite() {
            let angle = if x_negative { 3.0 * PIO4_HI } else { PIO4_HI };
            return signed_like_y(angle);
        }
        return match (x_negative, y_negative) {
            (false, false) => 0.0,
            (false, true) => -0.0,
            (true, _) => signed_like_y(PI),
        };
    }
    if y.is_infinite() {
        return signed_like_y(PIO2_HI);
    }

    let k = (iy - ix) >> 20;
    let z = if k > 60 {
        PIO2_HI + 0.5 * PI_LO
    } else if x_negative && k < -60 {
        0.0
    } else {
        atan((y / x).abs())
    };
    match (x_negative, y_negative) {
        (false, false) => z,
        (false, true) => -z,
        (true, false) => PI - (z - PI_LO),
        (true, true) => (z - PI_LO) - PI,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asin_values_match_strict_math() {
        assert_eq!(asin(0.5).to_bits(), 0x3FE0_C152_382D_7366);
        assert_eq!(asin(1.0).to_bits(), 0x3FF9_21FB_5444_2D18);
        assert_eq!(asin(-0.0).to_bits(), 0x8000_0000_0000_0000);
        assert!(asin(1.5).is_nan());
    }

    #[test]
    fn acos_values_match_strict_math() {
        assert_eq!(acos(0.5).to_bits(), 0x3FF0_C152_382D_7366);
        assert_eq!(acos(1.0), 0.0);
        assert_eq!(acos(-1.0).to_bits(), 0x4009_21FB_5444_2D18);
        assert!(acos(-1.5).is_nan());
    }

    #[test]
    fn atan_values_match_strict_math() {
        assert_eq!(atan(0.5).to_bits(), 0x3FDD_AC67_0561_BB4F);
        assert_eq!(atan(1.0).to_bits(), 0x3FE9_21FB_5444_2D18);
        assert_eq!(atan(f64::INFINITY).to_bits(), 0x3FF9_21FB_5444_2D18);
        assert_eq!(atan(-0.0).to_bits(), 0x8000_0000_0000_0000);
        assert!(atan(f64::NAN).is_nan());
    }

    #[test]
    fn atan2_covers_every_quadrant_and_special_case() {
        assert_eq!(atan2(1.0, 2.0).to_bits(), 0x3FDD_AC67_0561_BB4F);
        assert_eq!(atan2(0.0, 2.0).to_bits(), 0);
        assert_eq!(atan2(-0.0, 2.0).to_bits(), 0x8000_0000_0000_0000);
        assert_eq!(atan2(0.0, -2.0).to_bits(), 0x4009_21FB_5444_2D18);
        assert_eq!(atan2(-0.0, -0.0).to_bits(), 0xC009_21FB_5444_2D18);
        assert_eq!(atan2(1.0, 0.0).to_bits(), 0x3FF9_21FB_5444_2D18);
        assert_eq!(atan2(-1.0, f64::INFINITY).to_bits(), 0x8000_0000_0000_0000);
        assert_eq!(atan2(1.0, f64::NEG_INFINITY).to_bits(), 0x4009_21FB_5444_2D18);
        assert!(atan2(f64::NAN, 1.0).is_nan());
    }
}
