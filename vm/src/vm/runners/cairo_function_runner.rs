//! A Cairo function runner for testing purposes.
//!
//! This module provides [`CairoFunctionRunner`], a high-level interface for running individual
//! Cairo 0 functions with automatic builtin initialization. It allows direct invocation of specific
//! entrypoints with custom arguments.

use crate::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor;
use crate::hint_processor::hint_processor_definition::HintProcessor;
use crate::types::builtin_name::BuiltinName;
use crate::types::errors::program_errors::ProgramError;
use crate::types::instance_definitions::mod_instance_def::ModInstanceDef;
use crate::types::layout::CairoLayoutParams;
use crate::types::layout_name::LayoutName;
use crate::types::program::Program;
use crate::types::relocatable::MaybeRelocatable;
use crate::vm::errors::cairo_run_errors::CairoRunError;
use crate::vm::errors::memory_errors::MemoryError;
use crate::vm::errors::runner_errors::RunnerError;
use crate::vm::errors::vm_errors::VirtualMachineError;
use crate::vm::errors::vm_exception::VmException;
use crate::vm::runners::builtin_runner::{
    BitwiseBuiltinRunner, EcOpBuiltinRunner, HashBuiltinRunner, KeccakBuiltinRunner,
    ModBuiltinRunner, OutputBuiltinRunner, PoseidonBuiltinRunner, RangeCheckBuiltinRunner,
    SignatureBuiltinRunner, RC_N_PARTS_96, RC_N_PARTS_STANDARD,
};
use crate::vm::runners::cairo_runner::{CairoArg, CairoRunner};
use crate::vm::security::verify_secure_runner;

/// Identifies a Cairo function entrypoint either by function name or by program counter.
pub enum EntryPoint<'a> {
    Name(&'a str),
    Pc(usize),
}

/// A runner for executing individual Cairo functions.
/// Used for testing purposes only.
pub struct CairoFunctionRunner {
    /// The Cairo runner instance that manages VM execution.
    pub runner: CairoRunner,
}

impl CairoFunctionRunner {
    /// Creates a new `CairoFunctionRunner`.
    ///
    /// Initializes a basic `CairoRunner` with:
    /// - `LayoutName::plain`
    /// - `dynamic_layout_params = None`
    /// - `proof_mode = false`
    /// - `trace_enabled = false`
    /// - `disable_trace_padding = false`
    ///
    /// and then preloads a fixed set of commonly used builtins.
    ///
    /// # Arguments
    /// - `program`: The compiled Cairo program to execute.
    ///
    /// # Returns
    /// - `Ok(CairoFunctionRunner)`: On successful initialization.
    /// - `Err(CairoRunError)`: If the runner cannot be created.
    #[allow(clippy::result_large_err)]
    pub fn new(program: &Program) -> std::result::Result<Self, CairoRunError> {
        let mut runner = CairoRunner::new(
            program,
            LayoutName::plain,
            None,  // dynamic_layout_params
            false, // proof_mode
            false, // trace_enabled
            false, // disable_trace_padding
        )?;

        Self::initialize_all_builtins(&mut runner)?;
        runner.initialize_segments(None);

        Ok(Self { runner })
    }
    /// Creates a new `CairoFunctionRunner` with custom `CairoRunner` initialization parameters.
    ///
    /// Unlike [`Self::new`], this constructor does not preload builtins or initialize segments.
    #[allow(clippy::result_large_err)]
    pub fn new_custom(
        program: &Program,
        layout: LayoutName,
        dynamic_layout_params: Option<CairoLayoutParams>,
        proof_mode: bool,
        trace_enabled: bool,
        disable_trace_padding: bool,
    ) -> std::result::Result<Self, CairoRunError> {
        let runner = CairoRunner::new(
            program,
            layout,
            dynamic_layout_params,
            proof_mode,
            trace_enabled,
            disable_trace_padding,
        )?;

        Ok(Self { runner })
    }

    /// Initializes a fixed set of 11 builtins used by this function runner.
    fn initialize_all_builtins(runner: &mut CairoRunner) -> Result<(), RunnerError> {
        runner.vm.builtin_runners.clear();
        runner
            .vm
            .builtin_runners
            .push(HashBuiltinRunner::new(Some(32), true).into());
        runner
            .vm
            .builtin_runners
            .push(RangeCheckBuiltinRunner::<RC_N_PARTS_STANDARD>::new(Some(1), true).into());
        runner
            .vm
            .builtin_runners
            .push(OutputBuiltinRunner::new(true).into());
        runner
            .vm
            .builtin_runners
            .push(SignatureBuiltinRunner::new(Some(1), true).into());
        runner
            .vm
            .builtin_runners
            .push(BitwiseBuiltinRunner::new(Some(1), true).into());
        runner
            .vm
            .builtin_runners
            .push(EcOpBuiltinRunner::new(Some(1), true).into());
        runner
            .vm
            .builtin_runners
            .push(KeccakBuiltinRunner::new(Some(1), true).into());
        runner
            .vm
            .builtin_runners
            .push(PoseidonBuiltinRunner::new(Some(1), true).into());
        runner
            .vm
            .builtin_runners
            .push(RangeCheckBuiltinRunner::<RC_N_PARTS_96>::new(Some(1), true).into());
        runner
            .vm
            .builtin_runners
            .push(ModBuiltinRunner::new_add_mod(&ModInstanceDef::new(Some(1), 1, 96), true).into());
        runner
            .vm
            .builtin_runners
            .push(ModBuiltinRunner::new_mul_mod(&ModInstanceDef::new(Some(1), 1, 96), true).into());

        Ok(())
    }

