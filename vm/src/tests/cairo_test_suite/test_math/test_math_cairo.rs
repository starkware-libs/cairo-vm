//! Tests for `math.cairo`.

use std::sync::LazyLock;

use super::math_test_utils::{is_quad_residue_mod_prime, MAX_DIV, RC_BOUND};
use crate::test_helpers::error_utils::{
    expect_assert_lt_felt252, expect_assert_not_equal_fail, expect_diff_index_comp,
    expect_diff_type_comparison, expect_hint_assert_not_zero, expect_hint_out_of_valid_range,
    expect_hint_value_outside_250_bit_range, expect_hint_value_outside_valid_range,
    expect_non_le_felt252, expect_ok, expect_split_int_limb_out_of_range,
    expect_split_int_not_zero, VmCheck,
};
use crate::assert_mr_eq;
use crate::cairo_args;
use crate::load_cairo_program;
use crate::types::builtin_name::BuiltinName;
use crate::types::program::Program;
use crate::types::relocatable::MaybeRelocatable;
use crate::utils::CAIRO_PRIME;
use crate::vm::runners::function_runner::CairoFunctionRunner;
use crate::Felt252;
use num_bigint::{BigInt, BigUint, RandBigInt};
use num_traits::{One, Signed, Zero};
use rand::thread_rng;
use rstest::{fixture, rstest};

// ===================== Shared constants (LazyLock) =====================

/// The compiled Cairo math program, loaded once and shared across all tests.
static PROGRAM: LazyLock<Program> = LazyLock::new(|| load_cairo_program!("main_math_test.json"));

/// Interesting felt values used in several tests.
static INTERESTING_FELTS: LazyLock<Vec<BigUint>> = LazyLock::new(|| {
    let p = &*CAIRO_PRIME;
    vec![
        BigUint::from(0u64),
        BigUint::from(1u64),
        BigUint::from(2u64).pow(128) - BigUint::one(),
        BigUint::from(2u64).pow(128),
        BigUint::from(2u64).pow(128) + BigUint::one(),
        p / BigUint::from(3u64) - BigUint::one(),
        p / BigUint::from(3u64),
        p / BigUint::from(3u64) + BigUint::one(),
        p / BigUint::from(2u64) - BigUint::one(),
        p / BigUint::from(2u64),
        p / BigUint::from(2u64) + BigUint::one(),
        BigUint::from(2u64).pow(251) - BigUint::one(),
        BigUint::from(2u64).pow(251),
        BigUint::from(2u64).pow(251) + BigUint::one(),
        p - BigUint::from(2u64),
        p - BigUint::one(),
    ]
});

// ===================== Helpers =====================

// ===================== Fixture =====================

/// Creates a fresh CairoFunctionRunner from the shared PROGRAM.
#[fixture]
fn runner() -> CairoFunctionRunner {
    CairoFunctionRunner::new_for_testing(&PROGRAM).unwrap()
}

// ===================== test_assert_not_zero =====================

#[rstest]
// Case: value=7
// Expected: Success.
#[case(Some(BigUint::from(7u64)), expect_ok)]
// Case: value=random
// Expected: Success.
#[case::random(None, expect_ok)]
// Case: value=0
// Expected: Error.
#[case(Some(BigUint::zero()), expect_hint_assert_not_zero)]
fn test_assert_not_zero(#[case] value: Option<BigUint>, #[case] check: VmCheck<()>) {
    let value = match value {
        Some(v) => v,
        None => {
            let mut rng = thread_rng();
            rng.gen_biguint_range(&BigUint::one(), &CAIRO_PRIME)
        }
    };

    let mut runner = runner();
    let args = cairo_args!(value);
    let res = runner.run_default_cairo0("assert_not_zero", &args);
    check(&res);
}

// ===================== test_assert_not_equal =====================

