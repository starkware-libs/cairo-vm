use thiserror::Error;

use super::cairo_pie_errors::CairoPieValidationError;
use super::memory_errors::MemoryError;
use super::vm_exception::VmException;
use crate::types::errors::program_errors::ProgramError;
use crate::vm::errors::{
    runner_errors::RunnerError, trace_errors::TraceError, vm_errors::VirtualMachineError,
};
// In case you need to add a CairoRunError enum variant
// Add it with #[error(transparent)]
// If not it can cause some performance regressions, like in https://github.com/starkware-libs/cairo-vm/pull/1720
#[derive(Debug, Error)]
pub enum CairoRunError {
    #[error(transparent)]
    Program(#[from] ProgramError),
    #[error(transparent)]
    VirtualMachine(#[from] VirtualMachineError),
    #[error(transparent)]
    Trace(#[from] TraceError),
    #[error(transparent)]
    Runner(#[from] RunnerError),
    #[error(transparent)]
    MemoryError(#[from] MemoryError),
    #[error(transparent)]
    VmException(#[from] VmException),
    #[error(transparent)]
    CairoPieValidation(#[from] CairoPieValidationError),
}
