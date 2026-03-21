use crate::{
    hint_processor::hint_processor_definition::HintProcessor,
    types::{
        builtin_name::BuiltinName,
        layout::{CairoLayout, CairoLayoutParams},
        layout_name::LayoutName,
        program::Program,
    },
    vm::{
        errors::{
            cairo_run_errors::CairoRunError, runner_errors::RunnerError, vm_exception::VmException,
        },
        runners::{
            cairo_pie::CairoPie,
            cairo_runner::{CairoRunConfig, CairoRunner, RunnerMode},
        },
        security::verify_secure_runner,
        trace::trace_entry::RelocatedTraceEntry,
        vm_core::DEFAULT_MAX_TRACEBACK_ENTRIES,
    },
    Felt252,
};

use crate::types::exec_scope::ExecutionScopes;
#[cfg(feature = "test_utils")]
use arbitrary::{self, Arbitrary};
use core::fmt;
use tracing::{info, span, Level};

#[cfg_attr(feature = "test_utils", derive(Arbitrary))]
pub struct Cairo0RunConfig<'a> {
    #[cfg_attr(feature = "test_utils", arbitrary(value = "main"))]
    pub entrypoint: &'a str,
    pub trace_enabled: bool,
    /// Relocate memory if `true`, otherwise memory is not relocated.
    pub relocate_mem: bool,
    // When `relocate_trace` is set to `false`, the trace will not be relocated even if `trace_enabled` is `true`.
    pub relocate_trace: bool,
    pub layout: LayoutName,
    /// The `dynamic_layout_params` argument should only be used with dynamic layout.
    /// It is ignored otherwise.
    pub dynamic_layout_params: Option<CairoLayoutParams>,
    pub proof_mode: bool,
    pub fill_holes: bool,
    pub secure_run: Option<bool>,
    /// Disable padding of the trace.
    /// By default, the trace is padded to accommodate the expected builtins-n_steps relationships
    /// according to the layout.
    /// When the padding is disabled:
    /// - It doesn't modify/pad n_steps.
    /// - It still pads each builtin segment to the next power of 2 (w.r.t the number of used
    ///   instances of the builtin) compared to their sizes at the end of the execution.
    pub disable_trace_padding: bool,
    pub allow_missing_builtins: Option<bool>,
    pub max_traceback_entries: u32,
}

impl Default for Cairo0RunConfig<'_> {
    fn default() -> Self {
        Cairo0RunConfig {
            entrypoint: "main",
            trace_enabled: false,
            relocate_mem: false,
            // Set to true to match expected behavior: trace is relocated only if trace_enabled is true.
            relocate_trace: true,
            layout: LayoutName::plain,
            proof_mode: false,
            fill_holes: false,
            secure_run: None,
            disable_trace_padding: false,
            allow_missing_builtins: None,
            dynamic_layout_params: None,
            max_traceback_entries: DEFAULT_MAX_TRACEBACK_ENTRIES,
        }
    }
}

impl Cairo0RunConfig<'_> {
    pub fn run_config(&self) -> Result<CairoRunConfig, RunnerError> {
        let runner_mode = if self.proof_mode {
            RunnerMode::ProofModeCanonical
        } else {
            RunnerMode::ExecutionMode
        };
        CairoRunConfig::new(
            CairoLayout::new(self.layout, self.dynamic_layout_params.clone())?,
            self.trace_enabled,
            self.disable_trace_padding,
            runner_mode,
            self.max_traceback_entries,
        )
    }
}

#[derive(Clone)]
pub struct StwoCairoRunConfig {
    pub trace_enabled: bool,
    pub relocate_mem: bool,
    pub relocate_trace: bool,
    pub fill_holes: bool,
    pub secure_run: bool,
    pub disable_trace_padding: bool,
    pub runner_mode: RunnerMode,
    pub max_traceback_entries: u32,
}

impl Default for StwoCairoRunConfig {
    fn default() -> Self {
        StwoCairoRunConfig {
            trace_enabled: true,
            relocate_mem: false,
            relocate_trace: true,
            fill_holes: false,
            secure_run: true,
            disable_trace_padding: true,
            runner_mode: RunnerMode::ProofModeCanonical,
            max_traceback_entries: DEFAULT_MAX_TRACEBACK_ENTRIES,
        }
    }
}

impl StwoCairoRunConfig {
    pub fn run_config(&self) -> Result<CairoRunConfig, RunnerError> {
        CairoRunConfig::new(
            CairoLayout::all_cairo_stwo_instance(),
            self.trace_enabled,
            self.disable_trace_padding,
            self.runner_mode.clone(),
            self.max_traceback_entries,
        )
    }
}

