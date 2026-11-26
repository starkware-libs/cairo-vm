use std::collections::HashMap;

use crate::Felt252;
use crate::{
    hint_processor::{
        builtin_hint_processor::hint_utils::{
            get_integer_from_var_name, get_ptr_from_var_name, insert_value_from_var_name,
        },
        hint_processor_definition::HintReference,
    },
    serde::deserialize_program::ApTracking,
    types::errors::math_errors::MathError,
    vm::{errors::hint_errors::HintError, vm_core::VirtualMachine},
};
use num_traits::{ToPrimitive, Zero};

pub fn set_add(
    vm: &mut VirtualMachine,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), HintError> {
    let set_ptr = get_ptr_from_var_name("set_ptr", vm, ids_data, ap_tracking)?;
    let elm_size =
        get_integer_from_var_name("elm_size", vm, ids_data, ap_tracking).and_then(|x| {
            x.to_usize()
                .ok_or_else(|| MathError::Felt252ToUsizeConversion(Box::new(x)).into())
        })?;
    let elm_ptr = get_ptr_from_var_name("elm_ptr", vm, ids_data, ap_tracking)?;
    let set_end_ptr = get_ptr_from_var_name("set_end_ptr", vm, ids_data, ap_tracking)?;

    if elm_size.is_zero() {
        Err(HintError::AssertionFailed(
            "assert ids.elm_size > 0".to_string().into_boxed_str(),
        ))?;
    }
    if set_ptr > set_end_ptr {
        return Err(HintError::InvalidSetRange(Box::new((set_ptr, set_end_ptr))));
    }

    let range_limit = (set_end_ptr - set_ptr)?;

    for i in 0..range_limit {
        if vm.mem_eq(elm_ptr, (set_ptr + elm_size * i)?, elm_size) {
            insert_value_from_var_name("index", Felt252::from(i), vm, ids_data, ap_tracking)?;
            return insert_value_from_var_name(
                "is_elm_in_set",
                Felt252::ONE,
                vm,
                ids_data,
                ap_tracking,
            );
        }
    }
    insert_value_from_var_name("is_elm_in_set", Felt252::ZERO, vm, ids_data, ap_tracking)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        any_box,
        hint_processor::{
            builtin_hint_processor::builtin_hint_processor_definition::{
                BuiltinHintProcessor, HintProcessorData,
            },
            hint_processor_definition::HintProcessorLogic,
        },
        types::relocatable::MaybeRelocatable,
        utils::test_utils::*,
        vm::vm_core::VirtualMachine,
    };
    use assert_matches::assert_matches;

    const HINT_CODE: &str = "assert ids.elm_size > 0\nassert ids.set_ptr <= ids.set_end_ptr\nelm_list = memory.get_range(ids.elm_ptr, ids.elm_size)\nfor i in range(0, ids.set_end_ptr - ids.set_ptr, ids.elm_size):\n    if memory.get_range(ids.set_ptr + i, ids.elm_size) == elm_list:\n        ids.index = i // ids.elm_size\n        ids.is_elm_in_set = 1\n        break\nelse:\n    ids.is_elm_in_set = 0";

    fn init_vm_ids_data(
        set_ptr: Option<(isize, usize)>,
        elm_size: Option<i32>,
        elm_a: Option<isize>,
        elm_b: Option<usize>,
    ) -> (VirtualMachine, HashMap<String, HintReference>) {
        let mut vm = vm_with_range_check!();

        vm.run_context.fp = 6;

        let set_ptr = set_ptr.unwrap_or((2, 0));
        let elm_size = elm_size.unwrap_or(2);
        let elm_a = elm_a.unwrap_or(2);
        let elm_b = elm_b.unwrap_or(3);

        vm.segments = segments![
            ((1, 2), (set_ptr.0, set_ptr.1)),
            ((1, 3), elm_size),
            ((1, 4), (3, 0)),
            ((1, 5), (2, 2)),
            ((2, 0), 1),
            ((2, 1), 3),
            ((2, 2), 5),
            ((2, 3), 7),
            ((3, 0), elm_a),
            ((3, 1), elm_b)
        ];
        let ids_data = ids_data![
            "is_elm_in_set",
            "index",
            "set_ptr",
            "elm_size",
            "elm_ptr",
            "set_end_ptr"
        ];

        (vm, ids_data)
    }

    #[test]
    fn set_add_new_elem() {
        let (mut vm, ids_data) = init_vm_ids_data(None, None, None, None);
        assert_matches!(run_hint!(vm, ids_data, HINT_CODE), Ok(()));
        assert_eq!(
            vm.segments
                .memory
                .get(&MaybeRelocatable::from((1, 0)))
                .unwrap(),
            MaybeRelocatable::Int(Felt252::ZERO)
        )
    }

    #[test]
    fn set_add_already_exists() {
        let (mut vm, ids_data) = init_vm_ids_data(None, None, Some(1), Some(3));
        assert_matches!(run_hint!(vm, ids_data, HINT_CODE), Ok(()));
        check_memory![vm.segments.memory, ((1, 0), 1), ((1, 1), 0)];
    }

    #[test]
    fn elm_size_negative() {
        let (mut vm, ids_data) = init_vm_ids_data(None, Some(-2), None, None);
        assert_matches!(
            run_hint!(vm, ids_data, HINT_CODE),
            Err(HintError::Math(MathError::Felt252ToUsizeConversion(_)))
        );
    }

    #[test]
    fn elm_size_zero() {
        let (mut vm, ids_data) = init_vm_ids_data(None, Some(0), None, None);
        assert_matches!(
            run_hint!(vm, ids_data, HINT_CODE),
            Err(HintError::AssertionFailed(
                bx
            )) if bx.as_ref() == "assert ids.elm_size > 0"
        );
    }
    #[test]
    fn set_ptr_gt_set_end_ptr() {
        let (mut vm, ids_data) = init_vm_ids_data(Some((2, 3)), None, None, None);
        assert_matches!(
            run_hint!(vm, ids_data, HINT_CODE),
            Err(HintError::InvalidSetRange(bx)) if *bx == ((2, 3).into(), (2, 2).into())
        );
    }
}
