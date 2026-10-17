//! Arithmetic on unsigned magnitudes: little-endian `u64` limbs without high zero limbs.

use std::cmp::Ordering;

/// Operand size in limbs from which Karatsuba beats schoolbook multiplication.
const KARATSUBA_THRESHOLD: usize = 32;

/// Removes high zero limbs.
pub(super) fn normalize(limbs: &mut Vec<u64>) {
    while limbs.last() == Some(&0) {
        limbs.pop();
    }
}

/// Number of significant bits; 0 for zero.
pub(super) fn bit_length(limbs: &[u64]) -> u64 {
    limbs.last().map_or(0, |top| limbs.len() as u64 * u64::from(u64::BITS) - u64::from(top.leading_zeros()))
}

/// Number of low zero bits of a non-zero magnitude; 0 for zero.
pub(super) fn trailing_zeros(limbs: &[u64]) -> u64 {
    let zero_limbs = limbs.iter().take_while(|limb| **limb == 0).count();
    limbs.get(zero_limbs).map_or(0, |limb| zero_limbs as u64 * u64::from(u64::BITS) + u64::from(limb.trailing_zeros()))
}

/// Compares two normalized magnitudes.
pub(super) fn compare(a: &[u64], b: &[u64]) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.iter().rev().cmp(b.iter().rev()))
}

/// Adds `b` into `acc`, which must be long enough to hold the sum.
fn add_in_place(acc: &mut [u64], b: &[u64]) {
    let mut carry = false;
    let mut b_limbs = b.iter();
    for limb in acc.iter_mut() {
        let addend = b_limbs.next();
        if addend.is_none() && !carry {
            break;
        }
        let (sum, overflow_a) = limb.overflowing_add(addend.copied().unwrap_or(0));
        let (sum, overflow_b) = sum.overflowing_add(u64::from(carry));
        *limb = sum;
        carry = overflow_a || overflow_b;
    }
}

/// Subtracts `b` from `acc`, which must be at least as large as `b`.
fn sub_in_place(acc: &mut [u64], b: &[u64]) {
    let mut borrow = false;
    let mut b_limbs = b.iter();
    for limb in acc.iter_mut() {
        let subtrahend = b_limbs.next();
        if subtrahend.is_none() && !borrow {
            break;
        }
        let (difference, overflow_a) = limb.overflowing_sub(subtrahend.copied().unwrap_or(0));
        let (difference, overflow_b) = difference.overflowing_sub(u64::from(borrow));
        *limb = difference;
        borrow = overflow_a || overflow_b;
    }
}

/// `a + b`.
pub(super) fn add(a: &[u64], b: &[u64]) -> Vec<u64> {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut sum = Vec::with_capacity(long.len() + 1);
    sum.extend_from_slice(long);
    sum.push(0);
    add_in_place(&mut sum, short);
    normalize(&mut sum);
    sum
}

/// `a - b` for `a >= b`.
pub(super) fn sub(a: &[u64], b: &[u64]) -> Vec<u64> {
    let mut difference = a.to_vec();
    sub_in_place(&mut difference, b);
    normalize(&mut difference);
    difference
}

/// `a * b`.
pub(super) fn mul(a: &[u64], b: &[u64]) -> Vec<u64> {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    if short.is_empty() {
        return Vec::new();
    }
    let mut product =
        if short.len() < KARATSUBA_THRESHOLD { schoolbook(long, short) } else { mul_unbalanced(long, short) };
    normalize(&mut product);
    product
}

/// Quadratic multiplication; the result has `a.len() + b.len()` limbs.
fn schoolbook(a: &[u64], b: &[u64]) -> Vec<u64> {
    let mut product = vec![0; a.len() + b.len()];
    for (offset, &multiplier) in b.iter().enumerate() {
        let mut carry = 0u64;
        for (slot, &limb) in product[offset..].iter_mut().zip(a) {
            let wide = u128::from(limb) * u128::from(multiplier) + u128::from(*slot) + u128::from(carry);
            *slot = wide as u64;
            carry = (wide >> u64::BITS) as u64;
        }
        product[offset + a.len()] = carry;
    }
    product
}

