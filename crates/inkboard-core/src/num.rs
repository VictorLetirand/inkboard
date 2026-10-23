//! Lossy numeric conversions, in one place: each function says what happens out of range instead of a bare `as` at every
//! call site. Float to integer saturates (NaN gives 0), like `as`; integer to float rounds to the nearest representable value.
#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "this module is the single home of the lossy conversions; their behaviour is documented on each function"
)]

/// A count or an index as `f64` (exact up to 2^53, far beyond any board).
#[must_use]
pub const fn f64_of(n: usize) -> f64 {
    n as f64
}

/// A count or an index as `f32` (exact up to 2^24).
#[must_use]
pub const fn f32_of(n: usize) -> f32 {
    n as f32
}

/// A model value (`f64`) as a screen value (`f32`), rounded to the nearest `f32`.
#[must_use]
pub const fn f32(v: f64) -> f32 {
    v as f32
}

/// Saturating: negative and NaN give 0, too large gives `usize::MAX`. The fraction is dropped.
#[must_use]
pub const fn to_usize(v: f64) -> usize {
    v as usize
}

/// Saturating: negative and NaN give 0, too large gives `u64::MAX`. The fraction is dropped.
#[must_use]
pub const fn to_u64(v: f64) -> u64 {
    v as u64
}

/// Saturating: negative and NaN give 0, too large gives `u32::MAX`. The fraction is dropped.
#[must_use]
pub const fn to_u32(v: f64) -> u32 {
    v as u32
}

/// Saturating: negative and NaN give 0, above 255 gives 255. The fraction is dropped (round first for a color channel).
#[must_use]
pub const fn to_u8(v: f64) -> u8 {
    v as u8
}

/// Saturating: NaN gives 0. The fraction is dropped.
#[must_use]
pub const fn to_i64(v: f64) -> i64 {
    v as i64
}

/// Saturating: NaN gives 0. The fraction is dropped.
#[must_use]
pub const fn to_i32(v: f64) -> i32 {
    v as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_saturate() {
        assert_eq!(to_u8(300.0), 255);
        assert_eq!(to_u8(-1.0), 0);
        assert_eq!(to_usize(f64::NAN), 0);
        assert_eq!(to_u32(2.9), 2);
        assert_eq!(to_i32(-1e12), i32::MIN);
        assert!((f64::from(f32(0.1)) - 0.1).abs() < 1e-7);
        assert!((f64_of(3) - 3.0).abs() < f64::EPSILON);
    }
}
