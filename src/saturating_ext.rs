use duplicate::duplicate_item;

pub trait SaturatingMath: Copy {
    fn saturating_add(self, other: Self) -> Self;

    //fn saturating_sub(self, other: Self) -> Self;
    //
    //fn saturating_mul(self, other: Self) -> Self;
}

#[duplicate_item(
    saturating_impl;
    [f32];
    [f64];
)]
impl SaturatingMath for saturating_impl {
    fn saturating_add(self, other: Self) -> Self {
        if other > 0.0 && self > Self::MAX - other {
            Self::MAX
        } else if other < 0.0 && self < Self::MIN - other {
            Self::MIN
        } else {
            self + other
        }
    }

    //fn saturating_sub(self, other: Self) -> Self {
    //    if other > 0.0 && self < Self::MIN + other {
    //        Self::MIN
    //    } else if other < 0.0 && self > Self::MAX + other {
    //        Self::MAX
    //    } else {
    //        self - other
    //    }
    //}
    //
    //fn saturating_mul(self, other: Self) -> Self {
    //    if self == 0.0 || other == 0.0 {
    //        return 0.0;
    //    }
    //
    //    if self < 0.0 {
    //        return (-self).saturating_mul(-other);
    //    }
    //
    //    if other > 0.0 {
    //        if self > Self::MAX / other {
    //            return Self::MAX;
    //        }
    //    } else if other < 0.0 {
    //        if self > Self::MIN / other {
    //            return Self::MIN;
    //        }
    //    }
    //
    //    self * other
    //}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[duplicate_item(
        test_type test_name;
        [f32]     [test_saturating_add_f32];
        [f64]     [test_saturating_add_f64];
    )]
    #[rstest::rstest]
    #[test]
    #[case(1.0, 1.0, 2.0)]
    #[case(test_type::MAX - 1.0, 2.0, test_type::MAX)]
    #[case(test_type::MIN + 1.0, -2.0, test_type::MIN)]
    fn test_name(#[case] a: test_type, #[case] b: test_type, #[case] expected: test_type) {
        assert_eq!(a.saturating_add(b), expected);
    }
}
