//! Tests for `math_cmp.cairo`.

use std::sync::LazyLock;
use cairo_vm::types::program::Program;

use super::math_test_utils::{sub_mod_prime, RC_BOUND};
use cairo_vm::cairo_args;
use cairo_vm::types::builtin_name::BuiltinName;
use cairo_vm::utils::CAIRO_PRIME;
use cairo_vm::vm::runners::cairo_function_runner::CairoFunctionRunner;
use num_bigint::{BigUint, RandBigInt};
use num_traits::{One, Zero};
use rand::thread_rng;
use rstest::{fixture, rstest};

// ===================== Shared constants (LazyLock) =====================

/// The compiled Cairo math_cmp program, loaded once and shared across all tests.
static PROGRAM: LazyLock<Program> =
    LazyLock::new(|| load_cairo_program!("main_math_cmp_test.json"));

/// Interesting felt values used in test_is_le_felt.
static INTERESTING_FELTS: LazyLock<Vec<BigUint>> = LazyLock::new(|| {
    let p = &*CAIRO_PRIME;
    vec![
        BigUint::zero(),
        BigUint::one(),
        &*RC_BOUND - BigUint::one(),
        RC_BOUND.clone(),
        &*RC_BOUND + BigUint::one(),
        BigUint::from(2u64).pow(251) - BigUint::one(),
        BigUint::from(2u64).pow(251),
        BigUint::from(2u64).pow(251) + BigUint::one(),
        p - BigUint::from(2u64),
        p - BigUint::one(),
    ]
});

/// Pair examples used in is_le / is_nn_le / is_in_range tests.
static PAIR_EXAMPLES: LazyLock<Vec<(BigUint, BigUint)>> = LazyLock::new(|| {
    vec![
        (BigUint::from(0u64), BigUint::from(0u64)),
        (BigUint::from(0u64), BigUint::from(1u64)),
        (BigUint::from(1u64), BigUint::from(0u64)),
        (BigUint::from(2u64).pow(200), BigUint::from(2u64).pow(200)),
        (
            BigUint::from(2u64).pow(200),
            BigUint::from(2u64).pow(200) + &*RC_BOUND - BigUint::one(),
        ),
        (
            BigUint::from(2u64).pow(200),
            BigUint::from(2u64).pow(200) + &*RC_BOUND,
        ),
    ]
});

// ===================== Fixture =====================

/// Creates a fresh CairoFunctionRunner from the shared PROGRAM.
#[fixture]
fn runner() -> CairoFunctionRunner {
    CairoFunctionRunner::new(&PROGRAM).unwrap()
}

// ===================== test_is_not_zero =====================

#[rstest]
// Case: value=0
// Expected: returns 0 (false).
#[case(Some(BigUint::zero()), 0i64)]
// Case: value=random (non-zero)
// Expected: returns 1 (true).
#[case::random(None, 1i64)]
fn test_is_not_zero(
    mut runner: CairoFunctionRunner,
    #[case] value: Option<BigUint>,
    #[case] expected_res: i64,
) {
    let value = value.unwrap_or_else(|| {
        let mut rng = thread_rng();
        rng.gen_biguint_range(&BigUint::one(), &CAIRO_PRIME)
    });

    let args = cairo_args!(value);
    runner.run_default_cairo0("is_not_zero", &args).unwrap();
    let ret = runner.get_return_values(1).unwrap();
    assert_mr_eq!(&ret[0], expected_res);
}

// ===================== test_is_le_felt =====================

#[rstest]
fn test_is_le_felt(
    mut runner: CairoFunctionRunner,
    #[values(0, 1, 2, 3, 4, 5, 6, 7, 8, 9)] idx0: usize,
    #[values(0, 1, 2, 3, 4, 5, 6, 7, 8, 9)] idx1: usize,
) {
    let value0 = &INTERESTING_FELTS[idx0];
    let value1 = &INTERESTING_FELTS[idx1];

    let expected_res = if value0 <= value1 { 1i64 } else { 0i64 };

    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");

    let args = cairo_args!(rc_base, value0, value1);
    runner.run_default_cairo0("is_le_felt", &args).unwrap();
    let ret = runner.get_return_values(2).unwrap();
    assert_mr_eq!(&ret[1], expected_res);
}

