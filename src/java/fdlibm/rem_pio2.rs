// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Permission to use, copy, modify, and distribute this software is freely granted, provided that
// this notice is preserved.

//! Reduction of an argument modulo pi/2 (`e_rem_pio2.c`, `k_rem_pio2.c`).

use super::{high_word, low_word, scalbn, with_high_word};

/// 396 hexadecimal digits of 2/pi, in 24-bit chunks.
const TWO_OVER_PI: [i32; 66] = [
    0xA2F983, 0x6E4E44, 0x1529FC, 0x2757D1, 0xF534DD, 0xC0DB62, 0x95993C, 0x439041, 0xFE5163, 0xABDEBB, 0xC561B7,
    0x246E3A, 0x424DD2, 0xE00649, 0x2EEA09, 0xD1921C, 0xFE1DEB, 0x1CB129, 0xA73EE8, 0x8235F5, 0x2EBB44, 0x84E99C,
    0x7026B4, 0x5F7E41, 0x3991D6, 0x398353, 0x39F49C, 0x845F8B, 0xBDF928, 0x3B1FF8, 0x97FFDE, 0x05980F, 0xEF2F11,
    0x8B5A0A, 0x6D1F6D, 0x367ECF, 0x27CB09, 0xB74F46, 0x3F669E, 0x5FEA2D, 0x7527BA, 0xC7EBE5, 0xF17B3D, 0x0739F7,
    0x8A5292, 0xEA6BFB, 0x5FB11F, 0x8D5D08, 0x560330, 0x46FC7B, 0x6BABF0, 0xCFBC20, 0x9AF436, 0x1DA9E3, 0x91615E,
    0xE61B08, 0x659985, 0x5F14A0, 0x68408D, 0xFFD880, 0x4D7327, 0x310606, 0x1556CA, 0x73A8C9, 0x60E27B, 0xC08C6B,
];

/// High words of `n * pi/2` for `n` in `1..=32`.
const NPIO2_HW: [i32; 32] = [
    0x3FF921FB, 0x400921FB, 0x4012D97C, 0x401921FB, 0x401F6A7A, 0x4022D97C, 0x4025FDBB, 0x402921FB, 0x402C463A,
    0x402F6A7A, 0x4031475C, 0x4032D97C, 0x40346B9C, 0x4035FDBB, 0x40378FDB, 0x403921FB, 0x403AB41B, 0x403C463A,
    0x403DD85A, 0x403F6A7A, 0x40407E4C, 0x4041475C, 0x4042106C, 0x4042D97C, 0x4043A28C, 0x40446B9C, 0x404534AC,
    0x4045FDBB, 0x4046C6CB, 0x40478FDB, 0x404858EB, 0x404921FB,
];

/// Pi/2 in 24-bit chunks, each stored in a double.
const PIO2: [f64; 8] = [
    f64::from_bits(0x3FF9_21FB_4000_0000),
    f64::from_bits(0x3E74_442D_0000_0000),
    f64::from_bits(0x3CF8_4698_8000_0000),
    f64::from_bits(0x3B78_CC51_6000_0000),
    f64::from_bits(0x39F0_1B83_8000_0000),
    f64::from_bits(0x387A_2520_4000_0000),
    f64::from_bits(0x36E3_8222_8000_0000),
    f64::from_bits(0x3569_F31D_0000_0000),
];

const TWO24: f64 = 16_777_216.0;
const TWON24: f64 = f64::from_bits(0x3E70_0000_0000_0000);
/// 53 bits of 2/pi.
const INVPIO2: f64 = f64::from_bits(0x3FE4_5F30_6DC9_C883);
/// First 33 bits of pi/2.
const PIO2_1: f64 = f64::from_bits(0x3FF9_21FB_5440_0000);
/// `pi/2 - PIO2_1`.
const PIO2_1T: f64 = f64::from_bits(0x3DD0_B461_1A62_6331);
/// Second 33 bits of pi/2.
const PIO2_2: f64 = f64::from_bits(0x3DD0_B461_1A60_0000);
/// `pi/2 - (PIO2_1 + PIO2_2)`.
const PIO2_2T: f64 = f64::from_bits(0x3BA3_198A_2E03_7073);
/// Third 33 bits of pi/2.
const PIO2_3: f64 = f64::from_bits(0x3BA3_198A_2E00_0000);
/// `pi/2 - (PIO2_1 + PIO2_2 + PIO2_3)`.
const PIO2_3T: f64 = f64::from_bits(0x397B_839A_2520_49C1);

/// Number of 24-bit terms of the first approximation (`jk`, also `jp`, for two-word output).
const INITIAL_TERMS: usize = 4;
/// Capacity of the working arrays. A double cancels at most about 61 bits against a multiple of
/// pi/2, so recomputation adds at most three terms and the indices stay far below this bound.
const MAX_TERMS: usize = 20;

