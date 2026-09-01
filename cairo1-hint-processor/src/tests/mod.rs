use cairo_lang_starknet_classes::casm_contract_class::CasmContractClass;
use cairo_vm::cairo_run::Cairo0RunConfig;
use cairo_vm::types::builtin_name::BuiltinName;
use cairo_vm::types::layout_name::LayoutName;
use cairo_vm::types::relocatable::MaybeRelocatable;
use cairo_vm::vm::errors::cairo_run_errors::CairoRunError;
use cairo_vm::vm::runners::cairo_runner::{CairoArg, CairoRunner, RunResources};
use cairo_vm::vm::runners::function_runner::EntryPoint;
use cairo_vm::Felt252;

use crate::hint_processor::Cairo1HintProcessor;
use crate::program_from_casm_contract_class;

mod run_from_entrypoint_tests;

// Runs a contract entrypoint with given arguments and checks its return values
// Doesn't use a syscall_handler
fn run_cairo_1_entrypoint(
    program_content: &[u8],
    entrypoint_offset: usize,
    args: &[MaybeRelocatable],
    expected_retdata: &[Felt252],
) {
    let contract_class: CasmContractClass = serde_json::from_slice(program_content).unwrap();
    let mut hint_processor =
        Cairo1HintProcessor::new(&contract_class.hints, RunResources::default(), false);

    let mut runner = CairoRunner::new(
        &program_from_casm_contract_class(&contract_class).unwrap(),
        &Cairo0RunConfig {
            layout: LayoutName::all_cairo,
            proof_mode: false,
            trace_enabled: false,
            disable_trace_padding: false,
            ..Default::default()
        }
        .run_config()
        .unwrap(),
    );

    let program_builtins = get_casm_contract_builtins(&contract_class, entrypoint_offset);
    runner
        .initialize_function_runner_cairo_1(&program_builtins)
        .unwrap();

    // Implicit Args
    let syscall_segment = MaybeRelocatable::from(runner.vm.add_memory_segment());

    let builtins = runner.get_program_builtins();

    let builtin_segment: Vec<MaybeRelocatable> = runner
        .vm
        .get_builtin_runners()
        .iter()
        .filter(|b| builtins.contains(&b.name()))
        .flat_map(|b| b.initial_stack())
        .collect();

    let initial_gas = MaybeRelocatable::from(usize::MAX);

    let mut implicit_args = builtin_segment;
    implicit_args.extend([initial_gas]);
    implicit_args.extend([syscall_segment]);

    // Other args

    // Load builtin costs
    let builtin_costs: Vec<MaybeRelocatable> =
        vec![0.into(), 0.into(), 0.into(), 0.into(), 0.into()];
    let builtin_costs_ptr = runner.vm.add_memory_segment();
    runner
        .vm
        .load_data(builtin_costs_ptr, &builtin_costs)
        .unwrap();

    // Load extra data
    let core_program_end_ptr =
        (runner.program_base.unwrap() + runner.get_program().data_len()).unwrap();
    let program_extra_data: Vec<MaybeRelocatable> =
        vec![0x208B7FFF7FFF7FFE_u64.into(), builtin_costs_ptr.into()];
    runner
        .vm
        .load_data(core_program_end_ptr, &program_extra_data)
        .unwrap();

    // Load calldata
    let calldata_start = runner.vm.add_memory_segment();
    let calldata_end = runner.vm.load_data(calldata_start, args).unwrap();

    // Create entrypoint_args

    let mut entrypoint_args: Vec<CairoArg> = implicit_args
        .iter()
        .map(|m| CairoArg::from(m.clone()))
        .collect();
    entrypoint_args.extend([
        MaybeRelocatable::from(calldata_start).into(),
        MaybeRelocatable::from(calldata_end).into(),
    ]);

    // Run contract entrypoint

    let program_segment_size = runner.get_program().data_len() + program_extra_data.len();
    runner
        .run_from_entrypoint(
            EntryPoint::Pc(entrypoint_offset),
            &entrypoint_args,
            true,
            Some(program_segment_size),
            &mut hint_processor,
        )
        .unwrap();

    // Check return values
    let return_values = runner.vm.get_return_values(5).unwrap();
    let retdata_start = return_values[3].get_relocatable().unwrap();
    let retdata_end = return_values[4].get_relocatable().unwrap();
    let retdata: Vec<Felt252> = runner
        .vm
        .get_integer_range(retdata_start, (retdata_end - retdata_start).unwrap())
        .unwrap()
        .iter()
        .map(|c| c.clone().into_owned())
        .collect();
    assert_eq!(expected_retdata, &retdata);
}

