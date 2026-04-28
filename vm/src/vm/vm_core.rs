use crate::math_utils::signed_felt;
use crate::types::builtin_name::BuiltinName;
#[cfg(feature = "extensive_hints")]
use crate::types::program::HintRange;
use crate::vm::vm_memory::memory::MemoryCell;
use crate::{
    hint_processor::{
        builtin_hint_processor::blake2s_hash::blake2s_compress,
        hint_processor_definition::HintProcessor,
    },
    typed_operations::{typed_add, typed_div, typed_mul, typed_sub},
    types::{
        errors::math_errors::MathError,
        exec_scope::ExecutionScopes,
        instruction::{
            is_call_instruction, ApUpdate, FpUpdate, Instruction, Opcode, OpcodeExtension,
            PcUpdate, Res,
        },
        relocatable::{MaybeRelocatable, Relocatable},
    },
    vm::{
        context::run_context::RunContext,
        decoding::decoder::decode_instruction,
        errors::{
            exec_scope_errors::ExecScopeError, memory_errors::MemoryError,
            vm_errors::VirtualMachineError,
        },
        runners::builtin_runner::{
            BuiltinRunner, OutputBuiltinRunner, RangeCheckBuiltinRunner, SignatureBuiltinRunner,
        },
        trace::trace_entry::TraceEntry,
        vm_memory::memory_segments::MemorySegmentManager,
    },
};
use std::{any::Any, borrow::Cow, collections::HashMap};

use crate::Felt252;
use core::cmp::Ordering;
#[cfg(feature = "extensive_hints")]
use core::num::NonZeroUsize;
use num_traits::{ToPrimitive, Zero};

use super::errors::runner_errors::RunnerError;
use super::runners::builtin_runner::{ModBuiltinRunner, RC_N_PARTS_STANDARD};
use super::runners::cairo_pie::CairoPie;

pub const DEFAULT_MAX_TRACEBACK_ENTRIES: u32 = 20;

#[derive(PartialEq, Eq, Debug)]
pub struct Operands {
    dst: MaybeRelocatable,
    res: Option<MaybeRelocatable>,
    op0: MaybeRelocatable,
    op1: MaybeRelocatable,
}

#[derive(PartialEq, Eq, Debug)]
pub struct OperandsAddresses {
    dst_addr: Relocatable,
    op0_addr: Relocatable,
    op1_addr: Relocatable,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Hash)]
pub enum ExtendedExecutionResourceType {
    /// Describe opcode extensions that are not a builtin, yet imply added resources to the
    /// execution.
    BlakeFinalize,
    Blake,
}

impl TryFrom<OpcodeExtension> for ExtendedExecutionResourceType {
    type Error = &'static str; // Using a simple string instead of a custom struct

