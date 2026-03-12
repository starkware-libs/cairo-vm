// CairoFunctionRunner unit tests.
// Tested functions: new, new_custom, run, run_default_cairo0, get_builtin_base, get_return_values.
use crate::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor;
use crate::types::builtin_name::BuiltinName;
use crate::types::errors::program_errors::ProgramError;
use crate::types::layout_name::LayoutName;
use crate::types::program::Program;
use crate::types::relocatable::MaybeRelocatable;
use crate::vm::errors::cairo_run_errors::CairoRunError;
use crate::vm::runners::cairo_function_runner::{CairoFunctionRunner, EntryPoint};
use crate::vm::runners::cairo_runner::CairoArg;
use assert_matches::assert_matches;

fn load_program(program_bytes: &[u8]) -> Program {
    Program::from_bytes(program_bytes, None).unwrap()
}

#[test]
// Test that `new` initializes all 11 builtins, creates program/execution segments, and does not initialize segment_arena.
fn new_initializes_expected_builtin_bases() {
    let program = load_program(include_bytes!(
        "../../../../cairo_programs/example_program.json"
    ));
    let function_runner = CairoFunctionRunner::new(&program).unwrap();

    assert_eq!(function_runner.runner.vm.builtin_runners.len(), 11);
    let expected_builtins = [
        BuiltinName::pedersen,
        BuiltinName::range_check,
        BuiltinName::output,
        BuiltinName::ecdsa,
        BuiltinName::bitwise,
        BuiltinName::ec_op,
        BuiltinName::keccak,
        BuiltinName::poseidon,
        BuiltinName::range_check96,
        BuiltinName::add_mod,
        BuiltinName::mul_mod,
    ];

    for builtin in expected_builtins {
        assert!(function_runner.get_builtin_base(builtin).is_some());
    }
    assert!(function_runner
        .get_builtin_base(BuiltinName::segment_arena)
        .is_none());
    assert_eq!(function_runner.runner.vm.segments.num_segments(), 11 + 2);
}

#[test]
// Test that `new_custom` does not initialize builtins or memory segments automatically.
fn new_custom_does_not_initialize_builtins_or_segments() {
    let program = load_program(include_bytes!(
        "../../../../cairo_programs/example_program.json"
    ));
    let function_runner =
        CairoFunctionRunner::new_custom(&program, LayoutName::plain, None, false, false, false)
            .unwrap();

    assert!(function_runner
        .get_builtin_base(BuiltinName::range_check)
        .is_none());
    assert_eq!(function_runner.runner.vm.segments.num_segments(), 0);
}

#[test]
// Test successful function execution by entrypoint name for multiple functions in one program.
fn run_from_entrypoint_custom_program_test() {
    let program = load_program(include_bytes!(
        "../../../../cairo_programs/example_program.json"
    ));

    let mut function_runner = CairoFunctionRunner::new(&program).unwrap();
    let mut hint_processor = BuiltinHintProcessor::new_empty();
    let range_check_ptr = function_runner
        .get_builtin_base(BuiltinName::range_check)
        .unwrap();
    let main_args = vec![
        CairoArg::from(MaybeRelocatable::from(2_i64)),
        CairoArg::from(range_check_ptr.clone()),
    ];
    assert_matches!(
        function_runner.run(
            EntryPoint::Name("main"),
            true,
            None,
            &mut hint_processor,
            &main_args,
        ),
        Ok(())
    );

    let mut second_function_runner = CairoFunctionRunner::new(&program).unwrap();
    let mut second_hint_processor = BuiltinHintProcessor::new_empty();
    let second_range_check_ptr = second_function_runner
        .get_builtin_base(BuiltinName::range_check)
        .unwrap();
    let fib_args = vec![
        CairoArg::from(MaybeRelocatable::from(2_i64)),
        CairoArg::from(second_range_check_ptr),
    ];
    assert_matches!(
        second_function_runner.run(
            EntryPoint::Name("evaluate_fib"),
            true,
            None,
            &mut second_hint_processor,
            &fib_args,
        ),
        Ok(())
    );
}

