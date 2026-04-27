//! Test utilities for Cairo VM result assertions.

use crate::vm::errors::{
    cairo_run_errors::CairoRunError, hint_errors::HintError, vm_errors::VirtualMachineError,
    vm_exception::VmException,
};

/// Asserts VM result is `Ok` or matches an error pattern.
#[macro_export]
macro_rules! assert_vm_result {
    ($res:expr, ok $(,)?) => {{
        match &$res {
            Ok(_) => {}
            Err(e) => panic!("Expected Ok, got Err: {:#?}", e),
        }
    }};

    ($res:expr, err $pat:pat $(,)?) => {{
        match &$res {
            Ok(v) => panic!("Expected Err, got Ok: {v:?}"),
            Err(e) => assert!(
                matches!(e, $pat),
                "Unexpected error variant.\nExpected: {}\nGot: {:#?}",
                stringify!($pat),
                e
            ),
        }
    }};

    ($res:expr, err $pat:pat if $guard:expr $(,)?) => {{
        match &$res {
            Ok(v) => panic!("Expected Err, got Ok: {v:?}"),
            Err(e) => assert!(
                matches!(e, $pat if $guard),
                "Unexpected error variant.\nExpected: {} (with guard)\nGot: {:#?}",
                stringify!($pat),
                e
            ),
        }
    }};
}

/// Type alias for check functions that validate test results.
pub type VmCheck<T> = fn(&Result<T, CairoRunError>);

/// Asserts that the result is `Ok`.
pub fn expect_ok(res: &Result<(), CairoRunError>) {
    assert_vm_result!(res, ok);
}

/// Asserts that the result is a `HintError` satisfying `predicate`.
fn expect_hint_error(res: &Result<(), CairoRunError>, predicate: impl Fn(&HintError) -> bool) {
    assert_vm_result!(
        res,
        err CairoRunError::VmException(VmException {
            inner_exc: VirtualMachineError::Hint(boxed),
            ..
        }) if predicate(&boxed.as_ref().1)
    );
}

/// Asserts that the result is `HintError::AssertNotZero`.
pub fn expect_hint_assert_not_zero(res: &Result<(), CairoRunError>) {
    expect_hint_error(res, |e| matches!(e, HintError::AssertNotZero(_)));
}

/// Asserts that the result is `HintError::AssertNotEqualFail`.
pub fn expect_assert_not_equal_fail(res: &Result<(), CairoRunError>) {
    expect_hint_error(res, |e| matches!(e, HintError::AssertNotEqualFail(_)));
}

/// Asserts that the result is `VirtualMachineError::DiffTypeComparison` wrapped in a hint.
pub fn expect_diff_type_comparison(res: &Result<(), CairoRunError>) {
    expect_hint_error(res, |e| {
        matches!(
            e,
            HintError::Internal(VirtualMachineError::DiffTypeComparison(_))
        )
    });
}

/// Asserts that the result is `VirtualMachineError::DiffIndexComp` wrapped in a hint.
pub fn expect_diff_index_comp(res: &Result<(), CairoRunError>) {
    expect_hint_error(res, |e| {
        matches!(
            e,
            HintError::Internal(VirtualMachineError::DiffIndexComp(_))
        )
    });
}

/// Asserts that the result is `HintError::ValueOutside250BitRange`.
pub fn expect_hint_value_outside_250_bit_range(res: &Result<(), CairoRunError>) {
    expect_hint_error(res, |e| matches!(e, HintError::ValueOutside250BitRange(_)));
}

/// Asserts that the result is `HintError::NonLeFelt252`.
pub fn expect_non_le_felt252(res: &Result<(), CairoRunError>) {
    expect_hint_error(res, |e| matches!(e, HintError::NonLeFelt252(_)));
}

/// Asserts that the result is `HintError::AssertLtFelt252`.
pub fn expect_assert_lt_felt252(res: &Result<(), CairoRunError>) {
    expect_hint_error(res, |e| matches!(e, HintError::AssertLtFelt252(_)));
}