#[rstest]
// Not equal integers
// Case: a=3, b=7
// Expected: Success.
#[case::not_equal_ints(MaybeRelocatable::from(3), MaybeRelocatable::from(7), expect_ok)]
// Not equal relocatables (same segment, different offset)
// Case: a=(2, 5), b=(2, 10)
// Expected: Success.
#[case::not_equal_relocs(
    MaybeRelocatable::from((2isize, 5)),
    MaybeRelocatable::from((2isize, 10)),
    expect_ok
)]
// Equal integers
// Case: a=5, b=5
// Expected: Error.
#[case::equal_ints(
    MaybeRelocatable::from(5),
    MaybeRelocatable::from(5),
    expect_assert_not_equal_fail
)]
// Equal relocatables
// Case: a=(1, 5), b=(1, 5)
// Expected: Error.
#[case::equal_relocs(
    MaybeRelocatable::from((1isize, 5)),
    MaybeRelocatable::from((1isize, 5)),
    expect_assert_not_equal_fail
)]
// Non-comparable: relocatable vs int
// Case: a=(1, 5), b=0
// Expected: Error.
#[case::non_comparable_reloc_vs_int(
   MaybeRelocatable::from((1isize, 5)),
    MaybeRelocatable::from(0),
    expect_diff_type_comparison
)]
// Non-comparable: different segments
// Case: a=(1, 5), b=(2, 3)
// Expected: Error.
#[case::non_comparable_diff_segments(
    MaybeRelocatable::from((1isize, 5)),
   MaybeRelocatable::from((2isize, 3)),
    expect_diff_index_comp
)]

fn test_assert_not_equal(
    #[case] a: MaybeRelocatable,
    #[case] b: MaybeRelocatable,
    #[case] check: VmCheck<()>,
) {
    let mut runner = runner();
    let args = cairo_args!(a, b);
    let res = runner.run_default_cairo0("assert_not_equal", &args);
    check(&res);
}

// ===================== test_assert_250_bit =====================
#[rstest]
// Valid cases (should pass)
// Case: value=0
// Expected: Success.
#[case::zero(BigUint::from(0u64), expect_ok)]
// Case: value=1
// Expected: Success.
#[case::one(BigUint::from(1u64), expect_ok)]
// Case: value=(2^250)-1
// Expected: Success.
#[case::max_valid(BigUint::from(2u64).pow(250) - BigUint::one(), expect_ok)]
// Invalid cases (should fail)
// Case: value=2^250
// Expected: Error.
#[case::at_boundary(BigUint::from(2u64).pow(250), expect_hint_value_outside_250_bit_range)]
// Case: value=(2^250)+1
// Expected: Error.
#[case::above_boundary(BigUint::from(2u64).pow(250) + BigUint::one(), expect_hint_value_outside_250_bit_range)]
// Case: value=2^251
// Expected: Error.
#[case::way_above(BigUint::from(2u64).pow(251), expect_hint_value_outside_250_bit_range)]
// Case: value=PRIME-1
// Expected: Error.
#[case::near_prime(&*CAIRO_PRIME - BigUint::one(), expect_hint_value_outside_250_bit_range)]
fn test_assert_250_bit(
    mut runner: CairoFunctionRunner,
    #[case] value: BigUint,
    #[case] check: VmCheck<()>,
) {
    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");

    let args = cairo_args!(rc_base.clone(), value);
    let res = runner.run_default_cairo0("assert_250_bit", &args);
    check(&res);

    // If successful, verify the return value
    if res.is_ok() {
        let ret = runner.vm.get_return_values(1).unwrap();
        assert_mr_eq!(&ret[0], &rc_base.add_usize(3usize).unwrap());
    }
}

// ===================== test_split_felt =====================

#[rstest]
// Case: idx=0
// Expected: Success.
#[case::idx_0(0)]
// Case: idx=1
// Expected: Success.
#[case::idx_1(1)]
// Case: idx=2
// Expected: Success.
#[case::idx_2(2)]
// Case: idx=3
// Expected: Success.
#[case::idx_3(3)]
// Case: idx=4
// Expected: Success.
#[case::idx_4(4)]
// Case: idx=5
// Expected: Success.
#[case::idx_5(5)]
// Case: idx=6
// Expected: Success.
#[case::idx_6(6)]
// Case: idx=7
// Expected: Success.
#[case::idx_7(7)]
// Case: idx=8
// Expected: Success.
#[case::idx_8(8)]
// Case: idx=9
// Expected: Success.
#[case::idx_9(9)]
// Case: idx=10
// Expected: Success.
#[case::idx_10(10)]
// Case: idx=11
// Expected: Success.
#[case::idx_11(11)]
// Case: idx=12
// Expected: Success.
#[case::idx_12(12)]
// Case: idx=13
// Expected: Success.
#[case::idx_13(13)]
// Case: idx=14
// Expected: Success.
#[case::idx_14(14)]
// Case: idx=15
// Expected: Success.
#[case::idx_15(15)]
fn test_split_felt(mut runner: CairoFunctionRunner, #[case] idx: usize) {
    let mask_128 = BigUint::from(2u64).pow(128) - BigUint::one();
    let value = &INTERESTING_FELTS[idx];

    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");

    let expected_high: BigUint = value >> 128;
    let expected_low = value & &mask_128;

    let args = cairo_args!(rc_base.clone(), value);
    runner
        .run_default_cairo0("split_felt", &args)
        .unwrap_or_else(|e| panic!("split_felt failed for value {value}: {e}"));

    let ret = runner.vm.get_return_values(3).unwrap();
    // ret = [range_check_ptr, high, low]
    assert_mr_eq!(
        &ret[0],
        &rc_base.add_usize(3usize).unwrap(),
        "range_check_ptr mismatch for value {value}"
    );
    assert_mr_eq!(&ret[1], &expected_high, "high mismatch for value {value}");
    assert_mr_eq!(&ret[2], &expected_low, "low mismatch for value {value}");
}

