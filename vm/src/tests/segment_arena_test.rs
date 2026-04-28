// This test mirrors the test on cairo-lang for segment_arena
// https://github.com/starkware-libs/cairo-lang/blob/master/src/starkware/starknet/builtins/segment_arena/segment_arena_test.py
use crate::types::layout_name::LayoutName;
use std::{borrow::Cow, collections::HashMap, rc::Rc};

use crate::any_box;
use crate::cairo_run::{cairo_run, Cairo0RunConfig};
use crate::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor;
use crate::hint_processor::builtin_hint_processor::hint_utils::insert_value_into_ap;
use crate::hint_processor::hint_processor_definition::HintReference;
use crate::hint_processor::{
    builtin_hint_processor::{
        builtin_hint_processor_definition::HintFunc,
        hint_utils::{get_integer_from_var_name, get_ptr_from_var_name},
    },
    hint_processor_utils::felt_to_usize,
};
use crate::serde::deserialize_program::ApTracking;
use crate::types::exec_scope::ExecutionScopes;
use crate::types::relocatable::MaybeRelocatable;
use crate::vm::errors::hint_errors::HintError;
use crate::vm::vm_core::VirtualMachine;
use crate::Felt252;
use indoc::indoc;

fn get_variable_from_root_exec_scope<T>(
    exec_scopes: &ExecutionScopes,
    name: &str,
) -> Result<T, HintError>
where
    T: Clone + 'static,
{
    exec_scopes.data[0]
        .get(name)
        .and_then(|var| var.downcast_ref::<T>().cloned())
        .ok_or(HintError::VariableNotInScopeError(
            name.to_string().into_boxed_str(),
        ))
}

const SEGMENTS_ADD: &str = "memory[ap] = to_felt_or_relocatable(segments.add())";

fn segments_add(
    vm: &mut VirtualMachine,
    _exec_scopes: &mut ExecutionScopes,
    _ids_data: &HashMap<String, HintReference>,
    _ap_tracking: &ApTracking,
    _constants: &HashMap<String, Felt252>,
) -> Result<(), HintError> {
    let segment = vm.add_memory_segment();
    insert_value_into_ap(vm, segment)
}

