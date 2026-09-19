use riscbox::softfp::{
    FLAG_DIVIDE_ZERO, FLAG_INEXACT, FLAG_INVALID, RoundingMode, add_f32, class_f32, div_f32,
    eq_f32, f32_to_f64, f32_to_i32, f64_to_f32, fma_f64, i64_to_f32, le_f32, lt_f32, max_f32,
    min_f32, mul_f64, sqrt_f64, u64_to_f64,
};

#[test]
fn arithmetic_is_bit_exact_for_representative_results() {
    let mut flags = 0;
    assert_eq!(
        add_f32(
            1.5_f32.to_bits(),
            2.25_f32.to_bits(),
            RoundingMode::NearEven,
            &mut flags
        ),
        3.75_f32.to_bits()
    );
    assert_eq!(
        mul_f64(
            1.5_f64.to_bits(),
            2.25_f64.to_bits(),
            RoundingMode::NearEven,
            &mut flags
        ),
        3.375_f64.to_bits()
    );
    assert_eq!(
        sqrt_f64(4.0_f64.to_bits(), RoundingMode::NearEven, &mut flags),
        2.0_f64.to_bits()
    );
    assert_eq!(
        fma_f64(
            1.5_f64.to_bits(),
            2.0_f64.to_bits(),
            (-1.0_f64).to_bits(),
            RoundingMode::NearEven,
            &mut flags
        ),
        2.0_f64.to_bits()
    );
    assert_eq!(flags, 0);
}

#[test]
fn fused_operations_preserve_tiny_products_and_exact_cancellation() {
    let mut flags = 0;
    assert_eq!(
        fma_f64(
            0x26f0_0000_0000_0000,
            0x26f0_0000_0000_0000,
            0,
            RoundingMode::NearEven,
            &mut flags,
        ),
        0x0df0_0000_0000_0000
    );
    assert_eq!(flags, 0);

    assert_eq!(
        fma_f64(
            0x3ff0_0000_0000_0001,
            0x3fef_ffff_ffff_fffe,
            0xbff0_0000_0000_0000,
            RoundingMode::NearEven,
            &mut flags,
        ),
        0xb970_0000_0000_0000
    );
    assert_eq!(flags, 0);
}

#[test]
fn division_normalizes_subnormal_operands_before_rounding() {
    let mut flags = 0;
    assert_eq!(
        riscbox::softfp::div_f64(
            0x0000_0000_0000_0001,
            0x0010_0000_0000_0001,
            RoundingMode::NearEven,
            &mut flags,
        ),
        0x3caf_ffff_ffff_fffe
    );
    assert_eq!(flags, FLAG_INEXACT);
}

#[test]
fn rounding_modes_and_sticky_flags_are_observed() {
    let mut flags = 0;
    let one = 1.0_f32.to_bits();
    let tiny = 2.0_f32.powi(-25).to_bits();
    assert_eq!(add_f32(one, tiny, RoundingMode::NearEven, &mut flags), one);
    assert_eq!(flags, FLAG_INEXACT);
    flags = 0;
    assert_eq!(add_f32(one, tiny, RoundingMode::Up, &mut flags), one + 1);
    flags = 0;
    assert_eq!(
        div_f32(one, 0, RoundingMode::NearEven, &mut flags),
        f32::INFINITY.to_bits()
    );
    assert_eq!(flags, FLAG_DIVIDE_ZERO);
}

#[test]
fn nan_comparison_minimum_and_classification_follow_riscv() {
    let mut flags = 0;
    let qnan = 0x7fc0_0001;
    let snan = 0x7f80_0001;
    let one = 1.0_f32.to_bits();
    assert_eq!(min_f32(qnan, one, &mut flags), one);
    assert_eq!(max_f32(snan, one, &mut flags), one);
    assert_eq!(flags, FLAG_INVALID);
    flags = 0;
    assert!(!eq_f32(qnan, one, &mut flags));
    assert_eq!(flags, 0);
    assert!(!lt_f32(qnan, one, &mut flags));
    assert_eq!(flags, FLAG_INVALID);
    flags = 0;
    assert!(le_f32((-0.0_f32).to_bits(), 0, &mut flags));
    assert_eq!(class_f32(snan), 1 << 8);
    assert_eq!(class_f32(qnan), 1 << 9);
}

#[test]
fn format_and_integer_conversions_cover_boundaries() {
    let mut flags = 0;
    assert_eq!(f32_to_f64(1.5_f32.to_bits(), &mut flags), 1.5_f64.to_bits());
    assert_eq!(
        f64_to_f32(1.5_f64.to_bits(), RoundingMode::NearEven, &mut flags),
        1.5_f32.to_bits()
    );
    assert_eq!(
        f32_to_i32(2.5_f32.to_bits(), RoundingMode::NearEven, &mut flags),
        2
    );
    assert_eq!(
        f32_to_i32(2.5_f32.to_bits(), RoundingMode::NearMaxMag, &mut flags),
        3
    );
    assert_eq!(
        i64_to_f32(i64::MIN, RoundingMode::NearEven, &mut flags),
        (-9_223_372_036_854_775_808.0_f32).to_bits()
    );
    assert_eq!(
        u64_to_f64(u64::MAX, RoundingMode::Zero, &mut flags),
        0x43ef_ffff_ffff_ffff
    );
}