// ===================== test_assert_le_felt =====================

#[rstest]
fn test_assert_le_felt(
    mut runner: CairoFunctionRunner,
    #[values(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15)] idx0: usize,
    #[values(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15)] idx1: usize,
) {
    let value0 = &INTERESTING_FELTS[idx0];
    let value1 = &INTERESTING_FELTS[idx1];

    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");

    let args = cairo_args!(rc_base.clone(), value0, value1);

    if value0 <= value1 {
        runner
            .run_default_cairo0("assert_le_felt", &args)
            .unwrap_or_else(|e| panic!("assert_le_felt failed for {value0} <= {value1}: {e}"));
        let ret = runner.vm.get_return_values(1).unwrap();
        assert_mr_eq!(
            &ret[0],
            &rc_base.add_usize(4usize).unwrap(),
            "range_check_ptr mismatch for {value0} <= {value1}"
        );
    } else {
        let result = runner.run_default_cairo0("assert_le_felt", &args);
        expect_non_le_felt252(&result);
    }
}

// ===================== test_assert_lt_felt =====================

#[rstest]
fn test_assert_lt_felt(
    mut runner: CairoFunctionRunner,
    #[values(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15)] idx0: usize,
    #[values(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15)] idx1: usize,
) {
    let value0 = &INTERESTING_FELTS[idx0];
    let value1 = &INTERESTING_FELTS[idx1];

    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");

    let args = cairo_args!(rc_base.clone(), value0, value1);

    if value0 < value1 {
        runner
            .run_default_cairo0("assert_lt_felt", &args)
            .unwrap_or_else(|e| panic!("assert_lt_felt failed for {value0} < {value1}: {e}"));
        let ret = runner.vm.get_return_values(1).unwrap();
        assert_mr_eq!(
            &ret[0],
            &rc_base.add_usize(4usize).unwrap(),
            "range_check_ptr mismatch for {value0} < {value1}"
        );
    } else {
        let result = runner.run_default_cairo0("assert_lt_felt", &args);
        expect_assert_lt_felt252(&result);
    }
}

// ===================== test_abs_value =====================

#[rstest]
// Case: value_case=17
// Expected: Success.
#[case(BigInt::from(17), expect_ok)]
// Case: value_case=-42
// Expected: Success.
#[case(BigInt::from(-42), expect_ok)]
// Case: value_case=0
// Expected: Success.
#[case(BigInt::from(0), expect_ok)]
// Case: value_case=RC_BOUND
// Expected: Error.
#[case(BigInt::from(RC_BOUND.clone()), expect_hint_value_outside_valid_range)]
// Case: value_case=-RC_BOUND
// Expected: Error.
#[case(-BigInt::from(RC_BOUND.clone()), expect_hint_value_outside_valid_range)]
fn test_abs_value(
    mut runner: CairoFunctionRunner,
    #[case] value_case: BigInt,
    #[case] check: VmCheck<()>,
) {
    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");
    let rc_bound_biguint = runner
        .vm
        .get_range_check_builtin()
        .expect("range_check builtin not found")
        .bound()
        .to_biguint();

    let args = cairo_args!(rc_base.clone(), value_case.clone());
    let result = runner.run_default_cairo0("abs_value", &args);
    check(&result);
    let abs_value = value_case.magnitude();
    if abs_value < &rc_bound_biguint {
        let ret = runner.vm.get_return_values(2).unwrap();
        assert_mr_eq!(&ret[0], &rc_base.add_usize(1usize).unwrap());
        assert_mr_eq!(&ret[1], abs_value);
    }
}

