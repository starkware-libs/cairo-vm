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