    /// Runs a Cairo function from the specified entrypoint.
    ///
    /// # Arguments
    /// - `entrypoint`: The entrypoint to execute, either by function name or by PC.
    /// - `verify_secure`: If `true`, runs additional security verification after execution.
    /// - `program_segment_size`: Optional size limit for the program segment.
    /// - `program_input`: Optional program input to inject into the execution scopes.
    /// - `hint_processor`: The hint processor used during VM execution.
    /// - `args`: The function arguments.
    ///
    /// # Returns
    /// - `Ok(())`: On successful execution.
    /// - `Err(CairoRunError)`: If the entrypoint is not found, execution fails, or security
    ///   verification fails.
    #[allow(clippy::result_large_err)]
    pub fn run(
        &mut self,
        entrypoint: EntryPoint<'_>,
        verify_secure: bool,
        program_segment_size: Option<usize>,
        hint_processor: &mut dyn HintProcessor,
        args: &[CairoArg],
    ) -> std::result::Result<(), CairoRunError> {
        let entrypoint_pc = match entrypoint {
            EntryPoint::Name(name) => self.get_function_pc(name)?,
            EntryPoint::Pc(pc) => pc,
        };

        let cairo_args: Vec<&CairoArg> = args.iter().collect();

        self.run_from_entrypoint(
            entrypoint_pc,
            &cairo_args,
            verify_secure,
            program_segment_size,
            hint_processor,
        )?;

        Ok(())
    }

    #[allow(clippy::result_large_err)]
    // Builds the call stack from Cairo args, runs until the function's end PC, and optionally verifies security constraints.
    fn run_from_entrypoint(
        &mut self,
        entrypoint: usize,
        args: &[&CairoArg],
        verify_secure: bool,
        program_segment_size: Option<usize>,
        hint_processor: &mut dyn HintProcessor,
    ) -> std::result::Result<(), CairoRunError> {
        let stack = args
            .iter()
            .map(|arg| self.runner.vm.segments.gen_cairo_arg(arg))
            .collect::<Result<Vec<MaybeRelocatable>, VirtualMachineError>>()?;
        let return_fp = MaybeRelocatable::from(0_i64);
        let end = self
            .runner
            .initialize_function_entrypoint(entrypoint, stack, return_fp)?;

        self.runner.initialize_vm()?;

        self.runner
            .run_until_pc(end, hint_processor)
            .map_err(|err| VmException::from_vm_error(&self.runner, err))?;
        self.runner
            .end_run(true, false, hint_processor, self.runner.is_proof_mode())?;

        if verify_secure {
            verify_secure_runner(&self.runner, false, program_segment_size)?;
        }

        Ok(())
    }

    /// Runs a Cairo 0 function with a default empty `BuiltinHintProcessor`for cairo0 files.
    #[allow(clippy::result_large_err)]
    pub fn run_default_cairo0(
        &mut self,
        entrypoint: &str,
        args: &[CairoArg],
    ) -> std::result::Result<(), CairoRunError> {
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        self.run(
            EntryPoint::Name(entrypoint),
            false,
            None,
            &mut hint_processor,
            args,
        )
    }

    /// Retrieves return values from the VM's memory after function execution.
    ///
    /// Reads the last `n_return_values` values from the allocation pointer (AP).
    pub fn get_return_values(
        &self,
        n_return_values: usize,
    ) -> Result<Vec<MaybeRelocatable>, MemoryError> {
        self.runner.vm.get_return_values(n_return_values)
    }

    /// Gets the base pointer for a specific builtin.
    pub fn get_builtin_base(&self, builtin_name: BuiltinName) -> Option<MaybeRelocatable> {
        self.runner
            .vm
            .builtin_runners
            .iter()
            .find(|builtin_runner| builtin_runner.name() == builtin_name)
            .map(|builtin_runner| MaybeRelocatable::from((builtin_runner.base() as isize, 0)))
    }

    /// Gets the program counter (PC) for a function entrypoint.
    #[allow(clippy::result_large_err)]
    fn get_function_pc(&self, entrypoint: &str) -> std::result::Result<usize, CairoRunError> {
        let full_name = format!("__main__.{entrypoint}");
        let identifier = self
            .runner
            .program
            .get_identifier(&full_name)
            .ok_or_else(|| ProgramError::EntrypointNotFound(entrypoint.to_string()))?;

        let pc = identifier.pc.ok_or(RunnerError::NoPC)?;

        Ok(pc)
    }
}