// ===================== test_is_nn =====================

#[rstest]
// Case: value=0
// Expected: returns 1 (true, 0 is non-negative).
#[case::zero(BigUint::from(0u64))]
// Case: value=1
// Expected: returns 1 (true).
#[case::one(BigUint::from(1u64))]
// Case: value=100
// Expected: returns 1 (true).
#[case::hundred(BigUint::from(100u64))]
// Case: value=RC_BOUND-1
// Expected: returns 1 (true, last valid non-negative value).
#[case::rc_minus_one(&*RC_BOUND - BigUint::one())]
// Case: value=RC_BOUND
// Expected: returns 0 (false, at boundary).
#[case::rc(RC_BOUND.clone())]
// Case: value=PRIME/2-1
// Expected: returns 0 (false, too large).
#[case::prime_half_minus_one((&*CAIRO_PRIME / BigUint::from(2u64)) - BigUint::one())]
// Case: value=PRIME/2
// Expected: returns 0 (false, too large).
#[case::prime_half(&*CAIRO_PRIME / BigUint::from(2u64))]
// Case: value=PRIME/2+1
// Expected: returns 0 (false, too large).
#[case::prime_half_plus_one((&*CAIRO_PRIME / BigUint::from(2u64)) + BigUint::one())]
// Case: value=PRIME-10
// Expected: returns 0 (false, near prime).
#[case::prime_minus_ten(&*CAIRO_PRIME - BigUint::from(10u64))]
// Case: value=PRIME-1
// Expected: returns 0 (false, near prime).
#[case::prime_minus_one(&*CAIRO_PRIME - BigUint::one())]
fn test_is_nn(mut runner: CairoFunctionRunner, #[case] value: BigUint) {
    let expected_res = if value < *RC_BOUND { 1i64 } else { 0i64 };

    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");

    let args = cairo_args!(rc_base, value);
    runner.run_default_cairo0("is_nn", &args).unwrap();
    let ret = runner.get_return_values(2).unwrap();
    assert_mr_eq!(&ret[1], expected_res);
}

// ===================== test_is_le =====================
// Tests is_le(a, b) which returns 1 if (b - a) % PRIME < RC_BOUND, else 0.

#[rstest]
// Case: pair_0 = (0, 0)
// Expected: returns 1 (0 <= 0 in modular sense).
#[case::pair_0(0)]
// Case: pair_1 = (0, 1)
// Expected: returns 1 (0 <= 1).
#[case::pair_1(1)]
// Case: pair_2 = (1, 0)
// Expected: returns 0 (1 > 0, diff wraps around).
#[case::pair_2(2)]
// Case: pair_3 = (2^200, 2^200)
// Expected: returns 1 (equal values).
#[case::pair_3(3)]
// Case: pair_4 = (2^200, 2^200 + RC_BOUND - 1)
// Expected: returns 1 (diff < RC_BOUND).
#[case::pair_4(4)]
// Case: pair_5 = (2^200, 2^200 + RC_BOUND)
// Expected: returns 0 (diff = RC_BOUND, not less than).
#[case::pair_5(5)]
fn test_is_le(mut runner: CairoFunctionRunner, #[case] pair_idx: usize) {
    let (value0, value1) = &PAIR_EXAMPLES[pair_idx];
    // diff = (value1 - value0) % PRIME
    let diff = sub_mod_prime(value0, value1);
    let expected_res = if diff < *RC_BOUND { 1i64 } else { 0i64 };

    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");

    let args = cairo_args!(rc_base, value0, value1);
    runner.run_default_cairo0("is_le", &args).unwrap();
    let ret = runner.get_return_values(2).unwrap();
    assert_mr_eq!(&ret[1], expected_res);
}

// ===================== test_is_nn_le =====================
// Tests is_nn_le(a, b) which returns 1 if 0 <= a <= b < RC_BOUND, else 0.