#[allow(clippy::result_large_err)]
pub fn cairo_run_stwo(
    program: &Program,
    allowed_builtins: &[BuiltinName],
    hint_processor: &mut dyn HintProcessor,
    exec_scopes: ExecutionScopes,
    cairo_run_config: &StwoCairoRunConfig,
) -> Result<CairoRunner, CairoRunError> {
    let _span = span!(Level::INFO, "cairo run stwo").entered();

    let proof_mode = cairo_run_config.runner_mode != RunnerMode::ExecutionMode;
    let mut cairo_runner = CairoRunner::new(program, &cairo_run_config.run_config()?);
    cairo_runner.exec_scopes = exec_scopes;

    let end = cairo_runner.initialize_stwo(allowed_builtins)?;

    cairo_runner
        .run_until_pc(end, hint_processor)
        .map_err(|err| VmException::from_vm_error(&cairo_runner, err))?;

    if proof_mode {
        cairo_runner.run_for_steps(1, hint_processor)?;
    }

    cairo_runner.end_run(
        cairo_run_config.disable_trace_padding,
        false,
        hint_processor,
        cairo_run_config.fill_holes,
    )?;

    cairo_runner.read_return_values(false)?;
    if proof_mode {
        cairo_runner.finalize_segments()?;
    }

    if cairo_run_config.secure_run {
        verify_secure_runner(&cairo_runner, true, None)?;
    }

    cairo_runner.relocate(
        cairo_run_config.relocate_mem,
        cairo_run_config.relocate_trace,
    )?;

    Ok(cairo_runner)
}

#[allow(clippy::result_large_err)]
/// Runs a program with a customized execution scope.
pub fn cairo_run_program_with_initial_scope(
    program: &Program,
    cairo_run_config: &Cairo0RunConfig,
    hint_processor: &mut dyn HintProcessor,
    exec_scopes: ExecutionScopes,
) -> Result<CairoRunner, CairoRunError> {
    let _span = span!(Level::INFO, "cairo run").entered();
    let secure_run = cairo_run_config
        .secure_run
        .unwrap_or(!cairo_run_config.proof_mode);

    let allow_missing_builtins = cairo_run_config
        .allow_missing_builtins
        .unwrap_or(cairo_run_config.proof_mode);

    let mut cairo_runner = CairoRunner::new(program, &cairo_run_config.run_config()?);

    cairo_runner.exec_scopes = exec_scopes;

    info!(layout = ?cairo_run_config.layout, proof_mode = ?cairo_run_config.proof_mode, trace_enabled = ?cairo_run_config.trace_enabled, disable_trace_padding = ?cairo_run_config.disable_trace_padding, allow_missing_builtins = ?allow_missing_builtins, "Initializing Cairo runner.");
    let end = cairo_runner.initialize(allow_missing_builtins)?;
    // check step calculation

    info!("Running until PC.");
    cairo_runner
        .run_until_pc(end, hint_processor)
        .map_err(|err| VmException::from_vm_error(&cairo_runner, err))?;

    if cairo_run_config.proof_mode {
        // we run an additional step to ensure that `end` is the last step execute,
        // rather than the one after it.
        cairo_runner.run_for_steps(1, hint_processor)?;
    }

    info!(disable_trace_padding = ?cairo_run_config.disable_trace_padding, fill_holes = ?cairo_run_config.fill_holes, "Ending run.");
    cairo_runner.end_run(
        cairo_run_config.disable_trace_padding,
        false,
        hint_processor,
        cairo_run_config.fill_holes,
    )?;

    info!("Reading return values.");
    cairo_runner.read_return_values(allow_missing_builtins)?;
    if cairo_run_config.proof_mode {
        info!("In proof mode, finalizing segments.");
        cairo_runner.finalize_segments()?;
    }

    if secure_run {
        info!("In secure run, verifying secure runner.");
        verify_secure_runner(&cairo_runner, true, None)?;
    }

    info!(relocate_mem = ?cairo_run_config.relocate_mem, relocate_trace = ?cairo_run_config.relocate_trace, "Relocating.");
    cairo_runner.relocate(
        cairo_run_config.relocate_mem,
        cairo_run_config.relocate_trace,
    )?;

    Ok(cairo_runner)
}

#[allow(clippy::result_large_err)]
pub fn cairo_run_program(
    program: &Program,
    cairo_run_config: &Cairo0RunConfig,
    hint_processor: &mut dyn HintProcessor,
) -> Result<CairoRunner, CairoRunError> {
    cairo_run_program_with_initial_scope(
        program,
        cairo_run_config,
        hint_processor,
        ExecutionScopes::new(),
    )
}

