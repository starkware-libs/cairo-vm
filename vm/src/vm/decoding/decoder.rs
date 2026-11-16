use crate::{
    types::instruction::{
        ApUpdate, FpUpdate, Instruction, Op1Addr, Opcode, OpcodeExtension, PcUpdate, Register, Res,
    },
    vm::errors::vm_errors::VirtualMachineError,
};

// opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
//  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0

/// Decodes an instruction. The encoding is little endian, so flags go from bit 127 to 48.
/// The bits 63 and beyond are reserved for the opcode extension.
/// opcode_extension_num=0 means the instruction is a Stone instruction.
/// opcode_extension_num>0 is for new Stwo opcodes.
pub fn decode_instruction(encoded_instr: u128) -> Result<Instruction, VirtualMachineError> {
    const DST_REG_MASK: u128 = 0x0001;
    const DST_REG_OFF: u128 = 0;
    const OP0_REG_MASK: u128 = 0x0002;
    const OP0_REG_OFF: u128 = 1;
    const OP1_SRC_MASK: u128 = 0x001C;
    const OP1_SRC_OFF: u128 = 2;
    const RES_LOGIC_MASK: u128 = 0x0060;
    const RES_LOGIC_OFF: u128 = 5;
    const PC_UPDATE_MASK: u128 = 0x0380;
    const PC_UPDATE_OFF: u128 = 7;
    const AP_UPDATE_MASK: u128 = 0x0C00;
    const AP_UPDATE_OFF: u128 = 10;
    const OPCODE_MASK: u128 = 0x7000;
    const OPCODE_OFF: u128 = 12;
    const OPCODE_EXTENSION_OFF: u128 = 63;

    // Flags start on the 48th bit.
    const FLAGS_OFFSET: u128 = 48;
    const OFF0_OFF: u128 = 0;
    const OFF1_OFF: u128 = 16;
    const OFF2_OFF: u128 = 32;
    const OFFX_MASK: u128 = 0xFFFF;

    // Grab offsets and convert them from little endian format.
    let off0 = decode_offset((encoded_instr >> OFF0_OFF) & OFFX_MASK);
    let off1 = decode_offset((encoded_instr >> OFF1_OFF) & OFFX_MASK);
    let off2 = decode_offset((encoded_instr >> OFF2_OFF) & OFFX_MASK);

    // Grab flags
    let flags = encoded_instr >> FLAGS_OFFSET;
    // Grab individual flags
    let dst_reg_num = (flags & DST_REG_MASK) >> DST_REG_OFF;
    let op0_reg_num = (flags & OP0_REG_MASK) >> OP0_REG_OFF;
    let op1_src_num = (flags & OP1_SRC_MASK) >> OP1_SRC_OFF;
    let res_logic_num = (flags & RES_LOGIC_MASK) >> RES_LOGIC_OFF;
    let pc_update_num = (flags & PC_UPDATE_MASK) >> PC_UPDATE_OFF;
    let ap_update_num = (flags & AP_UPDATE_MASK) >> AP_UPDATE_OFF;
    let opcode_num = (flags & OPCODE_MASK) >> OPCODE_OFF;

    // Grab opcode_extension
    let opcode_extension_num = encoded_instr >> OPCODE_EXTENSION_OFF;

    // Match each flag to its corresponding enum value
    let dst_register = if dst_reg_num == 1 {
        Register::FP
    } else {
        Register::AP
    };

    let op0_register = if op0_reg_num == 1 {
        Register::FP
    } else {
        Register::AP
    };

    let op1_addr = match op1_src_num {
        0 => Op1Addr::Op0,
        1 => Op1Addr::Imm,
        2 => Op1Addr::FP,
        4 => Op1Addr::AP,
        _ => return Err(VirtualMachineError::InvalidOp1Reg(op1_src_num)),
    };

    let pc_update = match pc_update_num {
        0 => PcUpdate::Regular,
        1 => PcUpdate::Jump,
        2 => PcUpdate::JumpRel,
        4 => PcUpdate::Jnz,
        _ => return Err(VirtualMachineError::InvalidPcUpdate(pc_update_num)),
    };

    let res = match (res_logic_num, pc_update == PcUpdate::Jnz) {
        (0, true) => Res::Unconstrained,
        (0, false) => Res::Op1,
        (1, false) => Res::Add,
        (2, false) => Res::Mul,
        _ => return Err(VirtualMachineError::InvalidRes(res_logic_num)),
    };

    let opcode = match opcode_num {
        0 => Opcode::NOp,
        1 => Opcode::Call,
        2 => Opcode::Ret,
        4 => Opcode::AssertEq,
        _ => return Err(VirtualMachineError::InvalidOpcode(opcode_num)),
    };

    let opcode_extension = match opcode_extension_num {
        0 => OpcodeExtension::Stone,
        1 => OpcodeExtension::Blake,
        2 => OpcodeExtension::BlakeFinalize,
        3 => OpcodeExtension::QM31Operation,
        _ => {
            return Err(VirtualMachineError::InvalidOpcodeExtension(
                opcode_extension_num,
            ))
        }
    };

    let blake_flags_valid = opcode == Opcode::NOp
        && (op1_addr == Op1Addr::FP || op1_addr == Op1Addr::AP)
        && res == Res::Op1
        && pc_update == PcUpdate::Regular
        && (ap_update_num == 0 || ap_update_num == 2);

    if (opcode_extension == OpcodeExtension::Blake
        || opcode_extension == OpcodeExtension::BlakeFinalize)
        && !blake_flags_valid
    {
        return Err(VirtualMachineError::InvalidBlake2sFlags(flags & 0x7FFF));
    }

    let qm31_operation_flags_valid = (res == Res::Add || res == Res::Mul)
        && op1_addr != Op1Addr::Op0
        && pc_update == PcUpdate::Regular
        && opcode == Opcode::AssertEq
        && (ap_update_num == 0 || ap_update_num == 2);

    if opcode_extension == OpcodeExtension::QM31Operation && !qm31_operation_flags_valid {
        return Err(VirtualMachineError::InvalidQM31AddMulFlags(flags & 0x7FFF));
    }

    let ap_update = match (ap_update_num, opcode == Opcode::Call) {
        (0, true) => ApUpdate::Add2,
        (0, false) => ApUpdate::Regular,
        (1, false) => ApUpdate::Add,
        (2, false) => ApUpdate::Add1,
        _ => return Err(VirtualMachineError::InvalidApUpdate(ap_update_num)),
    };

    let fp_update = match opcode {
        Opcode::Call => {
            if off0 != 0
                || off1 != 1
                || ap_update != ApUpdate::Add2
                || dst_register != Register::AP
                || op0_register != Register::AP
            {
                return Err(VirtualMachineError::InvalidOpcode(opcode_num));
            };
            FpUpdate::APPlus2
        }
        Opcode::Ret => {
            if off0 != -2
                || off2 != -1
                || dst_register != Register::FP
                || op1_addr != Op1Addr::FP
                || res != Res::Op1
                || pc_update != PcUpdate::Jump
            {
                return Err(VirtualMachineError::InvalidOpcode(opcode_num));
            };
            FpUpdate::Dst
        }
        _ => FpUpdate::Regular,
    };

    Ok(Instruction {
        off0,
        off1,
        off2,
        dst_register,
        op0_register,
        op1_addr,
        res,
        pc_update,
        ap_update,
        fp_update,
        opcode,
        opcode_extension,
    })
}

