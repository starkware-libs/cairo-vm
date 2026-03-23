//! Function runner extension methods for [`CairoRunner`].
//!
//! Provides a simplified API for executing individual Cairo 0 functions by name or PC.
//! This entire module is compiled only when the `test_utils` feature is enabled.

use crate::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor;
use crate::hint_processor::hint_processor_definition::HintProcessor;
use crate::serde::deserialize_program::Identifier;
use crate::types::builtin_name::BuiltinName;
use crate::types::errors::program_errors::ProgramError;
use crate::types::layout_name::LayoutName;
use crate::types::program::Program;
use crate::types::relocatable::MaybeRelocatable;
use crate::vm::errors::cairo_run_errors::CairoRunError;
use crate::vm::errors::runner_errors::RunnerError;
use crate::vm::errors::vm_errors::VirtualMachineError;
use crate::vm::errors::vm_exception::VmException;
use crate::vm::runners::cairo_runner::{CairoArg, CairoRunner, ORDERED_BUILTIN_LIST};
use crate::vm::security::verify_secure_runner;

/// Identifies a Cairo function entrypoint either by function name or by program counter.
#[allow(dead_code)]
pub enum EntryPoint<'a> {
    Name(&'a str),
    Pc(usize),
}

impl CairoRunner {
    /// Creates a `CairoRunner` pre-initialized with all 11 standard builtins and segments.
    /// This is the common-case constructor for executing individual Cairo 0 functions.
    #[allow(clippy::result_large_err)]
    pub fn new_for_testing(program: &Program) -> Result<Self, CairoRunError> {
        let mut runner = CairoRunner::new(program, LayoutName::plain, None, false, false, false)?;
        runner.initialize_all_builtins()?;
        runner.initialize_segments(None);
        Ok(runner)
    }

    /// Initializes the 11 standard builtins used for Cairo function testing.
    pub fn initialize_all_builtins(&mut self) -> Result<(), RunnerError> {
        self.vm.builtin_runners.clear();
        self.program.builtins = ORDERED_BUILTIN_LIST.to_vec();
        self.initialize_program_builtins()
    }

    /// Gets the base pointer for a specific builtin.
    pub fn get_builtin_base(&self, builtin_name: BuiltinName) -> Option<MaybeRelocatable> {
        self.vm
            .builtin_runners
            .iter()
            .find(|b| b.name() == builtin_name)
            .map(|b| MaybeRelocatable::from((b.base() as isize, 0)))
    }

    /// Runs a Cairo 0 function by name using a default empty `BuiltinHintProcessor`.
    #[allow(clippy::result_large_err)]
    pub fn run_default_cairo0(
        &mut self,
        entrypoint: &str,
        args: &[CairoArg],
    ) -> Result<(), CairoRunError> {
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        self.run_from_entrypoint(
            EntryPoint::Name(entrypoint),
            args,
            false,
            None,
            &mut hint_processor,
        )
    }

    /// Resolves the entrypoint, builds the call stack, runs until the function's end PC,
    /// and optionally verifies security constraints.
    #[allow(clippy::result_large_err)]
    pub(crate) fn run_from_entrypoint(
        &mut self,
        entrypoint: EntryPoint<'_>,
        args: &[CairoArg],
        verify_secure: bool,
        program_segment_size: Option<usize>,
        hint_processor: &mut dyn HintProcessor,
    ) -> Result<(), CairoRunError> {
        let entrypoint_pc = match entrypoint {
            EntryPoint::Name(name) => self.get_function_pc(name)?,
            EntryPoint::Pc(pc) => pc,
        };
        let stack = args
            .iter()
            .map(|arg| self.vm.segments.gen_cairo_arg(arg))
            .collect::<Result<Vec<MaybeRelocatable>, VirtualMachineError>>()?;
        let end = self.initialize_function_entrypoint(
            entrypoint_pc,
            stack,
            MaybeRelocatable::from(0_i64),
        )?;
        self.initialize_vm()?;
        self.run_until_pc(end, hint_processor)
            .map_err(|err| VmException::from_vm_error(self, err))?;
        let is_proof_mode = self.is_proof_mode();
        self.end_run(true, false, hint_processor, is_proof_mode)?;
        if verify_secure {
            verify_secure_runner(self, false, program_segment_size)?;
        }
        Ok(())
    }

    /// Resolves `__main__.<entrypoint>` to its PC, following alias chains.
    #[allow(clippy::result_large_err)]
    pub(crate) fn get_function_pc(&self, entrypoint: &str) -> Result<usize, CairoRunError> {
        let full_name = format!("__main__.{entrypoint}");
        let identifier = self
            .program
            .get_identifier(&full_name)
            .ok_or_else(|| ProgramError::EntrypointNotFound(entrypoint.to_string()))?;
        self.get_pc_from_identifier(identifier)
    }