// ===================== test_sign =====================
#[rstest]
// Case: value_case=17
// Expected: Success.
#[case(BigInt::from(17), expect_ok)]
// Case: value_case=-42
// Expected: Success.
#[case(BigInt::from(-42), expect_ok)]
// Case: value_case=0
// Expected: Success.
#[case(BigInt::from(0), expect_ok)]
// Case: value_case=RC_BOUND
// Expected: Error.
#[case(BigInt::from(RC_BOUND.clone()), expect_hint_value_outside_valid_range)]
// Case: value_case=-RC_BOUND
// Expected: Error.
#[case(-BigInt::from(RC_BOUND.clone()), expect_hint_value_outside_valid_range)]
fn test_sign(
    mut runner: CairoFunctionRunner,
    #[case] value_case: BigInt,
    #[case] check: VmCheck<()>,
) {
    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");
    let rc_bound_biguint = runner
        .vm
        .get_range_check_builtin()
        .expect("range_check builtin not found")
        .bound()
        .to_biguint();

    let args = cairo_args!(rc_base.clone(), value_case.clone());
    let result = runner.run_default_cairo0("sign", &args);
    check(&result);
    let abs_value = value_case.magnitude();
    if abs_value < &rc_bound_biguint {
        let ret = runner.vm.get_return_values(2).unwrap();
        // range_check_ptr == rc_base + (1 if value != 0 else 0)
        let expected_rc_ptr = if value_case.is_zero() {
            rc_base
        } else {
            rc_base.add_usize(1usize).unwrap()
        };
        assert_mr_eq!(&ret[0], &expected_rc_ptr);

        // res == (0 if value == 0 else 1 if value > 0 else PRIME - 1)
        let expected_sign = if value_case.is_zero() {
            BigUint::zero()
        } else if value_case.is_positive() {
            BigUint::one()
        } else {
            &*CAIRO_PRIME - BigUint::one()
        };
        assert_mr_eq!(&ret[1], &expected_sign);
    }
}

// ===================== test_unsigned_div_rem =====================

#[rstest]
// 1) q=1333, div=17, r=3
// Case: q=1333, div=17, r=3
// Expected: Success.
#[case::case_1_basic(
    Some(BigUint::from(1333u64)),
    Some(BigUint::from(17u64)),
    Some(BigUint::from(3u64)),
    expect_ok
)]
// 2) q=RC_BOUND-1, div=MAX_DIV, r=MAX_DIV-1
// Case: q=RC_BOUND-1, div=MAX_DIV, r=MAX_DIV-1
// Expected: Success.
#[case::case_2_max_values(
    Some(&*RC_BOUND - BigUint::one()),
    Some(MAX_DIV.clone()),
    Some(&*MAX_DIV - BigUint::one()),
    expect_ok
)]
// 3) q=random, div=MAX_DIV, r=0
// Case: q=random, div=MAX_DIV, r=0
// Expected: Success.
#[case::case_3_random_q(
    None,
    Some(MAX_DIV.clone()),
    Some(BigUint::zero()),
    expect_ok
)]
// 4) q=random, div=MAX_DIV, r=MAX_DIV-1
// Case: q=random, div=MAX_DIV, r=MAX_DIV-1
// Expected: Success.
#[case::case_4_random_q(
    None,
    Some(MAX_DIV.clone()),
    Some(&*MAX_DIV - BigUint::one()),
    expect_ok
)]
// 5) q=random, div=MAX_DIV, r=random
// Case: q=random, div=MAX_DIV, r=random
// Expected: Success.
#[case::case_5_random_q_and_r(
    None,
    Some(MAX_DIV.clone()),
    None,
    expect_ok
)]
// 6) q=random, div=random, r=random
// Case: q=random, div=random, r=random
// Expected: Success.
#[case::case_6_all_random(None, None, None, expect_ok)]
// 7) q=1, div=MAX_DIV+1, r=random -> expected error.
// Case: q=1, div=MAX_DIV+1, r=random
// Expected: Error.
#[case::case_7_invalid_div(
    Some(BigUint::one()),
    Some(&*MAX_DIV + BigUint::one()),
    None,
    expect_hint_out_of_valid_range
)]
fn test_unsigned_div_rem(
    mut runner: CairoFunctionRunner,
    #[case] q: Option<BigUint>,
    #[case] div: Option<BigUint>,
    #[case] r: Option<BigUint>,
    #[case] check: VmCheck<()>,
) {
    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");

    // Verify rc_bound matches expected RC_BOUND (2^128)
    let rc_bound = runner
        .vm
        .get_range_check_builtin()
        .expect("range_check builtin not found")
        .bound()
        .to_biguint();
    assert_eq!(rc_bound, *RC_BOUND, "Unexpected rc_bound");

    let mut rng = thread_rng();
    // Python uses div in [0, MAX_DIV], but remainder generation requires div > 0.
    let div = match div {
        Some(v) => v,
        None => rng.gen_biguint_range(&BigUint::one(), &(&*MAX_DIV + BigUint::one())),
    };
    let r = match r {
        Some(v) => v,
        None => rng.gen_biguint_range(&BigUint::zero(), &div),
    };
    let q = match q {
        Some(v) => v,
        None => rng.gen_biguint_range(&BigUint::zero(), &RC_BOUND),
    };

    let value = &q * &div + &r;

    // Assert value < PRIME (as in Python test)
    assert!(
        value < *CAIRO_PRIME,
        "Generated value is too large. q={q}, div={div}, r={r}"
    );

    let args = cairo_args!(rc_base.clone(), value, div);
    let result = runner.run_default_cairo0("unsigned_div_rem", &args);
    check(&result);

    // If successful, verify the results match expected values
    if result.is_ok() {
        let ret = runner.vm.get_return_values(3).unwrap();
        assert_mr_eq!(
            &ret[0],
            &rc_base.add_usize(3usize).unwrap(),
            "range_check_ptr mismatch"
        );
        assert_mr_eq!(&ret[1], &q, "quotient mismatch");
        assert_mr_eq!(&ret[2], &r, "remainder mismatch");
    }
}