#[test]
// Test successful execution using `EntryPoint::Pc` instead of function name lookup.
fn run_by_program_counter_happy_path() {
    let program = load_program(include_bytes!(
        "../../../../cairo_programs/example_program.json"
    ));
    let mut function_runner = CairoFunctionRunner::new(&program).unwrap();
    let mut hint_processor = BuiltinHintProcessor::new_empty();
    let range_check_ptr = function_runner
        .get_builtin_base(BuiltinName::range_check)
        .unwrap();
    let args = vec![
        CairoArg::from(MaybeRelocatable::from(2_i64)),
        CairoArg::from(range_check_ptr),
    ];
    let entrypoint_pc = function_runner
        .runner
        .program
        .get_identifier("__main__.main")
        .unwrap()
        .pc
        .unwrap();

    assert_matches!(
        function_runner.run(
            EntryPoint::Pc(entrypoint_pc),
            true,
            None,
            &mut hint_processor,
            &args,
        ),
        Ok(())
    );
}

#[test]
// Test `run_default_cairo0` happy path and verify zero requested return values.
fn run_default_cairo0_happy_path() {
    let program = load_program(include_bytes!(
        "../../../../cairo_programs/example_program.json"
    ));
    let mut function_runner = CairoFunctionRunner::new(&program).unwrap();
    let range_check_ptr = function_runner
        .get_builtin_base(BuiltinName::range_check)
        .unwrap();
    let args = vec![
        CairoArg::from(MaybeRelocatable::from(2_i64)),
        CairoArg::from(range_check_ptr),
    ];

    assert_matches!(function_runner.run_default_cairo0("main", &args), Ok(()));
    assert_eq!(function_runner.get_return_values(0).unwrap(), vec![]);
}

#[test]
// Test that running a missing function name returns `EntrypointNotFound`.
fn run_missing_entrypoint_returns_entrypoint_not_found() {
    let program = load_program(include_bytes!(
        "../../../../cairo_programs/example_program.json"
    ));
    let mut function_runner = CairoFunctionRunner::new(&program).unwrap();
    let mut hint_processor = BuiltinHintProcessor::new_empty();

    assert_matches!(
        function_runner.run(
            EntryPoint::Name("missing_entrypoint"),
            false,
            None,
            &mut hint_processor,
            &[],
        ),
        Err(CairoRunError::Program(ProgramError::EntrypointNotFound(entrypoint)))
            if entrypoint == "missing_entrypoint"
    );
}

#[test]
// Test that `run_default_cairo0` propagates missing entrypoint errors.
fn run_default_cairo0_missing_entrypoint_returns_entrypoint_not_found() {
    let program = load_program(include_bytes!(
        "../../../../cairo_programs/example_program.json"
    ));
    let mut function_runner = CairoFunctionRunner::new(&program).unwrap();

    assert_matches!(
        function_runner.run_default_cairo0("missing_entrypoint", &[]),
        Err(CairoRunError::Program(ProgramError::EntrypointNotFound(entrypoint)))
            if entrypoint == "missing_entrypoint"
    );
}

#[test]
// Test bitwise builtin execution and verify no memory holes remain.
fn run_from_entrypoint_bitwise_test_check_memory_holes() {
    let program = load_program(include_bytes!(
        "../../../../cairo_programs/bitwise_builtin_test.json"
    ));
    let mut function_runner = CairoFunctionRunner::new(&program).unwrap();
    let mut hint_processor = BuiltinHintProcessor::new_empty();
    let bitwise_ptr = function_runner
        .get_builtin_base(BuiltinName::bitwise)
        .unwrap();
    let args = vec![CairoArg::from(bitwise_ptr)];

    assert!(function_runner
        .run(
            EntryPoint::Name("main"),
            true,
            None,
            &mut hint_processor,
            &args,
        )
        .is_ok());

    assert_eq!(function_runner.runner.get_memory_holes().unwrap(), 0);
}

#[test]
// Test VM exception error message substitution from `error_msg` attributes.
fn run_from_entrypoint_substitute_error_message_test() {
    let program = load_program(include_bytes!(
        "../../../../cairo_programs/bad_programs/error_msg_function.json"
    ));
    let mut function_runner = CairoFunctionRunner::new(&program).unwrap();
    let mut hint_processor = BuiltinHintProcessor::new_empty();
    let result = function_runner.run(
        EntryPoint::Name("main"),
        true,
        None,
        &mut hint_processor,
        &[],
    );

    match result {
        Err(CairoRunError::VmException(exception)) => {
            assert_eq!(
                exception.error_attr_value,
                Some(String::from("Error message: Test error\n"))
            )
        }
        Err(_) => panic!("Wrong error returned, expected VmException"),
        Ok(_) => panic!("Expected run to fail"),
    }
}