#[rstest]
// Case: pair_0 = (0, 0)
// Expected: returns 0 (0 is not < RC_BOUND in this context, but 0 <= 0 < RC_BOUND is true).
#[case::pair_0(0)]
// Case: pair_1 = (0, 1)
// Expected: returns 0 (1 < RC_BOUND, 0 <= 1).
#[case::pair_1(1)]
// Case: pair_2 = (1, 0)
// Expected: returns 0 (1 > 0, fails a <= b).
#[case::pair_2(2)]
// Case: pair_3 = (2^200, 2^200)
// Expected: returns 0 (2^200 >= RC_BOUND).
#[case::pair_3(3)]
// Case: pair_4 = (2^200, 2^200 + RC_BOUND - 1)
// Expected: returns 0 (values >= RC_BOUND).
#[case::pair_4(4)]
// Case: pair_5 = (2^200, 2^200 + RC_BOUND)
// Expected: returns 0 (values >= RC_BOUND).
#[case::pair_5(5)]
fn test_is_nn_le(mut runner: CairoFunctionRunner, #[case] pair_idx: usize) {
    let (value0, value1) = &PAIR_EXAMPLES[pair_idx];
    let expected_res = if value0 <= value1 && value1 < &*RC_BOUND {
        1i64
    } else {
        0i64
    };

    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");

    let args = cairo_args!(rc_base, value0, value1);
    runner.run_default_cairo0("is_nn_le", &args).unwrap();
    let ret = runner.get_return_values(2).unwrap();
    assert_mr_eq!(&ret[1], expected_res);
}

// ===================== test_is_in_range =====================
// Tests is_in_range(value, lower, upper) which returns 1 if:
//   (value - lower) % PRIME < RC_BOUND AND (upper - 1 - value) % PRIME < RC_BOUND
// This checks if value is in the range [lower, upper) in modular arithmetic.

#[rstest]
// Case: pair_0 = (0, 0) with various shifts
#[case::pair_0(0)]
// Case: pair_1 = (0, 1) with various shifts
#[case::pair_1(1)]
// Case: pair_2 = (1, 0) with various shifts
#[case::pair_2(2)]
// Case: pair_3 = (2^200, 2^200) with various shifts
#[case::pair_3(3)]
// Case: pair_4 = (2^200, 2^200 + RC_BOUND - 1) with various shifts
#[case::pair_4(4)]
// Case: pair_5 = (2^200, 2^200 + RC_BOUND) with various shifts
#[case::pair_5(5)]
fn test_is_in_range(
    mut runner: CairoFunctionRunner,
    #[case] pair_idx: usize,
    // shift values: 0, RC_BOUND, 2^200, PRIME-10
    #[values(
        BigUint::zero(),
        RC_BOUND.clone(),
        BigUint::from(2u64).pow(200),
        &*CAIRO_PRIME - BigUint::from(10u64),
    )]
    shift: BigUint,
) {
    let lower = &shift;
    let value = (&PAIR_EXAMPLES[pair_idx].0 + &shift) % &*CAIRO_PRIME;
    let upper = (&PAIR_EXAMPLES[pair_idx].1 + &shift) % &*CAIRO_PRIME;
    let value_plus_one = &value + BigUint::one();
    // Check: (value - lower) % PRIME < RC_BOUND AND (upper - (value + 1)) % PRIME < RC_BOUND
    let expected_res = if sub_mod_prime(lower, &value) < *RC_BOUND
        && sub_mod_prime(&value_plus_one, &upper) < *RC_BOUND
    {
        1i64
    } else {
        0i64
    };

    let rc_base = runner
        .get_builtin_base(BuiltinName::range_check)
        .expect("range_check builtin not found");

    let args = cairo_args!(rc_base, value, lower, upper);
    runner.run_default_cairo0("is_in_range", &args).unwrap();
    let ret = runner.get_return_values(2).unwrap();
    assert_mr_eq!(&ret[1], expected_res);
}