#[allow(clippy::result_large_err)]
pub fn cairo_run(
    program_content: &[u8],
    cairo_run_config: &Cairo0RunConfig,
    hint_processor: &mut dyn HintProcessor,
) -> Result<CairoRunner, CairoRunError> {
    let program = Program::from_bytes(program_content, Some(cairo_run_config.entrypoint))?;

    cairo_run_program(&program, cairo_run_config, hint_processor)
}

#[allow(clippy::result_large_err)]
/// Runs a Cairo PIE generated by a previous cairo execution
/// To generate a cairo pie use the runner's method `get_cairo_pie`
/// Note: Cairo PIEs cannot be ran in proof_mode
/// WARNING: As the RunResources are part of the HintProcessor trait, the caller should make sure that
/// the number of steps in the `RunResources` matches that of the `ExecutionResources` in the `CairoPie`.
/// An error will be returned if this doesn't hold.
pub fn cairo_run_pie(
    pie: &CairoPie,
    cairo_run_config: &Cairo0RunConfig,
    hint_processor: &mut dyn HintProcessor,
) -> Result<CairoRunner, CairoRunError> {
    if cairo_run_config.proof_mode {
        return Err(RunnerError::CairoPieProofMode.into());
    }
    if hint_processor
        .get_n_steps()
        .is_none_or(|steps| steps != pie.execution_resources.n_steps)
    {
        return Err(RunnerError::PieNStepsVsRunResourcesNStepsMismatch.into());
    }
    pie.run_validity_checks()?;
    let secure_run = cairo_run_config.secure_run.unwrap_or(true);

    let allow_missing_builtins = cairo_run_config.allow_missing_builtins.unwrap_or_default();

    let program = Program::from_stripped_program(&pie.metadata.program);
    let mut cairo_runner = CairoRunner::new(&program, &cairo_run_config.run_config()?);

    let end = cairo_runner.initialize(allow_missing_builtins)?;
    cairo_runner.vm.finalize_segments_by_cairo_pie(pie);
    // Load builtin additional data
    for (name, data) in pie.additional_data.0.iter() {
        // Data is not trusted in secure_run, therefore we skip extending the hash builtin's data
        if matches!(name, BuiltinName::pedersen) && secure_run {
            continue;
        }
        if let Some(builtin) = cairo_runner
            .vm
            .builtin_runners
            .iter_mut()
            .find(|b| b.name() == *name)
        {
            builtin.extend_additional_data(data)?;
        }
    }
    // Load previous execution memory
    let has_zero_segment = cairo_runner.vm.segments.has_zero_segment() as usize;
    let n_extra_segments = pie.metadata.extra_segments.len() - has_zero_segment;
    cairo_runner
        .vm
        .segments
        .load_pie_memory(&pie.memory, n_extra_segments)?;

    cairo_runner
        .run_until_pc(end, hint_processor)
        .map_err(|err| VmException::from_vm_error(&cairo_runner, err))?;

    cairo_runner.end_run(
        cairo_run_config.disable_trace_padding,
        false,
        hint_processor,
        cairo_run_config.fill_holes,
    )?;

    cairo_runner.read_return_values(allow_missing_builtins)?;

    if secure_run {
        verify_secure_runner(&cairo_runner, true, None)?;
        // Check that the Cairo PIE produced by this run is compatible with the Cairo PIE received
        cairo_runner.get_cairo_pie()?.check_pie_compatibility(pie)?;
    }
    cairo_runner.relocate(
        cairo_run_config.relocate_mem,
        cairo_run_config.relocate_trace,
    )?;

    Ok(cairo_runner)
}