/// Cuts `long` into pieces of `short.len()` limbs, each multiplied with Karatsuba.
fn mul_unbalanced(long: &[u64], short: &[u64]) -> Vec<u64> {
    let mut product = vec![0; long.len() + short.len()];
    for (index, piece) in long.chunks(short.len()).enumerate() {
        let part = if piece.len() == short.len() { karatsuba(piece, short) } else { mul(short, piece) };
        add_in_place(&mut product[index * short.len()..], &part);
    }
    product
}

/// Karatsuba multiplication of equally long operands.
fn karatsuba(a: &[u64], b: &[u64]) -> Vec<u64> {
    if a.len() < KARATSUBA_THRESHOLD {
        return schoolbook(a, b);
    }
    let half = a.len() / 2;
    let (a_low, a_high) = a.split_at(half);
    let (b_low, b_high) = b.split_at(half);
    let low = mul(a_low, b_low);
    let high = mul(a_high, b_high);
    let mut middle = mul(&add(a_low, a_high), &add(b_low, b_high));
    sub_in_place(&mut middle, &low);
    sub_in_place(&mut middle, &high);
    let mut product = vec![0; a.len() + b.len()];
    product[..low.len()].copy_from_slice(&low);
    product[2 * half..2 * half + high.len()].copy_from_slice(&high);
    normalize(&mut middle);
    add_in_place(&mut product[half..], &middle);
    product
}

/// `base ^ exponent`.
pub(super) fn pow(base: &[u64], exponent: u32) -> Vec<u64> {
    let mut result = vec![1];
    let mut square = base.to_vec();
    let mut remaining = exponent;
    while remaining != 0 {
        if remaining & 1 == 1 {
            result = mul(&result, &square);
        }
        remaining >>= 1;
        if remaining != 0 {
            square = mul(&square, &square);
        }
    }
    result
}

/// Multiplies `limbs` by `factor` and adds `addend`.
pub(super) fn mul_add_small(limbs: &mut Vec<u64>, factor: u64, addend: u64) {
    let mut carry = addend;
    for limb in limbs.iter_mut() {
        let wide = u128::from(*limb) * u128::from(factor) + u128::from(carry);
        *limb = wide as u64;
        carry = (wide >> u64::BITS) as u64;
    }
    if carry != 0 {
        limbs.push(carry);
    }
}

/// Divides `limbs` in place by a non-zero `divisor` and returns the remainder.
pub(super) fn div_small(limbs: &mut Vec<u64>, divisor: u64) -> u64 {
    let divisor = u128::from(divisor);
    let mut remainder = 0u128;
    for limb in limbs.iter_mut().rev() {
        let wide = remainder << u64::BITS | u128::from(*limb);
        *limb = (wide / divisor) as u64;
        remainder = wide % divisor;
    }
    normalize(limbs);
    remainder as u64
}

/// `a << bits`.
pub(super) fn shl(a: &[u64], bits: u64) -> Vec<u64> {
    if a.is_empty() {
        return Vec::new();
    }
    let word_shift = (bits / u64::from(u64::BITS)) as usize;
    let bit_shift = (bits % u64::from(u64::BITS)) as u32;
    let mut shifted = vec![0; word_shift];
    shifted.reserve(a.len() + 1);
    if bit_shift == 0 {
        shifted.extend_from_slice(a);
    } else {
        let mut carry = 0;
        for &limb in a {
            shifted.push(limb << bit_shift | carry);
            carry = limb >> (u64::BITS - bit_shift);
        }
        shifted.push(carry);
    }
    normalize(&mut shifted);
    shifted
}

/// `a >> bits`, discarding the bits shifted out.
pub(super) fn shr(a: &[u64], bits: u64) -> Vec<u64> {
    let word_shift = bits / u64::from(u64::BITS);
    let bit_shift = (bits % u64::from(u64::BITS)) as u32;
    let kept = match usize::try_from(word_shift) {
        Ok(words) if words < a.len() => &a[words..],
        _ => return Vec::new(),
    };
    let mut shifted: Vec<u64> = if bit_shift == 0 {
        kept.to_vec()
    } else {
        kept.iter()
            .enumerate()
            .map(|(index, &limb)| {
                let above = kept.get(index + 1).copied().unwrap_or(0);
                limb >> bit_shift | above << (u64::BITS - bit_shift)
            })
            .collect()
    };
    normalize(&mut shifted);
    shifted
}