// ===================== test_signed_div_rem =====================
#[rstest]
// Case: q=1333, div=17, r=3, bound=random in chosen range [q+1,RC_BOUND/2])
// Expected: Success.
#[case::basic(
    Some(BigInt::from(1333)),
    Some(BigUint::from(17u64)),
    Some(BigUint::from(3u64)),
    None,
    expect_ok
)]
// Case: q=-1333, div=17, r=3, bound=random in chosen range [-q,RC_BOUND/2])
// Expected: Success.
#[case::negative_basic(
    Some(BigInt::from(-1333)),
    Some(BigUint::from(17u64)),
    Some(BigUint::from(3u64)),
    None,
    expect_ok
)]
// Case: q=RC_BOUND/2-1, div=MAX_DIV, r=MAX_DIV-1, bound=random in chosen range [q+1,RC_BOUND/2])
// Expected: Success.
#[case::max_pos(
    Some(BigInt::from(&*RC_BOUND / BigUint::from(2u64) - BigUint::one())),
    Some(MAX_DIV.clone()),
    Some(&*MAX_DIV - BigUint::one()),
    None,
    expect_ok
)]
// Case: q=-(RC_BOUND/2)+1, div=MAX_DIV, r=0, bound=random in chosen range [-q,RC_BOUND/2])
// Expected: Success.
#[case::max_neg(
    Some(-(BigInt::from(&*RC_BOUND / BigUint::from(2u64))) + BigInt::one()),
    Some(MAX_DIV.clone()),
    Some(BigUint::zero()),
    None,
    expect_ok
)]
// Case: q=random, div=MAX_DIV, r=0, bound=random in chosen range ([q+1,RC_BOUND/2] or
// [-q,RC_BOUND/2]) Expected: Success.
#[case::random_q_max_div_r_zero(None, Some(MAX_DIV.clone()), Some(BigUint::zero()), None, expect_ok)]
// Case: q=random, div=MAX_DIV, r=MAX_DIV-1, bound=random in chosen range ([q+1,RC_BOUND/2] or
// [-q,RC_BOUND/2]) Expected: Success.
#[case::random_q_max_div_r_max(None, Some(MAX_DIV.clone()), Some(&*MAX_DIV - BigUint::one()), None, expect_ok)]
// Case: q=random, div=MAX_DIV, r=random, bound=random in chosen range ([q+1,RC_BOUND/2] or
// [-q,RC_BOUND/2]) Expected: Success.
#[case::random_q_max_div_random_r(None, Some(MAX_DIV.clone()), None, None, expect_ok)]
// Case: q=RC_BOUND/2-1, div=random, r=random, bound=RC_BOUND/2
// Expected: Success.
#[case::bound_eq_half_pos_q(
    Some(BigInt::from(&*RC_BOUND / BigUint::from(2u64) - BigUint::one())),
    None,
    None,
    Some(&*RC_BOUND / BigUint::from(2u64)),
    expect_ok
)]
// Case: q=-RC_BOUND/2, div=random, r=random, bound=RC_BOUND/2
// Expected: Success.
#[case::bound_eq_half_neg_q(
    Some(-BigInt::from(&*RC_BOUND / BigUint::from(2u64))),
    None,
    None,
    Some(&*RC_BOUND / BigUint::from(2u64)),
    expect_ok
)]
// Case: q=1, div=MAX_DIV+1, r=random, bound=random in chosen range [q+1,RC_BOUND/2])
// Expected: Error.
#[case::invalid_div(
    Some(BigInt::one()),
    Some(&*MAX_DIV + BigUint::one()),
    None,
    None,
    expect_hint_out_of_valid_range
)]
// Case: q=random, div=random, r=random, bound=RC_BOUND/2+1
// Expected: Error.
#[case::invalid_bound(
    None,
    None,
    None,
    Some(&*RC_BOUND / BigUint::from(2u64) + BigUint::one()),
    expect_hint_out_of_valid_range
)]
fn test_signed_div_rem(
    mut runner: CairoFunctionRunner,
    #[case] q: Option<BigInt>,
    #[case] div: Option<BigUint>,
    #[case] r: Option<BigUint>,
    #[case] bound: Option<BigUint>,
    #[case] check: VmCheck<()>,
) {
    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");

    let half_rc_bound = &*RC_BOUND / BigUint::from(2u64);
    let mut rng = thread_rng();

    let div = match div {
        Some(v) => v,
        None => rng.gen_biguint_range(&BigUint::one(), &(&*MAX_DIV + BigUint::one())),
    };
    let r = match r {
        Some(v) => v,
        None => rng.gen_biguint_range(&BigUint::zero(), &div),
    };
    let q = match q {
        Some(v) => v,
        None => {
            let min = -BigInt::from(half_rc_bound.clone());
            let max = BigInt::from(half_rc_bound.clone());
            rng.gen_bigint_range(&min, &max)
        }
    };
    let bound = match bound {
        Some(v) => v,
        None => {
            let lower = if q >= BigInt::zero() {
                q.clone() + BigInt::one()
            } else {
                -q.clone()
            };
            let upper = BigInt::from(half_rc_bound.clone() + BigUint::one());
            rng.gen_bigint_range(&lower, &upper)
                .to_biguint()
                .expect("bound should be non-negative")
        }
    };

    let value = q.clone() * BigInt::from(div.clone()) + BigInt::from(r.clone());
    let half_prime = BigInt::from((&*CAIRO_PRIME) >> 1usize);
    let neg_half_prime = -half_prime.clone();
    assert!(
        value >= neg_half_prime && value < half_prime,
        "Generated value is too large."
    );

    let args = cairo_args!(rc_base.clone(), value, div, bound);
    let result = runner.run_default_cairo0("signed_div_rem", &args);
    check(&result);

    if result.is_ok() {
        let ret = runner.vm.get_return_values(3).unwrap();
        let rc_ptr = &ret[0];
        let result_q = &ret[1];
        let result_r = &ret[2];

        assert_mr_eq!(rc_ptr, &rc_base.add_usize(4usize).unwrap());
        // Expected_q = q % PRIME (field element conversion).
        let expected_q = Felt252::from(&q);
        assert_mr_eq!(result_q, &expected_q);
        assert_mr_eq!(result_r, &r);
    }
}

