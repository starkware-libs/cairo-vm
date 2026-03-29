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
pub type VmCheck<T> = fn(&std::result::Result<T, CairoRunError>);

/// Asserts that the result is `Ok`.
pub fn expect_ok(res: &std::result::Result<(), CairoRunError>) {
    assert_vm_result!(res, ok);
}

/// Asserts that the result is `HintError::AssertNotZero`.
pub fn expect_hint_assert_not_zero(res: &std::result::Result<(), CairoRunError>) {
    assert_vm_result!(
        res,
        err CairoRunError::VmException(VmException {
            inner_exc: VirtualMachineError::Hint(boxed),
            ..
        }) if matches!(boxed.as_ref(), (_, HintError::AssertNotZero(_)))
    );
}

/// Asserts that the result is `HintError::AssertNotEqualFail`.
pub fn expect_assert_not_equal_fail(res: &std::result::Result<(), CairoRunError>) {
    assert_vm_result!(
        res,
        err CairoRunError::VmException(VmException {
            inner_exc: VirtualMachineError::Hint(boxed),
            ..
        }) if matches!(boxed.as_ref(), (_, HintError::AssertNotEqualFail(_)))
    );
}

/// Asserts that the result is `HintError::Internal(VirtualMachineError::DiffTypeComparison)`.
pub fn expect_diff_type_comparison(res: &std::result::Result<(), CairoRunError>) {
    assert_vm_result!(
        res,
        err CairoRunError::VmException(VmException {
            inner_exc: VirtualMachineError::Hint(boxed),
            ..
        }) if matches!(boxed.as_ref(), (_, HintError::Internal(VirtualMachineError::DiffTypeComparison(_))))
    );
}

/// Asserts that the result is `HintError::Internal(VirtualMachineError::DiffIndexComp)`.
pub fn expect_diff_index_comp(res: &std::result::Result<(), CairoRunError>) {
    assert_vm_result!(
        res,
        err CairoRunError::VmException(VmException {
            inner_exc: VirtualMachineError::Hint(boxed),
            ..
        }) if matches!(boxed.as_ref(), (_, HintError::Internal(VirtualMachineError::DiffIndexComp(_))))
    );
}

/// Asserts that the result is `HintError::ValueOutside250BitRange`.
pub fn expect_hint_value_outside_250_bit_range(res: &std::result::Result<(), CairoRunError>) {
    assert_vm_result!(
        res,
        err CairoRunError::VmException(VmException {
            inner_exc: VirtualMachineError::Hint(boxed),
            ..
        }) if matches!(boxed.as_ref(), (_, HintError::ValueOutside250BitRange(_)))
    );
}

/// Asserts that the result is `HintError::NonLeFelt252`.
pub fn expect_non_le_felt252(res: &std::result::Result<(), CairoRunError>) {
    assert_vm_result!(
        res,
        err CairoRunError::VmException(VmException {
            inner_exc: VirtualMachineError::Hint(boxed),
            ..
        }) if matches!(boxed.as_ref(), (_, HintError::NonLeFelt252(_)))
    );
}

/// Asserts that the result is `HintError::AssertLtFelt252`.
pub fn expect_assert_lt_felt252(res: &std::result::Result<(), CairoRunError>) {
    assert_vm_result!(
        res,
        err CairoRunError::VmException(VmException {
            inner_exc: VirtualMachineError::Hint(boxed),
            ..
        }) if matches!(boxed.as_ref(), (_, HintError::AssertLtFelt252(_)))
    );
}

/// Asserts that the result is `HintError::ValueOutsideValidRange`.
pub fn expect_hint_value_outside_valid_range(res: &std::result::Result<(), CairoRunError>) {
    assert_vm_result!(
        res,
        err CairoRunError::VmException(VmException {
            inner_exc: VirtualMachineError::Hint(boxed),
            ..
        }) if matches!(boxed.as_ref(), (_, HintError::ValueOutsideValidRange(_)))
    );
}

/// Asserts that the result is `HintError::OutOfValidRange`.
pub fn expect_hint_out_of_valid_range(res: &std::result::Result<(), CairoRunError>) {
    assert_vm_result!(
        res,
        err CairoRunError::VmException(VmException {
            inner_exc: VirtualMachineError::Hint(boxed),
            ..
        }) if matches!(boxed.as_ref(), (_, HintError::OutOfValidRange(_)))
    );
}