const SETUP_SEGMENT_INDEX: &str = indoc! {r#"if 'segment_index_to_arena_index' not in globals():
            # A map from the relocatable value segment index to the index in the arena.
            segment_index_to_arena_index = {}

        # The segment is placed at the end of the arena.
        index = ids.n_segments

        # Create a segment or a temporary segment.
        start = segments.add_temp_segment() if index > 0 else segments.add()

        # Update 'SegmentInfo::start' and 'segment_index_to_arena_index'.
        ids.prev_segment_arena.infos[index].start = start
        segment_index_to_arena_index[start.segment_index] = index"#};

fn setup_segment_index(
    vm: &mut VirtualMachine,
    exec_scopes: &mut ExecutionScopes,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
    _constants: &HashMap<String, Felt252>,
) -> Result<(), HintError> {
    let mut segment_index_to_arena_index: HashMap<isize, Felt252> =
        get_variable_from_root_exec_scope(exec_scopes, "segment_index_to_arena_index")
            .unwrap_or_default();
    let n_segments = get_integer_from_var_name("n_segments", vm, ids_data, ap_tracking)?;
    let start = if n_segments > Felt252::ZERO {
        vm.add_temporary_segment()
    } else {
        vm.add_memory_segment()
    };

    let prev_segment_arena_ptr =
        get_ptr_from_var_name("prev_segment_arena", vm, ids_data, ap_tracking)?;

    let infos_ptr = vm.get_relocatable(prev_segment_arena_ptr).unwrap();

    vm.insert_value((infos_ptr + (felt_to_usize(&n_segments)? * 3))?, start)?;

    exec_scopes.insert_value("index", n_segments);
    exec_scopes.insert_value("start", start);

    segment_index_to_arena_index.insert(start.segment_index, n_segments);

    exec_scopes.data[0].insert(
        "segment_index_to_arena_index".to_string(),
        any_box!(segment_index_to_arena_index),
    );

    Ok(())
}

const SET_SEGMENT_TO_ARENA_INDEX: &str = "memory[ap] = to_felt_or_relocatable(segment_index_to_arena_index[ids.segment_end.segment_index])";

fn set_segment_to_arena_index(
    vm: &mut VirtualMachine,
    exec_scopes: &mut ExecutionScopes,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
    _constants: &HashMap<String, Felt252>,
) -> Result<(), HintError> {
    let segment_index_to_arena_index: HashMap<isize, Felt252> =
        get_variable_from_root_exec_scope(exec_scopes, "segment_index_to_arena_index")?;

    let segment_end = get_ptr_from_var_name("segment_end", vm, ids_data, ap_tracking)?;

    let index = segment_index_to_arena_index[&segment_end.segment_index];
    insert_value_into_ap(vm, index)?;
    Ok(())
}

#[test]
fn test_segment_arena() {
    let program_data =
        include_bytes!("../../../cairo_programs/segment_arena/test_segment_arena.json");

    let mut extra_hints = HashMap::new();
    let run_resources = Default::default();

    extra_hints.insert(
        SEGMENTS_ADD.to_owned(),
        Rc::new(HintFunc(Box::new(segments_add))),
    );
    extra_hints.insert(
        SETUP_SEGMENT_INDEX.to_owned(),
        Rc::new(HintFunc(Box::new(setup_segment_index))),
    );

    extra_hints.insert(
        SET_SEGMENT_TO_ARENA_INDEX.to_owned(),
        Rc::new(HintFunc(Box::new(set_segment_to_arena_index))),
    );

    let mut hint_executor = BuiltinHintProcessor::new(extra_hints, run_resources);
    let cairo_run_config = Cairo0RunConfig {
        entrypoint: "test_segment_arena",
        layout: LayoutName::all_cairo,
        relocate_mem: true,
        trace_enabled: true,
        proof_mode: false,
        fill_holes: false,
        ..Default::default()
    };
    let runner =
        cairo_run(program_data, &cairo_run_config, &mut hint_executor).expect("Execution failed");
    let return_values = runner.vm.get_return_values(2).unwrap();
    let concat_segments = return_values.first().unwrap().get_relocatable().unwrap();
    let infos = return_values.get(1).unwrap();

    let concat_segments_data = runner.vm.get_range(concat_segments, 10);

    let expected_concat_segment_data: Vec<Option<_>> = vec![
        Some(1),
        Some(2),
        None,
        Some(3),
        Some(4),
        None,
        Some(5),
        None,
        Some(6),
        Some(7),
    ]
    .into_iter()
    .map(|val| val.map(|int| Cow::Owned(MaybeRelocatable::Int(Felt252::from(int)))))
    .collect();

    assert_eq!(concat_segments_data, expected_concat_segment_data);

    let infos_data = runner.vm.get_range(infos.get_relocatable().unwrap(), 12);

    let expected_infos_data: Vec<_> = [
        // segment0.
        MaybeRelocatable::RelocatableValue(concat_segments),
        MaybeRelocatable::RelocatableValue((concat_segments + 2_usize).unwrap()),
        MaybeRelocatable::Int(Felt252::from(0)),
        // segment1.
        MaybeRelocatable::RelocatableValue((concat_segments + 3_usize).unwrap()),
        MaybeRelocatable::RelocatableValue((concat_segments + 5_usize).unwrap()),
        MaybeRelocatable::Int(Felt252::from(1)),
        // segment2.
        MaybeRelocatable::RelocatableValue((concat_segments + 6_usize).unwrap()),
        MaybeRelocatable::RelocatableValue((concat_segments + 7_usize).unwrap()),
        MaybeRelocatable::Int(Felt252::from(3)),
        // segment3.
        MaybeRelocatable::RelocatableValue((concat_segments + 8_usize).unwrap()),
        MaybeRelocatable::RelocatableValue((concat_segments + 10_usize).unwrap()),
        MaybeRelocatable::Int(Felt252::from(2)),
    ]
    .into_iter()
    .map(|val| Some(Cow::Owned::<MaybeRelocatable>(val)))
    .collect();

    assert_eq!(infos_data, expected_infos_data);
}
