//! `java.math.RoundingMode` and `java.math.MathContext`.

use crate::java::{JResult, Thrown};

/// `java.math.RoundingMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundingMode {
    /// Away from zero.
    Up,
    /// Toward zero.
    Down,
    /// Toward positive infinity.
    Ceiling,
    /// Toward negative infinity.
    Floor,
    /// To nearest, ties away from zero.
    HalfUp,
    /// To nearest, ties toward zero.
    HalfDown,
    /// To nearest, ties to the even neighbour.
    HalfEven,
    /// Throws when rounding would change the value.
    Unnecessary,
}

impl RoundingMode {
    /// `RoundingMode.valueOf(name)`, with `None` where Java throws.
    pub fn from_name(name: &[u16]) -> Option<RoundingMode> {
        let name = String::from_utf16(name).ok()?;
        match name.as_str() {
            "UP" => Some(RoundingMode::Up),
            "DOWN" => Some(RoundingMode::Down),
            "CEILING" => Some(RoundingMode::Ceiling),
            "FLOOR" => Some(RoundingMode::Floor),
            "HALF_UP" => Some(RoundingMode::HalfUp),
            "HALF_DOWN" => Some(RoundingMode::HalfDown),
            "HALF_EVEN" => Some(RoundingMode::HalfEven),
            "UNNECESSARY" => Some(RoundingMode::Unnecessary),
            _ => None,
        }
    }
}

/// `java.math.MathContext`: a precision in significant digits, 0 meaning unlimited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MathContext {
    /// Significant digits of results; 0 for exact arithmetic.
    pub precision: i32,
    /// How results are rounded to `precision`.
    pub rounding: RoundingMode,
}

impl MathContext {
    /// `new MathContext(precision, rounding)`.
    pub fn new(precision: i32, rounding: RoundingMode) -> JResult<MathContext> {
        if precision < 0 {
            return Err(Thrown::new("java.lang.IllegalArgumentException", "Digits < 0"));
        }
        Ok(MathContext { precision, rounding })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::js;

    #[test]
    fn rounding_mode_names_are_exact() {
        assert_eq!(RoundingMode::from_name(&js("HALF_EVEN")), Some(RoundingMode::HalfEven));
        assert_eq!(RoundingMode::from_name(&js("half_even")), None);
        assert_eq!(RoundingMode::from_name(&[0xD800]), None);
    }

    #[test]
    fn negative_precision_is_rejected() {
        let error = MathContext::new(-1, RoundingMode::Up);
        assert_eq!(error, Err(Thrown::new("java.lang.IllegalArgumentException", "Digits < 0")));
    }
}