/// Sum of `x[j] * f[jx + i - j]`, the `i`-th term of the product of `x` and 2/pi.
fn product_term(x: &[f64], f: &[f64; MAX_TERMS], i: usize) -> f64 {
    let jx = x.len() - 1;
    x.iter().enumerate().fold(0.0, |sum, (j, &digit)| sum + digit * f[jx + i - j])
}

/// Reduces a number given as 24-bit chunks `x * 2^e0` modulo pi/2.
///
/// `x` holds one to three chunks with a non-zero last one. Returns the low three bits of the
/// quotient and the remainder in two pieces.
fn kernel_rem_pio2(x: &[f64], e0: i32) -> (i32, f64, f64) {
    let jk = INITIAL_TERMS;
    let jx = x.len() - 1;
    let jv = ((e0 - 3) / 24).max(0);
    let mut q0 = e0 - 24 * (jv + 1);
    let jv = jv as usize;

    let mut f = [0.0; MAX_TERMS];
    for (i, slot) in f.iter_mut().enumerate().take(jx + jk + 1) {
        if let Some(index) = (jv + i).checked_sub(jx) {
            *slot = f64::from(TWO_OVER_PI[index]);
        }
    }

    let mut q = [0.0; MAX_TERMS];
    for (i, slot) in q.iter_mut().enumerate().take(jk + 1) {
        *slot = product_term(x, &f, i);
    }

    let mut iq = [0_i32; MAX_TERMS];
    let mut jz = jk;
    let (mut z, n, ih) = loop {
        // Distill q[] into iq[] in reverse order.
        let mut z = q[jz];
        for (i, j) in (1..=jz).rev().enumerate() {
            let fw = f64::from((TWON24 * z) as i32);
            iq[i] = (z - TWO24 * fw) as i32;
            z = q[j - 1] + fw;
        }

        z = scalbn(z, q0);
        z -= 8.0 * (z * 0.125).floor();
        let mut n = z as i32;
        z -= f64::from(n);

        let mut ih = 0;
        if q0 > 0 {
            let top = iq[jz - 1] >> (24 - q0);
            n += top;
            iq[jz - 1] -= top << (24 - q0);
            ih = iq[jz - 1] >> (23 - q0);
        } else if q0 == 0 {
            ih = iq[jz - 1] >> 23;
        } else if z >= 0.5 {
            ih = 2;
        }

        if ih > 0 {
            // The fraction exceeds 1/2: continue with its complement.
            n += 1;
            let mut borrowed = false;
            for digit in &mut iq[..jz] {
                if borrowed {
                    *digit = 0xFF_FFFF - *digit;
                } else if *digit != 0 {
                    borrowed = true;
                    *digit = 0x100_0000 - *digit;
                }
            }
            match q0 {
                1 => iq[jz - 1] &= 0x7F_FFFF,
                2 => iq[jz - 1] &= 0x3F_FFFF,
                _ => {}
            }
            if ih == 2 {
                z = 1.0 - z;
                if borrowed {
                    z -= scalbn(1.0, q0);
                }
            }
        }

        // All retained digits cancelled: more terms of 2/pi are needed.
        if z == 0.0 && iq[jk..jz].iter().all(|&digit| digit == 0) {
            let zeros = iq[..jk].iter().rev().take_while(|&&d| d == 0).count();
            let added = zeros + 1;
            for i in jz + 1..=jz + added {
                f[jx + i] = f64::from(TWO_OVER_PI[jv + i]);
                q[i] = product_term(x, &f, i);
            }
            jz += added;
            continue;
        }
        break (z, n, ih);
    };

    // Chop off zero terms, or break z into 24-bit chunks.
    if z == 0.0 {
        jz -= 1;
        q0 -= 24;
        while jz > 0 && iq[jz] == 0 {
            jz -= 1;
            q0 -= 24;
        }
    } else {
        z = scalbn(z, -q0);
        if z >= TWO24 {
            let fw = f64::from((TWON24 * z) as i32);
            iq[jz] = (z - TWO24 * fw) as i32;
            jz += 1;
            q0 += 24;
            iq[jz] = fw as i32;
        } else {
            iq[jz] = z as i32;
        }
    }

    // Convert the integer chunks to floating point.
    let mut fw = scalbn(1.0, q0);
    for (slot, &digit) in q.iter_mut().zip(&iq).take(jz + 1).rev() {
        *slot = fw * f64::from(digit);
        fw *= TWON24;
    }

    // fq[jz - i] = PIO2[0..=jp] * q[i..=jz]
    let mut fq = [0.0; MAX_TERMS];
    for (j, slot) in fq.iter_mut().enumerate().take(jz + 1) {
        let i = jz - j;
        *slot = PIO2.iter().take(jk + 1).zip(&q[i..=jz]).fold(0.0, |sum, (pio2, term)| sum + pio2 * term);
    }

    // Compress fq[] into the two output words.
    let head = fq[..=jz].iter().rev().fold(0.0, |sum, term| sum + term);
    let tail = fq[1..=jz].iter().fold(fq[0] - head, |sum, term| sum + term);
    if ih == 0 {
        (n & 7, head, tail)
    } else {
        (n & 7, -head, -tail)
    }
}