/// Runs a Cairo PIE using the Stwo runtime API.
/// Same as `cairo_run_pie` but uses [StwoCairoRunConfig] + `initialize_stwo` instead of layouts.
/// Note: Cairo PIEs cannot be run in proof mode.
/// WARNING: As the RunResources are part of the HintProcessor trait, the caller should make sure that
/// the number of steps in the `RunResources` matches that of the `ExecutionResources` in the `CairoPie`.
/// An error will be returned if this doesn't hold.
#[allow(clippy::result_large_err)]
pub fn cairo_run_pie_stwo(
    pie: &CairoPie,
    allowed_builtins: &[BuiltinName],
    hint_processor: &mut dyn HintProcessor,
    cairo_run_config: &StwoCairoRunConfig,
) -> Result<CairoRunner, CairoRunError> {
    if hint_processor
        .get_n_steps()
        .is_none_or(|steps| steps != pie.execution_resources.n_steps)
    {
        return Err(RunnerError::PieNStepsVsRunResourcesNStepsMismatch.into());
    }
    pie.run_validity_checks()?;

    let program = Program::from_stripped_program(&pie.metadata.program);
    let mut cairo_runner = CairoRunner::new(
        &program,
        &StwoCairoRunConfig {
            runner_mode: RunnerMode::ExecutionMode,
            ..cairo_run_config.clone()
        }
        .run_config()?,
    );

    let end = cairo_runner.initialize_stwo(allowed_builtins)?;
    cairo_runner.vm.finalize_segments_by_cairo_pie(pie);
    // Load builtin additional data
    for (name, data) in pie.additional_data.0.iter() {
        // Data is not trusted in secure_run, therefore we skip extending the hash builtin's data
        if matches!(name, BuiltinName::pedersen) && cairo_run_config.secure_run {
            continue;
        }
        if let Some(builtin) = cairo_runner
            .vm
            .builtin_runners
            .iter_mut()
            .find(|b| b.name() == *name)
        {
            builtin.extend_additional_data(data)?;
        }
    }
    // Load previous execution memory
    let has_zero_segment = cairo_runner.vm.segments.has_zero_segment() as usize;
    let n_extra_segments = pie.metadata.extra_segments.len() - has_zero_segment;
    cairo_runner
        .vm
        .segments
        .load_pie_memory(&pie.memory, n_extra_segments)?;

    cairo_runner
        .run_until_pc(end, hint_processor)
        .map_err(|err| VmException::from_vm_error(&cairo_runner, err))?;

    cairo_runner.end_run(
        cairo_run_config.disable_trace_padding,
        false,
        hint_processor,
        cairo_run_config.fill_holes,
    )?;

    cairo_runner.read_return_values(false)?;

    if cairo_run_config.secure_run {
        verify_secure_runner(&cairo_runner, true, None)?;
        // Check that the Cairo PIE produced by this run is compatible with the Cairo PIE received
        cairo_runner.get_cairo_pie()?.check_pie_compatibility(pie)?;
    }
    cairo_runner.relocate(
        cairo_run_config.relocate_mem,
        cairo_run_config.relocate_trace,
    )?;

    Ok(cairo_runner)
}

#[cfg(feature = "test_utils")]
#[allow(clippy::result_large_err)]
pub fn cairo_run_fuzzed_program(
    program: Program,
    cairo_run_config: &Cairo0RunConfig,
    hint_processor: &mut dyn HintProcessor,
    steps_limit: usize,
) -> Result<CairoRunner, CairoRunError> {
    use crate::vm::errors::vm_errors::VirtualMachineError;

    let secure_run = cairo_run_config
        .secure_run
        .unwrap_or(!cairo_run_config.proof_mode);

    let allow_missing_builtins = cairo_run_config
        .allow_missing_builtins
        .unwrap_or(cairo_run_config.proof_mode);

    let mut cairo_runner = CairoRunner::new(&program, &cairo_run_config.run_config()?);

    let _end = cairo_runner.initialize(allow_missing_builtins)?;

    let res = match cairo_runner.run_until_steps(steps_limit, hint_processor) {
        Err(VirtualMachineError::EndOfProgram(_remaining)) => Ok(()), // program ran OK but ended before steps limit
        res => res,
    };

    res.map_err(|err| VmException::from_vm_error(&cairo_runner, err))?;

    cairo_runner.end_run(false, false, hint_processor, cairo_run_config.fill_holes)?;

    cairo_runner.read_return_values(allow_missing_builtins)?;
    if cairo_run_config.proof_mode {
        cairo_runner.finalize_segments()?;
    }
    if secure_run {
        verify_secure_runner(&cairo_runner, true, None)?;
    }
    cairo_runner.relocate(
        cairo_run_config.relocate_mem,
        cairo_run_config.relocate_trace,
    )?;

    Ok(cairo_runner)
}

/// Error returned by [`BinaryWrite::write_all`].
#[derive(Debug)]
pub struct WriteError;

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Failed to write bytes")
    }
}

impl std::error::Error for WriteError {}

/// A minimal binary write trait that works in both std and no_std environments.
///
/// This trait provides a simple interface for writing bytes, similar to `std::io::Write`,
/// but without requiring the standard library.
pub trait BinaryWrite {
    /// Writes all bytes from the buffer to the writer.
    ///
    /// This method must write the entire buffer or return an error.
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), WriteError>;
}

impl<W: std::io::Write> BinaryWrite for W {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        std::io::Write::write_all(self, bytes).map_err(|_| WriteError)
    }
}

/// Error returned when encoding trace or memory fails.
#[derive(Debug)]
pub struct EncodeTraceError(usize, WriteError);