/// Asserts that the result is `HintError::SplitIntNotZero`.
pub fn expect_split_int_not_zero(res: &std::result::Result<(), CairoRunError>) {
    assert_vm_result!(
        res,
        err CairoRunError::VmException(VmException {
            inner_exc: VirtualMachineError::Hint(boxed),
            ..
        }) if matches!(boxed.as_ref(), (_, HintError::SplitIntNotZero))
    );
}

/// Asserts that the result is `HintError::SplitIntLimbOutOfRange`.
pub fn expect_split_int_limb_out_of_range(res: &std::result::Result<(), CairoRunError>) {
    assert_vm_result!(
        res,
        err CairoRunError::VmException(VmException {
            inner_exc: VirtualMachineError::Hint(boxed),
            ..
        }) if matches!(boxed.as_ref(), (_, HintError::SplitIntLimbOutOfRange(_)))
    );
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

    use super::*;

    /// Wraps a `HintError` in the full `CairoRunError::VmException` chain expected by the checkers.
    fn hint_err(hint_error: HintError) -> std::result::Result<(), CairoRunError> {
        Err(CairoRunError::VmException(VmException {
            pc: Relocatable::from((0, 0)),
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

    /// `expect_hint_assert_not_zero` does not panic on `HintError::AssertNotZero`.
    #[test]
    fn expect_hint_assert_not_zero_passes() {
        let res = hint_err(HintError::AssertNotZero(Box::default()));
        expect_hint_assert_not_zero(&res);
    }

    /// `expect_assert_not_equal_fail` does not panic on `HintError::AssertNotEqualFail`.
    #[test]
    fn expect_assert_not_equal_fail_passes() {
        let res = hint_err(HintError::AssertNotEqualFail(Box::new((
            MaybeRelocatable::from(0),
            MaybeRelocatable::from(0),
        ))));
        expect_assert_not_equal_fail(&res);
    }

    /// `expect_diff_type_comparison` does not panic on `VirtualMachineError::DiffTypeComparison`.
    #[test]
    fn expect_diff_type_comparison_passes() {
        let res = hint_err(HintError::Internal(VirtualMachineError::DiffTypeComparison(
            Box::new((MaybeRelocatable::from(0), MaybeRelocatable::from((0, 0)))),
        )));
        expect_diff_type_comparison(&res);
    }

    /// `expect_diff_index_comp` does not panic on `VirtualMachineError::DiffIndexComp`.
    #[test]
    fn expect_diff_index_comp_passes() {
        let res = hint_err(HintError::Internal(VirtualMachineError::DiffIndexComp(
            Box::new((Relocatable::from((0, 0)), Relocatable::from((1, 0)))),
        )));
        expect_diff_index_comp(&res);
    }

    /// `expect_hint_value_outside_250_bit_range` does not panic on `HintError::ValueOutside250BitRange`.
    #[test]
    fn expect_hint_value_outside_250_bit_range_passes() {
        let res = hint_err(HintError::ValueOutside250BitRange(Box::default()));
        expect_hint_value_outside_250_bit_range(&res);
    }

    /// `expect_non_le_felt252` does not panic on `HintError::NonLeFelt252`.
    #[test]
    fn expect_non_le_felt252_passes() {
        let res = hint_err(HintError::NonLeFelt252(Box::default()));
        expect_non_le_felt252(&res);
    }

    /// `expect_assert_lt_felt252` does not panic on `HintError::AssertLtFelt252`.
    #[test]
    fn expect_assert_lt_felt252_passes() {
        let res = hint_err(HintError::AssertLtFelt252(Box::default()));
        expect_assert_lt_felt252(&res);
    }

    /// `expect_hint_value_outside_valid_range` does not panic on `HintError::ValueOutsideValidRange`.
    #[test]
    fn expect_hint_value_outside_valid_range_passes() {
        let res = hint_err(HintError::ValueOutsideValidRange(Box::default()));
        expect_hint_value_outside_valid_range(&res);
    }

    /// `expect_hint_out_of_valid_range` does not panic on `HintError::OutOfValidRange`.
    #[test]
    fn expect_hint_out_of_valid_range_passes() {
        let res = hint_err(HintError::OutOfValidRange(Box::default()));
        expect_hint_out_of_valid_range(&res);
    }

    /// `expect_split_int_not_zero` does not panic on `HintError::SplitIntNotZero`.
    #[test]
    fn expect_split_int_not_zero_passes() {
        let res = hint_err(HintError::SplitIntNotZero);
        expect_split_int_not_zero(&res);
    }

    /// `expect_split_int_limb_out_of_range` does not panic on `HintError::SplitIntLimbOutOfRange`.
    #[test]
    fn expect_split_int_limb_out_of_range_passes() {
        let res = hint_err(HintError::SplitIntLimbOutOfRange(Box::default()));
        expect_split_int_limb_out_of_range(&res);
    }

    // --- unhappy path: wrong error variant should panic ---

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
        expect_ok(&hint_err(HintError::SplitIntNotZero));
    }

    /// Each `expect_*` checker panics when given a different error variant.
    #[test]
    #[should_panic(expected = "Unexpected error variant")]
    fn expect_hint_assert_not_zero_panics_on_wrong_variant() {
        expect_hint_assert_not_zero(&hint_err(HintError::SplitIntNotZero));
    }

    /// `expect_assert_not_equal_fail` panics when given a different error variant.
    #[test]
    #[should_panic(expected = "Unexpected error variant")]
    fn expect_assert_not_equal_fail_panics_on_wrong_variant() {
        expect_assert_not_equal_fail(&hint_err(HintError::SplitIntNotZero));
    }

    /// `expect_diff_type_comparison` panics when given a different error variant.
    #[test]
    #[should_panic(expected = "Unexpected error variant")]
    fn expect_diff_type_comparison_panics_on_wrong_variant() {
        expect_diff_type_comparison(&hint_err(HintError::SplitIntNotZero));
    }

    /// `expect_diff_index_comp` panics when given a different error variant.
    #[test]
    #[should_panic(expected = "Unexpected error variant")]
    fn expect_diff_index_comp_panics_on_wrong_variant() {
        expect_diff_index_comp(&hint_err(HintError::SplitIntNotZero));
    }

    /// `expect_hint_value_outside_250_bit_range` panics when given a different error variant.
    #[test]
    #[should_panic(expected = "Unexpected error variant")]
    fn expect_hint_value_outside_250_bit_range_panics_on_wrong_variant() {
        expect_hint_value_outside_250_bit_range(&hint_err(HintError::SplitIntNotZero));
    }

    /// `expect_non_le_felt252` panics when given a different error variant.
    #[test]
    #[should_panic(expected = "Unexpected error variant")]
    fn expect_non_le_felt252_panics_on_wrong_variant() {
        expect_non_le_felt252(&hint_err(HintError::SplitIntNotZero));
    }

    /// `expect_assert_lt_felt252` panics when given a different error variant.
    #[test]
    #[should_panic(expected = "Unexpected error variant")]
    fn expect_assert_lt_felt252_panics_on_wrong_variant() {
        expect_assert_lt_felt252(&hint_err(HintError::SplitIntNotZero));
    }

    /// `expect_hint_value_outside_valid_range` panics when given a different error variant.
    #[test]
    #[should_panic(expected = "Unexpected error variant")]
    fn expect_hint_value_outside_valid_range_panics_on_wrong_variant() {
        expect_hint_value_outside_valid_range(&hint_err(HintError::SplitIntNotZero));
    }

    /// `expect_hint_out_of_valid_range` panics when given a different error variant.
    #[test]
    #[should_panic(expected = "Unexpected error variant")]
    fn expect_hint_out_of_valid_range_panics_on_wrong_variant() {
        expect_hint_out_of_valid_range(&hint_err(HintError::SplitIntNotZero));
    }

    /// `expect_split_int_not_zero` panics when given a different error variant.
    #[test]
    #[should_panic(expected = "Unexpected error variant")]
    fn expect_split_int_not_zero_panics_on_wrong_variant() {
        expect_split_int_not_zero(&hint_err(HintError::SplitIntLimbOutOfRange(Box::default())));
    }

    /// `expect_split_int_limb_out_of_range` panics when given a different error variant.
    #[test]
    #[should_panic(expected = "Unexpected error variant")]
    fn expect_split_int_limb_out_of_range_panics_on_wrong_variant() {
        expect_split_int_limb_out_of_range(&hint_err(HintError::SplitIntNotZero));
    }
}