// ===================== test_split_int =====================
#[rstest]
// Case: value=0x1234FCDA, n=10, base=16, bound=16, expected_output=vec![0xA, 0xD,
// 0xC, 0xF, 0x4, 0x3, 0x2, 0x1, 0, 0] Expected: Success.
#[case::hex_digits(
    0x1234FCDA_i64,
    10_i64,
    16_i64,
    16_i64,
    Some(vec![0xA, 0xD, 0xC, 0xF, 0x4, 0x3, 0x2, 0x1, 0, 0]),
    expect_ok
)]
// Case: value=0x1234FCDA, n=10, base=256, bound=256, expected_output=vec![0xDA,
// 0xFC, 0x34, 0x12, 0, 0, 0, 0, 0, 0] Expected: Success.
#[case::byte_pairs(
    0x1234FCDA_i64,
    10_i64,
    256_i64,
    256_i64,
    Some(vec![0xDA, 0xFC, 0x34, 0x12, 0, 0, 0, 0, 0, 0]),
    expect_ok
)]
// Case: value=0x1234FCDA, n=10, base=16, bound=15, expected_output=random
// Expected: Error.
#[case::out_of_bound_limb(
    0x1234FCDA_i64,
    10_i64,
    16_i64,
    15_i64,
    None,
    expect_split_int_limb_out_of_range
)]
// Case: value=0xAAA, n=3, base=16, bound=11, expected_output=vec![0xA, 0xA, 0xA]
// Expected: Success.
#[case::exact_fit(
    0xAAA_i64,
    3_i64,
    16_i64,
    11_i64,
    Some(vec![0xA, 0xA, 0xA]),
    expect_ok
)]
// Case: value=0xAAA, n=3, base=16, bound=10, expected_output=random
// Expected: Error.
#[case::bound_too_small(
    0xAAA_i64,
    3_i64,
    16_i64,
    10_i64,
    None,
    expect_split_int_limb_out_of_range
)]
// Case: value=0xAAA, n=2, base=16, bound=16, expected_output=random
// Expected: Error.
#[case::value_out_of_range(0xAAA_i64, 2_i64, 16_i64, 16_i64, None, expect_split_int_not_zero)]
fn test_split_int(
    mut runner: CairoFunctionRunner,
    #[case] value: i64,
    #[case] n: i64,
    #[case] base: i64,
    #[case] bound: i64,
    #[case] expected_output: Option<Vec<i64>>,
    #[case] check: VmCheck<()>,
) {
    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");

    let output = runner.vm.add_memory_segment();
    let output_mr = MaybeRelocatable::from(output);

    let args = cairo_args!(rc_base.clone(), value, n, base, bound, output_mr);
    let result = runner.run_default_cairo0("split_int", &args);
    check(&result);

    if result.is_ok() {
        let expected_output =
            expected_output.expect("expected_output must be set for success case");
        let ret = runner.vm.get_return_values(1).unwrap();
        assert_mr_eq!(&ret[0], &rc_base.add_usize(2usize * n as usize).unwrap());

        let range = runner.vm.get_range(output, n as usize);
        assert_eq!(
            range.len(),
            expected_output.len(),
            "split_int output length mismatch"
        );
        for (i, (actual, exp)) in range.iter().zip(expected_output.iter()).enumerate() {
            let actual_val = actual
                .as_ref()
                .unwrap_or_else(|| panic!("Missing output at index {i}"));
            assert_mr_eq!(
                actual_val.as_ref(),
                *exp,
                "split_int output mismatch at index {i}"
            );
        }
    }
}
// ===================== test_sqrt =====================

