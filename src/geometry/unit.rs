//! Fixed-point layout unit: 1/64 px stored in an `i32`.
//!
//! All arithmetic saturates so that overflow can never wrap into a negative
//! width. Saturations and non-finite inputs are counted in [`Saturation`] so
//! the caller can report them as a single warning.

/// Counters for lossy conversions and saturating arithmetic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Saturation {
    pub(crate) saturated: u32,
    pub(crate) non_finite: u32,
}

impl Saturation {
    pub(crate) fn is_clean(&self) -> bool {
        self.saturated == 0 && self.non_finite == 0
    }
}

/// 1/64 px fixed-point value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LayoutUnit(i32);

impl LayoutUnit {
    pub(crate) const SCALE: i32 = 64;
    pub(crate) const ZERO: Self = Self(0);
    pub(crate) const MAX: Self = Self(i32::MAX);
    pub(crate) const MIN: Self = Self(i32::MIN);

    pub(crate) const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub(crate) const fn raw(self) -> i32 {
        self.0
    }

    /// Converts px to layout units, rounding to the nearest 1/64 px. Used for
    /// every caller-supplied value.
    pub(crate) fn from_f32_round(value: f32, sat: &mut Saturation) -> Self {
        Self::from_scaled(value, sat, f64::round)
    }

    /// Converts px to layout units, rounding up. Only for values shodo derives
    /// itself (for example line heights from font metrics).
    pub(crate) fn from_f32_ceil(value: f32, sat: &mut Saturation) -> Self {
        Self::from_scaled(value, sat, f64::ceil)
    }

    fn from_scaled(value: f32, sat: &mut Saturation, rounding: fn(f64) -> f64) -> Self {
        if !value.is_finite() {
            sat.non_finite += 1;
            return Self::ZERO;
        }
        let scaled = rounding(f64::from(value) * f64::from(Self::SCALE));
        if scaled > f64::from(i32::MAX) {
            sat.saturated += 1;
            Self::MAX
        } else if scaled < f64::from(i32::MIN) {
            sat.saturated += 1;
            Self::MIN
        } else {
            Self(scaled as i32)
        }
    }

    pub(crate) fn to_f32(self) -> f32 {
        self.0 as f32 / Self::SCALE as f32
    }

    pub(crate) fn add(self, rhs: Self, sat: &mut Saturation) -> Self {
        match self.0.checked_add(rhs.0) {
            Some(v) => Self(v),
            None => {
                sat.saturated += 1;
                Self(self.0.saturating_add(rhs.0))
            }
        }
    }

    pub(crate) fn sub(self, rhs: Self, sat: &mut Saturation) -> Self {
        match self.0.checked_sub(rhs.0) {
            Some(v) => Self(v),
            None => {
                sat.saturated += 1;
                Self(self.0.saturating_sub(rhs.0))
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn mul_i32(self, rhs: i32, sat: &mut Saturation) -> Self {
        match self.0.checked_mul(rhs) {
            Some(v) => Self(v),
            None => {
                sat.saturated += 1;
                Self(self.0.saturating_mul(rhs))
            }
        }
    }

    /// Division by zero yields zero.
    pub(crate) fn div_i32(self, rhs: i32) -> Self {
        if rhs == 0 {
            Self::ZERO
        } else {
            Self(self.0.saturating_div(rhs))
        }
    }
}

impl std::ops::Add for LayoutUnit {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
}

impl std::ops::Sub for LayoutUnit {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_and_ceil_differ_on_fractions() {
        let mut sat = Saturation::default();
        // 100.004 px * 64 = 6400.256
        assert_eq!(LayoutUnit::from_f32_round(100.004, &mut sat).raw(), 6400);
        assert_eq!(LayoutUnit::from_f32_ceil(100.004, &mut sat).raw(), 6401);
        assert!(sat.is_clean());
    }

    #[test]
    fn non_finite_becomes_zero_and_is_counted() {
        let mut sat = Saturation::default();
        assert_eq!(
            LayoutUnit::from_f32_round(f32::NAN, &mut sat),
            LayoutUnit::ZERO
        );
        assert_eq!(
            LayoutUnit::from_f32_round(f32::INFINITY, &mut sat),
            LayoutUnit::ZERO
        );
        assert_eq!(sat.non_finite, 2);
    }

    #[test]
    fn out_of_range_saturates_and_is_counted() {
        let mut sat = Saturation::default();
        assert_eq!(LayoutUnit::from_f32_round(1.0e9, &mut sat), LayoutUnit::MAX);
        assert_eq!(
            LayoutUnit::from_f32_round(-1.0e9, &mut sat),
            LayoutUnit::MIN
        );
        assert_eq!(sat.saturated, 2);
    }

    #[test]
    fn arithmetic_saturates_instead_of_wrapping() {
        let mut sat = Saturation::default();
        let near_max = LayoutUnit::from_raw(i32::MAX - 1);
        assert_eq!(
            near_max.add(LayoutUnit::from_raw(10), &mut sat),
            LayoutUnit::MAX
        );
        assert_eq!(
            LayoutUnit::MIN.sub(LayoutUnit::from_raw(1), &mut sat),
            LayoutUnit::MIN
        );
        assert_eq!(near_max.mul_i32(2, &mut sat), LayoutUnit::MAX);
        assert_eq!(sat.saturated, 3);
        assert_eq!(LayoutUnit::MIN.div_i32(-1), LayoutUnit::MAX);
        assert_eq!(LayoutUnit::from_raw(64).div_i32(0), LayoutUnit::ZERO);
    }

    #[test]
    fn converts_back_to_px() {
        assert_eq!(LayoutUnit::from_raw(64).to_f32(), 1.0);
        assert_eq!(LayoutUnit::from_raw(32).to_f32(), 0.5);
    }
}