fn decode_offset(offset: u128) -> isize {
    let vectorized_offset: [u8; 16] = offset.to_le_bytes();
    let offset_16b_encoded = u16::from_le_bytes([vectorized_offset[0], vectorized_offset[1]]);
    let complement_const = 0x8000u16;
    let (offset_16b, _) = offset_16b_encoded.overflowing_sub(complement_const);
    isize::from(offset_16b as i16)
}

/// The instructions that are supported by the VM and by the Stwo prover.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SupportedInstruction {
    /// `ret`.
    Ret,
    /// `ap += imm` / `ap += [ap/fp + off2]`.
    AddAp,
    /// `jmp rel imm`.
    JumpRelImm,
    /// `jmp rel [ap/fp + off2]`.
    JumpRel,
    /// `jmp abs [[ap/fp + off1] + off2]`.
    JumpDoubleDeref,
    /// `jmp abs [ap/fp + off2]`.
    JumpAbs,
    /// `call rel imm`.
    CallRelImm,
    /// `call abs [ap/fp + off2]`.
    CallAbs,
    /// `jmp rel imm if [ap/fp + off0] != 0`.
    Jnz,
    /// `[ap/fp + off0] = imm`.
    AssertEqImm,
    /// `[ap/fp + off0] = [[ap/fp + off1] + off2]`.
    AssertEqDoubleDeref,
    /// `[ap/fp + off0] = [ap/fp + off2]`.
    AssertEq,
    /// `[ap/fp + off0] = [ap/fp + off1] * imm/[ap/fp + off2]`.
    Mul,
    /// `[ap/fp + off0] = [ap/fp + off1] + imm/[ap/fp + off2]`.
    Add,
    /// Blake2s compress / finalize.
    Blake,
    /// QM31 add / mul.
    QM31AddMul,
}