#[rstest]
// Case: value=0
// Expected: Success.
#[case::zero(Some(BigUint::from(0u64)), expect_ok)]
// Case: value=1
// Expected: Success.
#[case::one(Some(BigUint::from(1u64)), expect_ok)]
// Case: value=2
// Expected: Success.
#[case::two(Some(BigUint::from(2u64)), expect_ok)]
// Case: value=3
// Expected: Success.
#[case::three(Some(BigUint::from(3u64)), expect_ok)]
// Case: value=4
// Expected: Success.
#[case::four(Some(BigUint::from(4u64)), expect_ok)]
// Case: value=5
// Expected: Success.
#[case::five(Some(BigUint::from(5u64)), expect_ok)]
// Case: value=6
// Expected: Success.
#[case::six(Some(BigUint::from(6u64)), expect_ok)]
// Case: value=7
// Expected: Success.
#[case::seven(Some(BigUint::from(7u64)), expect_ok)]
// Case: value=8
// Expected: Success.
#[case::eight(Some(BigUint::from(8u64)), expect_ok)]
// Case: value=9
// Expected: Success.
#[case::nine(Some(BigUint::from(9u64)), expect_ok)]
// Case: value=(2^250)-1
// Expected: Success.
#[case::max_valid(Some(BigUint::from(2u64).pow(250) - BigUint::one()), expect_ok)]
// Case: value=random
// Expected: Success.
#[case::random(None, expect_ok)]
// Case: value=2^250
// Expected: Error.
#[case::out_of_range_2_pow_250(Some(BigUint::from(2u64).pow(250)), expect_hint_value_outside_250_bit_range)]
// Case: value=PRIME-1
// Expected: Error.
#[case::out_of_range_prime_minus_one(
    Some(&*CAIRO_PRIME - BigUint::one()),
    expect_hint_value_outside_250_bit_range
)]
fn test_sqrt(
    mut runner: CairoFunctionRunner,
    #[case] value: Option<BigUint>,
    #[case] check: VmCheck<()>,
) {
    let value = value.unwrap_or_else(|| {
        let mut rng = thread_rng();
        let upper = BigUint::one() << 250usize;
        rng.gen_biguint_range(&BigUint::zero(), &upper)
    });

    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");

    let args = cairo_args!(rc_base.clone(), value.clone());
    let result = runner.run_default_cairo0("sqrt", &args);
    check(&result);

    if result.is_ok() {
        let ret = runner.vm.get_return_values(2).unwrap();
        assert_mr_eq!(
            &ret[0],
            &rc_base.add_usize(4usize).unwrap(),
            "range_check_ptr mismatch for sqrt({value})"
        );

        let expected_root = value.sqrt();
        assert_mr_eq!(
            &ret[1],
            &expected_root,
            "sqrt result mismatch for value={value}"
        );
    }
}