/// Asserts that the result is `HintError::ValueOutsideValidRange`.
pub fn expect_hint_value_outside_valid_range(res: &Result<(), CairoRunError>) {
    expect_hint_error(res, |e| matches!(e, HintError::ValueOutsideValidRange(_)));
}

/// Asserts that the result is `HintError::OutOfValidRange`.
pub fn expect_hint_out_of_valid_range(res: &Result<(), CairoRunError>) {
    expect_hint_error(res, |e| matches!(e, HintError::OutOfValidRange(_)));
}

/// Asserts that the result is `HintError::SplitIntNotZero`.
pub fn expect_split_int_not_zero(res: &Result<(), CairoRunError>) {
    expect_hint_error(res, |e| matches!(e, HintError::SplitIntNotZero));
}

/// Asserts that the result is `HintError::SplitIntLimbOutOfRange`.
pub fn expect_split_int_limb_out_of_range(res: &Result<(), CairoRunError>) {
    expect_hint_error(res, |e| matches!(e, HintError::SplitIntLimbOutOfRange(_)));
}

#[cfg(test)]
mod tests {
    use crate::{
        types::relocatable::{MaybeRelocatable, Relocatable},
        vm::errors::{
            cairo_run_errors::CairoRunError, hint_errors::HintError,
            vm_errors::VirtualMachineError, vm_exception::VmException,
        },
    };
    use rstest::rstest;

    use super::*;

    /// Wraps a `HintError` in the full `CairoRunError::VmException` chain expected by the checkers.
    #[allow(clippy::result_large_err)]
    fn hint_err(hint_error: HintError) -> Result<(), CairoRunError> {
        Err(CairoRunError::VmException(VmException {
            pc: Relocatable::default(),
            inst_location: None,
            inner_exc: VirtualMachineError::Hint(Box::new((0, hint_error))),
            error_attr_value: None,
            traceback: None,
        }))
    }

    /// `assert_vm_result!(ok)` does not panic on `Ok`.
    #[test]
    fn assert_vm_result_ok_passes() {
        assert_vm_result!(Ok::<(), i32>(()), ok);
    }

    /// `assert_vm_result!(err pat)` does not panic when the error matches the pattern.
    #[test]
    fn assert_vm_result_err_passes() {
        assert_vm_result!(Err::<(), i32>(42), err 42);
    }

    /// `assert_vm_result!(err pat if guard)` does not panic when both pattern and guard match.
    #[test]
    fn assert_vm_result_err_with_guard_passes() {
        assert_vm_result!(Err::<(), i32>(42), err x if *x == 42);
    }

    /// `expect_ok` does not panic on `Ok(())`.
    #[test]
    fn expect_ok_passes() {
        expect_ok(&Ok(()));
    }

    // --- happy path: each checker passes on its correct error variant ---