/// Classifies instructions. Returns `None` if `instruction` is unsupported.
pub const fn classify_instruction(instruction: &Instruction) -> Option<SupportedInstruction> {
    // An immediate op1 is read from `[pc + off2]`, which must be the cell right after the
    // instruction, i.e. `off2 == 1`.
    if matches!(instruction.op1_addr, Op1Addr::Imm) && instruction.off2 != 1 {
        return None;
    }

    match *instruction {
        // ret.
        Instruction {
            off0: -2,
            off1: -1,
            off2: -1,
            dst_register: Register::FP,
            op0_register: Register::FP,
            op1_addr: Op1Addr::FP,
            res: Res::Op1,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular,
            fp_update: _,
            opcode: Opcode::Ret,
            opcode_extension: OpcodeExtension::Stone,
        } => Some(SupportedInstruction::Ret),

        // add ap.
        Instruction {
            off0: -1,
            off1: -1,
            off2: _,
            dst_register: Register::FP,
            op0_register: Register::FP,
            op1_addr: Op1Addr::Imm | Op1Addr::FP | Op1Addr::AP,
            res: Res::Op1,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Add,
            fp_update: _,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        } => {
            // ap += imm.
            // ap += [ap/fp + off2].
            Some(SupportedInstruction::AddAp)
        }

        // jump rel imm.
        Instruction {
            off0: -1,
            off1: -1,
            off2: 1,
            dst_register: Register::FP,
            op0_register: Register::FP,
            op1_addr: Op1Addr::Imm,
            res: Res::Op1,
            pc_update: PcUpdate::JumpRel,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        } => Some(SupportedInstruction::JumpRelImm),

        // jump rel [ap/fp + off2].
        Instruction {
            off0: -1,
            off1: -1,
            off2: _,
            dst_register: Register::FP,
            op0_register: Register::FP,
            op1_addr: Op1Addr::FP | Op1Addr::AP,
            res: Res::Op1,
            pc_update: PcUpdate::JumpRel,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        } => Some(SupportedInstruction::JumpRel),

        // jump abs [[ap/fp + off1] + off2].
        Instruction {
            off0: -1,
            off1: _,
            off2: _,
            dst_register: Register::FP,
            op0_register: _,
            op1_addr: Op1Addr::Op0,
            res: Res::Op1,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        } => Some(SupportedInstruction::JumpDoubleDeref),

        // jump abs [ap/fp + off2].
        Instruction {
            off0: -1,
            off1: -1,
            off2: _,
            dst_register: Register::FP,
            op0_register: Register::FP,
            op1_addr: Op1Addr::FP | Op1Addr::AP,
            res: Res::Op1,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        } => Some(SupportedInstruction::JumpAbs),

        // call rel imm.
        Instruction {
            off0: 0,
            off1: 1,
            off2: 1,
            dst_register: Register::AP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::Imm,
            res: Res::Op1,
            pc_update: PcUpdate::JumpRel,
            ap_update: ApUpdate::Add2,
            fp_update: _,
            opcode: Opcode::Call,
            opcode_extension: OpcodeExtension::Stone,
        } => Some(SupportedInstruction::CallRelImm),

        // call abs [ap/fp + off2].
        Instruction {
            off0: 0,
            off1: 1,
            off2: _,
            dst_register: Register::AP,
            op0_register: Register::AP,
            op1_addr: Op1Addr::FP | Op1Addr::AP,
            res: Res::Op1,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Add2,
            fp_update: _,
            opcode: Opcode::Call,
            opcode_extension: OpcodeExtension::Stone,
        } => Some(SupportedInstruction::CallAbs),

        // jnz.
        Instruction {
            off0: _,
            off1: -1,
            off2: 1,
            dst_register: _,
            op0_register: Register::FP,
            op1_addr: Op1Addr::Imm,
            res: Res::Unconstrained,
            pc_update: PcUpdate::Jnz,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        } => {
            // jump rel imm if [ap/fp + off0] != 0.
            Some(SupportedInstruction::Jnz)
        }

        // [ap/fp + off0] = imm.
        Instruction {
            off0: _,
            off1: -1,
            off2: 1,
            dst_register: _,
            op0_register: Register::FP,
            op1_addr: Op1Addr::Imm,
            res: Res::Op1,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        } => Some(SupportedInstruction::AssertEqImm),

        // [ap/fp + off0] = [[ap/fp + off1] + off2].
        Instruction {
            off0: _,
            off1: _,
            off2: _,
            dst_register: _,
            op0_register: _,
            op1_addr: Op1Addr::Op0,
            res: Res::Op1,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        } => Some(SupportedInstruction::AssertEqDoubleDeref),

        // [ap/fp + off0] = [ap/fp + off1].
        Instruction {
            off0: _,
            off1: -1,
            off2: _,
            dst_register: _,
            op0_register: Register::FP,
            op1_addr: Op1Addr::FP | Op1Addr::AP,
            res: Res::Op1,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        } => Some(SupportedInstruction::AssertEq),

        // mul.
        Instruction {
            off0: _,
            off1: _,
            off2: _,
            dst_register: _,
            op0_register: _,
            op1_addr: Op1Addr::Imm | Op1Addr::FP | Op1Addr::AP,
            res: Res::Mul,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        } => {
            // [ap/fp + off0] = [ap/fp + off1] * imm.
            // [ap/fp + off0] = [ap/fp + off1] * [ap/fp + off2].
            Some(SupportedInstruction::Mul)
        }

        // add.
        Instruction {
            off0: _,
            off1: _,
            off2: _,
            dst_register: _,
            op0_register: _,
            op1_addr: Op1Addr::Imm | Op1Addr::FP | Op1Addr::AP,
            res: Res::Add,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        } => {
            // [ap/fp + off0] = [ap/fp + off1] + imm.
            // [ap/fp + off0] = [ap/fp + off1] + [ap/fp + off2].
            Some(SupportedInstruction::Add)
        }

        // Blake.
        Instruction {
            off0: _,
            off1: _,
            off2: _,
            dst_register: _,
            op0_register: _,
            op1_addr: Op1Addr::FP | Op1Addr::AP,
            res: Res::Op1,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Blake | OpcodeExtension::BlakeFinalize,
        } => Some(SupportedInstruction::Blake),

        // QM31 add mul.
        Instruction {
            off0: _,
            off1: _,
            off2: _,
            dst_register: _,
            op0_register: _,
            op1_addr: Op1Addr::Imm | Op1Addr::FP | Op1Addr::AP,
            res: Res::Add | Res::Mul,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::QM31Operation,
        } => {
            // [ap/fp + off0] = [ap/fp + off1] +/* [ap/fp/pc + off2]
            Some(SupportedInstruction::QM31AddMul)
        }

        _ => None,
    }
}

#[cfg(test)]
mod decoder_test {
    use super::*;
    use assert_matches::assert_matches;
    use itertools::iproduct;
    use rstest::rstest;
    use std::collections::HashMap;

    #[test]
    fn non_zero_high_bits() {
        let error = decode_instruction(0x214a7800080008000);
        assert_eq!(
            error.unwrap_err().to_string(),
            "Invalid opcode extension value: 4",
        )
    }

    #[test]
    fn invalid_op1_reg() {
        let error = decode_instruction(0x294F800080008000);
        assert_matches!(error, Err(VirtualMachineError::InvalidOp1Reg(3)));
        assert_eq!(
            error.unwrap_err().to_string(),
            "Invalid op1_register value: 3"
        )
    }

    #[test]
    fn invalid_pc_update() {
        let error = decode_instruction(0x29A8800080008000);
        assert_matches!(error, Err(VirtualMachineError::InvalidPcUpdate(3)));
        assert_eq!(error.unwrap_err().to_string(), "Invalid pc_update value: 3")
    }

    #[test]
    fn invalid_res_logic() {
        let error = decode_instruction(0x2968800080008000);
        assert_matches!(error, Err(VirtualMachineError::InvalidRes(3)));
        assert_eq!(error.unwrap_err().to_string(), "Invalid res value: 3")
    }

    #[test]
    fn invalid_opcode() {
        let error = decode_instruction(0x3948800080008000);
        assert_matches!(error, Err(VirtualMachineError::InvalidOpcode(3)));
        assert_eq!(error.unwrap_err().to_string(), "Invalid opcode value: 3")
    }

    #[test]
    fn invalid_ap_update() {
        let error = decode_instruction(0x2D48800080008000);
        assert_matches!(error, Err(VirtualMachineError::InvalidApUpdate(3)));
        assert_eq!(error.unwrap_err().to_string(), "Invalid ap_update value: 3")
    }