/// Returns `(n, y0, y1)` with `x = n * pi/2 + y0 + y1` and `|y0 + y1| <= pi/4`.
///
/// `x` must be finite. Only the low bits of `n` are meaningful for huge arguments.
pub(super) fn rem_pio2(x: f64) -> (i32, f64, f64) {
    let hx = high_word(x);
    let ix = hx & 0x7FFF_FFFF;

    if ix <= 0x3FE9_21FB {
        return (0, x, 0.0);
    }
    if ix < 0x4002_D97C {
        return near_pio2(x, ix);
    }
    if ix <= 0x4139_21FB {
        return medium(x, hx, ix);
    }

    // Split |x| into 24-bit chunks: z = scalbn(|x|, ilogb(x) - 23).
    let e0 = (ix >> 20) - 1046;
    let mut z = with_high_word(f64::from_bits(u64::from(low_word(x))), ix - (e0 << 20));
    let mut tx = [0.0; 3];
    for chunk in &mut tx[..2] {
        *chunk = f64::from(z as i32);
        z = (z - *chunk) * TWO24;
    }
    tx[2] = z;
    let nx = tx.iter().rposition(|&chunk| chunk != 0.0).map_or(1, |i| i + 1);
    let (n, y0, y1) = kernel_rem_pio2(&tx[..nx], e0);
    if hx < 0 {
        (-n, -y0, -y1)
    } else {
        (n, y0, y1)
    }
}

/// Reduction for `pi/4 < |x| < 3pi/4`, where `n` is 1 or -1.
fn near_pio2(x: f64, ix: i32) -> (i32, f64, f64) {
    let sign = if x > 0.0 { 1.0 } else { -1.0 };
    let mut z = x - sign * PIO2_1;
    let tail = if ix == 0x3FF9_21FB {
        // Near pi/2: use 33+33+53 bits of pi.
        z -= sign * PIO2_2;
        sign * PIO2_2T
    } else {
        sign * PIO2_1T
    };
    let y0 = z - tail;
    let y1 = (z - y0) - tail;
    (sign as i32, y0, y1)
}

/// Reduction for `3pi/4 <= |x| <= 2^19 * pi/2`.
fn medium(x: f64, hx: i32, ix: i32) -> (i32, f64, f64) {
    let t = x.abs();
    let n = (t * INVPIO2 + 0.5) as i32;
    let multiple = f64::from(n);
    let mut r = t - multiple * PIO2_1;
    // The first round is good to 85 bits.
    let mut w = multiple * PIO2_1T;
    let mut y0 = r - w;
    if n >= 32 || ix == NPIO2_HW[(n - 1) as usize] {
        let j = ix >> 20;
        let lost_bits = |y: f64| j - ((high_word(y) >> 20) & 0x7FF);
        if lost_bits(y0) > 16 {
            // Second iteration, good to 118 bits.
            let t = r;
            w = multiple * PIO2_2;
            r = t - w;
            w = multiple * PIO2_2T - ((t - r) - w);
            y0 = r - w;
            if lost_bits(y0) > 49 {
                // Third iteration, 151 bits, which covers all possible cases.
                let t = r;
                w = multiple * PIO2_3;
                r = t - w;
                w = multiple * PIO2_3T - ((t - r) - w);
                y0 = r - w;
            }
        }
    }
    let y1 = (r - y0) - w;
    if hx < 0 {
        (-n, -y0, -y1)
    } else {
        (n, y0, y1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_arguments_are_returned_unreduced() {
        assert_eq!(rem_pio2(0.5), (0, 0.5, 0.0));
    }

    #[test]
    fn pi_over_two_reduces_to_its_tail() {
        let (n, y0, _) = rem_pio2(std::f64::consts::FRAC_PI_2);
        assert_eq!(n, 1);
        assert_eq!(y0.to_bits(), 0xBC91_A626_3314_5C07);
    }

    #[test]
    fn largest_double_reduces_to_the_known_quadrant() {
        assert_eq!(rem_pio2(f64::MAX).0, 2);
        assert_eq!(rem_pio2(-f64::MAX).0, -2);
    }
}
