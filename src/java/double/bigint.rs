//! The minimum of arbitrary-precision unsigned arithmetic that exact float conversion needs.

use std::cmp::Ordering;

/// Largest power of ten that fits a limb.
const POW10_LIMB: u32 = 1_000_000_000;
/// The exponent of [`POW10_LIMB`].
const POW10_LIMB_DIGITS: u32 = 9;

/// An unsigned integer as little-endian 32-bit limbs without leading zero limbs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BigUint {
    limbs: Vec<u32>,
}

impl BigUint {
    /// The integer `value`.
    pub(super) fn from_u64(value: u64) -> BigUint {
        let mut result = BigUint { limbs: vec![value as u32, (value >> 32) as u32] };
        result.normalize();
        result
    }

    /// The integer written by the ASCII decimal `digits`.
    pub(super) fn from_digits(digits: &[u8]) -> BigUint {
        let mut result = BigUint::from_u64(0);
        for chunk in digits.chunks(POW10_LIMB_DIGITS as usize) {
            let value = chunk.iter().fold(0, |value, digit| value * 10 + u32::from(digit - b'0'));
            result.mul_small(10_u32.pow(chunk.len() as u32));
            result.add_small(value);
        }
        result
    }

    /// True if the value is zero.
    pub(super) fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// The position of the highest set bit plus one; zero for the value zero.
    pub(super) fn bit_length(&self) -> usize {
        self.limbs.last().map_or(0, |top| self.limbs.len() * 32 - top.leading_zeros() as usize)
    }

    /// Multiplies by `factor`.
    pub(super) fn mul_small(&mut self, factor: u32) {
        let mut carry = 0_u64;
        for limb in &mut self.limbs {
            let product = u64::from(*limb) * u64::from(factor) + carry;
            *limb = product as u32;
            carry = product >> 32;
        }
        if carry != 0 {
            self.limbs.push(carry as u32);
        }
        self.normalize();
    }

    /// Multiplies by `10^exponent`.
    pub(super) fn mul_pow10(&mut self, exponent: u32) {
        for _ in 0..exponent / POW10_LIMB_DIGITS {
            self.mul_small(POW10_LIMB);
        }
        self.mul_small(10_u32.pow(exponent % POW10_LIMB_DIGITS));
    }

    /// Multiplies by `2^bits`.
    pub(super) fn shl(&mut self, bits: usize) {
        if self.is_zero() {
            return;
        }
        let small = (bits % 32) as u32;
        if small != 0 {
            self.mul_small(1 << small);
        }
        self.limbs.splice(0..0, std::iter::repeat_n(0, bits / 32));
    }

    /// Subtracts `other`, which must not exceed `self`.
    pub(super) fn sub_assign(&mut self, other: &BigUint) {
        let mut borrow = false;
        let subtrahend = other.limbs.iter().chain(std::iter::repeat(&0));
        for (limb, &digit) in self.limbs.iter_mut().zip(subtrahend) {
            let (partial, first) = limb.overflowing_sub(digit);
            let (difference, second) = partial.overflowing_sub(u32::from(borrow));
            *limb = difference;
            borrow = first || second;
        }
        self.normalize();
    }

    fn add_small(&mut self, addend: u32) {
        let mut carry = addend;
        for limb in &mut self.limbs {
            let (sum, overflowed) = limb.overflowing_add(carry);
            *limb = sum;
            carry = u32::from(overflowed);
            if carry == 0 {
                return;
            }
        }
        if carry != 0 {
            self.limbs.push(carry);
        }
    }

    fn normalize(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }
}

impl Ord for BigUint {
    fn cmp(&self, other: &BigUint) -> Ordering {
        self.limbs.len().cmp(&other.limbs.len()).then_with(|| self.limbs.iter().rev().cmp(other.limbs.iter().rev()))
    }
}

impl PartialOrd for BigUint {
    fn partial_cmp(&self, other: &BigUint) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_and_powers_of_ten_agree() {
        let mut power = BigUint::from_u64(1);
        power.mul_pow10(30);
        let digits = format!("1{}", "0".repeat(30));
        assert_eq!(BigUint::from_digits(digits.as_bytes()), power);
    }

    #[test]
    fn bit_length_counts_from_the_top_set_bit() {
        assert_eq!(BigUint::from_u64(0).bit_length(), 0);
        assert_eq!(BigUint::from_u64(1).bit_length(), 1);
        assert_eq!(BigUint::from_u64(1 << 40).bit_length(), 41);
    }

    #[test]
    fn shifting_crosses_limbs_and_keeps_zero_normalized() {
        let mut value = BigUint::from_u64(3);
        value.shl(69);
        assert_eq!(value.bit_length(), 71);
        let mut zero = BigUint::from_u64(0);
        zero.shl(100);
        assert!(zero.is_zero());
    }

    #[test]
    fn subtraction_borrows_across_limbs() {
        let mut value = BigUint::from_u64(1 << 40);
        value.sub_assign(&BigUint::from_u64(1));
        assert_eq!(value, BigUint::from_u64((1 << 40) - 1));
        value.sub_assign(&BigUint::from_u64((1 << 40) - 1));
        assert!(value.is_zero());
    }

    #[test]
    fn ordering_compares_by_magnitude() {
        assert!(BigUint::from_u64(1 << 33) > BigUint::from_u64(5));
        assert!(BigUint::from_u64(6) > BigUint::from_u64(5));
        assert_eq!(BigUint::from_u64(5).cmp(&BigUint::from_u64(5)), Ordering::Equal);
    }
}