// ===================== test_horner_eval =====================

#[rstest]
// Case: n=0
// Expected: Success.
#[case::zero_coefficients(0)]
// Case: n=16
// Expected: Success.
#[case::sixteen_coefficients(16)]
fn test_horner_eval(mut runner: CairoFunctionRunner, #[case] n: usize) {
    let mut rng = thread_rng();
    let prime = &*CAIRO_PRIME;

    // Generate random coefficients in [0, PRIME)
    let coefficients: Vec<BigUint> = (0..n)
        .map(|_| rng.gen_biguint_range(&BigUint::zero(), prime))
        .collect();
    let coeff_mr: Vec<MaybeRelocatable> = coefficients.iter().map(MaybeRelocatable::from).collect();

    // Generate random point in [0, PRIME)
    let point = rng.gen_biguint_range(&BigUint::zero(), prime);

    // horner_eval takes (n, coefficients_ptr, point) - coefficients is an array
    let args = cairo_args!(n, coeff_mr, point.clone());
    runner.run_default_cairo0("horner_eval", &args).unwrap();

    let ret = runner.vm.get_return_values(1).unwrap();

    // Compute expected result: sum(coef * point^i for i, coef in enumerate(coefficients)) % PRIME
    let expected: BigUint = coefficients
        .iter()
        .enumerate()
        .map(|(i, coef)| coef * point.modpow(&BigUint::from(i), prime))
        .fold(BigUint::zero(), |acc, x| (acc + x) % prime);

    assert_mr_eq!(&ret[0], &expected);
}

// ===================== test_is_quad_residue =====================

#[rstest]
// Case: x=0
// Expected: Success.
#[case::zero(Some(BigUint::zero()))]
// Case: x=random
// Expected: Success.
#[case::random(None)]
fn test_is_quad_residue(mut runner: CairoFunctionRunner, #[case] x: Option<BigUint>) {
    let prime = &*CAIRO_PRIME;

    let x = x.unwrap_or_else(|| {
        let mut rng = thread_rng();
        rng.gen_biguint_range(&BigUint::one(), prime)
    });

    // Test is_quad_residue(x)
    let args = cairo_args!(x.clone());
    runner.run_default_cairo0("is_quad_residue", &args).unwrap();
    let ret = runner.vm.get_return_values(1).unwrap();

    let expected = is_quad_residue_mod_prime(&x);
    assert_mr_eq!(
        &ret[0],
        expected,
        "is_quad_residue({x}) should return {expected}"
    );

    // Test is_quad_residue(3 * x)
    // 3 is not a quadratic residue modulo PRIME
    let mut runner2 = CairoFunctionRunner::new_for_testing(&PROGRAM).unwrap();
    let three_x = (BigUint::from(3u64) * &x) % prime;
    let args2 = cairo_args!(three_x);
    runner2
        .run_default_cairo0("is_quad_residue", &args2)
        .unwrap();
    let ret2 = runner2.vm.get_return_values(1).unwrap();

    let expected2 = if x.is_zero() {
        1i64 // 3 * 0 = 0, which is QR
    } else if is_quad_residue_mod_prime(&x) == 1 {
        0i64 // x is QR, 3 is not QR, so 3*x is not QR
    } else {
        1i64 // x is not QR, 3 is not QR, so 3*x is QR (product of two non-QR is QR)
    };
    assert_mr_eq!(
        &ret2[0],
        expected2,
        "is_quad_residue(3 * {x}) should return {expected2}"
    );
}