#[allow(clippy::result_large_err)]
/// Equals to fn run_cairo_1_entrypoint
/// But with run_resources as an input
fn run_cairo_1_entrypoint_with_run_resources(
    contract_class: CasmContractClass,
    entrypoint_offset: usize,
    hint_processor: &mut Cairo1HintProcessor,
    args: &[MaybeRelocatable],
) -> Result<Vec<Felt252>, CairoRunError> {
    let mut runner = CairoRunner::new(
        &program_from_casm_contract_class(&contract_class).unwrap(),
        &Cairo0RunConfig {
            layout: LayoutName::all_cairo,
            proof_mode: false,
            trace_enabled: false,
            disable_trace_padding: false,
            ..Default::default()
        }
        .run_config()
        .unwrap(),
    );

    let program_builtins = get_casm_contract_builtins(&contract_class, entrypoint_offset);
    runner
        .initialize_function_runner_cairo_1(&program_builtins)
        .unwrap();

    // Implicit Args
    let syscall_segment = MaybeRelocatable::from(runner.vm.add_memory_segment());

    let builtins = runner.get_program_builtins();

    let builtin_segment: Vec<MaybeRelocatable> = runner
        .vm
        .get_builtin_runners()
        .iter()
        .filter(|b| builtins.contains(&b.name()))
        .flat_map(|b| b.initial_stack())
        .collect();

    let initial_gas = MaybeRelocatable::from(usize::MAX);

    let mut implicit_args = builtin_segment;
    implicit_args.extend([initial_gas]);
    implicit_args.extend([syscall_segment]);

    // Other args

    // Load builtin costs
    let builtin_costs: Vec<MaybeRelocatable> =
        vec![0.into(), 0.into(), 0.into(), 0.into(), 0.into()];
    let builtin_costs_ptr = runner.vm.add_memory_segment();
    runner
        .vm
        .load_data(builtin_costs_ptr, &builtin_costs)
        .unwrap();

    // Load extra data
    let core_program_end_ptr =
        (runner.program_base.unwrap() + runner.get_program().data_len()).unwrap();
    let program_extra_data: Vec<MaybeRelocatable> =
        vec![0x208B7FFF7FFF7FFE_u64.into(), builtin_costs_ptr.into()];
    runner
        .vm
        .load_data(core_program_end_ptr, &program_extra_data)
        .unwrap();

    // Load calldata
    let calldata_start = runner.vm.add_memory_segment();
    let calldata_end = runner.vm.load_data(calldata_start, args).unwrap();

    // Create entrypoint_args

    let mut entrypoint_args: Vec<CairoArg> = implicit_args
        .iter()
        .map(|m| CairoArg::from(m.clone()))
        .collect();
    entrypoint_args.extend([
        MaybeRelocatable::from(calldata_start).into(),
        MaybeRelocatable::from(calldata_end).into(),
    ]);

    // Run contract entrypoint

    let program_segment_size = runner.get_program().data_len() + program_extra_data.len();
    runner.run_from_entrypoint(
        EntryPoint::Pc(entrypoint_offset),
        &entrypoint_args,
        true,
        Some(program_segment_size),
        hint_processor,
    )?;

    // Check return values
    let return_values = runner.vm.get_return_values(5).unwrap();
    let retdata_start = return_values[3].get_relocatable().unwrap();
    let retdata_end = return_values[4].get_relocatable().unwrap();
    let retdata: Vec<Felt252> = runner
        .vm
        .get_integer_range(retdata_start, (retdata_end - retdata_start).unwrap())
        .unwrap()
        .iter()
        .map(|c| c.clone().into_owned())
        .collect();
    Ok(retdata)
}

fn get_casm_contract_builtins(
    contract_class: &CasmContractClass,
    entrypoint_offset: usize,
) -> Vec<BuiltinName> {
    contract_class
        .entry_points_by_type
        .external
        .iter()
        .find(|e| e.offset == entrypoint_offset)
        .unwrap()
        .builtins
        .iter()
        .map(|s| BuiltinName::from_str(s).expect("Invalid builtin name"))
        .collect()
}