    #[allow(clippy::result_large_err)]
    fn get_pc_from_identifier(&self, identifier: &Identifier) -> Result<usize, CairoRunError> {
        match identifier.type_.as_deref() {
            Some("function") => Ok(identifier.pc.ok_or(RunnerError::NoPC)?),
            Some("alias") => {
                let dest = identifier.destination.as_deref().ok_or_else(|| {
                    ProgramError::AliasMissingDestination(
                        identifier.full_name.as_deref().unwrap_or("").to_string(),
                    )
                })?;
                let dest_id = self
                    .program
                    .get_identifier(dest)
                    .ok_or_else(|| ProgramError::EntrypointNotFound(dest.to_string()))?;
                self.get_pc_from_identifier(dest_id)
            }
            v => Err(ProgramError::InvalidIdentifierTypeForPc(
                identifier
                    .full_name
                    .clone()
                    .unwrap_or_else(|| "<unknown>".to_string()),
                v.unwrap_or("<unknown>").to_string(),
            )
            .into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor;
    use crate::types::builtin_name::BuiltinName;
    use crate::types::errors::program_errors::ProgramError;
    use crate::types::layout_name::LayoutName;
    use crate::types::program::Program;
    use crate::types::relocatable::MaybeRelocatable;
    use crate::vm::errors::cairo_run_errors::CairoRunError;
    use crate::vm::runners::cairo_runner::CairoArg;
    use crate::vm::runners::function_runner::EntryPoint;
    use assert_matches::assert_matches;

    fn load_program(program_bytes: &[u8]) -> Program {
        Program::from_bytes(program_bytes, None).unwrap()
    }

    #[test]
    fn new_for_testing_initializes_expected_builtin_bases() {
        let program = load_program(include_bytes!(
            "../../../../cairo_programs/example_program.json"
        ));
        let runner = CairoRunner::new_for_testing(&program).unwrap();

        assert_eq!(runner.vm.builtin_runners.len(), 11);
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
            assert!(runner.get_builtin_base(builtin).is_some());
        }
        assert!(runner
            .get_builtin_base(BuiltinName::segment_arena)
            .is_none());
        assert_eq!(runner.vm.segments.num_segments(), 11 + 2);
    }

    #[test]
    fn new_without_builtins_has_no_builtin_bases_or_segments() {
        let program = load_program(include_bytes!(
            "../../../../cairo_programs/example_program.json"
        ));
        let runner =
            CairoRunner::new(&program, LayoutName::plain, None, false, false, false).unwrap();

        assert!(runner.get_builtin_base(BuiltinName::range_check).is_none());
        assert_eq!(runner.vm.segments.num_segments(), 0);
    }

    #[test]
    fn run_from_entrypoint_custom_program_test() {
        let program = load_program(include_bytes!(
            "../../../../cairo_programs/example_program.json"
        ));

        let mut runner = CairoRunner::new_for_testing(&program).unwrap();
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let range_check_ptr = runner.get_builtin_base(BuiltinName::range_check).unwrap();
        let main_args = vec![
            CairoArg::from(MaybeRelocatable::from(2_i64)),
            CairoArg::from(range_check_ptr),
        ];
        assert_matches!(
            runner.run_from_entrypoint(
                EntryPoint::Name("main"),
                &main_args,
                true,
                None,
                &mut hint_processor
            ),
            Ok(())
        );

        let mut runner2 = CairoRunner::new_for_testing(&program).unwrap();
        let mut hint_processor2 = BuiltinHintProcessor::new_empty();
        let range_check_ptr2 = runner2.get_builtin_base(BuiltinName::range_check).unwrap();
        let fib_args = vec![
            CairoArg::from(MaybeRelocatable::from(2_i64)),
            CairoArg::from(range_check_ptr2),
        ];
        assert_matches!(
            runner2.run_from_entrypoint(
                EntryPoint::Name("evaluate_fib"),
                &fib_args,
                true,
                None,
                &mut hint_processor2
            ),
            Ok(())
        );
    }

    #[test]
    fn run_by_program_counter_happy_path() {
        let program = load_program(include_bytes!(
            "../../../../cairo_programs/example_program.json"
        ));
        let mut runner = CairoRunner::new_for_testing(&program).unwrap();
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let range_check_ptr = runner.get_builtin_base(BuiltinName::range_check).unwrap();
        let args = vec![
            CairoArg::from(MaybeRelocatable::from(2_i64)),
            CairoArg::from(range_check_ptr),
        ];
        let entrypoint_pc = runner
            .program
            .get_identifier("__main__.main")
            .unwrap()
            .pc
            .unwrap();

        assert_matches!(
            runner.run_from_entrypoint(
                EntryPoint::Pc(entrypoint_pc),
                &args,
                true,
                None,
                &mut hint_processor
            ),
            Ok(())
        );
    }

    #[test]
    fn run_default_cairo0_happy_path() {
        let program = load_program(include_bytes!(
            "../../../../cairo_programs/example_program.json"
        ));
        let mut runner = CairoRunner::new_for_testing(&program).unwrap();
        let range_check_ptr = runner.get_builtin_base(BuiltinName::range_check).unwrap();
        let args = vec![
            CairoArg::from(MaybeRelocatable::from(2_i64)),
            CairoArg::from(range_check_ptr),
        ];

        assert_matches!(runner.run_default_cairo0("main", &args), Ok(()));
        assert_eq!(runner.vm.get_return_values(0).unwrap(), vec![]);
    }

    #[test]
    fn get_function_pc_assert_nn_resolves_alias_to_pc_0() {
        let program = load_program(include_bytes!(
            "../../../../cairo_programs/example_program.json"
        ));
        let runner = CairoRunner::new_for_testing(&program).unwrap();

        let pc = runner.get_function_pc("assert_nn").unwrap();
        assert_eq!(
            pc, 0,
            "assert_nn is an alias to starkware.cairo.common.math.assert_nn which has pc 0"
        );
    }

    #[test]
    fn get_function_pc_assert_nn_manual_implementation_returns_pc_4() {
        let program = load_program(include_bytes!(
            "../../../../cairo_programs/example_program.json"
        ));
        let runner = CairoRunner::new_for_testing(&program).unwrap();

        let pc = runner
            .get_function_pc("assert_nn_manual_implementation")
            .unwrap();
        assert_eq!(
            pc, 4,
            "assert_nn_manual_implementation is a function with pc 4"
        );
    }

    #[test]
    fn run_missing_entrypoint_returns_entrypoint_not_found() {
        let program = load_program(include_bytes!(
            "../../../../cairo_programs/example_program.json"
        ));
        let mut runner = CairoRunner::new_for_testing(&program).unwrap();
        let mut hint_processor = BuiltinHintProcessor::new_empty();

        assert_matches!(
            runner.run_from_entrypoint(EntryPoint::Name("missing_entrypoint"), &[], false, None, &mut hint_processor),
            Err(CairoRunError::Program(ProgramError::EntrypointNotFound(ep))) if ep == "missing_entrypoint"
        );
    }

    #[test]
    fn run_default_cairo0_missing_entrypoint_returns_entrypoint_not_found() {
        let program = load_program(include_bytes!(
            "../../../../cairo_programs/example_program.json"
        ));
        let mut runner = CairoRunner::new_for_testing(&program).unwrap();

        assert_matches!(
            runner.run_default_cairo0("missing_entrypoint", &[]),
            Err(CairoRunError::Program(ProgramError::EntrypointNotFound(ep))) if ep == "missing_entrypoint"
        );
    }

    #[test]
    fn run_from_entrypoint_bitwise_test_check_memory_holes() {
        let program = load_program(include_bytes!(
            "../../../../cairo_programs/bitwise_builtin_test.json"
        ));
        let mut runner = CairoRunner::new_for_testing(&program).unwrap();
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let bitwise_ptr = runner.get_builtin_base(BuiltinName::bitwise).unwrap();

        assert!(runner
            .run_from_entrypoint(
                EntryPoint::Name("main"),
                &[CairoArg::from(bitwise_ptr)],
                true,
                None,
                &mut hint_processor
            )
            .is_ok());
        assert_eq!(runner.get_memory_holes().unwrap(), 0);
    }

    #[test]
    fn run_from_entrypoint_substitute_error_message_test() {
        let program = load_program(include_bytes!(
            "../../../../cairo_programs/bad_programs/error_msg_function.json"
        ));
        let mut runner = CairoRunner::new_for_testing(&program).unwrap();
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let result = runner.run_from_entrypoint(
            EntryPoint::Name("main"),
            &[],
            true,
            None,
            &mut hint_processor,
        );

        match result {
            Err(CairoRunError::VmException(exception)) => assert_eq!(
                exception.error_attr_value,
                Some(String::from("Error message: Test error\n"))
            ),
            Err(_) => panic!("Wrong error returned, expected VmException"),
            Ok(_) => panic!("Expected run to fail"),
        }
    }
}