/// Quotient and remainder of `a / b` for non-zero `b` (Knuth, TAOCP vol. 2, algorithm D).
pub(super) fn div_rem(a: &[u64], b: &[u64]) -> (Vec<u64>, Vec<u64>) {
    if compare(a, b) == Ordering::Less {
        return (Vec::new(), a.to_vec());
    }
    if let [divisor] = *b {
        let mut quotient = a.to_vec();
        let remainder = div_small(&mut quotient, divisor);
        let remainder = if remainder == 0 { Vec::new() } else { vec![remainder] };
        return (quotient, remainder);
    }
    let shift = u64::from(b.last().map_or(0, |top| top.leading_zeros()));
    let divisor = shl(b, shift);
    let mut numerator = shl(a, shift);
    numerator.resize(a.len() + 1, 0);
    let mut quotient = vec![0; a.len() + 1 - b.len()];
    for position in (0..quotient.len()).rev() {
        let window = &mut numerator[position..position + divisor.len() + 1];
        quotient[position] = divide_step(window, &divisor);
    }
    normalize(&mut quotient);
    numerator.truncate(b.len());
    normalize(&mut numerator);
    (quotient, shr(&numerator, shift))
}

/// Replaces `window` by `window mod divisor` and returns the quotient limb.
///
/// `divisor` has its top bit set and at least two limbs, `window` is one limb longer than
/// `divisor` and the quotient fits one limb.
fn divide_step(window: &mut [u64], divisor: &[u64]) -> u64 {
    let ([.., third, second, top], [.., divisor_second, divisor_top]) = (&*window, divisor) else {
        return 0;
    };
    let mut estimate = estimate_quotient([*top, *second, *third], [*divisor_top, *divisor_second]);
    if multiply_subtract(window, divisor, estimate) {
        estimate -= 1;
        add_back(window, divisor);
    }
    estimate
}

/// The quotient limb estimate from the top limbs, at most one too large.
fn estimate_quotient(top: [u64; 3], divisor: [u64; 2]) -> u64 {
    let [high, middle, low] = top;
    let [divisor_high, divisor_low] = divisor;
    if high >= divisor_high {
        return u64::MAX;
    }
    let numerator = u128::from(high) << u64::BITS | u128::from(middle);
    let mut estimate = (numerator / u128::from(divisor_high)) as u64;
    let mut remainder = numerator % u128::from(divisor_high);
    while remainder >> u64::BITS == 0
        && u128::from(estimate) * u128::from(divisor_low) > remainder << u64::BITS | u128::from(low)
    {
        estimate -= 1;
        remainder += u128::from(divisor_high);
    }
    estimate
}

/// `window -= divisor * factor`; returns true when the result went negative.
fn multiply_subtract(window: &mut [u64], divisor: &[u64], factor: u64) -> bool {
    let mut carry = 0u64;
    let mut borrow = false;
    let (low, high) = window.split_at_mut(divisor.len());
    for (limb, &divisor_limb) in low.iter_mut().zip(divisor) {
        let product = u128::from(divisor_limb) * u128::from(factor) + u128::from(carry);
        carry = (product >> u64::BITS) as u64;
        let (difference, overflow_a) = limb.overflowing_sub(product as u64);
        let (difference, overflow_b) = difference.overflowing_sub(u64::from(borrow));
        *limb = difference;
        borrow = overflow_a || overflow_b;
    }
    let Some(top) = high.first_mut() else {
        return false;
    };
    let (difference, overflow_a) = top.overflowing_sub(carry);
    let (difference, overflow_b) = difference.overflowing_sub(u64::from(borrow));
    *top = difference;
    overflow_a || overflow_b
}

/// `window += divisor`, dropping the carry out of the top limb.
fn add_back(window: &mut [u64], divisor: &[u64]) {
    let mut carry = false;
    let mut divisor_limbs = divisor.iter();
    for limb in window.iter_mut() {
        let addend = divisor_limbs.next().copied().unwrap_or(0);
        let (sum, overflow_a) = limb.overflowing_add(addend);
        let (sum, overflow_b) = sum.overflowing_add(u64::from(carry));
        *limb = sum;
        carry = overflow_a || overflow_b;
    }
}