    fn try_from(value: OpcodeExtension) -> Result<Self, Self::Error> {
        match value {
            OpcodeExtension::Blake => Ok(ExtendedExecutionResourceType::Blake),
            OpcodeExtension::BlakeFinalize => Ok(ExtendedExecutionResourceType::BlakeFinalize),
            _ => Err("Unsupported OpcodeExtension for ExtendedExecutionResourceType"),
        }
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct DeducedOperands(u8);

impl DeducedOperands {
    fn set_dst(&mut self, value: bool) {
        self.0 |= value as u8;
    }
    fn set_op0(&mut self, value: bool) {
        self.0 |= (value as u8) << 1;
    }
    fn set_op1(&mut self, value: bool) {
        self.0 |= (value as u8) << 2;
    }

    fn was_dest_deducted(&self) -> bool {
        self.0 & 1 != 0
    }
    fn was_op0_deducted(&self) -> bool {
        self.0 & (1 << 1) != 0
    }
    fn was_op1_deducted(&self) -> bool {
        self.0 & (1 << 2) != 0
    }
}

pub struct VirtualMachineConfig {
    pub trace_enabled: bool,
    pub disable_trace_padding: bool,
    pub max_traceback_entries: u32,
}

impl Default for VirtualMachineConfig {
    fn default() -> Self {
        Self {
            trace_enabled: false,
            disable_trace_padding: false,
            max_traceback_entries: DEFAULT_MAX_TRACEBACK_ENTRIES,
        }
    }
}

pub struct VirtualMachine {
    pub(crate) run_context: RunContext,
    pub builtin_runners: Vec<BuiltinRunner>,
    /// A simulated builtin is being verified in the executed Cairo
    /// code (so proving them only involves proving cairo steps), rather
    /// than being outputted as their own segment and proven later using
    /// dedicated AIRs.
    ///
    /// These builtins won't be included in the layout, hence the builtin
    /// segment pointer won't be passed as argument to the program. The program
    /// needs a mechanism for obtaining the segment pointer. See example
    /// implementation of this mechanism in `simulated_builtins.cairo`, or
    /// cairo-lang's `simple_bootloader.cairo`.
    pub simulated_builtin_runners: Vec<BuiltinRunner>,
    pub(crate) extended_resource_counter: HashMap<ExtendedExecutionResourceType, usize>,
    pub segments: MemorySegmentManager,
    pub(crate) trace: Option<Vec<TraceEntry>>,
    pub(crate) current_step: usize,
    pub(crate) rc_limits: Option<(isize, isize)>,
    skip_instruction_execution: bool,
    run_finished: bool,
    // This flag is a parallel to the one in `struct Cairo0RunConfig`.
    pub(crate) disable_trace_padding: bool,
    instruction_cache: Vec<Option<Instruction>>,
    #[cfg(feature = "test_utils")]
    pub(crate) hooks: Option<Box<dyn crate::vm::hooks::StepHooks>>,
    pub(crate) relocation_table: Option<Vec<usize>>,
    pub(crate) max_traceback_entries: u32,
}

impl Default for VirtualMachine {
    fn default() -> Self {
        Self::new(&VirtualMachineConfig::default())
    }
}

impl VirtualMachine {
    pub fn new(config: &VirtualMachineConfig) -> VirtualMachine {
        let run_context = RunContext {
            pc: Relocatable::from((0, 0)),
            ap: 0,
            fp: 0,
        };

        let trace = if config.trace_enabled {
            Some(Vec::<TraceEntry>::new())
        } else {
            None
        };

        VirtualMachine {
            run_context,
            builtin_runners: Vec::new(),
            simulated_builtin_runners: Vec::new(),
            extended_resource_counter: HashMap::new(),
            trace,
            current_step: 0,
            skip_instruction_execution: false,
            segments: MemorySegmentManager::new(),
            rc_limits: None,
            run_finished: false,
            disable_trace_padding: config.disable_trace_padding,
            instruction_cache: Vec::new(),
            #[cfg(feature = "test_utils")]
            hooks: None,
            relocation_table: None,
            max_traceback_entries: config.max_traceback_entries,
        }
    }

    pub fn compute_segments_effective_sizes(&mut self) {
        self.segments.compute_effective_sizes();
    }

    fn update_fp(
        &mut self,
        instruction: &Instruction,
        operands: &Operands,
    ) -> Result<(), VirtualMachineError> {
        let new_fp_offset: usize = match instruction.fp_update {
            FpUpdate::APPlus2 => self.run_context.ap + 2,
            FpUpdate::Dst => match operands.dst {
                MaybeRelocatable::RelocatableValue(ref rel) => rel.offset,
                MaybeRelocatable::Int(ref num) => num
                    .to_usize()
                    .ok_or_else(|| MathError::Felt252ToUsizeConversion(Box::new(*num)))?,
            },
            FpUpdate::Regular => return Ok(()),
        };
        self.run_context.fp = new_fp_offset;
        Ok(())
    }

    fn update_ap(
        &mut self,
        instruction: &Instruction,
        operands: &Operands,
    ) -> Result<(), VirtualMachineError> {
        let new_apset: usize = match instruction.ap_update {
            ApUpdate::Add => match &operands.res {
                Some(res) => (self.run_context.get_ap() + res)?.offset,
                None => return Err(VirtualMachineError::UnconstrainedResAdd),
            },
            ApUpdate::Add1 => self.run_context.ap + 1,
            ApUpdate::Add2 => self.run_context.ap + 2,
            ApUpdate::Regular => return Ok(()),
        };
        self.run_context.ap = new_apset;
        Ok(())
    }

    fn update_pc(
        &mut self,
        instruction: &Instruction,
        operands: &Operands,
    ) -> Result<(), VirtualMachineError> {
        let new_pc: Relocatable = match instruction.pc_update {
            PcUpdate::Regular => (self.run_context.pc + instruction.size())?,
            PcUpdate::Jump => match operands.res.as_ref().and_then(|x| x.get_relocatable()) {
                Some(ref res) => *res,
                None => return Err(VirtualMachineError::UnconstrainedResJump),
            },
            PcUpdate::JumpRel => match &operands.res {
                Some(res) => match res {
                    MaybeRelocatable::Int(num_res) => (self.run_context.pc + num_res)?,
                    _ => return Err(VirtualMachineError::JumpRelNotInt),
                },
                None => return Err(VirtualMachineError::UnconstrainedResJumpRel),
            },
            PcUpdate::Jnz => match VirtualMachine::is_zero(&operands.dst) {
                true => (self.run_context.pc + instruction.size())?,
                false => (self.run_context.pc + &operands.op1)?,
            },
        };
        self.run_context.pc = new_pc;
        Ok(())
    }

    fn update_registers(
        &mut self,
        instruction: &Instruction,
        operands: Operands,
    ) -> Result<(), VirtualMachineError> {
        self.update_fp(instruction, &operands)?;
        self.update_ap(instruction, &operands)?;
        self.update_pc(instruction, &operands)?;
        Ok(())
    }

    /// Returns true if the value is zero
    /// Used for JNZ instructions
    fn is_zero(addr: &MaybeRelocatable) -> bool {
        match addr {
            MaybeRelocatable::Int(num) => num.is_zero(),
            _ => false,
        }
    }

    ///Returns a tuple (deduced_op0, deduced_res).
    ///Deduces the value of op0 if possible (based on dst and op1). Otherwise, returns None.
    ///If res was already deduced, returns its deduced value as well.
    fn deduce_op0(
        &self,
        instruction: &Instruction,
        dst: Option<&MaybeRelocatable>,
        op1: Option<&MaybeRelocatable>,
    ) -> Result<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError> {
        match instruction.opcode {
            Opcode::Call => Ok((
                Some(MaybeRelocatable::from(
                    (self.run_context.pc + instruction.size())?,
                )),
                None,
            )),
            Opcode::AssertEq => match (&instruction.res, dst, op1) {
                (Res::Add, Some(dst_addr), Some(op1_addr)) => Ok((
                    Some(typed_sub(dst_addr, op1_addr, instruction.opcode_extension)?),
                    dst.cloned(),
                )),
                (
                    Res::Mul,
                    Some(MaybeRelocatable::Int(num_dst)),
                    Some(MaybeRelocatable::Int(num_op1)),
                ) if !num_op1.is_zero() => {
                    let num_op0 = typed_div(num_dst, num_op1, instruction.opcode_extension)?;
                    Ok((Some(MaybeRelocatable::Int(num_op0)), dst.cloned()))
                }
                _ => Ok((None, None)),
            },
            _ => Ok((None, None)),
        }
    }

    /// Returns a tuple (deduced_op1, deduced_res).
    ///Deduces the value of op1 if possible (based on dst and op0). Otherwise, returns None.
    ///If res was already deduced, returns its deduced value as well.
    fn deduce_op1(
        &self,
        instruction: &Instruction,
        dst: Option<&MaybeRelocatable>,
        op0: Option<MaybeRelocatable>,
    ) -> Result<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError> {
        if let Opcode::AssertEq = instruction.opcode {
            match instruction.res {
                Res::Op1 => return Ok((dst.cloned(), dst.cloned())),
                Res::Add => {
                    return Ok((
                        dst.zip(op0).and_then(|(dst, op0)| {
                            typed_sub(dst, &op0, instruction.opcode_extension).ok()
                        }),
                        dst.cloned(),
                    ))
                }
                Res::Mul => match (dst, op0) {
                    (
                        Some(MaybeRelocatable::Int(num_dst)),
                        Some(MaybeRelocatable::Int(num_op0)),
                    ) if !num_op0.is_zero() => {
                        let num_op1 = typed_div(num_dst, &num_op0, instruction.opcode_extension)?;
                        return Ok((Some(MaybeRelocatable::Int(num_op1)), dst.cloned()));
                    }
                    _ => (),
                },
                _ => (),
            };
        };
        Ok((None, None))
    }

    fn deduce_memory_cell(
        &self,
        address: Relocatable,
    ) -> Result<Option<MaybeRelocatable>, VirtualMachineError> {
        let memory = &self.segments.memory;

        for runner in self
            .builtin_runners
            .iter()
            .chain(self.simulated_builtin_runners.iter())
        {
            if runner.base() as isize == address.segment_index {
                return runner
                    .deduce_memory_cell(address, memory)
                    .map_err(VirtualMachineError::RunnerError);
            }
        }
        Ok(None)
    }

    ///Computes the value of res if possible
    fn compute_res(
        &self,
        instruction: &Instruction,
        op0: &MaybeRelocatable,
        op1: &MaybeRelocatable,
    ) -> Result<Option<MaybeRelocatable>, VirtualMachineError> {
        match instruction.res {
            Res::Op1 => Ok(Some(op1.clone())),
            Res::Add => Ok(Some(typed_add(op0, op1, instruction.opcode_extension)?)),
            Res::Mul => Ok(Some(typed_mul(op0, op1, instruction.opcode_extension)?)),
            Res::Unconstrained => Ok(None),
        }
    }

    fn deduce_dst(
        &self,
        instruction: &Instruction,
        res: &Option<MaybeRelocatable>,
    ) -> Result<MaybeRelocatable, VirtualMachineError> {
        let dst = match (instruction.opcode, res) {
            (Opcode::AssertEq, Some(res)) => res.clone(),
            (Opcode::Call, _) => MaybeRelocatable::from(self.run_context.get_fp()),
            _ => return Err(VirtualMachineError::NoDst),
        };
        Ok(dst)
    }

    fn opcode_assertions(
        &self,
        instruction: &Instruction,
        operands: &Operands,
    ) -> Result<(), VirtualMachineError> {
        match instruction.opcode {
            Opcode::AssertEq => match &operands.res {
                None => Err(VirtualMachineError::UnconstrainedResAssertEq),
                Some(res) if res != &operands.dst => Err(VirtualMachineError::DiffAssertValues(
                    Box::new((operands.dst.clone(), res.clone())),
                )),
                _ => Ok(()),
            },
            Opcode::Call => {
                let return_pc = MaybeRelocatable::from((self.run_context.pc + instruction.size())?);
                if operands.op0 != return_pc {
                    return Err(VirtualMachineError::CantWriteReturnPc(Box::new((
                        operands.op0.clone(),
                        return_pc,
                    ))));
                };

                if MaybeRelocatable::from(self.run_context.get_fp()) != operands.dst {
                    return Err(VirtualMachineError::CantWriteReturnFp(Box::new((
                        operands.dst.clone(),
                        MaybeRelocatable::from(self.run_context.get_fp()),
                    ))));
                };
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn insert_deduced_operands(
        &mut self,
        deduced_operands: DeducedOperands,
        operands: &Operands,
        operands_addresses: &OperandsAddresses,
    ) -> Result<(), VirtualMachineError> {
        if deduced_operands.was_op0_deducted() {
            self.segments
                .memory
                .insert(operands_addresses.op0_addr, &operands.op0)
                .map_err(VirtualMachineError::Memory)?;
        }
        if deduced_operands.was_op1_deducted() {
            self.segments
                .memory
                .insert(operands_addresses.op1_addr, &operands.op1)
                .map_err(VirtualMachineError::Memory)?;
        }
        if deduced_operands.was_dest_deducted() {
            self.segments
                .memory
                .insert(operands_addresses.dst_addr, &operands.dst)
                .map_err(VirtualMachineError::Memory)?;
        }

        Ok(())
    }

    fn run_instruction(&mut self, instruction: &Instruction) -> Result<(), VirtualMachineError> {
        let (operands, operands_addresses, deduced_operands) =
            self.compute_operands(instruction)?;
        self.insert_deduced_operands(deduced_operands, &operands, &operands_addresses)?;
        self.opcode_assertions(instruction, &operands)?;

        if let Some(ref mut trace) = &mut self.trace {
            trace.push(TraceEntry {
                pc: self.run_context.pc,
                ap: self.run_context.ap,
                fp: self.run_context.fp,
            });
        }

        // Update range check limits
        const OFFSET_BITS: u32 = 16;
        let (off0, off1, off2) = (
            instruction.off0 + (1_isize << (OFFSET_BITS - 1)),
            instruction.off1 + (1_isize << (OFFSET_BITS - 1)),
            instruction.off2 + (1_isize << (OFFSET_BITS - 1)),
        );
        let (min, max) = self.rc_limits.unwrap_or((off0, off0));
        self.rc_limits = Some((
            min.min(off0).min(off1).min(off2),
            max.max(off0).max(off1).max(off2),
        ));

        self.segments
            .memory
            .mark_as_accessed(operands_addresses.dst_addr);
        self.segments
            .memory
            .mark_as_accessed(operands_addresses.op0_addr);
        self.segments
            .memory
            .mark_as_accessed(operands_addresses.op1_addr);
        self.segments.memory.mark_as_accessed(self.run_context.pc);

        if instruction.opcode_extension == OpcodeExtension::Blake
            || instruction.opcode_extension == OpcodeExtension::BlakeFinalize
        {
            self.handle_blake2s_instruction(
                &operands_addresses,
                instruction.opcode_extension == OpcodeExtension::BlakeFinalize,
            )?;

            let resource_type = <ExtendedExecutionResourceType as TryFrom<_>>::try_from(
                instruction.opcode_extension,
            )
            .expect("OpcodeExtension Blake and BlakeFinalize should always map to ExtendedExecutionResourceType");

            *self
                .extended_resource_counter
                .entry(resource_type)
                .or_insert(0) += 1;
        }

        self.update_registers(instruction, operands)?;
        self.current_step += 1;

        Ok(())
    }

    /// Executes a Blake2s or Blake2sLastBlock instruction.
    /// Expects operands to be RelocatableValue and to point to segments of memory.
    /// op0 is expected to point to a sequence of 8 u32 values (state).
    /// op1 is expected to point to a sequence of 16 u32 values (message).
    /// dst is expected hold the u32 value of the counter (t).
    /// [ap] is expected to point to a sequence of 8 cells each being either unitialised or
    /// containing the Blake2s compression output at that index.
    /// Deviation from the aforementioned expectations will result in an error.
    /// The instruction will update the memory segment pointed by [ap] with the new state.
    /// Note: the byte counter should count the number of message bytes processed so far including
    /// the current portion of the message (i.e. it starts at 64, not 0).
    fn handle_blake2s_instruction(
        &mut self,
        operands_addresses: &OperandsAddresses,
        is_last_block: bool,
    ) -> Result<(), VirtualMachineError> {
        let counter = self.segments.memory.get_u32(operands_addresses.dst_addr)?;

        let state: [u32; 8] = (self.get_u32_range(
            self.segments
                .memory
                .get_relocatable(operands_addresses.op0_addr)?,
            8,
        )?)
        .try_into()
        .map_err(|_| VirtualMachineError::Blake2sInvalidOperand(0, 8))?;

        let message: [u32; 16] = (self.get_u32_range(
            self.segments
                .memory
                .get_relocatable(operands_addresses.op1_addr)?,
            16,
        )?)
        .try_into()
        .map_err(|_| VirtualMachineError::Blake2sInvalidOperand(1, 16))?;

        let f0 = if is_last_block { 0xffffffff } else { 0 };

        let ap = self.run_context.get_ap();
        let output_address = self.segments.memory.get_relocatable(ap)?;

        let new_state = blake2s_compress(&state, &message, counter, 0, f0, 0);

        for (i, &val) in new_state.iter().enumerate() {
            self.segments.memory.insert_as_accessed(
                (output_address + i)?,
                MaybeRelocatable::Int(Felt252::from(val)),
            )?;
        }

        Ok(())
    }

    fn decode_current_instruction(&self) -> Result<Instruction, VirtualMachineError> {
        let instruction = self
            .segments
            .memory
            .get_integer(self.run_context.pc)?
            .to_u128()
            .ok_or(VirtualMachineError::InvalidInstructionEncoding)?;
        decode_instruction(instruction)
    }

    #[cfg(not(feature = "extensive_hints"))]
    pub fn step_hint(
        &mut self,
        hint_processor: &mut dyn HintProcessor,
        exec_scopes: &mut ExecutionScopes,
        hint_datas: &[Box<dyn Any>],
    ) -> Result<(), VirtualMachineError> {
        for (hint_index, hint_data) in hint_datas.iter().enumerate() {
            hint_processor
                .execute_hint(self, exec_scopes, hint_data)
                .map_err(|err| VirtualMachineError::Hint(Box::new((hint_index, err))))?
        }
        Ok(())
    }

    #[cfg(feature = "extensive_hints")]
    pub fn step_hint(
        &mut self,
        hint_processor: &mut dyn HintProcessor,
        exec_scopes: &mut ExecutionScopes,
        hint_datas: &mut Vec<Box<dyn Any>>,
        hint_ranges: &mut HashMap<Relocatable, HintRange>,
    ) -> Result<(), VirtualMachineError> {
        // Check if there is a hint range for the current pc
        if let Some((s, l)) = hint_ranges.get(&self.run_context.pc) {
            // Re-binding to avoid mutability problems
            let s = *s;
            // Execute each hint for the given range
            for idx in s..(s + l.get()) {
                let hint_extension = hint_processor
                    .execute_hint_extensive(
                        self,
                        exec_scopes,
                        hint_datas.get(idx).ok_or(VirtualMachineError::Unexpected)?,
                    )
                    .map_err(|err| VirtualMachineError::Hint(Box::new((idx - s, err))))?;
                // Update the hint_ranges & hint_datas with the hints added by the executed hint
                for (hint_pc, hints) in hint_extension {
                    if let Ok(len) = NonZeroUsize::try_from(hints.len()) {
                        hint_ranges.insert(hint_pc, (hint_datas.len(), len));
                        hint_datas.extend(hints);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn step_instruction(&mut self) -> Result<(), VirtualMachineError> {
        if self.run_context.pc.segment_index == 0 {
            // Run instructions from program segment, using instruction cache
            let pc = self.run_context.pc.offset;

            if self.segments.memory.data[0].len() <= pc {
                return Err(MemoryError::UnknownMemoryCell(Box::new((0, pc).into())))?;
            }

            let mut inst_cache = core::mem::take(&mut self.instruction_cache);
            inst_cache.resize((pc + 1).max(inst_cache.len()), None);

            let instruction = inst_cache.get_mut(pc).unwrap();
            if instruction.is_none() {
                *instruction = Some(self.decode_current_instruction()?);
            }
            let instruction = instruction.as_ref().unwrap();

            if !self.skip_instruction_execution {
                self.run_instruction(instruction)?;
            } else {
                self.run_context.pc += instruction.size();
                self.skip_instruction_execution = false;
            }
            self.instruction_cache = inst_cache;
        } else {
            // Run instructions from programs loaded in other segments, without instruction cache
            let instruction = self.decode_current_instruction()?;

            if !self.skip_instruction_execution {
                self.run_instruction(&instruction)?;
            } else {
                self.run_context.pc += instruction.size();
                self.skip_instruction_execution = false;
            }
        }
        Ok(())
    }

    pub fn step(
        &mut self,
        hint_processor: &mut dyn HintProcessor,
        exec_scopes: &mut ExecutionScopes,
        #[cfg(feature = "extensive_hints")] hint_datas: &mut Vec<Box<dyn Any>>,
        #[cfg(not(feature = "extensive_hints"))] hint_datas: &[Box<dyn Any>],
        #[cfg(feature = "extensive_hints")] hint_ranges: &mut HashMap<Relocatable, HintRange>,
        #[cfg(feature = "test_utils")] constants: &HashMap<String, Felt252>,
    ) -> Result<(), VirtualMachineError> {
        self.step_hint(
            hint_processor,
            exec_scopes,
            hint_datas,
            #[cfg(feature = "extensive_hints")]
            hint_ranges,
        )?;

        #[cfg(feature = "test_utils")]
        self.execute_pre_step_instruction(hint_processor, exec_scopes, hint_datas, constants)?;
        self.step_instruction()?;
        #[cfg(feature = "test_utils")]
        self.execute_post_step_instruction(hint_processor, exec_scopes, hint_datas, constants)?;

        Ok(())
    }

    fn compute_op0_deductions(
        &self,
        op0_addr: Relocatable,
        res: &mut Option<MaybeRelocatable>,
        instruction: &Instruction,
        dst_op: &Option<MaybeRelocatable>,
        op1_op: &Option<MaybeRelocatable>,
    ) -> Result<MaybeRelocatable, VirtualMachineError> {
        let op0_op = match self.deduce_memory_cell(op0_addr)? {
            None => {
                let op0;
                (op0, *res) = self.deduce_op0(instruction, dst_op.as_ref(), op1_op.as_ref())?;
                op0
            }
            deduced_memory_cell => deduced_memory_cell,
        };
        let op0 = op0_op.ok_or_else(|| {
            VirtualMachineError::FailedToComputeOperands(Box::new(("op0".to_string(), op0_addr)))
        })?;
        Ok(op0)
    }

    fn compute_op1_deductions(
        &self,
        op1_addr: Relocatable,
        res: &mut Option<MaybeRelocatable>,
        instruction: &Instruction,
        dst_op: &Option<MaybeRelocatable>,
        op0: &MaybeRelocatable,
    ) -> Result<MaybeRelocatable, VirtualMachineError> {
        let op1_op = match self.deduce_memory_cell(op1_addr)? {
            None => {
                let (op1, deduced_res) =
                    self.deduce_op1(instruction, dst_op.as_ref(), Some(op0.clone()))?;
                if res.is_none() {
                    *res = deduced_res
                }
                op1
            }
            deduced_memory_cell => deduced_memory_cell,
        };
        let op1 = op1_op.ok_or_else(|| {
            VirtualMachineError::FailedToComputeOperands(Box::new(("op1".to_string(), op1_addr)))
        })?;
        Ok(op1)
    }

    /// Compute operands and result, trying to deduce them if normal memory access returns a None
    /// value.
    pub fn compute_operands(
        &self,
        instruction: &Instruction,
    ) -> Result<(Operands, OperandsAddresses, DeducedOperands), VirtualMachineError> {
        //Get operands from memory
        let dst_addr = self.run_context.compute_dst_addr(instruction)?;
        let dst_op = self.segments.memory.get(&dst_addr).map(Cow::into_owned);

        let op0_addr = self.run_context.compute_op0_addr(instruction)?;
        let op0_op = self.segments.memory.get(&op0_addr).map(Cow::into_owned);

        let op1_addr = self
            .run_context
            .compute_op1_addr(instruction, op0_op.as_ref())?;
        let op1_op = self.segments.memory.get(&op1_addr).map(Cow::into_owned);

        let mut res: Option<MaybeRelocatable> = None;

        let mut deduced_operands = DeducedOperands::default();

        //Deduce op0 if it wasnt previously computed
        let op0 = match op0_op {
            Some(op0) => op0,
            None => {
                deduced_operands.set_op0(true);
                self.compute_op0_deductions(op0_addr, &mut res, instruction, &dst_op, &op1_op)?
            }
        };

        //Deduce op1 if it wasnt previously computed
        let op1 = match op1_op {
            Some(op1) => op1,
            None => {
                deduced_operands.set_op1(true);
                self.compute_op1_deductions(op1_addr, &mut res, instruction, &dst_op, &op0)?
            }
        };

        //Compute res if it wasnt previously deduced
        if res.is_none() {
            res = self.compute_res(instruction, &op0, &op1)?;
        }

        //Deduce dst if it wasnt previously computed
        let dst = match dst_op {
            Some(dst) => dst,
            None => {
                deduced_operands.set_dst(true);
                self.deduce_dst(instruction, &res)?
            }
        };
        let accessed_addresses = OperandsAddresses {
            dst_addr,
            op0_addr,
            op1_addr,
        };
        Ok((
            Operands { dst, op0, op1, res },
            accessed_addresses,
            deduced_operands,
        ))
    }

    /// Updates the memory with missing built-in deductions and verifies the existing ones.
    pub fn complete_builtin_auto_deductions(&mut self) -> Result<(), VirtualMachineError> {
        for builtin in self.builtin_runners.iter() {
            let builtin_index: usize = builtin.base();

            // Output and SegmentArena do not need to be auto-deduced in the memory.
            if matches!(
                builtin,
                BuiltinRunner::Output(_) | BuiltinRunner::SegmentArena(_)
            ) {
                continue;
            }

            // Extend the segment size to a multiple of the number of cells per instance.
            let current_builtin_segment = &mut self.segments.memory.data[builtin_index];
            let len = current_builtin_segment.len();
            let cells_per_instance = builtin.cells_per_instance() as usize;
            current_builtin_segment.resize(
                len.div_ceil(cells_per_instance) * cells_per_instance,
                MemoryCell::NONE,
            );

            let mut missing_values: Vec<(usize, MemoryCell)> = vec![];

            for (offset, cell) in self.segments.memory.data[builtin_index].iter().enumerate() {
                if let Some(deduced_memory_cell) = builtin
                    .deduce_memory_cell(
                        Relocatable::from((builtin_index as isize, offset)),
                        &self.segments.memory,
                    )
                    .map_err(VirtualMachineError::RunnerError)?
                {
                    match cell.get_value() {
                        Some(memory_value) => {
                            // Checks that the value in the memory is correct.
                            if memory_value != deduced_memory_cell {
                                return Err(VirtualMachineError::InconsistentAutoDeduction(
                                    Box::new((
                                        builtin.name(),
                                        deduced_memory_cell,
                                        Some(memory_value),
                                    )),
                                ));
                            }
                        }
                        None => {
                            // Collect value to be stored in memory.
                            missing_values.push((offset, MemoryCell::new(deduced_memory_cell)));
                        }
                    }
                }
            }

            for (offset, value) in missing_values {
                self.segments.memory.data[builtin_index][offset] = value;
            }
        }
        Ok(())
    }

    ///Makes sure that all assigned memory cells are consistent with their auto deduction rules.
    pub fn verify_auto_deductions(&self) -> Result<(), VirtualMachineError> {
        for builtin in self.builtin_runners.iter() {
            let index: usize = builtin.base();
            for (offset, value) in self.segments.memory.data[index].iter().enumerate() {
                if let Some(deduced_memory_cell) = builtin
                    .deduce_memory_cell(
                        Relocatable::from((index as isize, offset)),
                        &self.segments.memory,
                    )
                    .map_err(VirtualMachineError::RunnerError)?
                {
                    let value = value.get_value();
                    if Some(&deduced_memory_cell) != value.as_ref() && value.is_some() {
                        return Err(VirtualMachineError::InconsistentAutoDeduction(Box::new((
                            builtin.name(),
                            deduced_memory_cell,
                            value,
                        ))));
                    }
                }
            }
        }
        Ok(())
    }

    //Makes sure that the value at the given address is consistent with the auto deduction rules.
    pub fn verify_auto_deductions_for_addr(
        &self,
        addr: Relocatable,
        builtin: &BuiltinRunner,
    ) -> Result<(), VirtualMachineError> {
        let value = match builtin.deduce_memory_cell(addr, &self.segments.memory)? {
            Some(value) => value,
            None => return Ok(()),
        };
        let current_value = match self.segments.memory.get(&addr) {
            Some(value) => value.into_owned(),
            None => return Ok(()),
        };
        if value != current_value {
            return Err(VirtualMachineError::InconsistentAutoDeduction(Box::new((
                builtin.name(),
                value,
                Some(current_value),
            ))));
        }
        Ok(())
    }

    pub fn end_run(
        &mut self,
        exec_scopes: &ExecutionScopes,
        fill_holes: bool,
    ) -> Result<(), VirtualMachineError> {
        if fill_holes {
            self.complete_builtin_auto_deductions()?;
        } else {
            self.verify_auto_deductions()?;
        }
        self.run_finished = true;
        match exec_scopes.data.len() {
            1 => Ok(()),
            _ => Err(ExecScopeError::NoScopeError.into()),
        }
    }

    pub fn mark_address_range_as_accessed(
        &mut self,
        base: Relocatable,
        len: usize,
    ) -> Result<(), VirtualMachineError> {
        if !self.run_finished {
            return Err(VirtualMachineError::RunNotFinished);
        }
        for i in 0..len {
            self.segments.memory.mark_as_accessed((base + i)?);
        }
        Ok(())
    }

    /// Returns the values (fp, pc) corresponding to each call instruction in the traceback.
    /// Returns the most recent call last.
    pub fn get_traceback_entries(&self) -> Vec<(Relocatable, Relocatable)> {
        let mut entries = Vec::<(Relocatable, Relocatable)>::new();
        let mut fp = Relocatable::from((1, self.run_context.fp));
        // Fetch the fp and pc traceback entries
        for _ in 0..self.max_traceback_entries {
            // Get return pc
            let ret_pc = match (fp - 1)
                .ok()
                .map(|r| self.segments.memory.get_relocatable(r))
            {
                Some(Ok(opt_pc)) => opt_pc,
                _ => break,
            };
            // Get fp traceback
            match (fp - 2)
                .ok()
                .map(|r| self.segments.memory.get_relocatable(r))
            {
                Some(Ok(opt_fp)) if opt_fp != fp => fp = opt_fp,
                _ => break,
            }
            // Try to check if the call instruction is (instruction0, instruction1) or just
            // instruction1 (with no immediate).
            let call_pc = match (ret_pc - 1)
                .ok()
                .map(|r| self.segments.memory.get_integer(r))
            {
                Some(Ok(instruction1)) => {
                    match is_call_instruction(&instruction1) {
                        true => (ret_pc - 1).unwrap(), // This unwrap wont fail as it is checked before
                        false => {
                            match (ret_pc - 2)
                                .ok()
                                .map(|r| self.segments.memory.get_integer(r))
                            {
                                Some(Ok(instruction0)) => {
                                    match is_call_instruction(&instruction0) {
                                        true => (ret_pc - 2).unwrap(), // This unwrap wont fail as it is checked before
                                        false => break,
                                    }
                                }
                                _ => break,
                            }
                        }
                    }
                }
                _ => break,
            };
            // Append traceback entries
            entries.push((fp, call_pc))
        }
        entries.reverse();
        entries
    }

    ///Adds a new segment and to the memory and returns its starting location as a Relocatable value.
    pub fn add_memory_segment(&mut self) -> Relocatable {
        self.segments.add()
    }

    pub fn get_ap(&self) -> Relocatable {
        self.run_context.get_ap()
    }

    pub fn get_fp(&self) -> Relocatable {
        self.run_context.get_fp()
    }

    pub fn get_pc(&self) -> Relocatable {
        self.run_context.get_pc()
    }

    pub fn get_current_step(&self) -> usize {
        self.current_step
    }

    ///Gets the integer value corresponding to the Relocatable address
    pub fn get_integer(&'_ self, key: Relocatable) -> Result<Cow<'_, Felt252>, MemoryError> {
        self.segments.memory.get_integer(key)
    }

    ///Gets the relocatable value corresponding to the Relocatable address
    pub fn get_relocatable(&self, key: Relocatable) -> Result<Relocatable, MemoryError> {
        self.segments.memory.get_relocatable(key)
    }

    ///Gets a MaybeRelocatable value from memory indicated by a generic address
    pub fn get_maybe<'a, 'b: 'a, K: 'a>(&'b self, key: &'a K) -> Option<MaybeRelocatable>
    where
        Relocatable: TryFrom<&'a K>,
    {
        self.segments.memory.get(key).map(|x| x.into_owned())
    }

    /// Returns a reference to the vector with all builtins present in the virtual machine
    pub fn get_builtin_runners(&self) -> &Vec<BuiltinRunner> {
        &self.builtin_runners
    }

    /// Returns a mutable reference to the vector with all builtins present in the virtual machine
    pub fn get_builtin_runners_as_mut(&mut self) -> &mut Vec<BuiltinRunner> {
        &mut self.builtin_runners
    }

    /// Returns a mutable iterator over all builtin runners used. That is, both builtin_runners and
    /// simulated_builtin_runners.
    pub fn get_all_builtin_runners_as_mut_iter(
        &mut self,
    ) -> impl Iterator<Item = &mut BuiltinRunner> {
        self.builtin_runners
            .iter_mut()
            .chain(self.simulated_builtin_runners.iter_mut())
    }

    ///Inserts a value into a memory address given by a Relocatable value
    pub fn insert_value<T: Into<MaybeRelocatable>>(
        &mut self,
        key: Relocatable,
        val: T,
    ) -> Result<(), MemoryError> {
        self.segments.memory.insert_value(key, val)
    }

    /// Removes (unsets) a value from a memory cell that was not accessed by the VM.
    ///
    /// This function can be used to implement lazy opening of merkelized contracts. The full
    /// program is initially loaded into memory via a hint. After execution, any entry points to
    /// contract segments that were not accessed are replaced with an invalid opcode.
    ///
    /// [Use case](https://github.com/starkware-libs/cairo-lang/blob/master/src/starkware/starknet/core/os/contract_class/compiled_class.cairo#L244-L253)
    pub fn delete_unaccessed(&mut self, addr: Relocatable) -> Result<(), MemoryError> {
        self.segments.memory.delete_unaccessed(addr)
    }

    ///Writes data into the memory from address ptr and returns the first address after the data.
    pub fn load_data(
        &mut self,
        ptr: Relocatable,
        data: &[MaybeRelocatable],
    ) -> Result<Relocatable, MemoryError> {
        if ptr.segment_index == 0 {
            self.instruction_cache.resize(data.len(), None);
        }
        self.segments.load_data(ptr, data)
    }

    /// Writes args into the memory from address ptr and returns the first address after the data.
    pub fn write_arg(
        &mut self,
        ptr: Relocatable,
        arg: &dyn Any,
    ) -> Result<MaybeRelocatable, MemoryError> {
        self.segments.write_arg(ptr, arg)
    }

    pub fn memcmp(&self, lhs: Relocatable, rhs: Relocatable, len: usize) -> (Ordering, usize) {
        self.segments.memory.memcmp(lhs, rhs, len)
    }

    pub fn mem_eq(&self, lhs: Relocatable, rhs: Relocatable, len: usize) -> bool {
        self.segments.memory.mem_eq(lhs, rhs, len)
    }

    pub fn is_accessed(&self, addr: &Relocatable) -> Result<bool, MemoryError> {
        self.segments.is_accessed(addr)
    }

    ///Gets `n_ret` return values from memory
    pub fn get_return_values(&self, n_ret: usize) -> Result<Vec<MaybeRelocatable>, MemoryError> {
        let addr = (self.run_context.get_ap() - n_ret)
            .map_err(|_| MemoryError::FailedToGetReturnValues(Box::new((n_ret, self.get_ap()))))?;
        self.segments.memory.get_continuous_range(addr, n_ret)
    }

    ///Gets n elements from memory starting from addr (n being size)
    pub fn get_range(
        &'_ self,
        addr: Relocatable,
        size: usize,
    ) -> Vec<Option<Cow<'_, MaybeRelocatable>>> {
        self.segments.memory.get_range(addr, size)
    }

    ///Gets n elements from memory starting from addr (n being size)
    pub fn get_continuous_range(
        &self,
        addr: Relocatable,
        size: usize,
    ) -> Result<Vec<MaybeRelocatable>, MemoryError> {
        self.segments.memory.get_continuous_range(addr, size)
    }

    ///Gets n integer values from memory starting from addr (n being size),
    pub fn get_integer_range(
        &'_ self,
        addr: Relocatable,
        size: usize,
    ) -> Result<Vec<Cow<'_, Felt252>>, MemoryError> {
        self.segments.memory.get_integer_range(addr, size)
    }

    /// Gets n u32 values from memory starting from addr (n being size).
    /// Returns an error if any of the values inside the range is missing (memory gap) or is not a u32.
    pub fn get_u32_range(&self, addr: Relocatable, size: usize) -> Result<Vec<u32>, MemoryError> {
        self.segments.memory.get_u32_range(addr, size)
    }

    pub fn get_range_check_builtin(
        &self,
    ) -> Result<&RangeCheckBuiltinRunner<RC_N_PARTS_STANDARD>, VirtualMachineError> {
        for builtin in &self.builtin_runners {
            if let BuiltinRunner::RangeCheck(range_check_builtin) = builtin {
                return Ok(range_check_builtin);
            };
        }
        Err(VirtualMachineError::NoRangeCheckBuiltin)
    }

    pub fn get_signature_builtin(
        &mut self,
    ) -> Result<&mut SignatureBuiltinRunner, VirtualMachineError> {
        for builtin in self.get_all_builtin_runners_as_mut_iter() {
            if let BuiltinRunner::Signature(signature_builtin) = builtin {
                return Ok(signature_builtin);
            };
        }

        Err(VirtualMachineError::NoSignatureBuiltin)
    }

    pub fn get_output_builtin_mut(
        &mut self,
    ) -> Result<&mut OutputBuiltinRunner, VirtualMachineError> {
        for builtin in self.get_builtin_runners_as_mut() {
            if let BuiltinRunner::Output(output_builtin) = builtin {
                return Ok(output_builtin);
            };
        }

        Err(VirtualMachineError::NoOutputBuiltin)
    }

    #[cfg(feature = "tracer")]
    pub fn relocate_segments(&self) -> Result<Vec<usize>, MemoryError> {
        self.segments.relocate_segments()
    }

    #[doc(hidden)]
    pub fn skip_next_instruction_execution(&mut self) {
        self.skip_instruction_execution = true;
    }

    #[doc(hidden)]
    pub fn set_ap(&mut self, ap: usize) {
        self.run_context.set_ap(ap)
    }

    #[doc(hidden)]
    pub fn set_fp(&mut self, fp: usize) {
        self.run_context.set_fp(fp)
    }

    #[doc(hidden)]
    pub fn set_pc(&mut self, pc: Relocatable) {
        self.run_context.set_pc(pc)
    }

    pub fn get_segment_used_size(&self, index: usize) -> Option<usize> {
        self.segments.get_segment_used_size(index)
    }

    pub fn get_segment_size(&self, index: usize) -> Option<usize> {
        self.segments.get_segment_size(index)
    }

    pub fn add_temporary_segment(&mut self) -> Relocatable {
        self.segments.add_temporary_segment()
    }

    /// Add a new relocation rule.
    ///
    /// When using feature "extensive_hints" the destination is allowed to be an Integer (via
    /// MaybeRelocatable). Relocating memory to anything other than a `Relocatable` is generally
    /// not useful, but it does make the implementation consistent with the pythonic version.
    ///
    /// Will return an error if any of the following conditions are not met:
    ///   - Source address's segment must be negative (temporary).
    ///   - Source address's offset must be zero.
    ///   - There shouldn't already be relocation at the source segment.
    pub fn add_relocation_rule(
        &mut self,
        src_ptr: Relocatable,
        #[cfg(not(feature = "extensive_hints"))] dst_ptr: Relocatable,
        #[cfg(feature = "extensive_hints")] dst_ptr: MaybeRelocatable,
    ) -> Result<(), MemoryError> {
        self.segments.memory.add_relocation_rule(src_ptr, dst_ptr)
    }

    pub fn gen_arg(&mut self, arg: &dyn Any) -> Result<MaybeRelocatable, MemoryError> {
        self.segments.gen_arg(arg)
    }

    /// Write the values hosted in the output builtin's segment.
    /// Does nothing if the output builtin is not present in the program.
    pub fn write_output(
        &mut self,
        writer: &mut impl core::fmt::Write,
    ) -> Result<(), VirtualMachineError> {
        let builtin = match self
            .builtin_runners
            .iter()
            .find(|b| b.name() == BuiltinName::output)
        {
            Some(x) => x,
            _ => return Ok(()),
        };

        let segment_used_sizes = self.segments.compute_effective_sizes();
        let segment_index = builtin.base();
        for i in 0..segment_used_sizes[segment_index] {
            let formatted_value = match self
                .segments
                .memory
                .get(&Relocatable::from((segment_index as isize, i)))
            {
                Some(val) => match val.as_ref() {
                    MaybeRelocatable::Int(num) => format!("{}", signed_felt(*num)),
                    MaybeRelocatable::RelocatableValue(rel) => format!("{}", rel),
                },
                _ => "<missing>".to_string(),
            };
            writeln!(writer, "{formatted_value}")
                .map_err(|_| VirtualMachineError::FailedToWriteOutput)?;
        }

        Ok(())
    }

    /// Returns a list of addresses of memory cells that constitute the public memory.
    pub fn get_public_memory_addresses(&self) -> Result<Vec<(usize, usize)>, VirtualMachineError> {
        if let Some(relocation_table) = &self.relocation_table {
            self.segments
                .get_public_memory_addresses(relocation_table)
                .map_err(VirtualMachineError::Memory)
        } else {
            Err(MemoryError::UnrelocatedMemory.into())
        }
    }

    #[doc(hidden)]
    pub fn builtins_final_stack_from_stack_pointer_dict(
        &mut self,
        builtin_name_to_stack_pointer: &HashMap<BuiltinName, Relocatable>,
        skip_output: bool,
    ) -> Result<(), RunnerError> {
        for builtin in self.builtin_runners.iter_mut() {
            if matches!(builtin, BuiltinRunner::Output(_)) && skip_output {
                continue;
            }
            builtin.final_stack(
                &self.segments,
                builtin_name_to_stack_pointer
                    .get(&builtin.name())
                    .cloned()
                    .unwrap_or_default(),
            )?;
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn set_output_stop_ptr_offset(&mut self, offset: usize) {
        if let Some(BuiltinRunner::Output(builtin)) = self.builtin_runners.first_mut() {
            builtin.set_stop_ptr_offset(offset);
            if let Some(segment_used_sizes) = &mut self.segments.segment_used_sizes {
                segment_used_sizes[builtin.base()] = offset;
            }
        }
    }

    /// Fetches add_mod & mul_mod builtins according to the optional arguments and executes `fill_memory`
    /// Returns an error if either of this optional parameters is true but the corresponding builtin is not present
    /// Verifies that both builtin's (if present) batch sizes match the batch_size arg if set
    // This method is needed as running `fill_memory` direclty from outside the vm struct would require cloning the builtin runners to avoid double borrowing
    pub fn mod_builtin_fill_memory(
        &mut self,
        add_mod_ptr_n: Option<(Relocatable, usize)>,
        mul_mod_ptr_n: Option<(Relocatable, usize)>,
        batch_size: Option<usize>,
    ) -> Result<(), VirtualMachineError> {
        let fetch_builtin_params = |mod_params: Option<(Relocatable, usize)>,
                                    mod_name: BuiltinName|
         -> Result<
            Option<(Relocatable, &ModBuiltinRunner, usize)>,
            VirtualMachineError,
        > {
            if let Some((ptr, n)) = mod_params {
                let mod_builtin = self
                    .builtin_runners
                    .iter()
                    .find_map(|b| match b {
                        BuiltinRunner::Mod(b) if b.name() == mod_name => Some(b),
                        _ => None,
                    })
                    .ok_or_else(|| VirtualMachineError::NoModBuiltin(mod_name))?;
                if let Some(batch_size) = batch_size {
                    if mod_builtin.batch_size() != batch_size {
                        return Err(VirtualMachineError::ModBuiltinBatchSize(Box::new((
                            mod_builtin.name(),
                            batch_size,
                        ))));
                    }
                }
                Ok(Some((ptr, mod_builtin, n)))
            } else {
                Ok(None)
            }
        };

        ModBuiltinRunner::fill_memory(
            &mut self.segments.memory,
            fetch_builtin_params(add_mod_ptr_n, BuiltinName::add_mod)?,
            fetch_builtin_params(mul_mod_ptr_n, BuiltinName::mul_mod)?,
        )
        .map_err(VirtualMachineError::RunnerError)
    }

    pub(crate) fn finalize_segments_by_cairo_pie(&mut self, pie: &CairoPie) {
        let mut segment_infos = vec![
            &pie.metadata.program_segment,
            &pie.metadata.execution_segment,
            &pie.metadata.ret_fp_segment,
            &pie.metadata.ret_pc_segment,
        ];
        segment_infos.extend(pie.metadata.builtin_segments.values());
        segment_infos.extend(pie.metadata.extra_segments.iter());
        for info in segment_infos {
            self.segments
                .finalize(Some(info.size), info.index as usize, None)
        }
    }
}

pub struct VirtualMachineBuilder {
    pub(crate) run_context: RunContext,
    pub(crate) builtin_runners: Vec<BuiltinRunner>,
    pub(crate) segments: MemorySegmentManager,
    pub(crate) trace: Option<Vec<TraceEntry>>,
    pub(crate) current_step: usize,
    skip_instruction_execution: bool,
    run_finished: bool,
    #[cfg(feature = "test_utils")]
    pub(crate) hooks: Option<Box<dyn crate::vm::hooks::StepHooks>>,
    pub(crate) max_traceback_entries: u32,
}

impl Default for VirtualMachineBuilder {
    fn default() -> Self {
        let run_context = RunContext {
            pc: Relocatable::from((0, 0)),
            ap: 0,
            fp: 0,
        };

        VirtualMachineBuilder {
            run_context,
            builtin_runners: Vec::new(),
            trace: None,
            current_step: 0,
            skip_instruction_execution: false,
            segments: MemorySegmentManager::new(),
            run_finished: false,
            #[cfg(feature = "test_utils")]
            hooks: None,
            max_traceback_entries: DEFAULT_MAX_TRACEBACK_ENTRIES,
        }
    }
}

impl VirtualMachineBuilder {
    pub fn run_context(mut self, run_context: RunContext) -> VirtualMachineBuilder {
        self.run_context = run_context;
        self
    }

    pub fn builtin_runners(mut self, builtin_runners: Vec<BuiltinRunner>) -> VirtualMachineBuilder {
        self.builtin_runners = builtin_runners;
        self
    }

    pub fn segments(mut self, segments: MemorySegmentManager) -> VirtualMachineBuilder {
        self.segments = segments;
        self
    }

    pub fn trace(mut self, trace: Option<Vec<TraceEntry>>) -> VirtualMachineBuilder {
        self.trace = trace;
        self
    }

    pub fn current_step(mut self, current_step: usize) -> VirtualMachineBuilder {
        self.current_step = current_step;
        self
    }

    pub fn skip_instruction_execution(
        mut self,
        skip_instruction_execution: bool,
    ) -> VirtualMachineBuilder {
        self.skip_instruction_execution = skip_instruction_execution;
        self
    }

    pub fn run_finished(mut self, run_finished: bool) -> VirtualMachineBuilder {
        self.run_finished = run_finished;
        self
    }

    #[cfg(feature = "test_utils")]
    pub fn hooks(mut self, hooks: Box<dyn crate::vm::hooks::StepHooks>) -> VirtualMachineBuilder {
        self.hooks = Some(hooks);
        self
    }

    pub fn max_traceback_entries(mut self, max_traceback_entries: u32) -> VirtualMachineBuilder {
        self.max_traceback_entries = max_traceback_entries;
        self
    }

    pub fn build(self) -> VirtualMachine {
        VirtualMachine {
            run_context: self.run_context,
            builtin_runners: self.builtin_runners,
            simulated_builtin_runners: Vec::new(),
            extended_resource_counter: HashMap::new(),
            trace: self.trace,
            current_step: self.current_step,
            skip_instruction_execution: self.skip_instruction_execution,
            segments: self.segments,
            rc_limits: None,
            run_finished: self.run_finished,
            instruction_cache: Vec::new(),
            #[cfg(feature = "test_utils")]
            hooks: self.hooks,
            relocation_table: None,
            disable_trace_padding: false,
            max_traceback_entries: self.max_traceback_entries,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::felt_hex;
    use crate::math_utils::STWO_PRIME;
    use crate::types::instruction::OpcodeExtension;
    use crate::types::layout_name::LayoutName;
    use crate::types::program::Program;
    use crate::{
        any_box,
        hint_processor::builtin_hint_processor::builtin_hint_processor_definition::{
            BuiltinHintProcessor, HintProcessorData,
        },
        relocatable,
        types::{
            instruction::{Op1Addr, Register},
            relocatable::Relocatable,
        },
        utils::test_utils::*,
        vm::{
            errors::memory_errors::MemoryError,
            runners::builtin_runner::{BitwiseBuiltinRunner, EcOpBuiltinRunner, HashBuiltinRunner},
        },
    };
    use assert_matches::assert_matches;
    use std::collections::HashMap;

    use starknet_types_core::qm31::QM31;

    #[test]
    fn update_fp_ap_plus2() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::APPlus2,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8))),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();
        run_context!(vm, 4, 5, 6);
        assert_matches!(
            vm.update_fp(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.fp, 7)
    }

    #[test]
    fn update_fp_dst() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Dst,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: mayberelocatable!(1, 6),
            res: Some(mayberelocatable!(8)),
            op0: mayberelocatable!(9),
            op1: mayberelocatable!(10),
        };

        let mut vm = vm!();

        assert_matches!(
            vm.update_fp(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.fp, 6)
    }

    #[test]
    fn update_fp_regular() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11_u64)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8_u64))),
            op0: MaybeRelocatable::Int(Felt252::from(9_u64)),
            op1: MaybeRelocatable::Int(Felt252::from(10_u64)),
        };

        let mut vm = vm!();

        assert_matches!(
            vm.update_fp(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.fp, 0)
    }

    #[test]
    fn update_fp_dst_num() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Dst,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8))),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();
        run_context!(vm, 4, 5, 6);

        assert_matches!(
            vm.update_fp(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.fp, 11)
    }

    #[test]
    fn update_ap_add_with_res() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Add,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8))),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = VirtualMachine::default();
        vm.run_context.pc = Relocatable::from((0, 4));
        vm.run_context.ap = 5;
        vm.run_context.fp = 6;

        assert_matches!(
            vm.update_ap(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.ap, 13);
    }

    #[test]
    fn update_ap_add_without_res() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Add,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: None,
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();
        vm.run_context.pc = Relocatable::from((0, 4));
        vm.run_context.ap = 5;
        vm.run_context.fp = 6;

        assert_matches!(
            vm.update_ap(&instruction, &operands),
            Err(VirtualMachineError::UnconstrainedResAdd)
        );
    }

    #[test]
    fn update_ap_add1() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Add1,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8))),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();
        vm.run_context.pc = Relocatable::from((0, 4));
        vm.run_context.ap = 5;
        vm.run_context.fp = 6;

        assert_matches!(
            vm.update_ap(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.ap, 6);
    }

    #[test]
    fn update_ap_add2() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Add2,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11_u64)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8_u64))),
            op0: MaybeRelocatable::Int(Felt252::from(9_u64)),
            op1: MaybeRelocatable::Int(Felt252::from(10_u64)),
        };

        let mut vm = vm!();
        vm.run_context.pc = Relocatable::from((0, 4));
        vm.run_context.ap = 5;
        vm.run_context.fp = 6;

        assert_matches!(
            vm.update_ap(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.ap, 7);
    }

    #[test]
    fn update_ap_regular() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8))),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();
        vm.run_context.pc = Relocatable::from((0, 4));
        vm.run_context.ap = 5;
        vm.run_context.fp = 6;

        assert_matches!(
            vm.update_ap(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.ap, 5);
    }

    #[test]
    fn update_pc_regular_instruction_no_imm() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8))),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();

        assert_matches!(
            vm.update_pc(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.pc, Relocatable::from((0, 1)));
    }

    #[test]
    fn update_pc_regular_instruction_has_imm() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::Imm,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8))),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();

        assert_matches!(
            vm.update_pc(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.pc, Relocatable::from((0, 2)));
    }

    #[test]
    fn update_pc_jump_with_res() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: mayberelocatable!(1, 11),
            res: Some(mayberelocatable!(0, 8)),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();

        assert_matches!(
            vm.update_pc(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.pc, Relocatable::from((0, 8)));
    }

    #[test]
    fn update_pc_jump_without_res() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: None,
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();
        vm.run_context.pc = Relocatable::from((0, 4));
        vm.run_context.ap = 5;
        vm.run_context.fp = 6;

        assert_matches!(
            vm.update_pc(&instruction, &operands),
            Err(VirtualMachineError::UnconstrainedResJump)
        );
    }

    #[test]
    fn update_pc_jump_rel_with_int_res() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::JumpRel,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8))),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();
        run_context!(vm, 1, 1, 1);

        assert_matches!(
            vm.update_pc(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.pc, Relocatable::from((0, 9)));
    }

    #[test]
    fn update_pc_jump_rel_without_res() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::JumpRel,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: None,
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();

        assert_matches!(
            vm.update_pc(&instruction, &operands),
            Err(VirtualMachineError::UnconstrainedResJumpRel)
        );
    }

    #[test]
    fn update_pc_jump_rel_with_non_int_res() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::JumpRel,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: Some(MaybeRelocatable::from((1, 4))),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();
        assert_matches!(
            vm.update_pc(&instruction, &operands),
            Err::<(), VirtualMachineError>(VirtualMachineError::JumpRelNotInt)
        );
    }

    #[test]
    fn update_pc_jnz_dst_is_zero() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Jnz,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(0)),
            res: Some(MaybeRelocatable::Int(Felt252::from(0))),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();

        assert_matches!(
            vm.update_pc(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.pc, Relocatable::from((0, 1)));
    }

    #[test]
    fn update_pc_jnz_dst_is_not_zero() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Jnz,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8))),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();

        assert_matches!(
            vm.update_pc(&instruction, &operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.pc, Relocatable::from((0, 10)));
    }

    #[test]
    fn update_registers_all_regular() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(11)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8))),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();
        vm.run_context.pc = Relocatable::from((0, 4));
        vm.run_context.ap = 5;
        vm.run_context.fp = 6;

        assert_matches!(
            vm.update_registers(&instruction, operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.pc, Relocatable::from((0, 5)));
        assert_eq!(vm.run_context.ap, 5);
        assert_eq!(vm.run_context.fp, 6);
    }

    #[test]
    fn update_registers_mixed_types() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::JumpRel,
            ap_update: ApUpdate::Add2,
            fp_update: FpUpdate::Dst,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::from((1, 11)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8))),
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let mut vm = vm!();
        run_context!(vm, 4, 5, 6);

        assert_matches!(
            vm.update_registers(&instruction, operands),
            Ok::<(), VirtualMachineError>(())
        );
        assert_eq!(vm.run_context.pc, Relocatable::from((0, 12)));
        assert_eq!(vm.run_context.ap, 7);
        assert_eq!(vm.run_context.fp, 11);
    }

    #[test]
    fn is_zero_int_value() {
        let value = MaybeRelocatable::Int(Felt252::from(1));
        assert!(!VirtualMachine::is_zero(&value));
    }

    #[test]
    fn is_zero_relocatable_value() {
        let value = MaybeRelocatable::from((1, 2));
        assert!(!VirtualMachine::is_zero(&value));
    }

    #[test]
    fn deduce_op0_opcode_call() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::Call,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        assert_matches!(
            vm.deduce_op0(&instruction, None, None),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                Some(x),
                None
            )) if x == MaybeRelocatable::from((0, 1))
        );
    }

    #[test]
    fn deduce_op0_opcode_assert_eq_res_add_with_optionals() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let dst = MaybeRelocatable::Int(Felt252::from(3));
        let op1 = MaybeRelocatable::Int(Felt252::from(2));

        assert_matches!(
            vm.deduce_op0(&instruction, Some(&dst), Some(&op1)),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                x,
                y
            )) if x == Some(MaybeRelocatable::Int(Felt252::from(1))) &&
                    y == Some(MaybeRelocatable::Int(Felt252::from(3)))
        );
    }

    #[test]
    fn deduce_op0_opcode_assert_eq_res_add_without_optionals() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        assert_matches!(
            vm.deduce_op0(&instruction, None, None),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                None, None
            ))
        );
    }

    #[test]
    fn deduce_op0_opcode_assert_eq_res_mul_non_zero_op1() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Mul,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let dst = MaybeRelocatable::Int(Felt252::from(4));
        let op1 = MaybeRelocatable::Int(Felt252::from(2));

        assert_matches!(
            vm.deduce_op0(&instruction, Some(&dst), Some(&op1)),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                Some(x),
                Some(y)
            )) if x == MaybeRelocatable::Int(Felt252::from(2)) &&
                    y == MaybeRelocatable::Int(Felt252::from(4))
        );
    }

    #[test]
    fn deduce_op0_opcode_assert_eq_res_mul_zero_op1() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Mul,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let dst = MaybeRelocatable::Int(Felt252::from(4));
        let op1 = MaybeRelocatable::Int(Felt252::from(0));
        assert_matches!(
            vm.deduce_op0(&instruction, Some(&dst), Some(&op1)),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                None, None
            ))
        );
    }

    #[test]
    fn deduce_op0_opcode_assert_eq_res_op1() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Op1,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let dst = MaybeRelocatable::Int(Felt252::from(4));
        let op1 = MaybeRelocatable::Int(Felt252::from(0));
        assert_matches!(
            vm.deduce_op0(&instruction, Some(&dst), Some(&op1)),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                None, None
            ))
        );
    }

    #[test]
    fn deduce_op0_opcode_ret() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Mul,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::Ret,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let dst = MaybeRelocatable::Int(Felt252::from(4));
        let op1 = MaybeRelocatable::Int(Felt252::from(0));

        assert_matches!(
            vm.deduce_op0(&instruction, Some(&dst), Some(&op1)),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                None, None
            ))
        );
    }

    #[test]
    fn deduce_op0_qm31_add_int_operands() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::QM31Operation,
        };

        let vm = vm!();

        let op1_qm31 = QM31::from_coefficients(STWO_PRIME - 10, 5, STWO_PRIME - 5, 1);
        let dst_qm31 = QM31::from_coefficients(STWO_PRIME - 4, 2, 12, 3);

        let op1 = MaybeRelocatable::Int(op1_qm31.pack_into_felt());
        let dst = MaybeRelocatable::Int(dst_qm31.pack_into_felt());
        assert_matches!(
            vm.deduce_op0(&instruction, Some(&dst), Some(&op1)),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                x,
                y
            )) if  QM31::unpack_from_felt(x.clone().unwrap().get_int_ref().unwrap()).unwrap() == QM31::from_coefficients(6, STWO_PRIME - 3, 17, 2) &&
                     QM31::unpack_from_felt(y.clone().unwrap().get_int_ref().unwrap()).unwrap() == dst_qm31
        );
    }

    #[test]
    fn deduce_op0_qm31_mul_int_operands() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Mul,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::QM31Operation,
        };

        let vm = vm!();

        let op1_qm31 = QM31::from_coefficients(0, 0, 1, 0);
        let dst_qm31 = QM31::from_coefficients(0, 0, 0, 1);
        let op1 = MaybeRelocatable::Int(op1_qm31.pack_into_felt());
        let dst = MaybeRelocatable::Int(dst_qm31.pack_into_felt());
        assert_matches!(
            vm.deduce_op0(&instruction, Some(&dst), Some(&op1)),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                x,
                y
            )) if QM31::unpack_from_felt(x.clone().unwrap().get_int_ref().unwrap()).unwrap() == QM31::from_coefficients(0, 1, 0, 0) &&
                    QM31::unpack_from_felt(y.clone().unwrap().get_int_ref().unwrap()).unwrap() == dst_qm31
        );
    }

    #[test]
    fn deduce_op0_blake_finalize_add_int_operands() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::BlakeFinalize,
        };

        let vm = vm!();

        let op1 = MaybeRelocatable::Int(Felt252::from(5));
        let dst = MaybeRelocatable::Int(Felt252::from(15));
        assert_matches!(
            vm.deduce_op0(&instruction, Some(&dst), Some(&op1)),
            Err(VirtualMachineError::InvalidTypedOperationOpcodeExtension(ref message)) if message.as_ref() == "typed_sub"
        );
    }

    #[test]
    fn deduce_op1_opcode_call() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::Call,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        assert_matches!(
            vm.deduce_op1(&instruction, None, None),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                None, None
            ))
        );
    }

    #[test]
    fn deduce_op1_opcode_assert_eq_res_add_with_optionals() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let dst = MaybeRelocatable::Int(Felt252::from(3));
        let op0 = MaybeRelocatable::Int(Felt252::from(2));
        assert_matches!(
            vm.deduce_op1(&instruction, Some(&dst), Some(op0)),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                x,
                y
            )) if x == Some(MaybeRelocatable::Int(Felt252::from(1))) &&
                    y == Some(MaybeRelocatable::Int(Felt252::from(3)))
        );
    }

    #[test]
    fn deduce_op1_opcode_assert_eq_res_add_without_optionals() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();
        assert_matches!(
            vm.deduce_op1(&instruction, None, None),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                None, None
            ))
        );
    }

    #[test]
    fn deduce_op1_opcode_assert_eq_res_mul_non_zero_op0() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Mul,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let dst = MaybeRelocatable::Int(Felt252::from(4));
        let op0 = MaybeRelocatable::Int(Felt252::from(2));
        assert_matches!(
            vm.deduce_op1(&instruction, Some(&dst), Some(op0)),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                x,
                y
            )) if x == Some(MaybeRelocatable::Int(Felt252::from(2))) &&
                    y == Some(MaybeRelocatable::Int(Felt252::from(4)))
        );
    }

    #[test]
    fn deduce_op1_opcode_assert_eq_res_mul_zero_op0() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Mul,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let dst = MaybeRelocatable::Int(Felt252::from(4));
        let op0 = MaybeRelocatable::Int(Felt252::from(0));
        assert_matches!(
            vm.deduce_op1(&instruction, Some(&dst), Some(op0)),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                None, None
            ))
        );
    }

    #[test]
    fn deduce_op1_opcode_assert_eq_res_op1_without_dst() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Op1,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let op0 = MaybeRelocatable::Int(Felt252::from(0));
        assert_matches!(
            vm.deduce_op1(&instruction, None, Some(op0)),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                None, None
            ))
        );
    }

    #[test]
    fn deduce_op1_opcode_assert_eq_res_op1_with_dst() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Op1,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let dst = MaybeRelocatable::Int(Felt252::from(7));
        assert_matches!(
            vm.deduce_op1(&instruction, Some(&dst), None),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                x,
                y
            )) if x == Some(MaybeRelocatable::Int(Felt252::from(7))) &&
                    y == Some(MaybeRelocatable::Int(Felt252::from(7)))
        );
    }

    #[test]
    fn deduce_op1_qm31_add_int_operands() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::QM31Operation,
        };

        let vm = vm!();

        let op0_qm31 = QM31::from_coefficients(4, STWO_PRIME - 13, 3, 7);
        let dst_qm31 = QM31::from_coefficients(8, 7, 6, 5);
        let op0 = MaybeRelocatable::Int(op0_qm31.pack_into_felt());
        let dst = MaybeRelocatable::Int(dst_qm31.pack_into_felt());
        assert_matches!(
            vm.deduce_op1(&instruction, Some(&dst), Some(op0)),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                x,
                y
            )) if  QM31::unpack_from_felt(x.clone().unwrap().get_int_ref().unwrap()).unwrap() == QM31::from_coefficients(4, 20, 3, STWO_PRIME - 2) &&
                     QM31::unpack_from_felt(y.clone().unwrap().get_int_ref().unwrap()).unwrap() == dst_qm31
        );
    }

    #[test]
    fn deduce_op1_qm31_mul_int_operands() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Mul,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::QM31Operation,
        };

        let vm = vm!();

        let op0_qm31 = QM31::from_coefficients(0, 1, 0, 0);
        let dst_qm31 = QM31::from_coefficients(STWO_PRIME - 1, 0, 0, 0);
        let op0 = MaybeRelocatable::Int(op0_qm31.pack_into_felt());
        let dst = MaybeRelocatable::Int(dst_qm31.pack_into_felt());
        assert_matches!(
            vm.deduce_op1(&instruction, Some(&dst), Some(op0)),
            Ok::<(Option<MaybeRelocatable>, Option<MaybeRelocatable>), VirtualMachineError>((
                x,
                y
            )) if QM31::unpack_from_felt(x.clone().unwrap().get_int_ref().unwrap()).unwrap() == QM31::from_coefficients(0, 1, 0, 0) &&
                    QM31::unpack_from_felt(y.clone().unwrap().get_int_ref().unwrap()).unwrap() == dst_qm31
        );
    }

    #[test]
    fn deduce_op1_blake_mul_int_operands() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Mul,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Blake,
        };

        let vm = vm!();

        let op0 = MaybeRelocatable::Int(Felt252::from(4));
        let dst = MaybeRelocatable::Int(Felt252::from(16));
        assert_matches!(
            vm.deduce_op1(&instruction, Some(&dst), Some(op0)),
            Err(VirtualMachineError::InvalidTypedOperationOpcodeExtension(ref message)) if message.as_ref() == "typed_div"
        );
    }

    #[test]
    fn compute_res_op1() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Op1,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let op1 = MaybeRelocatable::Int(Felt252::from(7));
        let op0 = MaybeRelocatable::Int(Felt252::from(9));
        assert_matches!(
            vm.compute_res(&instruction, &op0, &op1),
            Ok::<Option<MaybeRelocatable>, VirtualMachineError>(Some(MaybeRelocatable::Int(
                x
            ))) if x == Felt252::from(7)
        );
    }

    #[test]
    fn compute_res_add() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let op1 = MaybeRelocatable::Int(Felt252::from(7));
        let op0 = MaybeRelocatable::Int(Felt252::from(9));
        assert_matches!(
            vm.compute_res(&instruction, &op0, &op1),
            Ok::<Option<MaybeRelocatable>, VirtualMachineError>(Some(MaybeRelocatable::Int(
                x
            ))) if x == Felt252::from(16)
        );
    }

    #[test]
    fn compute_res_mul_int_operands() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Mul,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let op1 = MaybeRelocatable::Int(Felt252::from(7));
        let op0 = MaybeRelocatable::Int(Felt252::from(9));
        assert_matches!(
            vm.compute_res(&instruction, &op0, &op1),
            Ok::<Option<MaybeRelocatable>, VirtualMachineError>(Some(MaybeRelocatable::Int(
                x
            ))) if x == Felt252::from(63)
        );
    }

    #[test]
    fn compute_res_mul_relocatable_values() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Mul,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let op1 = MaybeRelocatable::from((2, 3));
        let op0 = MaybeRelocatable::from((2, 6));
        assert_matches!(
            vm.compute_res(&instruction, &op0, &op1),
            Err(VirtualMachineError::ComputeResRelocatableMul(bx)) if *bx == (op0, op1)
        );
    }

    #[test]
    fn compute_res_qm31_add_relocatable_values() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::QM31Operation,
        };

        let vm = vm!();

        let op1 = MaybeRelocatable::from((2, 3));
        let op0 = MaybeRelocatable::from(7);
        assert_matches!(
            vm.compute_res(&instruction, &op0, &op1),
            Err(VirtualMachineError::Math(MathError::RelocatableQM31Add(bx))) if *bx == (op0, op1)
        );
    }

    #[test]
    fn compute_res_qm31_add_int_operands() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::QM31Operation,
        };

        let vm = vm!();

        let op1_qm31 = QM31::from_coefficients(1, 2, 3, 4);
        let op0_qm31 = QM31::from_coefficients(10, 11, STWO_PRIME - 1, 13);
        let op1 = MaybeRelocatable::Int(op1_qm31.pack_into_felt());
        let op0 = MaybeRelocatable::Int(op0_qm31.pack_into_felt());
        assert_matches!(
            vm.compute_res(&instruction, &op0, &op1),
            Ok::<Option<MaybeRelocatable>, VirtualMachineError>(Some(MaybeRelocatable::Int(
                x
            ))) if QM31::unpack_from_felt(&x).unwrap() == QM31::from_coefficients(11, 13, 2, 17)
        );
    }

    #[test]
    fn compute_res_qm31_mul_int_operands() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Mul,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::QM31Operation,
        };

        let vm = vm!();

        let op1_qm31 = QM31::from_coefficients(0, 0, 1, 0);
        let op0_qm31 = QM31::from_coefficients(0, 0, 1, 0);
        let op1 = MaybeRelocatable::Int(op1_qm31.pack_into_felt());
        let op0 = MaybeRelocatable::Int(op0_qm31.pack_into_felt());
        assert_matches!(
            vm.compute_res(&instruction, &op0, &op1),
            Ok::<Option<MaybeRelocatable>, VirtualMachineError>(Some(MaybeRelocatable::Int(
                x
            ))) if QM31::unpack_from_felt(&x).unwrap() == QM31::from_coefficients(2, 1, 0, 0)
        );
    }

    #[test]
    fn compute_res_blake_mul_int_operands() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Mul,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Blake,
        };

        let vm = vm!();

        let op1 = MaybeRelocatable::Int(Felt252::from(11));
        let op0 = MaybeRelocatable::Int(Felt252::from(12));
        assert_matches!(
            vm.compute_res(&instruction, &op0, &op1),
            Err(VirtualMachineError::InvalidTypedOperationOpcodeExtension(ref message)) if message.as_ref() == "typed_mul"
        );
    }

    #[test]
    fn compute_res_unconstrained() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Unconstrained,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let op1 = MaybeRelocatable::Int(Felt252::from(7));
        let op0 = MaybeRelocatable::Int(Felt252::from(9));
        assert_matches!(
            vm.compute_res(&instruction, &op0, &op1),
            Ok::<Option<MaybeRelocatable>, VirtualMachineError>(None)
        );
    }

    #[test]
    fn deduce_dst_opcode_assert_eq_with_res() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Unconstrained,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        let res = MaybeRelocatable::Int(Felt252::from(7));
        assert_eq!(
            MaybeRelocatable::Int(Felt252::from(7)),
            vm.deduce_dst(&instruction, &Some(res)).unwrap()
        );
    }

    #[test]
    fn deduce_dst_opcode_assert_eq_without_res() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Unconstrained,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        assert!(vm.deduce_dst(&instruction, &None).is_err());
    }

    #[test]
    fn deduce_dst_opcode_call() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Unconstrained,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::Call,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        assert_eq!(
            MaybeRelocatable::from((1, 0)),
            vm.deduce_dst(&instruction, &None).unwrap()
        );
    }

    #[test]
    fn deduce_dst_opcode_ret() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Unconstrained,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::Ret,
            opcode_extension: OpcodeExtension::Stone,
        };

        let vm = vm!();

        assert!(vm.deduce_dst(&instruction, &None).is_err());
    }

    #[test]
    fn compute_operands_add_ap() {
        let inst = Instruction {
            off0: 0,
            off1: 1,
            off2: 2,
            dst_register: Register::AP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let mut vm = vm!();
        for _ in 0..2 {
            vm.segments.add();
        }

        vm.segments.memory.data.push(Vec::new());
        let dst_addr = Relocatable::from((1, 0));
        let dst_addr_value = MaybeRelocatable::Int(Felt252::from(5));
        let op0_addr = Relocatable::from((1, 1));
        let op0_addr_value = MaybeRelocatable::Int(Felt252::from(2));
        let op1_addr = Relocatable::from((1, 2));
        let op1_addr_value = MaybeRelocatable::Int(Felt252::from(3));
        vm.segments
            .memory
            .insert(dst_addr, &dst_addr_value)
            .unwrap();
        vm.segments
            .memory
            .insert(op0_addr, &op0_addr_value)
            .unwrap();
        vm.segments
            .memory
            .insert(op1_addr, &op1_addr_value)
            .unwrap();

        let expected_operands = Operands {
            dst: dst_addr_value.clone(),
            res: Some(dst_addr_value.clone()),
            op0: op0_addr_value.clone(),
            op1: op1_addr_value.clone(),
        };

        let expected_addresses = OperandsAddresses {
            dst_addr,
            op0_addr,
            op1_addr,
        };

        let (operands, addresses, _) = vm.compute_operands(&inst).unwrap();
        assert!(operands == expected_operands);
        assert!(addresses == expected_addresses);
    }

    #[test]
    fn compute_operands_mul_fp() {
        let inst = Instruction {
            off0: 0,
            off1: 1,
            off2: 2,
            dst_register: Register::FP,
            op0_register: Register::FP,
            op1_addr: Op1Addr::FP,
            res: Res::Mul,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };
        let mut vm = vm!();
        //Create program and execution segments
        for _ in 0..2 {
            vm.segments.add();
        }
        vm.segments.memory.data.push(Vec::new());
        let dst_addr = relocatable!(1, 0);
        let dst_addr_value = mayberelocatable!(6);
        let op0_addr = relocatable!(1, 1);
        let op0_addr_value = mayberelocatable!(2);
        let op1_addr = relocatable!(1, 2);
        let op1_addr_value = mayberelocatable!(3);
        vm.segments
            .memory
            .insert(dst_addr, &dst_addr_value)
            .unwrap();
        vm.segments
            .memory
            .insert(op0_addr, &op0_addr_value)
            .unwrap();
        vm.segments
            .memory
            .insert(op1_addr, &op1_addr_value)
            .unwrap();

        let expected_operands = Operands {
            dst: dst_addr_value.clone(),
            res: Some(dst_addr_value.clone()),
            op0: op0_addr_value.clone(),
            op1: op1_addr_value.clone(),
        };

        let expected_addresses = OperandsAddresses {
            dst_addr,
            op0_addr,
            op1_addr,
        };

        let (operands, addresses, _) = vm.compute_operands(&inst).unwrap();
        assert!(operands == expected_operands);
        assert!(addresses == expected_addresses);
    }

    #[test]
    fn compute_jnz() {
        let instruction = Instruction {
            off0: 1,
            off1: 1,
            off2: 1,
            dst_register: Register::AP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::Imm,
            res: Res::Unconstrained,
            pc_update: PcUpdate::Jnz,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let mut vm = vm!();
        vm.segments = segments![
            ((0, 0), 0x206800180018001_i64),
            ((1, 1), 0x4),
            ((0, 1), 0x4)
        ];

        let expected_operands = Operands {
            dst: mayberelocatable!(4),
            res: None,
            op0: mayberelocatable!(4),
            op1: mayberelocatable!(4),
        };

        let expected_addresses = OperandsAddresses {
            dst_addr: relocatable!(1, 1),
            op0_addr: relocatable!(1, 1),
            op1_addr: relocatable!(0, 1),
        };

        let (operands, addresses, _) = vm.compute_operands(&instruction).unwrap();
        assert!(operands == expected_operands);
        assert!(addresses == expected_addresses);
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        assert_matches!(
            vm.step(
                &mut hint_processor,
                exec_scopes_ref!(),
                &mut Vec::new(),
                #[cfg(feature = "extensive_hints")]
                &mut HashMap::new(),
                #[cfg(feature = "test_utils")]
                &HashMap::new(),
            ),
            Ok(())
        );
        assert_eq!(vm.run_context.pc, relocatable!(0, 4));
    }

    #[test]
    fn compute_operands_deduce_dst_none() {
        let instruction = Instruction {
            off0: 2,
            off1: 0,
            off2: 0,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Unconstrained,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        };

        let mut vm = vm!();

        vm.segments = segments!(((1, 0), 145944781867024385_i64));

        let error = vm.compute_operands(&instruction).unwrap_err();
        assert_matches!(error, VirtualMachineError::NoDst);
    }

    #[test]
    fn opcode_assertions_res_unconstrained() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::APPlus2,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(8)),
            res: None,
            op0: MaybeRelocatable::Int(Felt252::from(9)),
            op1: MaybeRelocatable::Int(Felt252::from(10)),
        };

        let vm = vm!();

        let error = vm.opcode_assertions(&instruction, &operands);
        assert_matches!(error, Err(VirtualMachineError::UnconstrainedResAssertEq));
    }

    #[test]
    fn opcode_assertions_instruction_failed() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::APPlus2,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::Int(Felt252::from(9_i32)),
            res: Some(MaybeRelocatable::Int(Felt252::from(8_i32))),
            op0: MaybeRelocatable::Int(Felt252::from(9_i32)),
            op1: MaybeRelocatable::Int(Felt252::from(10_i32)),
        };

        let vm = vm!();

        assert_matches!(
            vm.opcode_assertions(&instruction, &operands),
            Err(VirtualMachineError::DiffAssertValues(bx))
            if *bx == (MaybeRelocatable::Int(Felt252::from(9_i32)),
                 MaybeRelocatable::Int(Felt252::from(8_i32)))
        );
    }

    #[test]
    fn opcode_assertions_instruction_failed_relocatables() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::APPlus2,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: MaybeRelocatable::from((1, 1)),
            res: Some(MaybeRelocatable::from((1, 2))),
            op0: MaybeRelocatable::Int(Felt252::from(9_i32)),
            op1: MaybeRelocatable::Int(Felt252::from(10_i32)),
        };

        let vm = vm!();

        assert_matches!(
            vm.opcode_assertions(&instruction, &operands),
            Err(VirtualMachineError::DiffAssertValues(bx)) if *bx == (MaybeRelocatable::from((1, 1)), MaybeRelocatable::from((1, 2)))
        );
    }

    #[test]
    fn opcode_assertions_inconsistent_op0() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::APPlus2,
            opcode: Opcode::Call,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: mayberelocatable!(0, 8),
            res: Some(mayberelocatable!(8)),
            op0: mayberelocatable!(9),
            op1: mayberelocatable!(10),
        };

        let mut vm = vm!();
        vm.run_context.pc = relocatable!(0, 4);

        assert_matches!(
            vm.opcode_assertions(&instruction, &operands),
            Err(VirtualMachineError::CantWriteReturnPc(bx)) if *bx == (mayberelocatable!(9), mayberelocatable!(0, 5))
        );
    }

    #[test]
    fn opcode_assertions_inconsistent_dst() {
        let instruction = Instruction {
            off0: 1,
            off1: 2,
            off2: 3,
            dst_register: Register::FP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular,
            fp_update: FpUpdate::APPlus2,
            opcode: Opcode::Call,
            opcode_extension: OpcodeExtension::Stone,
        };

        let operands = Operands {
            dst: mayberelocatable!(8),
            res: Some(mayberelocatable!(8)),
            op0: mayberelocatable!(0, 1),
            op1: mayberelocatable!(10),
        };
        let mut vm = vm!();
        vm.run_context.fp = 6;

        assert_matches!(
            vm.opcode_assertions(&instruction, &operands),
            Err(VirtualMachineError::CantWriteReturnFp(bx)) if *bx == (mayberelocatable!(8), mayberelocatable!(1, 6))
        );
    }

    #[test]
    /// Test for a simple program execution
    /// Used program code:
    /// func main():
    ///     let a = 1
    ///     let b = 2
    ///     let c = a + b
    ///     return()
    /// end
    /// Memory taken from original vm
    /// {RelocatableValue(segment_index=0, offset=0): 2345108766317314046,
    ///  RelocatableValue(segment_index=1, offset=0): RelocatableValue(segment_index=2, offset=0),
    ///  RelocatableValue(segment_index=1, offset=1): RelocatableValue(segment_index=3, offset=0)}
    /// Current register values:
    /// AP 1:2
    /// FP 1:2
    /// PC 0:0
    fn test_step_for_preset_memory() {
        let mut vm = vm!(true);

        let mut hint_processor = BuiltinHintProcessor::new_empty();

        run_context!(vm, 0, 2, 2);

        vm.segments = segments![
            ((0, 0), 2345108766317314046_u64),
            ((1, 0), (2, 0)),
            ((1, 1), (3, 0))
        ];

        assert_matches!(
            vm.step(
                &mut hint_processor,
                exec_scopes_ref!(),
                &mut Vec::new(),
                #[cfg(feature = "extensive_hints")]
                &mut HashMap::new(),
                #[cfg(feature = "test_utils")]
                &HashMap::new(),
            ),
            Ok(())
        );
        let trace = vm.trace.unwrap();
        trace_check(&trace, &[((0, 0).into(), 2, 2)]);

        assert_eq!(vm.run_context.pc, Relocatable::from((3, 0)));
        assert_eq!(vm.run_context.ap, 2);
        assert_eq!(vm.run_context.fp, 0);

        //Check that the following addresses have been accessed:
        // Addresses have been copied from python execution:
        let mem = vm.segments.memory.data;
        assert!(mem[1][0].is_accessed());
        assert!(mem[1][1].is_accessed());
    }

    #[test]
    /*
    Test for a simple program execution
    Used program code:
        func myfunc(a: felt) -> (r: felt):
            let b = a * 2
            return(b)
        end
        func main():
            let a = 1
            let b = myfunc(a)
            return()
        end
    Memory taken from original vm:
    {RelocatableValue(segment_index=0, offset=0): 5207990763031199744,
    RelocatableValue(segment_index=0, offset=1): 2,
    RelocatableValue(segment_index=0, offset=2): 2345108766317314046,
    RelocatableValue(segment_index=0, offset=3): 5189976364521848832,
    RelocatableValue(segment_index=0, offset=4): 1,
    RelocatableValue(segment_index=0, offset=5): 1226245742482522112,
    RelocatableValue(segment_index=0, offset=6): 3618502788666131213697322783095070105623107215331596699973092056135872020476,
    RelocatableValue(segment_index=0, offset=7): 2345108766317314046,
    RelocatableValue(segment_index=1, offset=0): RelocatableValue(segment_index=2, offset=0),
    RelocatableValue(segment_index=1, offset=1): RelocatableValue(segment_index=3, offset=0)}
    Current register values:
    AP 1:2
    FP 1:2
    PC 0:3
    Final Pc (not executed): 3:0
    This program consists of 5 steps
    */
    fn test_step_for_preset_memory_function_call() {
        let mut vm = vm!(true);

        run_context!(vm, 3, 2, 2);

        //Insert values into memory
        vm.segments.memory =
            memory![
            ((0, 0), 5207990763031199744_i64),
            ((0, 1), 2),
            ((0, 2), 2345108766317314046_i64),
            ((0, 3), 5189976364521848832_i64),
            ((0, 4), 1),
            ((0, 5), 1226245742482522112_i64),
            (
                (0, 6),
                ("3618502788666131213697322783095070105623107215331596699973092056135872020476",10)
            ),
            ((0, 7), 2345108766317314046_i64),
            ((1, 0), (2, 0)),
            ((1, 1), (3, 0))
        ];

        let final_pc = Relocatable::from((3, 0));
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        //Run steps
        while vm.run_context.pc != final_pc {
            assert_matches!(
                vm.step(
                    &mut hint_processor,
                    exec_scopes_ref!(),
                    &mut Vec::new(),
                    #[cfg(feature = "extensive_hints")]
                    &mut HashMap::new(),
                    #[cfg(feature = "test_utils")]
                    &HashMap::new()
                ),
                Ok(())
            );
        }

        //Check final register values
        assert_eq!(vm.run_context.pc, Relocatable::from((3, 0)));

        assert_eq!(vm.run_context.ap, 6);

        assert_eq!(vm.run_context.fp, 0);
        //Check each TraceEntry in trace
        let trace = vm.trace.unwrap();
        assert_eq!(trace.len(), 5);
        trace_check(
            &trace,
            &[
                ((0, 3).into(), 2, 2),
                ((0, 5).into(), 3, 2),
                ((0, 0).into(), 5, 5),
                ((0, 2).into(), 6, 5),
                ((0, 7).into(), 6, 2),
            ],
        );
        //Check that the following addresses have been accessed:
        // Addresses have been copied from python execution + addresses of accessed code:
        let mem = &vm.segments.memory.data;
        assert!(mem[0][0].is_accessed());
        assert!(mem[0][1].is_accessed());
        assert!(mem[0][2].is_accessed());
        assert!(mem[0][3].is_accessed());
        assert!(mem[0][4].is_accessed());
        assert!(mem[0][5].is_accessed());
        assert!(mem[0][6].is_accessed());
        assert!(mem[0][7].is_accessed());
        assert!(mem[1][0].is_accessed());
        assert!(mem[1][1].is_accessed());
        assert!(mem[1][2].is_accessed());
        assert!(mem[1][3].is_accessed());
        assert!(mem[1][4].is_accessed());
        assert!(mem[1][5].is_accessed());
        assert_eq!(
            vm.segments
                .memory
                .get_amount_of_accessed_addresses_for_segment(0),
            Some(8)
        );
        assert_eq!(
            vm.segments
                .memory
                .get_amount_of_accessed_addresses_for_segment(1),
            Some(6)
        );
    }

    #[test]
    /// Test the following program:
    /// ...
    /// [ap] = 4
    /// ap += 1
    /// [ap] = 5; ap++
    /// [ap] = [ap - 1] * [ap - 2]
    /// ...
    /// Original vm memory:
    /// RelocatableValue(segment_index=0, offset=0): '0x400680017fff8000',
    /// RelocatableValue(segment_index=0, offset=1): '0x4',
    /// RelocatableValue(segment_index=0, offset=2): '0x40780017fff7fff',
    /// RelocatableValue(segment_index=0, offset=3): '0x1',
    /// RelocatableValue(segment_index=0, offset=4): '0x480680017fff8000',
    /// RelocatableValue(segment_index=0, offset=5): '0x5',
    /// RelocatableValue(segment_index=0, offset=6): '0x40507ffe7fff8000',
    /// RelocatableValue(segment_index=0, offset=7): '0x208b7fff7fff7ffe',
    /// RelocatableValue(segment_index=1, offset=0): RelocatableValue(segment_index=2, offset=0),
    /// RelocatableValue(segment_index=1, offset=1): RelocatableValue(segment_index=3, offset=0),
    /// RelocatableValue(segment_index=1, offset=2): '0x4',
    /// RelocatableValue(segment_index=1, offset=3): '0x5',
    /// RelocatableValue(segment_index=1, offset=4): '0x14'
    fn multiplication_and_different_ap_increase() {
        let mut vm = vm!();
        vm.segments = segments![
            ((0, 0), 0x400680017fff8000_i64),
            ((0, 1), 0x4),
            ((0, 2), 0x40780017fff7fff_i64),
            ((0, 3), 0x1),
            ((0, 4), 0x480680017fff8000_i64),
            ((0, 5), 0x5),
            ((0, 6), 0x40507ffe7fff8000_i64),
            ((0, 7), 0x208b7fff7fff7ffe_i64),
            ((1, 0), (2, 0)),
            ((1, 1), (3, 0)),
            ((1, 2), 0x4),
            ((1, 3), 0x5),
            ((1, 4), 0x14)
        ];

        run_context!(vm, 0, 2, 2);

        assert_eq!(vm.run_context.pc, Relocatable::from((0, 0)));
        assert_eq!(vm.run_context.ap, 2);
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        assert_matches!(
            vm.step(
                &mut hint_processor,
                exec_scopes_ref!(),
                &mut Vec::new(),
                #[cfg(feature = "extensive_hints")]
                &mut HashMap::new(),
                #[cfg(feature = "test_utils")]
                &HashMap::new()
            ),
            Ok(())
        );
        assert_eq!(vm.run_context.pc, Relocatable::from((0, 2)));
        assert_eq!(vm.run_context.ap, 2);

        assert_eq!(
            vm.segments
                .memory
                .get(&vm.run_context.get_ap())
                .unwrap()
                .as_ref(),
            &MaybeRelocatable::Int(Felt252::from(0x4)),
        );
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        assert_matches!(
            vm.step(
                &mut hint_processor,
                exec_scopes_ref!(),
                &mut Vec::new(),
                #[cfg(feature = "extensive_hints")]
                &mut HashMap::new(),
                #[cfg(feature = "test_utils")]
                &HashMap::new()
            ),
            Ok(())
        );
        assert_eq!(vm.run_context.pc, Relocatable::from((0, 4)));
        assert_eq!(vm.run_context.ap, 3);

        assert_eq!(
            vm.segments
                .memory
                .get(&vm.run_context.get_ap())
                .unwrap()
                .as_ref(),
            &MaybeRelocatable::Int(Felt252::from(0x5))
        );

        let mut hint_processor = BuiltinHintProcessor::new_empty();
        assert_matches!(
            vm.step(
                &mut hint_processor,
                exec_scopes_ref!(),
                &mut Vec::new(),
                #[cfg(feature = "extensive_hints")]
                &mut HashMap::new(),
                #[cfg(feature = "test_utils")]
                &HashMap::new()
            ),
            Ok(())
        );
        assert_eq!(vm.run_context.pc, Relocatable::from((0, 6)));
        assert_eq!(vm.run_context.ap, 4);

        assert_eq!(
            vm.segments
                .memory
                .get(&vm.run_context.get_ap())
                .unwrap()
                .as_ref(),
            &MaybeRelocatable::Int(Felt252::from(0x14)),
        );
    }

    #[test]
    fn deduce_memory_cell_no_pedersen_builtin() {
        let vm = vm!();
        assert_matches!(vm.deduce_memory_cell(Relocatable::from((0, 0))), Ok(None));
    }

    #[test]
    fn deduce_memory_cell_pedersen_builtin_valid() {
        let mut vm = vm!();
        let builtin = HashBuiltinRunner::new(Some(8), true);
        vm.builtin_runners.push(builtin.into());
        vm.segments = segments![((0, 3), 32), ((0, 4), 72), ((0, 5), 0)];
        assert_matches!(
            vm.deduce_memory_cell(Relocatable::from((0, 5))),
            Ok(i) if i == Some(MaybeRelocatable::from(crate::felt_hex!(
                "0x73b3ec210cccbb970f80c6826fb1c40ae9f487617696234ff147451405c339f"
            )))
        );
    }

    #[test]
    /* Program used:
    %builtins output pedersen
    from starkware.cairo.common.cairo_builtins import HashBuiltin
    from starkware.cairo.common.hash import hash2
    from starkware.cairo.common.serialize import serialize_word

    func foo(hash_ptr : HashBuiltin*) -> (
        hash_ptr : HashBuiltin*, z
    ):
        # Use a with-statement, since 'hash_ptr' is not an
        # implicit argument.
        with hash_ptr:
            let (z) = hash2(32, 72)
        end
        return (hash_ptr=hash_ptr, z=z)
    end

    func main{output_ptr: felt*, pedersen_ptr: HashBuiltin*}():
        let (pedersen_ptr, a) = foo(pedersen_ptr)
        serialize_word(a)
        return()
    end
     */
    fn compute_operands_pedersen() {
        let instruction = Instruction {
            off0: 0,
            off1: -5,
            off2: 2,
            dst_register: Register::AP,
            op0_register: Register::FP,
            op1_addr: Op1Addr::Op0,
            res: Res::Op1,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Add1,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };
        let mut builtin = HashBuiltinRunner::new(Some(8), true);
        builtin.base = 3;
        let mut vm = vm!();
        vm.builtin_runners.push(builtin.into());
        run_context!(vm, 0, 13, 12);

        //Insert values into memory (excluding those from the program segment (instructions))
        vm.segments = segments![
            ((3, 0), 32),
            ((3, 1), 72),
            ((1, 0), (2, 0)),
            ((1, 1), (3, 0)),
            ((1, 2), (4, 0)),
            ((1, 3), (5, 0)),
            ((1, 4), (3, 0)),
            ((1, 5), (1, 4)),
            ((1, 6), (0, 21)),
            ((1, 7), (3, 0)),
            ((1, 8), 32),
            ((1, 9), 72),
            ((1, 10), (1, 7)),
            ((1, 11), (0, 17)),
            ((1, 12), (3, 3))
        ];

        let expected_operands = Operands {
            dst: MaybeRelocatable::from(crate::felt_hex!(
                "0x73b3ec210cccbb970f80c6826fb1c40ae9f487617696234ff147451405c339f"
            )),
            res: Some(MaybeRelocatable::from(crate::felt_hex!(
                "0x73b3ec210cccbb970f80c6826fb1c40ae9f487617696234ff147451405c339f"
            ))),
            op0: MaybeRelocatable::from((3, 0)),
            op1: MaybeRelocatable::from(crate::felt_hex!(
                "0x73b3ec210cccbb970f80c6826fb1c40ae9f487617696234ff147451405c339f"
            )),
        };
        let expected_operands_mem_addresses = OperandsAddresses {
            dst_addr: Relocatable::from((1, 13)),
            op0_addr: Relocatable::from((1, 7)),
            op1_addr: Relocatable::from((3, 2)),
        };
        let (operands, operands_mem_address, _) = vm.compute_operands(&instruction).unwrap();
        assert_eq!(operands, expected_operands);
        assert_eq!(operands_mem_address, expected_operands_mem_addresses);
    }

    #[test]
    fn deduce_memory_cell_bitwise_builtin_valid_and() {
        let mut vm = vm!();
        let builtin = BitwiseBuiltinRunner::new(Some(256), true);
        vm.builtin_runners.push(builtin.into());
        vm.segments = segments![((0, 5), 10), ((0, 6), 12), ((0, 7), 0)];
        assert_matches!(
            vm.deduce_memory_cell(Relocatable::from((0, 7))),
            Ok(i) if i == Some(MaybeRelocatable::from(Felt252::from(8_i32)))
        );
    }

    #[test]
    /* Program used:
    %builtins bitwise
    from starkware.cairo.common.bitwise import bitwise_and
    from starkware.cairo.common.cairo_builtins import BitwiseBuiltin

    func main{bitwise_ptr: BitwiseBuiltin*}():
        let (result) = bitwise_and(12, 10)  # Binary (1100, 1010).
        assert result = 8  # Binary 1000.
        return()
    end
    */
    fn compute_operands_bitwise() {
        let instruction = Instruction {
            off0: 0,
            off1: -5,
            off2: 2,
            dst_register: Register::AP,
            op0_register: Register::FP,
            op1_addr: Op1Addr::Op0,
            res: Res::Op1,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Add1,
            fp_update: FpUpdate::Regular,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        };

        let mut builtin = BitwiseBuiltinRunner::new(Some(256), true);
        builtin.base = 2;
        let mut vm = vm!();

        vm.builtin_runners.push(builtin.into());
        run_context!(vm, 0, 9, 8);

        //Insert values into memory (excluding those from the program segment (instructions))
        vm.segments = segments![
            ((2, 0), 12),
            ((2, 1), 10),
            ((1, 0), (2, 0)),
            ((1, 1), (3, 0)),
            ((1, 2), (4, 0)),
            ((1, 3), (2, 0)),
            ((1, 4), 12),
            ((1, 5), 10),
            ((1, 6), (1, 3)),
            ((1, 7), (0, 13))
        ];

        let expected_operands = Operands {
            dst: MaybeRelocatable::from(Felt252::from(8_i32)),
            res: Some(MaybeRelocatable::from(Felt252::from(8_i32))),
            op0: MaybeRelocatable::from((2, 0)),
            op1: MaybeRelocatable::from(Felt252::from(8_i32)),
        };
        let expected_operands_mem_addresses = OperandsAddresses {
            dst_addr: Relocatable::from((1, 9)),
            op0_addr: Relocatable::from((1, 3)),
            op1_addr: Relocatable::from((2, 2)),
        };
        let (operands, operands_mem_address, _) = vm.compute_operands(&instruction).unwrap();
        assert_eq!(operands, expected_operands);
        assert_eq!(operands_mem_address, expected_operands_mem_addresses);
    }

    #[test]
    fn deduce_memory_cell_ec_op_builtin_valid() {
        let mut vm = vm!();
        let builtin = EcOpBuiltinRunner::new(Some(256), true);
        vm.builtin_runners.push(builtin.into());

        vm.segments = segments![
            (
                (0, 0),
                (
                    "0x68caa9509b7c2e90b4d92661cbf7c465471c1e8598c5f989691eef6653e0f38",
                    16
                )
            ),
            (
                (0, 1),
                (
                    "0x79a8673f498531002fc549e06ff2010ffc0c191cceb7da5532acb95cdcb591",
                    16
                )
            ),
            (
                (0, 2),
                (
                    "0x1ef15c18599971b7beced415a40f0c7deacfd9b0d1819e03d723d8bc943cfca",
                    16
                )
            ),
            (
                (0, 3),
                (
                    "0x5668060aa49730b7be4801df46ec62de53ecd11abe43a32873000c36e8dc1f",
                    16
                )
            ),
            ((0, 4), 34),
            (
                (0, 5),
                (
                    "0x6245403e2fafe5df3b79ea28d050d477771bc560fc59e915b302cc9b70a92f5",
                    16
                )
            )
        ];

        assert_matches!(
            vm.deduce_memory_cell(Relocatable::from((0, 6))),
            Ok(i) if i == Some(MaybeRelocatable::from(felt_hex!(
                "0x7f49de2c3a7d1671437406869edb1805ba43e1c0173b35f8c2e8fcc13c3fa6d"
            )))
        );
    }

    #[test]
    /* Data taken from this program execution:
       %builtins output ec_op
       from starkware.cairo.common.cairo_builtins import EcOpBuiltin
       from starkware.cairo.common.serialize import serialize_word
       from starkware.cairo.common.ec_point import EcPoint
       from starkware.cairo.common.ec import ec_op

       func main{output_ptr: felt*, ec_op_ptr: EcOpBuiltin*}():
           let x: EcPoint = EcPoint(2089986280348253421170679821480865132823066470938446095505822317253594081284, 1713931329540660377023406109199410414810705867260802078187082345529207694986)

           let y: EcPoint = EcPoint(874739451078007766457464989774322083649278607533249481151382481072868806602,152666792071518830868575557812948353041420400780739481342941381225525861407)
           let z: EcPoint = ec_op(x,34, y)
           serialize_word(z.x)
           return()
           end
    */
    fn verify_auto_deductions_for_ec_op_builtin_valid() {
        let mut builtin = EcOpBuiltinRunner::new(Some(256), true);
        builtin.base = 3;
        let mut vm = vm!();
        vm.builtin_runners.push(builtin.into());
        vm.segments = segments![
            (
                (3, 0),
                (
                    "2962412995502985605007699495352191122971573493113767820301112397466445942584",
                    10
                )
            ),
            (
                (3, 1),
                (
                    "214950771763870898744428659242275426967582168179217139798831865603966154129",
                    10
                )
            ),
            (
                (3, 2),
                (
                    "874739451078007766457464989774322083649278607533249481151382481072868806602",
                    10
                )
            ),
            (
                (3, 3),
                (
                    "152666792071518830868575557812948353041420400780739481342941381225525861407",
                    10
                )
            ),
            ((3, 4), 34),
            (
                (3, 5),
                (
                    "2778063437308421278851140253538604815869848682781135193774472480292420096757",
                    10
                )
            )
        ];
        assert_matches!(vm.verify_auto_deductions(), Ok(()));
    }

    #[test]
    fn verify_auto_deductions_for_ec_op_builtin_valid_points_invalid_result() {
        let mut builtin = EcOpBuiltinRunner::new(Some(256), true);
        builtin.base = 3;
        let mut vm = vm!();
        vm.builtin_runners.push(builtin.into());
        vm.segments = segments![
            (
                (3, 0),
                (
                    "2962412995502985605007699495352191122971573493113767820301112397466445942584",
                    10
                )
            ),
            (
                (3, 1),
                (
                    "214950771763870898744428659242275426967582168179217139798831865603966154129",
                    10
                )
            ),
            (
                (3, 2),
                (
                    "2089986280348253421170679821480865132823066470938446095505822317253594081284",
                    10
                )
            ),
            (
                (3, 3),
                (
                    "1713931329540660377023406109199410414810705867260802078187082345529207694986",
                    10
                )
            ),
            ((3, 4), 34),
            (
                (3, 5),
                (
                    "2778063437308421278851140253538604815869848682781135193774472480292420096757",
                    10
                )
            )
        ];
        let error = vm.verify_auto_deductions();
        assert_matches!(
            error,
            Err(VirtualMachineError::InconsistentAutoDeduction(bx))
            if *bx == (BuiltinName::ec_op,
                    MaybeRelocatable::Int(crate::felt_str!(
                        "2739017437753868763038285897969098325279422804143820990343394856167768859289"
                    )),
                    Some(MaybeRelocatable::Int(crate::felt_str!(
                        "2778063437308421278851140253538604815869848682781135193774472480292420096757"
                    ))))
        );
    }

    #[test]
    /* Program used:
    %builtins bitwise
    from starkware.cairo.common.bitwise import bitwise_and
    from starkware.cairo.common.cairo_builtins import BitwiseBuiltin

    func main{bitwise_ptr: BitwiseBuiltin*}():
        let (result) = bitwise_and(12, 10)  # Binary (1100, 1010).
        assert result = 8  # Binary 1000.
        return()
    end
    */
    fn verify_auto_deductions_bitwise() {
        let mut builtin = BitwiseBuiltinRunner::new(Some(256), true);
        builtin.base = 2;
        let mut vm = vm!();
        vm.builtin_runners.push(builtin.into());
        vm.segments = segments![((2, 0), 12), ((2, 1), 10)];
        assert_matches!(vm.verify_auto_deductions(), Ok(()));
    }

    #[test]
    /* Program used:
    %builtins bitwise
    from starkware.cairo.common.bitwise import bitwise_and
    from starkware.cairo.common.cairo_builtins import BitwiseBuiltin

    func main{bitwise_ptr: BitwiseBuiltin*}():
        let (result) = bitwise_and(12, 10)  # Binary (1100, 1010).
        assert result = 8  # Binary 1000.
        return()
    end
    */
    fn verify_auto_deductions_for_addr_bitwise() {
        let mut builtin = BitwiseBuiltinRunner::new(Some(256), true);
        builtin.base = 2;
        let builtin: BuiltinRunner = builtin.into();
        let mut vm = vm!();
        vm.segments = segments![((2, 0), 12), ((2, 1), 10)];
        assert_matches!(
            vm.verify_auto_deductions_for_addr(relocatable!(2, 0), &builtin),
            Ok(())
        );
        assert_matches!(
            vm.verify_auto_deductions_for_addr(relocatable!(2, 1), &builtin),
            Ok(())
        );
    }

    #[test]
    /* Program used:
    %builtins output pedersen
    from starkware.cairo.common.cairo_builtins import HashBuiltin
    from starkware.cairo.common.hash import hash2
    from starkware.cairo.common.serialize import serialize_word

    func foo(hash_ptr : HashBuiltin*) -> (
        hash_ptr : HashBuiltin*, z
    ):
        # Use a with-statement, since 'hash_ptr' is not an
        # implicit argument.
        with hash_ptr:
            let (z) = hash2(32, 72)
        end
        return (hash_ptr=hash_ptr, z=z)
    end

    func main{output_ptr: felt*, pedersen_ptr: HashBuiltin*}():
        let (pedersen_ptr, a) = foo(pedersen_ptr)
        serialize_word(a)
        return()
    end
     */
    fn verify_auto_deductions_pedersen() {
        let mut builtin = HashBuiltinRunner::new(Some(8), true);
        builtin.base = 3;
        let mut vm = vm!();
        vm.builtin_runners.push(builtin.into());
        vm.segments = segments![((3, 0), 32), ((3, 1), 72)];
        assert_matches!(vm.verify_auto_deductions(), Ok(()));
    }

    #[test]
    fn can_get_return_values() {
        let mut vm = vm!();
        vm.set_ap(4);
        vm.segments = segments![((1, 0), 1), ((1, 1), 2), ((1, 2), 3), ((1, 3), 4)];
        let expected = vec![
            MaybeRelocatable::Int(Felt252::from(1_i32)),
            MaybeRelocatable::Int(Felt252::from(2_i32)),
            MaybeRelocatable::Int(Felt252::from(3_i32)),
            MaybeRelocatable::Int(Felt252::from(4_i32)),
        ];
        assert_eq!(vm.get_return_values(4).unwrap(), expected);
    }

    #[test]
    fn get_return_values_fails_when_ap_is_0() {
        let mut vm = vm!();
        vm.segments = segments![((1, 0), 1), ((1, 1), 2), ((1, 2), 3), ((1, 3), 4)];
        assert_matches!(vm.get_return_values(3),
            Err(MemoryError::FailedToGetReturnValues(bx))
            if *bx == (3, Relocatable::from((1,0))));
    }

    /*
    Program used for this test:
    from starkware.cairo.common.alloc import alloc
    func main{}():
        let vec: felt* = alloc()
        assert vec[0] = 1
        return()
    end
    Memory: {RelocatableValue(segment_index=0, offset=0): 290341444919459839,
        RelocatableValue(segment_index=0, offset=1): 1,
        RelocatableValue(segment_index=0, offset=2): 2345108766317314046,
        RelocatableValue(segment_index=0, offset=3): 1226245742482522112,
        RelocatableValue(segment_index=0, offset=4): 3618502788666131213697322783095070105623107215331596699973092056135872020478,
        RelocatableValue(segment_index=0, offset=5): 5189976364521848832,
        RelocatableValue(segment_index=0, offset=6): 1,
        RelocatableValue(segment_index=0, offset=7): 4611826758063128575,
        RelocatableValue(segment_index=0, offset=8): 2345108766317314046,
        RelocatableValue(segment_index=1, offset=0): RelocatableValue(segment_index=2, offset=0),
        RelocatableValue(segment_index=1, offset=1): RelocatableValue(segment_index=3, offset=0)}
     */

    #[test]
    fn test_step_for_preset_memory_with_alloc_hint() {
        let mut vm = vm!(true);
        let hint_data = vec![any_box!(HintProcessorData::new_default(
            "memory[ap] = segments.add()".to_string(),
            HashMap::new(),
        ))];

        //Initialzie registers
        run_context!(vm, 3, 2, 2);

        //Create program and execution segments
        for _ in 0..2 {
            vm.segments.add();
        }
        //Initialize memory

        let mut hint_processor = BuiltinHintProcessor::new_empty();

        vm.segments = segments![
            ((0, 0), 290341444919459839_i64),
            ((0, 1), 1),
            ((0, 2), 2345108766317314046_i64),
            ((0, 3), 1226245742482522112_i64),
            (
                (0, 4),
                (
                    "3618502788666131213697322783095070105623107215331596699973092056135872020478",
                    10
                )
            ),
            ((0, 5), 5189976364521848832_i64),
            ((0, 6), 1),
            ((0, 7), 4611826758063128575_i64),
            ((0, 8), 2345108766317314046_i64),
            ((1, 0), (2, 0)),
            ((1, 1), (3, 0))
        ];

        #[cfg(feature = "extensive_hints")]
        let mut hint_data = hint_data;

        //Run Steps
        for _ in 0..6 {
            #[cfg(not(feature = "extensive_hints"))]
            let mut hint_data = if vm.run_context.pc == (0, 0).into() {
                &hint_data[0..]
            } else {
                &hint_data[0..0]
            };
            assert_matches!(
                vm.step(
                    &mut hint_processor,
                    exec_scopes_ref!(),
                    &mut hint_data,
                    #[cfg(feature = "extensive_hints")]
                    &mut HashMap::from([(
                        Relocatable::from((0, 0)),
                        (0_usize, NonZeroUsize::new(1).unwrap())
                    )]),
                    #[cfg(feature = "test_utils")]
                    &HashMap::new(),
                ),
                Ok(())
            );
        }
        //Compare trace
        let trace = vm.trace.unwrap();
        trace_check(
            &trace,
            &[
                ((0, 3).into(), 2, 2),
                ((0, 0).into(), 4, 4),
                ((0, 2).into(), 5, 4),
                ((0, 5).into(), 5, 2),
                ((0, 7).into(), 6, 2),
                ((0, 8).into(), 6, 2),
            ],
        );

        //Compare final register values
        assert_eq!(vm.run_context.pc, Relocatable::from((3, 0)));
        assert_eq!(vm.run_context.ap, 6);
        assert_eq!(vm.run_context.fp, 0);

        //Check that the array created through alloc contains the element we inserted
        //As there are no builtins present, the next segment crated will have the index 2
        check_memory!(vm.segments.memory, ((2, 0), 1));
    }

    #[test]
    fn test_get_builtin_runners() {
        let mut vm = vm!();
        let hash_builtin = HashBuiltinRunner::new(Some(8), true);
        let bitwise_builtin = BitwiseBuiltinRunner::new(Some(256), true);
        vm.builtin_runners.push(hash_builtin.into());
        vm.builtin_runners.push(bitwise_builtin.into());

        let builtins = vm.get_builtin_runners();

        assert_eq!(builtins[0].name(), BuiltinName::pedersen);
        assert_eq!(builtins[1].name(), BuiltinName::bitwise);
    }

    #[test]
    fn test_get_output_builtin_mut() {
        let mut vm = vm!();

        assert_matches!(
            vm.get_output_builtin_mut(),
            Err(VirtualMachineError::NoOutputBuiltin)
        );

        let output_builtin = OutputBuiltinRunner::new(true);
        vm.builtin_runners.push(output_builtin.clone().into());

        let vm_output_builtin = vm
            .get_output_builtin_mut()
            .expect("Output builtin should be returned");

        assert_eq!(vm_output_builtin.base(), output_builtin.base());
        assert_eq!(vm_output_builtin.pages, output_builtin.pages);
        assert_eq!(vm_output_builtin.attributes, output_builtin.attributes);
        assert_eq!(vm_output_builtin.stop_ptr, output_builtin.stop_ptr);
        assert_eq!(vm_output_builtin.included, output_builtin.included);
    }

    #[test]
    fn get_range_for_continuous_memory() {
        let mut vm = vm!();
        vm.segments = segments![((1, 0), 2), ((1, 1), 3), ((1, 2), 4)];

        let value1 = MaybeRelocatable::from(Felt252::from(2_i32));
        let value2 = MaybeRelocatable::from(Felt252::from(3_i32));
        let value3 = MaybeRelocatable::from(Felt252::from(4_i32));

        let expected_vec = vec![
            Some(Cow::Borrowed(&value1)),
            Some(Cow::Borrowed(&value2)),
            Some(Cow::Borrowed(&value3)),
        ];
        assert_eq!(vm.get_range(Relocatable::from((1, 0)), 3), expected_vec);
    }

    #[test]
    fn get_range_for_non_continuous_memory() {
        let mut vm = vm!();
        vm.segments = segments![((1, 0), 2), ((1, 1), 3), ((1, 3), 4)];

        let value1 = MaybeRelocatable::from(Felt252::from(2_i32));
        let value2 = MaybeRelocatable::from(Felt252::from(3_i32));
        let value3 = MaybeRelocatable::from(Felt252::from(4_i32));

        let expected_vec = vec![
            Some(Cow::Borrowed(&value1)),
            Some(Cow::Borrowed(&value2)),
            None,
            Some(Cow::Borrowed(&value3)),
        ];
        assert_eq!(vm.get_range(Relocatable::from((1, 0)), 4), expected_vec);
    }

    #[test]
    fn get_continuous_range_for_continuous_memory() {
        let mut vm = vm!();
        vm.segments = segments![((1, 0), 2), ((1, 1), 3), ((1, 2), 4)];

        let value1 = MaybeRelocatable::from(Felt252::from(2_i32));
        let value2 = MaybeRelocatable::from(Felt252::from(3_i32));
        let value3 = MaybeRelocatable::from(Felt252::from(4_i32));

        let expected_vec = vec![value1, value2, value3];
        assert_eq!(
            vm.get_continuous_range(Relocatable::from((1, 0)), 3),
            Ok(expected_vec)
        );
    }

    #[test]
    fn get_continuous_range_for_non_continuous_memory() {
        let mut vm = vm!();
        vm.segments = segments![((1, 0), 2), ((1, 1), 3), ((1, 3), 4)];

        assert_eq!(
            vm.get_continuous_range(Relocatable::from((1, 0)), 3),
            Err(MemoryError::GetRangeMemoryGap(Box::new(((1, 0).into(), 3))))
        );
    }

    #[test]
    fn get_segment_used_size_after_computing_used() {
        let mut vm = vm!();
        vm.segments = segments![
            ((0, 2), 1),
            ((0, 5), 1),
            ((0, 7), 1),
            ((1, 1), 1),
            ((2, 2), 1),
            ((2, 4), 1),
            ((2, 7), 1)
        ];
        vm.segments.compute_effective_sizes();
        assert_eq!(Some(8), vm.get_segment_used_size(2));
    }

    #[test]
    fn get_segment_size_before_computing_used() {
        let vm = vm!();
        assert_eq!(None, vm.get_segment_size(2));
    }

    #[test]
    fn get_segment_size_before_computing_used_set_size() {
        let mut vm = vm!();
        vm.segments.segment_sizes.insert(2, 2);
        assert_eq!(Some(2), vm.get_segment_size(2));
    }

    #[test]
    fn get_segment_size_after_computing_used() {
        let mut vm = vm!();
        vm.segments = segments![
            ((0, 2), 1),
            ((0, 5), 1),
            ((0, 7), 1),
            ((1, 1), 1),
            ((2, 2), 1),
            ((2, 4), 1),
            ((2, 7), 1)
        ];
        vm.segments.compute_effective_sizes();
        assert_eq!(Some(8), vm.get_segment_size(2));
    }

    #[test]
    fn get_segment_used_size_before_computing_used() {
        let vm = vm!();
        assert_eq!(None, vm.get_segment_used_size(2));
    }

    #[test]
    fn get_and_set_pc() {
        let mut vm = vm!();
        vm.set_pc(Relocatable {
            segment_index: 3,
            offset: 4,
        });
        assert_eq!(
            vm.get_pc(),
            Relocatable {
                segment_index: 3,
                offset: 4
            }
        )
    }

    #[test]
    fn get_and_set_fp() {
        let mut vm = vm!();
        vm.set_fp(3);
        assert_eq!(
            vm.get_fp(),
            Relocatable {
                segment_index: 1,
                offset: 3
            }
        )
    }

    #[test]
    fn get_maybe_key_not_in_memory() {
        let vm = vm!();
        assert_eq!(
            vm.get_maybe(&Relocatable {
                segment_index: 5,
                offset: 2
            }),
            None
        );
    }

    #[test]
    fn get_maybe_error() {
        let vm = vm!();
        assert_eq!(
            vm.get_maybe(&MaybeRelocatable::Int(Felt252::from(0_i32))),
            None,
        );
    }

    #[test]
    fn end_run_error() {
        let mut vm = vm!();
        let scopes = exec_scopes_ref!();
        scopes.enter_scope(HashMap::new());

        assert_matches!(
            vm.end_run(scopes, false),
            Err(VirtualMachineError::MainScopeError(
                ExecScopeError::NoScopeError
            ))
        );
    }

    #[test]
    fn add_temporary_segments() {
        let mut vm = vm!();
        let mut _base = vm.add_temporary_segment();
        assert_eq!(
            _base,
            Relocatable {
                segment_index: -1,
                offset: 0
            }
        );
        let mut _base = vm.add_temporary_segment();
        assert_eq!(
            _base,
            Relocatable {
                segment_index: -2,
                offset: 0
            }
        );
    }

    #[test]
    fn decode_current_instruction_invalid_encoding() {
        let mut vm = vm!();
        vm.segments = segments![((0, 0), ("112233445566778899112233445566778899", 16))];
        assert_matches!(
            vm.decode_current_instruction(),
            Err(VirtualMachineError::InvalidInstructionEncoding)
        );
    }

    #[test]
    fn add_relocation_rule_test() {
        let mut vm = vm!();

        assert_eq!(
            vm.add_relocation_rule((-1, 0).into(), (1, 2).into()),
            Ok(()),
        );
        assert_eq!(
            vm.add_relocation_rule((-2, 0).into(), (-1, 1).into()),
            Ok(()),
        );
        assert_eq!(
            vm.add_relocation_rule((5, 0).into(), (0, 0).into()),
            Err(MemoryError::AddressNotInTemporarySegment(5)),
        );
        assert_eq!(
            vm.add_relocation_rule((-3, 6).into(), (0, 0).into()),
            Err(MemoryError::NonZeroOffset(6)),
        );
        assert_eq!(
            vm.add_relocation_rule((-1, 0).into(), (0, 0).into()),
            Err(MemoryError::DuplicatedRelocation(-1)),
        );
    }

    #[test]
    fn gen_arg_relocatable() {
        let mut vm = vm!();

        assert_matches!(
            vm.gen_arg(&mayberelocatable!(0, 0)),
            Ok(x) if x == mayberelocatable!(0, 0)
        );
    }

    /// Test that the call to .gen_arg() with a bigint and no prime number just
    /// passes the value through.
    #[test]
    fn gen_arg_bigint() {
        let mut vm = vm!();

        assert_matches!(
            vm.gen_arg(&mayberelocatable!(1234)),
            Ok(x) if x == mayberelocatable!(1234)
        );
    }

    /// Test that the call to .gen_arg() with a bigint and a prime number passes
    /// the value through after applying the modulo.
    #[test]
    fn gen_arg_bigint_prime() {
        let mut vm = vm!();
        let prime = felt_hex!(crate::utils::PRIME_STR);
        let prime_maybe = MaybeRelocatable::from(prime);

        assert_matches!(vm.gen_arg(&prime_maybe), Ok(x) if x == mayberelocatable!(0));
    }

    /// Test that the call to .gen_arg() with a Vec<MaybeRelocatable> writes its
    /// contents into a new segment and returns a pointer to it.
    #[test]
    fn gen_arg_vec() {
        let mut vm = vm!();

        assert_matches!(
            vm.gen_arg(&vec![
                mayberelocatable!(0),
                mayberelocatable!(1),
                mayberelocatable!(2),
                mayberelocatable!(3),
                mayberelocatable!(0, 0),
                mayberelocatable!(0, 1),
                mayberelocatable!(0, 2),
                mayberelocatable!(0, 3),
            ]),
            Ok(x) if x == mayberelocatable!(0, 0)
        );
    }

    /// Test that compute_effective_sizes() works as intended.
    #[test]
    fn compute_effective_sizes() {
        let mut vm = vm!();

        let segment = vm.segments.add();
        vm.load_data(
            segment,
            &[
                mayberelocatable!(1),
                mayberelocatable!(2),
                mayberelocatable!(3),
                mayberelocatable!(4),
            ],
        )
        .expect("Could not load data into memory.");

        assert_eq!(vm.segments.compute_effective_sizes(), &vec![4]);
    }

    /// Test that compute_segment_effective_sizes() works as intended.
    #[test]
    fn compute_segment_effective_sizes() {
        let mut vm = vm!();

        let segment = vm.segments.add();
        vm.load_data(
            segment,
            &[
                mayberelocatable!(1),
                mayberelocatable!(2),
                mayberelocatable!(3),
                mayberelocatable!(4),
            ],
        )
        .expect("Could not load data into memory.");

        assert_eq!(vm.segments.compute_effective_sizes(), &vec![4]);
    }

    #[test]
    fn test_delete_unaccessed() {
        let mut vm = vm!();

        let segment0 = vm.segments.add();
        let segment1 = vm.segments.add();
        let segment2 = vm.segments.add();
        let segment3 = vm.segments.add();
        let tmp_segment = vm.add_temporary_segment();
        assert_eq!(segment0.segment_index, 0);
        assert_eq!(segment1.segment_index, 1);
        assert_eq!(segment2.segment_index, 2);
        assert_eq!(segment3.segment_index, 3);
        assert_eq!(tmp_segment.segment_index, -1);
        vm.segments.memory = memory![
            ((0, 1), 1),
            ((1, 0), 3),
            ((1, 1), 4),
            ((2, 0), 7),
            ((3, 0), 7),
            ((-1, 0), 5),
            ((-1, 1), 5),
            ((-1, 2), 5)
        ];
        vm.run_finished = true;

        vm.mark_address_range_as_accessed((2, 0).into(), 1).unwrap();

        let cell0 = Relocatable::from((0, 0));
        let cell1 = Relocatable::from((1, 1));
        let cell2 = Relocatable::from((2, 0));
        let cell3 = Relocatable::from((3, 7));
        let cell7 = Relocatable::from((7, 17));
        let cell_tmp = Relocatable::from((-1, 1));
        vm.delete_unaccessed(cell0).unwrap();
        vm.delete_unaccessed(cell1).unwrap();
        vm.delete_unaccessed(cell_tmp).unwrap();

        // Check that the cells were set to NONE.
        assert!(vm
            .segments
            .memory
            .get_cell_for_testing(cell0)
            .unwrap()
            .is_none());
        assert!(vm
            .segments
            .memory
            .get_cell_for_testing(cell1)
            .unwrap()
            .is_none());
        assert!(vm
            .segments
            .memory
            .get_cell_for_testing(cell_tmp)
            .unwrap()
            .is_none());
        // Segment 3 cell was out of offset range, so it should not be modified or allocated.
        assert!(vm.segments.memory.get_cell_for_testing(cell3).is_none());
        // Segment 2 cell was accessed, so attempting to unset the memory should result in error.
        assert_matches!(
            vm.delete_unaccessed(cell2).unwrap_err(),
            MemoryError::UnsetAccessedCell(relocatable) if relocatable == cell2
        );
        // Segment 3 is unallocated, so attempting to unset the memory should result in error.
        assert_matches!(
            vm.delete_unaccessed(cell3).unwrap_err(),
            MemoryError::UnsetUnallocatedCell(relocatable) if relocatable == cell3
        );
        // Segment 7 was not allocated, so attempting to unset the memory should result in error.
        assert_matches!(
            vm.delete_unaccessed(cell7).unwrap_err(),
            MemoryError::UnallocatedSegment(boxed)
            if *boxed == (cell7.segment_index.try_into().unwrap(), vm.segments.memory.data.len())
        );
    }

    #[test]
    fn mark_as_accessed() {
        let mut vm = vm!();
        vm.run_finished = true;
        vm.segments.memory = memory![
            ((0, 0), 0),
            ((0, 1), 0),
            ((0, 2), 1),
            ((0, 10), 10),
            ((1, 1), 1),
            ((1, 2), 0),
            ((2, 0), 0),
            ((-1, 0), 0),
            ((-1, 1), 0)
        ];
        vm.mark_address_range_as_accessed((0, 0).into(), 3).unwrap();
        vm.mark_address_range_as_accessed((0, 10).into(), 2)
            .unwrap();
        vm.mark_address_range_as_accessed((1, 1).into(), 1).unwrap();
        vm.mark_address_range_as_accessed((-1, 0).into(), 1)
            .unwrap();
        //Check that the following addresses have been accessed:
        // Addresses have been copied from python execution:
        let mem = &vm.segments.memory.data;
        assert!(mem[0][0].is_accessed());
        assert!(mem[0][1].is_accessed());
        assert!(mem[0][2].is_accessed());
        assert!(mem[0][10].is_accessed());
        assert!(mem[1][1].is_accessed());
        assert_eq!(
            vm.segments
                .memory
                .get_amount_of_accessed_addresses_for_segment(0),
            Some(4)
        );
        assert_eq!(
            vm.segments
                .memory
                .get_amount_of_accessed_addresses_for_segment(1),
            Some(1)
        );
        assert!(vm.is_accessed(&Relocatable::from((0, 0))).unwrap());
        assert!(vm.is_accessed(&Relocatable::from((0, 2))).unwrap());
        assert!(vm.is_accessed(&Relocatable::from((0, 10))).unwrap());
        assert!(vm.is_accessed(&Relocatable::from((1, 1))).unwrap());
        assert!(!vm.is_accessed(&Relocatable::from((1, 2))).unwrap());
        assert!(!vm.is_accessed(&Relocatable::from((2, 0))).unwrap());
        assert!(vm.is_accessed(&Relocatable::from((-1, 0))).unwrap());
        assert!(!vm.is_accessed(&Relocatable::from((-1, 1))).unwrap());
    }

    #[test]
    fn mark_as_accessed_run_not_finished() {
        let mut vm = vm!();
        assert_matches!(
            vm.mark_address_range_as_accessed((0, 0).into(), 3),
            Err(VirtualMachineError::RunNotFinished)
        );
    }

    #[test]
    fn mark_as_accessed_missing_accessed_addresses() {
        let mut vm = vm!();
        assert_matches!(
            vm.mark_address_range_as_accessed((0, 0).into(), 3),
            Err(VirtualMachineError::RunNotFinished)
        );
    }

    #[test]
    fn get_u32_range_ok() {
        let mut vm = vm!();
        vm.segments.memory = memory![((0, 0), 0), ((0, 1), 1), ((0, 2), 4294967295), ((0, 3), 3)];
        let expected_vector = vec![1, 4294967295];
        assert_eq!(vm.get_u32_range((0, 1).into(), 2), Ok(expected_vector));
    }

    #[test]
    fn get_u32_range_relocatable() {
        let mut vm = vm!();
        vm.segments.memory = memory![((0, 0), 0), ((0, 1), 1), ((0, 2), (0, 0)), ((0, 3), 3)];
        assert_matches!(vm.get_u32_range((0, 1).into(), 2), Err(MemoryError::ExpectedInteger(bx)) if *bx == (0, 2).into());
    }

    #[test]
    fn get_u32_range_over_32_bits() {
        let mut vm = vm!();
        vm.segments.memory = memory![((0, 0), 0), ((0, 1), 1), ((0, 2), 4294967296), ((0, 3), 3)];
        assert_matches!(vm.get_u32_range((0, 1).into(), 2), Err(MemoryError::Math(MathError::Felt252ToU32Conversion(bx))) if *bx == Felt252::from(4294967296_u64));
    }

    #[test]
    fn get_u32_range_memory_gap() {
        let mut vm = vm!();
        vm.segments.memory = memory![((0, 0), 0), ((0, 1), 1), ((0, 3), 3)];
        assert_matches!(vm.get_u32_range((0, 1).into(), 3), Err(MemoryError::UnknownMemoryCell(bx)) if *bx == (0, 2).into());
    }

    #[test]
    fn handle_blake2s_instruction_state_too_short() {
        let mut vm = vm!();
        vm.segments.memory = memory![
            ((0, 0), 0),
            ((0, 1), 0),
            ((0, 2), 0),
            ((0, 3), 0),
            ((0, 4), 0),
            ((0, 5), 0),
            ((0, 6), 0),
            ((2, 0), (0, 0))
        ];
        let operands_addresses = OperandsAddresses {
            dst_addr: (0, 0).into(),
            op0_addr: (2, 0).into(),
            op1_addr: (2, 0).into(),
        };
        vm.run_context = RunContext {
            pc: (0, 0).into(),
            ap: 0,
            fp: 0,
        };

        assert_matches!(
            vm.handle_blake2s_instruction(&operands_addresses, false),
            Err(VirtualMachineError::Memory(MemoryError::UnknownMemoryCell(bx))) if *bx == (0, 7).into()
        );
    }

    #[test]
    fn handle_blake2s_instruction_message_too_short() {
        let mut vm = vm!();
        vm.segments.memory = memory![
            ((0, 0), 0),
            ((0, 1), 0),
            ((0, 2), 0),
            ((0, 3), 0),
            ((0, 4), 0),
            ((0, 5), 0),
            ((0, 6), 0),
            ((0, 7), 0),
            ((2, 0), (0, 0))
        ];
        let operands_addresses = OperandsAddresses {
            dst_addr: (0, 0).into(),
            op0_addr: (2, 0).into(),
            op1_addr: (2, 0).into(),
        };
        vm.run_context = RunContext {
            pc: (0, 0).into(),
            ap: 0,
            fp: 0,
        };

        assert_matches!(
            vm.handle_blake2s_instruction(&operands_addresses, false),
            Err(VirtualMachineError::Memory(MemoryError::UnknownMemoryCell(bx))) if *bx == (0, 8).into()
        );
    }

    #[test]
    fn handle_blake2s_instruction_ap_points_to_inconsistent_memory() {
        let mut vm = vm!();
        vm.segments.memory = memory![
            ((0, 0), 0),
            ((0, 1), 0),
            ((0, 2), 0),
            ((0, 3), 0),
            ((0, 4), 0),
            ((0, 5), 0),
            ((0, 6), 0),
            ((0, 7), 0),
            ((0, 8), 0),
            ((0, 9), 0),
            ((0, 10), 0),
            ((0, 11), 0),
            ((0, 12), 0),
            ((0, 13), 0),
            ((0, 14), 0),
            ((0, 15), 0),
            ((1, 0), (0, 0))
        ];
        let operands_addresses = OperandsAddresses {
            dst_addr: (0, 0).into(),
            op0_addr: (1, 0).into(),
            op1_addr: (1, 0).into(),
        };
        vm.run_context = RunContext {
            pc: (0, 0).into(),
            ap: 0,
            fp: 0,
        };

        assert_matches!(
            vm.handle_blake2s_instruction(&operands_addresses, false),
            Err(VirtualMachineError::Memory(MemoryError::InconsistentMemory(bx))) if *bx == ((0, 0).into(),0.into(),1848029226.into())
        );
    }

    #[test]
    fn handle_blake2s_instruction_ok() {
        let mut vm = vm!();
        vm.segments.memory = memory![
            // State
            ((0, 0), 0x6B08E647),
            ((0, 1), 0xBB67AE85),
            ((0, 2), 0x3C6EF372),
            ((0, 3), 0xA54FF53A),
            ((0, 4), 0x510E527F),
            ((0, 5), 0x9B05688C),
            ((0, 6), 0x1F83D9AB),
            ((0, 7), 0x5BE0CD19),
            // Message
            ((0, 8), 930933030),
            ((0, 9), 1766240503),
            ((0, 10), 3660871006),
            ((0, 11), 388409270),
            ((0, 12), 1948594622),
            ((0, 13), 3119396969),
            ((0, 14), 3924579183),
            ((0, 15), 2089920034),
            ((0, 16), 3857888532),
            ((0, 17), 929304360),
            ((0, 18), 1810891574),
            ((0, 19), 860971754),
            ((0, 20), 1822893775),
            ((0, 21), 2008495810),
            ((0, 22), 2958962335),
            ((0, 23), 2340515744),
            // Counter
            ((0, 24), 64),
            // AP
            ((1, 0), (0, 25)),
            ((2, 0), (0, 0)),
            ((2, 1), (0, 8))
        ];
        let operands_addresses = OperandsAddresses {
            dst_addr: (0, 24).into(),
            op0_addr: (2, 0).into(),
            op1_addr: (2, 1).into(),
        };
        vm.run_context = RunContext {
            pc: (0, 0).into(),
            ap: 0,
            fp: 0,
        };
        assert_matches!(
            vm.handle_blake2s_instruction(&operands_addresses, false),
            Ok(())
        );

        let state: [u32; 8] = vm
            .get_u32_range((0, 0).into(), 8)
            .unwrap()
            .try_into()
            .unwrap();
        let message: [u32; 16] = vm
            .get_u32_range((0, 8).into(), 16)
            .unwrap()
            .try_into()
            .unwrap();
        let counter = vm.segments.memory.get_u32((0, 24).into()).unwrap();

        let expected_new_state: [u32; 8] = blake2s_compress(&state, &message, counter, 0, 0, 0)
            .try_into()
            .unwrap();

        let new_state: [u32; 8] = vm
            .get_u32_range((0, 25).into(), 8)
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(new_state, expected_new_state);
    }

    #[test]
    fn handle_blake2s_last_block_instruction_ok() {
        let mut vm = vm!();
        vm.segments.memory = memory![
            // State
            ((0, 0), 0x6B08E647),
            ((0, 1), 0xBB67AE85),
            ((0, 2), 0x3C6EF372),
            ((0, 3), 0xA54FF53A),
            ((0, 4), 0x510E527F),
            ((0, 5), 0x9B05688C),
            ((0, 6), 0x1F83D9AB),
            ((0, 7), 0x5BE0CD19),
            // Message
            ((0, 8), 930933030),
            ((0, 9), 1766240503),
            ((0, 10), 3660871006),
            ((0, 11), 388409270),
            ((0, 12), 1948594622),
            ((0, 13), 3119396969),
            ((0, 14), 3924579183),
            ((0, 15), 2089920034),
            ((0, 16), 3857888532),
            ((0, 17), 929304360),
            ((0, 18), 1810891574),
            ((0, 19), 860971754),
            ((0, 20), 1822893775),
            ((0, 21), 2008495810),
            ((0, 22), 2958962335),
            ((0, 23), 2340515744),
            // Counter
            ((0, 24), 64),
            // AP
            ((1, 0), (0, 25)),
            ((2, 0), (0, 0)),
            ((2, 1), (0, 8))
        ];
        let operands_addresses = OperandsAddresses {
            dst_addr: (0, 24).into(),
            op0_addr: (2, 0).into(),
            op1_addr: (2, 1).into(),
        };
        vm.run_context = RunContext {
            pc: (0, 0).into(),
            ap: 0,
            fp: 0,
        };
        assert_matches!(
            vm.handle_blake2s_instruction(&operands_addresses, true),
            Ok(())
        );

        let state: [u32; 8] = vm
            .get_u32_range((0, 0).into(), 8)
            .unwrap()
            .try_into()
            .unwrap();
        let message: [u32; 16] = vm
            .get_u32_range((0, 8).into(), 16)
            .unwrap()
            .try_into()
            .unwrap();
        let counter = vm.segments.memory.get_u32((0, 24).into()).unwrap();

        let expected_new_state: [u32; 8] =
            blake2s_compress(&state, &message, counter, 0, 0xffffffff, 0)
                .try_into()
                .unwrap();

        let new_state: [u32; 8] = vm
            .get_u32_range((0, 25).into(), 8)
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(new_state, expected_new_state);
    }

    #[test]
    fn get_traceback_entries_bad_usort() {
        let program = Program::from_bytes(
            include_bytes!("../../../cairo_programs/bad_programs/bad_usort.json"),
            Some("main"),
        )
        .unwrap();

        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let mut cairo_runner = cairo_runner!(program, LayoutName::all_cairo, false);

        let end = cairo_runner.initialize(false).unwrap();
        assert!(cairo_runner.run_until_pc(end, &mut hint_processor).is_err());
        let expected_traceback = vec![
            (Relocatable::from((1, 3)), Relocatable::from((0, 97))),
            (Relocatable::from((1, 14)), Relocatable::from((0, 30))),
            (Relocatable::from((1, 26)), Relocatable::from((0, 60))),
        ];
        assert_eq!(cairo_runner.vm.get_traceback_entries(), expected_traceback);
    }

    #[test]
    fn get_traceback_entries_bad_dict_update() {
        let program = Program::from_bytes(
            include_bytes!("../../../cairo_programs/bad_programs/bad_dict_update.json"),
            Some("main"),
        )
        .unwrap();

        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let mut cairo_runner = cairo_runner!(program, LayoutName::all_cairo, false);

        let end = cairo_runner.initialize(false).unwrap();
        assert!(cairo_runner.run_until_pc(end, &mut hint_processor).is_err());
        let expected_traceback = vec![(Relocatable::from((1, 2)), Relocatable::from((0, 34)))];
        assert_eq!(cairo_runner.vm.get_traceback_entries(), expected_traceback);
    }

    #[test]
    fn builder_test() {
        let virtual_machine_builder: VirtualMachineBuilder = VirtualMachineBuilder::default()
            .run_finished(true)
            .current_step(12)
            .builtin_runners(vec![BuiltinRunner::from(HashBuiltinRunner::new(
                Some(12),
                true,
            ))])
            .run_context(RunContext {
                pc: Relocatable::from((0, 0)),
                ap: 18,
                fp: 0,
            })
            .segments({
                let mut segments = MemorySegmentManager::new();
                segments.segment_used_sizes = Some(vec![1]);
                segments
            })
            .skip_instruction_execution(true)
            .max_traceback_entries(100)
            .trace(Some(vec![TraceEntry {
                pc: (0, 1).into(),
                ap: 1,
                fp: 1,
            }]));

        #[cfg(feature = "test_utils")]
        fn before_first_step_hook(
            _vm: &mut VirtualMachine,
            _hint_data: &[Box<dyn Any>],
        ) -> Result<(), VirtualMachineError> {
            Err(VirtualMachineError::Unexpected)
        }
        #[cfg(feature = "test_utils")]
        let virtual_machine_builder =
            virtual_machine_builder.hooks(Box::new(crate::vm::hooks::Hooks::new(
                Some(std::sync::Arc::new(before_first_step_hook)),
                None,
                None,
            )));

        #[allow(unused_mut)]
        let mut virtual_machine_from_builder = virtual_machine_builder.build();

        assert!(virtual_machine_from_builder.run_finished);
        assert_eq!(virtual_machine_from_builder.get_current_step(), 12);
        assert_eq!(
            virtual_machine_from_builder
                .builtin_runners
                .first()
                .unwrap()
                .name(),
            BuiltinName::pedersen
        );
        assert_eq!(virtual_machine_from_builder.run_context.ap, 18,);
        assert_eq!(
            virtual_machine_from_builder.segments.segment_used_sizes,
            Some(vec![1])
        );
        assert!(virtual_machine_from_builder.skip_instruction_execution,);
        assert_eq!(
            virtual_machine_from_builder.trace,
            Some(vec![TraceEntry {
                pc: (0, 1).into(),
                ap: 1,
                fp: 1,
            }])
        );
        #[cfg(feature = "test_utils")]
        {
            let program = crate::types::program::Program::from_bytes(
                include_bytes!("../../../cairo_programs/sqrt.json"),
                Some("main"),
            )
            .expect("Call to `Program::from_file()` failed.");
            let mut hint_processor = BuiltinHintProcessor::new_empty();
            let mut cairo_runner = cairo_runner!(program);
            cairo_runner.vm = virtual_machine_from_builder;
            let end = cairo_runner.initialize(false).unwrap();

            assert!(cairo_runner.run_until_pc(end, &mut hint_processor).is_err());
        }
    }

    #[test]
    /// Test for a simple program execution
    /// Used program code:
    /// func main():
    ///     let a = 1
    ///     let b = 2
    ///     let c = a + b
    ///     return()
    /// end
    /// Memory taken from original vm
    /// {RelocatableValue(segment_index=0, offset=0): 2345108766317314046,
    ///  RelocatableValue(segment_index=1, offset=0): RelocatableValue(segment_index=2, offset=0),
    ///  RelocatableValue(segment_index=1, offset=1): RelocatableValue(segment_index=3, offset=0)}
    /// Current register values:
    /// AP 1:2
    /// FP 1:2
    /// PC 0:0
    fn test_step_for_preset_memory_program_loaded_into_user_segment() {
        let mut vm = vm!(true);

        let mut hint_processor = BuiltinHintProcessor::new_empty();

        run_context!(vm, 0, 2, 2);

        vm.segments = segments![
            ((2, 0), 2345108766317314046_u64), // Load program into new segment
            ((1, 0), (2, 0)),
            ((1, 1), (3, 0))
        ];
        // set starting pc on new segemt to run loaded program
        vm.run_context.pc.segment_index = 2;

        assert_matches!(
            vm.step(
                &mut hint_processor,
                exec_scopes_ref!(),
                &mut Vec::new(),
                #[cfg(feature = "extensive_hints")]
                &mut HashMap::new(),
                #[cfg(feature = "test_utils")]
                &HashMap::new()
            ),
            Ok(())
        );
        let trace = vm.trace.unwrap();
        trace_check(&trace, &[((2, 0).into(), 2, 2)]);

        assert_eq!(vm.run_context.pc, Relocatable::from((3, 0)));
        assert_eq!(vm.run_context.ap, 2);
        assert_eq!(vm.run_context.fp, 0);

        //Check that the following addresses have been accessed:
        // Addresses have been copied from python execution:
        let mem = vm.segments.memory.data;
        assert!(mem[1][0].is_accessed());
        assert!(mem[1][1].is_accessed());
    }

    #[test]
    /*
    Test for a simple program execution
    Used program code:
        func myfunc(a: felt) -> (r: felt):
            let b = a * 2
            return(b)
        end
        func main():
            let a = 1
            let b = myfunc(a)
            return()
        end
    Memory taken from original vm:
    {RelocatableValue(segment_index=0, offset=0): 5207990763031199744,
    RelocatableValue(segment_index=0, offset=1): 2,
    RelocatableValue(segment_index=0, offset=2): 2345108766317314046,
    RelocatableValue(segment_index=0, offset=3): 5189976364521848832,
    RelocatableValue(segment_index=0, offset=4): 1,
    RelocatableValue(segment_index=0, offset=5): 1226245742482522112,
    RelocatableValue(segment_index=0, offset=6): 3618502788666131213697322783095070105623107215331596699973092056135872020476,
    RelocatableValue(segment_index=0, offset=7): 2345108766317314046,
    RelocatableValue(segment_index=1, offset=0): RelocatableValue(segment_index=2, offset=0),
    RelocatableValue(segment_index=1, offset=1): RelocatableValue(segment_index=3, offset=0)}
    Current register values:
    AP 1:2
    FP 1:2
    PC 0:3
    Final Pc (not executed): 3:0
    This program consists of 5 steps
    */
    fn test_step_for_preset_memory_function_call_program_loaded_into_user_segment() {
        let mut vm = vm!(true);

        run_context!(vm, 3, 2, 2);
        // set starting pc on new segemt to run loaded program
        vm.run_context.pc.segment_index = 4;

        //Insert values into memory
        vm.segments.memory =
            memory![
            // Load program into new segment
            ((4, 0), 5207990763031199744_i64),
            ((4, 1), 2),
            ((4, 2), 2345108766317314046_i64),
            ((4, 3), 5189976364521848832_i64),
            ((4, 4), 1),
            ((4, 5), 1226245742482522112_i64),
            (
                (4, 6),
                ("3618502788666131213697322783095070105623107215331596699973092056135872020476",10)
            ),
            ((4, 7), 2345108766317314046_i64),
            ((1, 0), (2, 0)),
            ((1, 1), (3, 0))
        ];

        let final_pc = Relocatable::from((3, 0));
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        //Run steps
        while vm.run_context.pc != final_pc {
            assert_matches!(
                vm.step(
                    &mut hint_processor,
                    exec_scopes_ref!(),
                    &mut Vec::new(),
                    #[cfg(feature = "extensive_hints")]
                    &mut HashMap::new(),
                    #[cfg(feature = "test_utils")]
                    &HashMap::new()
                ),
                Ok(())
            );
        }

        //Check final register values
        assert_eq!(vm.run_context.pc, Relocatable::from((3, 0)));

        assert_eq!(vm.run_context.ap, 6);

        assert_eq!(vm.run_context.fp, 0);
        //Check each TraceEntry in trace
        let trace = vm.trace.unwrap();
        assert_eq!(trace.len(), 5);
        trace_check(
            &trace,
            &[
                ((4, 3).into(), 2, 2),
                ((4, 5).into(), 3, 2),
                ((4, 0).into(), 5, 5),
                ((4, 2).into(), 6, 5),
                ((4, 7).into(), 6, 2),
            ],
        );
        //Check that the following addresses have been accessed:
        // Addresses have been copied from python execution + addresses of accessed code:
        let mem = &vm.segments.memory.data;
        assert!(mem[4][0].is_accessed());
        assert!(mem[4][1].is_accessed());
        assert!(mem[4][2].is_accessed());
        assert!(mem[4][3].is_accessed());
        assert!(mem[4][4].is_accessed());
        assert!(mem[4][5].is_accessed());
        assert!(mem[4][6].is_accessed());
        assert!(mem[4][7].is_accessed());
        assert!(mem[1][0].is_accessed());
        assert!(mem[1][1].is_accessed());
        assert!(mem[1][2].is_accessed());
        assert!(mem[1][3].is_accessed());
        assert!(mem[1][4].is_accessed());
        assert!(mem[1][5].is_accessed());
        assert_eq!(
            vm.segments
                .memory
                .get_amount_of_accessed_addresses_for_segment(4),
            Some(8)
        );
        assert_eq!(
            vm.segments
                .memory
                .get_amount_of_accessed_addresses_for_segment(1),
            Some(6)
        );
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn test_extended_execution_resources() {
        let result = ExtendedExecutionResourceType::try_from(OpcodeExtension::Stone);

        assert_eq!(
            result,
            Err("Unsupported OpcodeExtension for ExtendedExecutionResourceType")
        );
    }
}