impl fmt::Display for EncodeTraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Failed to encode trace at position {}", self.0)
    }
}

impl std::error::Error for EncodeTraceError {}

/// Writes the trace binary representation.
///
/// The trace entries (ap, fp, pc) are little-endian encoded and concatenated:
/// - ap: 8-byte
/// - fp: 8-byte
/// - pc: 8-byte
pub fn write_encoded_trace(
    relocated_trace: &[RelocatedTraceEntry],
    dest: &mut impl BinaryWrite,
) -> Result<(), EncodeTraceError> {
    for (i, entry) in relocated_trace.iter().enumerate() {
        dest.write_all(&(entry.ap as u64).to_le_bytes())
            .map_err(|e| EncodeTraceError(i, e))?;
        dest.write_all(&(entry.fp as u64).to_le_bytes())
            .map_err(|e| EncodeTraceError(i, e))?;
        dest.write_all(&(entry.pc as u64).to_le_bytes())
            .map_err(|e| EncodeTraceError(i, e))?;
    }

    Ok(())
}

/// Writes the relocated memory binary representation.
///
/// The memory pairs (address, value) are little-endian encoded and concatenated:
/// - address: 8-byte
/// - value: 32-byte
pub fn write_encoded_memory(
    relocated_memory: &[Option<Felt252>],
    dest: &mut impl BinaryWrite,
) -> Result<(), EncodeTraceError> {
    for (i, memory_cell) in relocated_memory.iter().enumerate() {
        if let Some(value) = memory_cell {
            dest.write_all(&(i as u64).to_le_bytes())
                .map_err(|e| EncodeTraceError(i, e))?;
            dest.write_all(&value.to_bytes_le())
                .map_err(|e| EncodeTraceError(i, e))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::runners::cairo_runner::RunResources;
    use crate::Felt252;
    use crate::{
        hint_processor::{
            builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor,
            hint_processor_definition::HintProcessor,
        },
        utils::test_utils::*,
    };

    use rstest::rstest;

    #[allow(clippy::result_large_err)]
    fn run_test_program(
        program_content: &[u8],
        hint_processor: &mut dyn HintProcessor,
    ) -> Result<CairoRunner, CairoRunError> {
        let program = Program::from_bytes(program_content, Some("main")).unwrap();
        let mut cairo_runner = cairo_runner!(program, LayoutName::all_cairo, false, true);
        let end = cairo_runner
            .initialize(false)
            .map_err(CairoRunError::Runner)?;

        assert!(cairo_runner.run_until_pc(end, hint_processor).is_ok());

        Ok(cairo_runner)
    }

    #[test]
    fn cairo_run_custom_entry_point() {
        let program = Program::from_bytes(
            include_bytes!("../../cairo_programs/not_main.json"),
            Some("not_main"),
        )
        .unwrap();
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let mut cairo_runner = cairo_runner!(program);

        let end = cairo_runner.initialize(false).unwrap();
        assert!(cairo_runner.run_until_pc(end, &mut hint_processor).is_ok());
        assert!(cairo_runner.relocate(true, true).is_ok());
        // `main` returns without doing nothing, but `not_main` sets `[ap]` to `1`
        // Memory location was found empirically and simply hardcoded
        assert_eq!(cairo_runner.relocated_memory[2], Some(Felt252::from(123)));
    }

    #[test]
    fn cairo_run_with_no_data_program() {
        // a compiled program with no `data` key.
        // it should fail when the program is loaded.
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let no_data_program_path =
            include_bytes!("../../cairo_programs/manually_compiled/no_data_program.json");
        let cairo_run_config = Cairo0RunConfig::default();
        assert!(cairo_run(no_data_program_path, &cairo_run_config, &mut hint_processor,).is_err());
    }

    #[test]
    fn cairo_run_with_no_main_program() {
        // a compiled program with no main scope
        // it should fail when trying to run initialize_main_entrypoint.
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let no_main_program =
            include_bytes!("../../cairo_programs/manually_compiled/no_main_program.json");
        let cairo_run_config = Cairo0RunConfig::default();
        assert!(cairo_run(no_main_program, &cairo_run_config, &mut hint_processor,).is_err());
    }

    #[test]
    fn cairo_run_with_invalid_memory() {
        // the program invalid_memory.json has an invalid memory cell and errors when trying to
        // decode the instruction.
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let invalid_memory =
            include_bytes!("../../cairo_programs/manually_compiled/invalid_memory.json");
        let cairo_run_config = Cairo0RunConfig::default();
        assert!(cairo_run(invalid_memory, &cairo_run_config, &mut hint_processor,).is_err());
    }

    #[test]
    fn write_output_program() {
        let program_content = include_bytes!("../../cairo_programs/bitwise_output.json");
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let mut runner = run_test_program(program_content, &mut hint_processor)
            .expect("Couldn't initialize cairo runner");

        let mut output_buffer = String::new();
        runner.vm.write_output(&mut output_buffer).unwrap();
        assert_eq!(&output_buffer, "0\n");
    }

    #[cfg(feature = "test_utils")]
    #[test]
    fn test_cairo_run_fuzzed_program() {
        let program = Program::from_bytes(
            include_bytes!("../../cairo_programs/struct.json"),
            Some("main"),
        )
        .unwrap();
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        cairo_run_fuzzed_program(
            program,
            &Cairo0RunConfig::default(),
            &mut hint_processor,
            1000,
        )
        .unwrap();
    }

    #[test]
    fn run_with_no_trace() {
        let program = Program::from_bytes(
            include_bytes!("../../cairo_programs/struct.json"),
            Some("main"),
        )
        .unwrap();

        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let mut cairo_runner = cairo_runner!(program);
        let end = cairo_runner.initialize(false).unwrap();
        assert!(cairo_runner.run_until_pc(end, &mut hint_processor).is_ok());
        assert!(cairo_runner.relocate(false, false).is_ok());
        assert!(cairo_runner.relocated_trace.is_none());
    }

    #[rstest]
    #[case(include_bytes!("../../cairo_programs/fibonacci.json"))]
    #[case(include_bytes!("../../cairo_programs/integration.json"))]
    #[case(include_bytes!("../../cairo_programs/common_signature.json"))]
    #[case(include_bytes!("../../cairo_programs/relocate_segments.json"))]
    #[case(include_bytes!("../../cairo_programs/ec_op.json"))]
    #[case(include_bytes!("../../cairo_programs/bitwise_output.json"))]
    #[case(include_bytes!("../../cairo_programs/value_beyond_segment.json"))]
    fn get_and_run_cairo_pie(#[case] program_content: &[u8]) {
        let cairo_run_config = Cairo0RunConfig {
            layout: LayoutName::starknet_with_keccak,
            ..Default::default()
        };
        // First run program to get Cairo PIE
        let cairo_pie = {
            let runner = cairo_run(
                program_content,
                &cairo_run_config,
                &mut BuiltinHintProcessor::new_empty(),
            )
            .unwrap();
            runner.get_cairo_pie().unwrap()
        };
        let mut hint_processor = BuiltinHintProcessor::new(
            Default::default(),
            RunResources::new(cairo_pie.execution_resources.n_steps),
        );
        // Default config runs with secure_run, which checks that the Cairo PIE produced by this run is compatible with the one received
        assert!(cairo_run_pie(&cairo_pie, &cairo_run_config, &mut hint_processor).is_ok());
    }

    #[test]
    fn cairo_run_pie_n_steps_not_set() {
        // First run program to get Cairo PIE
        let cairo_pie = {
            let runner = cairo_run(
                include_bytes!("../../cairo_programs/fibonacci.json"),
                &Cairo0RunConfig::default(),
                &mut BuiltinHintProcessor::new_empty(),
            )
            .unwrap();
            runner.get_cairo_pie().unwrap()
        };
        // Run Cairo PIE
        let res = cairo_run_pie(
            &cairo_pie,
            &Cairo0RunConfig::default(),
            &mut BuiltinHintProcessor::new_empty(),
        );
        assert!(res.is_err_and(|err| matches!(
            err,
            CairoRunError::Runner(RunnerError::PieNStepsVsRunResourcesNStepsMismatch)
        )));
    }

    fn make_cairo_pie(program_content: &[u8]) -> CairoPie {
        let runner = cairo_run(
            program_content,
            &Cairo0RunConfig {
                layout: LayoutName::all_cairo_stwo,
                ..Default::default()
            },
            &mut BuiltinHintProcessor::new_empty(),
        )
        .unwrap();
        runner.get_cairo_pie().unwrap()
    }

    fn stwo_allowed_builtins() -> Vec<BuiltinName> {
        let mut allowed = vec![
            BuiltinName::output,
            BuiltinName::pedersen,
            BuiltinName::range_check,
            BuiltinName::bitwise,
            BuiltinName::ec_op,
            BuiltinName::poseidon,
            BuiltinName::range_check96,
        ];
        if cfg!(feature = "mod_builtin") {
            allowed.push(BuiltinName::add_mod);
            allowed.push(BuiltinName::mul_mod);
        }
        allowed
    }

    fn make_cairo_pie_stwo(program_content: &[u8]) -> CairoPie {
        let program = Program::from_bytes(program_content, Some("main")).unwrap();
        let runner = cairo_run_stwo(
            &program,
            &stwo_allowed_builtins(),
            &mut BuiltinHintProcessor::new_empty(),
            ExecutionScopes::new(),
            &StwoCairoRunConfig {
                disable_trace_padding: false,
                runner_mode: RunnerMode::ExecutionMode,
                ..Default::default()
            },
        )
        .unwrap();
        runner.get_cairo_pie().unwrap()
    }

    fn stwo_pie_config() -> StwoCairoRunConfig {
        StwoCairoRunConfig {
            disable_trace_padding: false,
            ..Default::default()
        }
    }

    #[rstest]
    #[case(include_bytes!("../../cairo_programs/fibonacci.json"))]
    #[case(include_bytes!("../../cairo_programs/integration.json"))]
    #[case(include_bytes!("../../cairo_programs/relocate_segments.json"))]
    #[case(include_bytes!("../../cairo_programs/ec_op.json"))]
    #[case(include_bytes!("../../cairo_programs/bitwise_output.json"))]
    fn get_and_run_cairo_pie_stwo(#[case] program_content: &[u8]) {
        let cairo_pie = make_cairo_pie_stwo(program_content);
        let allowed: Vec<BuiltinName> = cairo_pie
            .metadata
            .builtin_segments
            .keys()
            .copied()
            .collect();
        let mut hint_processor = BuiltinHintProcessor::new(
            Default::default(),
            RunResources::new(cairo_pie.execution_resources.n_steps),
        );
        let config = stwo_pie_config();
        let runner =
            cairo_run_pie_stwo(&cairo_pie, &allowed, &mut hint_processor, &config).unwrap();
        assert!(runner.relocated_trace.is_some());
    }

    #[rstest]
    #[case(include_bytes!("../../cairo_programs/fibonacci.json"))]
    #[case(include_bytes!("../../cairo_programs/bitwise_output.json"))]
    fn cairo_run_pie_stwo_matches_legacy(#[case] program_content: &[u8]) {
        let cairo_pie = make_cairo_pie(program_content);
        let allowed: Vec<BuiltinName> = cairo_pie
            .metadata
            .builtin_segments
            .keys()
            .copied()
            .collect();
        let legacy_runner = cairo_run_pie(
            &cairo_pie,
            &Cairo0RunConfig {
                layout: LayoutName::all_cairo_stwo,
                trace_enabled: true,
                relocate_mem: true,
                ..Default::default()
            },
            &mut BuiltinHintProcessor::new(
                Default::default(),
                RunResources::new(cairo_pie.execution_resources.n_steps),
            ),
        )
        .unwrap();
        let stwo_config = StwoCairoRunConfig {
            relocate_mem: true,
            ..stwo_pie_config()
        };
        let stwo_runner = cairo_run_pie_stwo(
            &cairo_pie,
            &allowed,
            &mut BuiltinHintProcessor::new(
                Default::default(),
                RunResources::new(cairo_pie.execution_resources.n_steps),
            ),
            &stwo_config,
        )
        .unwrap();
        assert_eq!(legacy_runner.relocated_memory, stwo_runner.relocated_memory);
        assert_eq!(legacy_runner.relocated_trace, stwo_runner.relocated_trace);
    }

    #[test]
    fn cairo_run_pie_stwo_n_steps_not_set() {
        let cairo_pie = make_cairo_pie(include_bytes!("../../cairo_programs/fibonacci.json"));
        let res = cairo_run_pie_stwo(
            &cairo_pie,
            &[],
            &mut BuiltinHintProcessor::new_empty(),
            &stwo_pie_config(),
        );
        assert!(res.is_err_and(|err| matches!(
            err,
            CairoRunError::Runner(RunnerError::PieNStepsVsRunResourcesNStepsMismatch)
        )));
    }

    #[test]
    fn cairo_run_pie_stwo_n_steps_mismatch() {
        let cairo_pie = make_cairo_pie(include_bytes!("../../cairo_programs/fibonacci.json"));
        let wrong_steps = cairo_pie.execution_resources.n_steps + 1;
        let res = cairo_run_pie_stwo(
            &cairo_pie,
            &[],
            &mut BuiltinHintProcessor::new(Default::default(), RunResources::new(wrong_steps)),
            &stwo_pie_config(),
        );
        assert!(res.is_err_and(|err| matches!(
            err,
            CairoRunError::Runner(RunnerError::PieNStepsVsRunResourcesNStepsMismatch)
        )));
    }

    #[test]
    fn cairo_run_pie_stwo_without_secure_run() {
        let cairo_pie = make_cairo_pie(include_bytes!("../../cairo_programs/fibonacci.json"));
        let allowed: Vec<BuiltinName> = cairo_pie
            .metadata
            .builtin_segments
            .keys()
            .copied()
            .collect();
        let config = StwoCairoRunConfig {
            secure_run: false,
            ..stwo_pie_config()
        };
        let runner = cairo_run_pie_stwo(
            &cairo_pie,
            &allowed,
            &mut BuiltinHintProcessor::new(
                Default::default(),
                RunResources::new(cairo_pie.execution_resources.n_steps),
            ),
            &config,
        )
        .unwrap();
        assert!(runner.relocated_trace.is_some());
    }

    /// A simple slice writer for testing BinaryWrite in no_std-like conditions.
    struct SliceWriter<'a> {
        buf: &'a mut [u8],
        pos: usize,
    }

    impl<'a> SliceWriter<'a> {
        fn new(buf: &'a mut [u8]) -> Self {
            Self { buf, pos: 0 }
        }
    }

    impl BinaryWrite for SliceWriter<'_> {
        fn write_all(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
            if self.pos + bytes.len() > self.buf.len() {
                return Err(WriteError);
            }
            self.buf[self.pos..self.pos + bytes.len()].copy_from_slice(bytes);
            self.pos += bytes.len();
            Ok(())
        }
    }

    #[test]
    fn write_binary_trace_file() {
        let program_content = include_bytes!("../../cairo_programs/struct.json");
        let expected_encoded_trace =
            include_bytes!("../../cairo_programs/trace_memory/cairo_trace_struct");

        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let mut cairo_runner = run_test_program(program_content, &mut hint_processor).unwrap();

        assert!(cairo_runner.relocate(false, true).is_ok());

        let trace_entries = cairo_runner.relocated_trace.unwrap();
        let mut buffer = [0u8; 24];
        let mut writer = SliceWriter::new(&mut buffer);
        write_encoded_trace(&trace_entries, &mut writer).unwrap();

        assert_eq!(buffer, *expected_encoded_trace);
    }

    #[test]
    fn write_binary_memory_file() {
        let program_content = include_bytes!("../../cairo_programs/struct.json");
        let expected_encoded_memory =
            include_bytes!("../../cairo_programs/trace_memory/cairo_memory_struct");

        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let mut cairo_runner = run_test_program(program_content, &mut hint_processor).unwrap();

        assert!(cairo_runner.relocate(true, true).is_ok());

        let mut buffer = [0u8; 120];
        let mut writer = SliceWriter::new(&mut buffer);
        write_encoded_memory(&cairo_runner.relocated_memory, &mut writer).unwrap();

        assert_eq!(*expected_encoded_memory, buffer);
    }

    #[test]
    fn write_encoded_trace_error_on_small_buffer() {
        let trace = vec![RelocatedTraceEntry {
            ap: 1,
            fp: 2,
            pc: 3,
        }];
        let mut buffer = [0u8; 10]; // Too small (needs 24 bytes)
        let mut writer = SliceWriter::new(&mut buffer);
        let err = write_encoded_trace(&trace, &mut writer).unwrap_err();
        assert_eq!(err.to_string(), "Failed to encode trace at position 0");
    }

    #[test]
    fn write_encoded_memory_error_on_small_buffer() {
        let memory = vec![Some(Felt252::from(1u64))];
        let mut buffer = [0u8; 10]; // Too small (needs 40 bytes: 8 + 32)
        let mut writer = SliceWriter::new(&mut buffer);
        let err = write_encoded_memory(&memory, &mut writer).unwrap_err();
        assert_eq!(err.to_string(), "Failed to encode trace at position 0");
    }

    #[test]
    fn write_encoded_trace_with_std_io_writer() {
        let trace = vec![RelocatedTraceEntry {
            ap: 1,
            fp: 2,
            pc: 3,
        }];
        let mut buf = Vec::new();
        write_encoded_trace(&trace, &mut buf).unwrap();
        assert_eq!(buf.len(), 24);
        assert_eq!(&buf[0..8], &1u64.to_le_bytes());
        assert_eq!(&buf[8..16], &2u64.to_le_bytes());
        assert_eq!(&buf[16..24], &3u64.to_le_bytes());
    }

    #[test]
    fn write_encoded_memory_with_std_io_writer() {
        let memory = vec![None, Some(Felt252::from(42u64))];
        let mut buf = Vec::new();
        write_encoded_memory(&memory, &mut buf).unwrap();
        assert_eq!(buf.len(), 40); // 8 (addr) + 32 (value)
        assert_eq!(&buf[0..8], &1u64.to_le_bytes()); // address = 1 (index of Some)
    }
}