    #[test]
    fn decode_flags_nop_add_jmp_add_imm_fp_fp() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //            Stone|      NOp|      ADD|     JUMP|      ADD|    IMM|     FP|     FP
        //                0   0  0  0      0  1   0  0  1      0  1 0  0  1       1       1
        //  0000 0100 1010 0111 = 0x04A7; offx = 0
        let inst = decode_instruction(0x04A7800080008000).unwrap();
        assert_matches!(inst.dst_register, Register::FP);
        assert_matches!(inst.op0_register, Register::FP);
        assert_matches!(inst.op1_addr, Op1Addr::Imm);
        assert_matches!(inst.res, Res::Add);
        assert_matches!(inst.pc_update, PcUpdate::Jump);
        assert_matches!(inst.ap_update, ApUpdate::Add);
        assert_matches!(inst.opcode, Opcode::NOp);
        assert_matches!(inst.fp_update, FpUpdate::Regular);
        assert_matches!(inst.opcode_extension, OpcodeExtension::Stone);
    }

    #[test]
    fn decode_flags_nop_add1_jmp_rel_mul_fp_ap_ap() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //            Stone|      NOp|     ADD1| JUMP_REL|      MUL|     FP|     AP|     AP
        //                0   0  0  0      1  0   0  1  0      1  0 0  1  0       0       0
        //  0000 1001 0100 1000 = 0x0948; offx = 0
        let inst = decode_instruction(0x0948800080008000).unwrap();
        assert_matches!(inst.dst_register, Register::AP);
        assert_matches!(inst.op0_register, Register::AP);
        assert_matches!(inst.op1_addr, Op1Addr::FP);
        assert_matches!(inst.res, Res::Mul);
        assert_matches!(inst.pc_update, PcUpdate::JumpRel);
        assert_matches!(inst.ap_update, ApUpdate::Add1);
        assert_matches!(inst.opcode, Opcode::NOp);
        assert_matches!(inst.fp_update, FpUpdate::Regular);
        assert_matches!(inst.opcode_extension, OpcodeExtension::Stone);
    }

    #[test]
    fn decode_flags_assrt_add_regular_mul_ap_ap_ap() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //            Stone| ASSRT_EQ|      ADD|  REGULAR|      MUL|     AP|     AP|     AP
        //                0   1  0  0      1  0   0  0  0      1  0 1  0  0       0       0
        //  0100 1000 0101 0000 = 0x4850; offx = 0
        let inst = decode_instruction(0x4850800080008000).unwrap();
        assert_matches!(inst.dst_register, Register::AP);
        assert_matches!(inst.op0_register, Register::AP);
        assert_matches!(inst.op1_addr, Op1Addr::AP);
        assert_matches!(inst.res, Res::Mul);
        assert_matches!(inst.pc_update, PcUpdate::Regular);
        assert_matches!(inst.ap_update, ApUpdate::Add1);
        assert_matches!(inst.opcode, Opcode::AssertEq);
        assert_matches!(inst.fp_update, FpUpdate::Regular);
        assert_matches!(inst.opcode_extension, OpcodeExtension::Stone);
    }

    #[test]
    fn decode_flags_assrt_add2_jnz_uncon_op0_ap_ap() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //            Stone| ASSRT_EQ|     ADD2|      JNZ|UNCONSTRD|    OP0|     AP|     AP
        //                0   1  0  0      0  0   1  0  0      0  0 0  0  0       0       0
        //  0100 0010 0000 0000 = 0x4200; offx = 0
        let inst = decode_instruction(0x4200800080008000).unwrap();
        assert_matches!(inst.dst_register, Register::AP);
        assert_matches!(inst.op0_register, Register::AP);
        assert_matches!(inst.op1_addr, Op1Addr::Op0);
        assert_matches!(inst.res, Res::Unconstrained);
        assert_matches!(inst.pc_update, PcUpdate::Jnz);
        assert_matches!(inst.ap_update, ApUpdate::Regular);
        assert_matches!(inst.opcode, Opcode::AssertEq);
        assert_matches!(inst.fp_update, FpUpdate::Regular);
        assert_matches!(inst.opcode_extension, OpcodeExtension::Stone);
    }

    #[test]
    fn decode_flags_nop_regu_regu_op1_op0_ap_ap() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //            Stone|      NOP|  REGULAR|  REGULAR|      OP1|    OP0|     AP|     AP
        //                0   0  0  0      0  0   0  0  0      0  0 0  0  0       0       0
        //  0000 0000 0000 0000 = 0x0000; offx = 0
        let inst = decode_instruction(0x0000800080008000).unwrap();
        assert_matches!(inst.dst_register, Register::AP);
        assert_matches!(inst.op0_register, Register::AP);
        assert_matches!(inst.op1_addr, Op1Addr::Op0);
        assert_matches!(inst.res, Res::Op1);
        assert_matches!(inst.pc_update, PcUpdate::Regular);
        assert_matches!(inst.ap_update, ApUpdate::Regular);
        assert_matches!(inst.opcode, Opcode::NOp);
        assert_matches!(inst.fp_update, FpUpdate::Regular);
        assert_matches!(inst.opcode_extension, OpcodeExtension::Stone);
    }

    #[test]
    fn decode_offset_negative() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //            Stone|      NOP|  REGULAR|  REGULAR|      OP1|    OP0|     AP|     AP
        //                0   0  0  0      0  0   0  0  0      0  0 0  0  0       0       0
        //  0000 0000 0000 0000 = 0x0000; offx = 0
        let inst = decode_instruction(0x0000800180007FFF).unwrap();
        assert_eq!(inst.off0, -1);
        assert_eq!(inst.off1, 0);
        assert_eq!(inst.off2, 1);
    }

    #[test]
    fn decode_ret_cairo_standard() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //            Stone|      RET|  REGULAR|     JUMP|      Op1|     FP|     FP|     FP
        //                0   0  1  0      0  0   0  0  1      0  0 0  1  0       1       1
        //  0010 0000 1000 1011 = 0x208b; off0 = -2, off1 = -1
        let inst = decode_instruction(0x208b7fff7fff7ffe).unwrap();
        assert_matches!(inst.opcode, Opcode::Ret);
        assert_matches!(inst.off0, -2);
        assert_matches!(inst.off1, -1);
        assert_matches!(inst.dst_register, Register::FP);
        assert_matches!(inst.op0_register, Register::FP);
        assert_matches!(inst.op1_addr, Op1Addr::FP);
        assert_matches!(inst.res, Res::Op1);
        assert_matches!(inst.pc_update, PcUpdate::Jump);
        assert_matches!(inst.ap_update, ApUpdate::Regular);
        assert_matches!(inst.fp_update, FpUpdate::Dst);
        assert_matches!(inst.opcode_extension, OpcodeExtension::Stone);
    }

    #[test]
    fn decode_call_cairo_standard() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //            Stone|     CALL|  Regular|  JumpRel|      Op1|     FP|     FP|     FP
        //                0   0  0  1      0  0   0  1  0      0  0 0  0  1       0       0
        //  0001 0001 0000 0100 = 0x1104; off0 = 0, off1 = 1
        let inst = decode_instruction(0x1104800180018000).unwrap();
        assert_matches!(inst.opcode, Opcode::Call);
        assert_matches!(inst.off0, 0);
        assert_matches!(inst.off1, 1);
        assert_matches!(inst.dst_register, Register::AP);
        assert_matches!(inst.op0_register, Register::AP);
        assert_matches!(inst.op1_addr, Op1Addr::Imm);
        assert_matches!(inst.res, Res::Op1);
        assert_matches!(inst.pc_update, PcUpdate::JumpRel);
        assert_matches!(inst.ap_update, ApUpdate::Add2);
        assert_matches!(inst.fp_update, FpUpdate::APPlus2);
        assert_matches!(inst.opcode_extension, OpcodeExtension::Stone);
    }

    #[test]
    fn decode_ret_opcode_error() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //            Stone|      RET|  REGULAR|     JUMP|      Op1|     FP|     FP|     FP
        //                0   0  1  0      0  0   0  0  1      0  0 0  1  0       1       1
        //  0010 0000 1000 1011 = 0x208b; off0 = -1, off1 = -1
        let error = decode_instruction(0x208b7fff7fff7fff);
        assert_matches!(error, Err(VirtualMachineError::InvalidOpcode(2)));
    }

    #[test]
    fn decode_call_opcode_error() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //            Stone|     CALL|  REGULAR|  JumpRel|      Op1|    IMM|     AP|     AP
        //                0   0  0  1      0  0   0  1  0      0  0 0  0  1       0       0
        //  0001 0001 0000 0100 = 0x1104; off0 = 1, off1 = 1
        let error = decode_instruction(0x1104800180018001);
        assert_matches!(error, Err(VirtualMachineError::InvalidOpcode(1)));
    }

    #[test]
    fn decode_opcode_extension_clash() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //            Blake|     CALL|  REGULAR|  REGULAR|      Op1|     FP|     AP|     AP
        //                1   0  0  1      0  0   0  0  0      0  0 0  1  0       0       0
        //  1001 0000 0000 1000 = 0x9008; off0 = 1, off1 = 1
        let error = decode_instruction(0x9008800180018001);
        assert_matches!(error, Err(VirtualMachineError::InvalidBlake2sFlags(4104)));
    }

    #[test]
    fn decode_blake_imm() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //            Blake|      NOP|  REGULAR|  REGULAR|      Op1|    IMM|     AP|     AP
        //                1   0  0  0      0  0   0  0  0      0  0 0  0  1       0       0
        //  1000 0000 0000 0100 = 0x8004; off0 = 1, off1 = 1
        let error = decode_instruction(0x8004800180018001);
        assert_matches!(error, Err(VirtualMachineError::InvalidBlake2sFlags(4)));
    }

    #[test]
    fn decode_blake() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //            Blake|      NOP|     ADD1|  REGULAR|      Op1|     AP|     FP|     FP
        //                1   0  0  0      1  0   0  0  0      0  0 1  0  0       1       1
        //  1000 1000 0001 0011 = 0x8813; off0 = 1, off1 = 1
        let inst = decode_instruction(0x8813800180018001).unwrap();
        assert_matches!(inst.opcode, Opcode::NOp);
        assert_matches!(inst.off0, 1);
        assert_matches!(inst.off1, 1);
        assert_matches!(inst.dst_register, Register::FP);
        assert_matches!(inst.op0_register, Register::FP);
        assert_matches!(inst.op1_addr, Op1Addr::AP);
        assert_matches!(inst.res, Res::Op1);
        assert_matches!(inst.pc_update, PcUpdate::Regular);
        assert_matches!(inst.ap_update, ApUpdate::Add1);
        assert_matches!(inst.fp_update, FpUpdate::Regular);
        assert_matches!(inst.opcode_extension, OpcodeExtension::Blake);
    }

    #[test]
    fn decode_invalid_opcode_extension_error() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //              ???|     CALL|     Add2|  JumpRel|      Op1|    IMM|     FP|     FP
        //          1  1  1   0  0  1      0  0   0  1  0      0  0 0  0  1       0       0
        //  0011 1001 0001 0000 0100 = 0x39104; off0 = 0, off1 = 1
        let error = decode_instruction(0x39104800180018000);
        assert_matches!(error, Err(VirtualMachineError::InvalidOpcodeExtension(7)));
    }

    #[test]
    fn decode_qm31_operation_invalid_flags() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //    QM31Operation|     CALL|  REGULAR|  JumpRel|      Op1|     FP|     AP|     AP
        //             1  1   0  0  1      0  0   0  1  0      0  0 0  1  0       0       0
        //  1 1001 0001 0000 1000 = 0x19108; off0 = 1, off1 = 1
        let error = decode_instruction(0x19108800180018001);
        assert_matches!(
            error,
            Err(VirtualMachineError::InvalidQM31AddMulFlags(0x1108))
        );
    }

    #[test]
    fn decode_qm31_operation() {
        // opcode_extension|   opcode|ap_update|pc_update|res_logic|op1_src|op0_reg|dst_reg
        //  79 ... 17 16 15| 14 13 12|    11 10|  9  8  7|     6  5|4  3  2|      1|      0
        //    QM31Operation|ASSERT_EQ|  REGULAR|  REGULAR|      MUL|     FP|     AP|     AP
        //             1  1   1  0  0      0  0   0  0  0      1  0 0  1  0       0       0
        //  1 1100 0000 0100 1000 = 0x1c048; off0 = 1, off1 = 1
        let inst = decode_instruction(0x1c048800180018001).unwrap();
        assert_matches!(inst.opcode, Opcode::AssertEq);
        assert_matches!(inst.off0, 1);
        assert_matches!(inst.off1, 1);
        assert_matches!(inst.dst_register, Register::AP);
        assert_matches!(inst.op0_register, Register::AP);
        assert_matches!(inst.op1_addr, Op1Addr::FP);
        assert_matches!(inst.res, Res::Mul);
        assert_matches!(inst.pc_update, PcUpdate::Regular);
        assert_matches!(inst.ap_update, ApUpdate::Regular);
        assert_matches!(inst.fp_update, FpUpdate::Regular);
        assert_matches!(inst.opcode_extension, OpcodeExtension::QM31Operation);
    }

    // Flag bits (relative to bit 48 of the encoding).
    const DST_FP: u128 = 1 << 0;
    const OP0_FP: u128 = 1 << 1;
    const OP1_IMM: u128 = 1 << 2;
    const OP1_FP: u128 = 1 << 3;
    const OP1_AP: u128 = 1 << 4;
    const RES_ADD: u128 = 1 << 5;
    const RES_MUL: u128 = 1 << 6;
    const PC_JUMP: u128 = 1 << 7;
    const PC_JUMP_REL: u128 = 1 << 8;
    const PC_JNZ: u128 = 1 << 9;
    const AP_ADD: u128 = 1 << 10;
    const AP_ADD1: u128 = 1 << 11;
    const CALL: u128 = 1 << 12;
    const RET: u128 = 1 << 13;
    const ASSERT_EQ: u128 = 1 << 14;
    // Opcode extensions.
    const STONE: u128 = 0;
    const BLAKE: u128 = 1;
    const BLAKE_FINALIZE: u128 = 2;
    const QM31: u128 = 3;

    fn encode(offsets: [i16; 3], flags: u128, opcode_extension: u128) -> u128 {
        let biased = |off: i16| (off as u16).wrapping_add(0x8000) as u128;
        biased(offsets[0])
            | (biased(offsets[1]) << 16)
            | (biased(offsets[2]) << 32)
            | (flags << 48)
            | (opcode_extension << 63)
    }

    #[rstest]
    // ret.
    #[case::ret(encode([-2, -1, -1], DST_FP | OP0_FP | OP1_FP | PC_JUMP | RET, STONE), SupportedInstruction::Ret)]
    // ap += imm.
    #[case::add_ap_imm(encode([-1, -1, 1], DST_FP | OP0_FP | OP1_IMM | AP_ADD, STONE), SupportedInstruction::AddAp)]
    // ap += [fp].
    #[case::add_ap_fp(encode([-1, -1, 0], DST_FP | OP0_FP | OP1_FP | AP_ADD, STONE), SupportedInstruction::AddAp)]
    // jmp rel imm.
    #[case::jump_rel_imm(encode([-1, -1, 1], DST_FP | OP0_FP | OP1_IMM | PC_JUMP_REL, STONE), SupportedInstruction::JumpRelImm)]
    // jmp rel [ap - 2], ap++.
    #[case::jump_rel(encode([-1, -1, -2], DST_FP | OP0_FP | OP1_AP | PC_JUMP_REL | AP_ADD1, STONE), SupportedInstruction::JumpRel)]
    // jmp abs [[ap + 3] + 4].
    #[case::jump_double_deref(encode([-1, 3, 4], DST_FP | PC_JUMP, STONE), SupportedInstruction::JumpDoubleDeref)]
    // jmp abs [fp].
    #[case::jump_abs(encode([-1, -1, 0], DST_FP | OP0_FP | OP1_FP | PC_JUMP, STONE), SupportedInstruction::JumpAbs)]
    // call rel imm.
    #[case::call_rel_imm(encode([0, 1, 1], OP1_IMM | PC_JUMP_REL | CALL, STONE), SupportedInstruction::CallRelImm)]
    // call abs [fp - 1].
    #[case::call_abs(encode([0, 1, -1], OP1_FP | PC_JUMP | CALL, STONE), SupportedInstruction::CallAbs)]
    // jmp rel imm if [ap + 5] != 0.
    #[case::jnz(encode([5, -1, 1], OP0_FP | OP1_IMM | PC_JNZ, STONE), SupportedInstruction::Jnz)]
    // [ap] = imm, ap++.
    #[case::assert_eq_imm(encode([0, -1, 1], OP0_FP | OP1_IMM | AP_ADD1 | ASSERT_EQ, STONE), SupportedInstruction::AssertEqImm)]
    // [fp + 2] = [[ap - 2] + 2].
    #[case::assert_eq_double_deref(encode([2, -2, 2], DST_FP | ASSERT_EQ, STONE), SupportedInstruction::AssertEqDoubleDeref)]
    // [ap + 2] = [fp + 1].
    #[case::assert_eq(encode([2, -1, 1], OP0_FP | OP1_FP | ASSERT_EQ, STONE), SupportedInstruction::AssertEq)]
    // [fp + 2] = [fp] + imm.
    #[case::add_imm(encode([2, 0, 1], DST_FP | OP0_FP | OP1_IMM | RES_ADD | ASSERT_EQ, STONE), SupportedInstruction::Add)]
    // [ap] = [ap - 1] + [fp - 3], ap++.
    #[case::add(encode([0, -1, -3], OP1_FP | RES_ADD | AP_ADD1 | ASSERT_EQ, STONE), SupportedInstruction::Add)]
    // [ap] = [ap] * [ap], ap++.
    #[case::mul(encode([0, 0, 0], OP1_AP | RES_MUL | AP_ADD1 | ASSERT_EQ, STONE), SupportedInstruction::Mul)]
    #[case::blake(encode([1, 1, 1], DST_FP | OP0_FP | OP1_AP | AP_ADD1, BLAKE), SupportedInstruction::Blake)]
    #[case::blake_finalize(encode([-5, -4, -3], OP0_FP | OP1_FP, BLAKE_FINALIZE), SupportedInstruction::Blake)]
    #[case::qm31_mul(encode([1, 1, 1], OP1_FP | RES_MUL | ASSERT_EQ, QM31), SupportedInstruction::QM31AddMul)]
    #[case::qm31_add_imm(encode([1, 1, 1], OP1_IMM | RES_ADD | AP_ADD1 | ASSERT_EQ, QM31), SupportedInstruction::QM31AddMul)]
    fn non_generic_instruction_is_supported(
        #[case] encoded_instr: u128,
        #[case] expected: SupportedInstruction,
    ) {
        let instruction = decode_instruction(encoded_instr).unwrap();
        assert_eq!(classify_instruction(&instruction), Some(expected));
    }

    #[rstest]
    // Regular nop.
    #[case::nop(encode([0, 0, 0], 0, STONE))]
    // ret with off1 != -1.
    #[case::ret_bad_off1(encode([-2, 0, -1], DST_FP | OP0_FP | OP1_FP | PC_JUMP | RET, STONE))]
    // ret with op0 based on ap.
    #[case::ret_op0_ap(encode([-2, -1, -1], DST_FP | OP1_FP | PC_JUMP | RET, STONE))]
    // ret, ap++.
    #[case::ret_with_ap_add1(encode([-2, -1, -1], DST_FP | OP0_FP | OP1_FP | PC_JUMP | RET | AP_ADD1, STONE))]
    // A single instruction doing an add, a jump and an ap update.
    #[case::nop_add_jump_add_ap(encode([0, 0, 0], DST_FP | OP0_FP | OP1_IMM | RES_ADD | PC_JUMP | AP_ADD, STONE))]
    // ap += [[fp] + 0].
    #[case::add_ap_double_deref(encode([-1, -1, 0], DST_FP | OP0_FP | AP_ADD, STONE))]
    // ap += [fp] with dst off0 != -1.
    #[case::add_ap_bad_off0(encode([0, -1, 0], DST_FP | OP0_FP | OP1_FP | AP_ADD, STONE))]
    // jmp abs imm.
    #[case::jump_abs_imm(encode([-1, -1, 1], DST_FP | OP0_FP | OP1_IMM | PC_JUMP, STONE))]
    // jmp rel [[fp - 1] + 0].
    #[case::jump_rel_double_deref(encode([-1, -1, 0], DST_FP | OP0_FP | PC_JUMP_REL, STONE))]
    // jmp rel imm with dst off0 != -1.
    #[case::jump_rel_imm_bad_off0(encode([0, -1, 1], DST_FP | OP0_FP | OP1_IMM | PC_JUMP_REL, STONE))]
    // jmp abs [ap] with op0 based on ap.
    #[case::jump_abs_op0_ap(encode([-1, -1, 0], DST_FP | OP1_AP | PC_JUMP, STONE))]
    // jmp abs [ap] with off1 != -1.
    #[case::jump_abs_bad_off1(encode([-1, 0, 0], DST_FP | OP0_FP | OP1_AP | PC_JUMP, STONE))]
    // jmp rel imm with ap += [fp].
    #[case::jump_with_ap_add(encode([-1, -1, 1], DST_FP | OP0_FP | OP1_IMM | PC_JUMP_REL | AP_ADD, STONE))]
    // call that does not jump.
    #[case::call_no_jump(encode([0, 1, 1], OP1_IMM | CALL, STONE))]
    // call abs imm.
    #[case::call_abs_imm(encode([0, 1, 1], OP1_IMM | PC_JUMP | CALL, STONE))]
    // call abs [[ap + 1] + 0].
    #[case::call_abs_double_deref(encode([0, 1, 0], PC_JUMP | CALL, STONE))]
    // jmp rel [fp] if [ap] != 0.
    #[case::jnz_not_imm(encode([0, -1, 0], OP0_FP | OP1_FP | PC_JNZ, STONE))]
    // jmp rel imm if [ap] != 0 with off1 != -1.
    #[case::jnz_bad_off1(encode([0, 0, 1], OP0_FP | OP1_IMM | PC_JNZ, STONE))]
    // jmp rel imm if [ap] != 0 with op0 based on ap.
    #[case::jnz_op0_ap(encode([0, -1, 1], OP1_IMM | PC_JNZ, STONE))]
    // [ap] = imm with op0 based on ap.
    #[case::assert_eq_imm_op0_ap(encode([0, -1, 1], OP1_IMM | ASSERT_EQ, STONE))]
    // [ap] = imm with off1 != -1.
    #[case::assert_eq_imm_bad_off1(encode([0, 0, 1], OP0_FP | OP1_IMM | ASSERT_EQ, STONE))]
    // [ap] = [fp] with op0 based on ap.
    #[case::assert_eq_op0_ap(encode([0, -1, 0], OP1_FP | ASSERT_EQ, STONE))]
    // [ap] = [fp] with off1 != -1.
    #[case::assert_eq_bad_off1(encode([0, 0, 0], OP0_FP | OP1_FP | ASSERT_EQ, STONE))]
    // [ap] = [fp], with a jump.
    #[case::assert_eq_with_jump(encode([0, -1, 0], OP0_FP | OP1_FP | PC_JUMP | ASSERT_EQ, STONE))]
    // [ap] = [ap] + [[ap] + 0].
    #[case::add_double_deref(encode([0, 0, 0], RES_ADD | ASSERT_EQ, STONE))]
    // [ap] = [ap] * [[ap] + 0].
    #[case::mul_double_deref(encode([0, 0, 0], RES_MUL | ASSERT_EQ, STONE))]
    // [ap] = [[ap] + 0], ap += [[ap] + 0].
    #[case::assert_eq_double_deref_with_ap_add(encode([0, 0, 0], AP_ADD | ASSERT_EQ, STONE))]
    // [ap] = [ap] + imm, with a jump.
    #[case::add_imm_with_jump(encode([0, 0, 1], OP1_IMM | RES_ADD | PC_JUMP | ASSERT_EQ, STONE))]
    // [ap] * [ap] without an assert_eq.
    #[case::mul_nop(encode([0, 0, 0], OP1_AP | RES_MUL, STONE))]
    // [ap] = [ap] * [ap], ap += [ap].
    #[case::mul_with_ap_add(encode([0, 0, 0], OP1_AP | RES_MUL | AP_ADD | ASSERT_EQ, STONE))]
    // ap += imm with off2 != 1.
    #[case::add_ap_imm_bad_off2(encode([-1, -1, 2], DST_FP | OP0_FP | OP1_IMM | AP_ADD, STONE))]
    // [ap] = imm with off2 != 1.
    #[case::assert_eq_imm_bad_off2(encode([0, -1, 0], OP0_FP | OP1_IMM | ASSERT_EQ, STONE))]
    // call rel imm with off2 != 1.
    #[case::call_rel_imm_bad_off2(encode([0, 1, 2], OP1_IMM | PC_JUMP_REL | CALL, STONE))]
    // qm31 add imm with off2 != 1.
    #[case::qm31_add_imm_bad_off2(encode([1, 1, 2], OP1_IMM | RES_ADD | ASSERT_EQ, QM31))]
    // jmp abs [fp], ap += [fp].
    #[case::jump_abs_with_ap_add(encode([-1, -1, 0], DST_FP | OP0_FP | OP1_FP | PC_JUMP | AP_ADD, STONE))]
    // jmp abs [[fp - 1] + 0], ap += [[fp - 1] + 0].
    #[case::jump_double_deref_with_ap_add(encode([-1, -1, 0], DST_FP | OP0_FP | PC_JUMP | AP_ADD, STONE))]
    // jmp rel imm if [ap] != 0, ap += imm.
    #[case::jnz_with_ap_add(encode([0, -1, 1], OP0_FP | OP1_IMM | PC_JNZ | AP_ADD, STONE))]
    fn test_generic_instruction_is_unsupported(#[case] encoded_instr: u128) {
        let instruction = decode_instruction(encoded_instr).unwrap();
        assert_eq!(classify_instruction(&instruction), None);
    }

    /// Classifies every decodable instruction over all the flag combinations, all the opcode
    /// extensions and a representative set of offsets, and catches a change in how many encodings
    /// each kind of supported instruction accepts.
    #[test]
    fn classify_instruction_regression_test() {
        // Offsets only matter where `decode_instruction` or `classify_instruction` check them against
        // -2, -1, 0 or 1. Any other value takes the same path as 2, so these five values cover every
        // behaviour.
        const OFFSETS: [i16; 5] = [-2, -1, 0, 1, 2];
        const N_FLAG_COMBINATIONS: u128 = 1 << 15;

        let expected_counts = HashMap::from([
            (SupportedInstruction::Ret, 1),
            // op1 imm + op1 register * off2.
            (SupportedInstruction::AddAp, 1 + 2 * 5),
            // ap update.
            (SupportedInstruction::JumpRelImm, 2),
            // op1 register, off2, ap update.
            (SupportedInstruction::JumpRel, 2 * 5 * 2),
            // off1, off2, op0 register, ap update.
            (SupportedInstruction::JumpDoubleDeref, 5 * 5 * 2 * 2),
            // op1 register, off2, ap update.
            (SupportedInstruction::JumpAbs, 2 * 5 * 2),
            (SupportedInstruction::CallRelImm, 1),
            // op1 register, off2.
            (SupportedInstruction::CallAbs, 2 * 5),
            // off0, dst register, ap update.
            (SupportedInstruction::Jnz, 5 * 2 * 2),
            // off0, dst register, ap update.
            (SupportedInstruction::AssertEqImm, 5 * 2 * 2),
            // off0, off1, off2, dst register, op0 register, ap update.
            (
                SupportedInstruction::AssertEqDoubleDeref,
                5 * 5 * 5 * 2 * 2 * 2,
            ),
            // off0, off2, dst register, op1 register, ap update.
            (SupportedInstruction::AssertEq, 5 * 5 * 2 * 2 * 2),
            // off0, off1, dst register, op0 register, ap update, (op1 imm + op1 register * off2).
            (SupportedInstruction::Mul, 5 * 5 * 2 * 2 * 2 * (1 + 2 * 5)),
            // off0, off1, dst register, op0 register, ap update, (op1 imm + op1 register * off2).
            (SupportedInstruction::Add, 5 * 5 * 2 * 2 * 2 * (1 + 2 * 5)),
            // opcode extension, off0, off1, off2, dst register, op0 register, op1 register,
            // ap update.
            (SupportedInstruction::Blake, 2 * 5 * 5 * 5 * 2 * 2 * 2 * 2),
            // res, off0, off1, dst register, op0 register, ap update,
            // (op1 imm + op1 register * off2).
            (
                SupportedInstruction::QM31AddMul,
                2 * 5 * 5 * 2 * 2 * 2 * (1 + 2 * 5),
            ),
        ]);

        let mut counts = HashMap::new();
        for (opcode_extension, flags, off0, off1, off2) in iproduct!(
            [STONE, BLAKE, BLAKE_FINALIZE, QM31],
            0..N_FLAG_COMBINATIONS,
            OFFSETS,
            OFFSETS,
            OFFSETS
        ) {
            let encoded_instr = encode([off0, off1, off2], flags, opcode_extension);
            let Ok(instruction) = decode_instruction(encoded_instr) else {
                continue;
            };
            if let Some(supported) = classify_instruction(&instruction) {
                *counts.entry(supported).or_insert(0) += 1;
            }
        }
        assert_eq!(counts, expected_counts);
    }
}