    #[rstest]
    #[case::hint_assert_not_zero(
        expect_hint_assert_not_zero,
        hint_err(HintError::AssertNotZero(Box::default()))
    )]
    #[case::assert_not_equal_fail(
        expect_assert_not_equal_fail,
        hint_err(HintError::AssertNotEqualFail(Box::new((
            MaybeRelocatable::from(0),
            MaybeRelocatable::from(0),
        ))))
    )]
    #[case::diff_type_comparison(
        expect_diff_type_comparison,
        hint_err(HintError::Internal(VirtualMachineError::DiffTypeComparison(Box::new((
            MaybeRelocatable::from(0),
            MaybeRelocatable::from((0, 0)),
        )))))
    )]
    #[case::diff_index_comp(
        expect_diff_index_comp,
        hint_err(HintError::Internal(VirtualMachineError::DiffIndexComp(Box::default())))
    )]
    #[case::hint_value_outside_250_bit_range(
        expect_hint_value_outside_250_bit_range,
        hint_err(HintError::ValueOutside250BitRange(Box::default()))
    )]
    #[case::non_le_felt252(
        expect_non_le_felt252,
        hint_err(HintError::NonLeFelt252(Box::default()))
    )]
    #[case::assert_lt_felt252(
        expect_assert_lt_felt252,
        hint_err(HintError::AssertLtFelt252(Box::default()))
    )]
    #[case::hint_value_outside_valid_range(
        expect_hint_value_outside_valid_range,
        hint_err(HintError::ValueOutsideValidRange(Box::default()))
    )]
    #[case::hint_out_of_valid_range(
        expect_hint_out_of_valid_range,
        hint_err(HintError::OutOfValidRange(Box::default()))
    )]
    #[case::split_int_not_zero(expect_split_int_not_zero, hint_err(HintError::SplitIntNotZero))]
    #[case::split_int_limb_out_of_range(
        expect_split_int_limb_out_of_range,
        hint_err(HintError::SplitIntLimbOutOfRange(Box::default()))
    )]
    fn checker_passes_on_correct_variant(
        #[case] checker: VmCheck<()>,
        #[case] res: Result<(), CairoRunError>,
    ) {
        checker(&res);
    }

    // --- unhappy path: macro edge cases ---

    /// `assert_vm_result!(ok)` panics when given `Err`.
    #[test]
    #[should_panic(expected = "Expected Ok, got Err")]
    fn assert_vm_result_ok_panics_on_err() {
        assert_vm_result!(Err::<(), i32>(42), ok);
    }

    /// `assert_vm_result!(err pat)` panics when given `Ok`.
    #[test]
    #[should_panic(expected = "Expected Err, got Ok")]
    fn assert_vm_result_err_panics_on_ok() {
        assert_vm_result!(Ok::<(), i32>(()), err 42);
    }

    /// `assert_vm_result!(err pat)` panics when the error doesn't match the pattern.
    #[test]
    #[should_panic(expected = "Unexpected error variant")]
    fn assert_vm_result_err_panics_on_wrong_variant() {
        assert_vm_result!(Err::<(), i32>(1), err 42);
    }

    /// `assert_vm_result!(err pat if guard)` panics when the guard fails.
    #[test]
    #[should_panic(expected = "Unexpected error variant")]
    fn assert_vm_result_err_with_guard_panics_on_failed_guard() {
        assert_vm_result!(Err::<(), i32>(42), err x if *x == 0);
    }

    /// `expect_ok` panics when given an `Err`.
    #[test]
    #[should_panic(expected = "Expected Ok, got Err")]
    fn expect_ok_panics_on_err() {
        expect_ok(&hint_err(HintError::Dummy));
    }

    // --- unhappy path: each checker panics on a wrong error variant ---

    #[rstest]
    #[case::hint_assert_not_zero(expect_hint_assert_not_zero)]
    #[case::assert_not_equal_fail(expect_assert_not_equal_fail)]
    #[case::diff_type_comparison(expect_diff_type_comparison)]
    #[case::diff_index_comp(expect_diff_index_comp)]
    #[case::hint_value_outside_250_bit_range(expect_hint_value_outside_250_bit_range)]
    #[case::non_le_felt252(expect_non_le_felt252)]
    #[case::assert_lt_felt252(expect_assert_lt_felt252)]
    #[case::hint_value_outside_valid_range(expect_hint_value_outside_valid_range)]
    #[case::hint_out_of_valid_range(expect_hint_out_of_valid_range)]
    #[case::split_int_not_zero(expect_split_int_not_zero)]
    #[case::split_int_limb_out_of_range(expect_split_int_limb_out_of_range)]
    #[should_panic(expected = "Unexpected error variant")]
    fn hint_checker_panics_on_dummy_hint_error(#[case] checker: VmCheck<()>) {
        checker(&hint_err(HintError::Dummy));
    }
}
