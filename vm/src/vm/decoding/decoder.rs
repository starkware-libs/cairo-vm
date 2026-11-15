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

/// A classification of CASM instructions according to Stwo's AIR components.
///
/// The classification is used by:
/// - the VM (when `disable_generic_instructions` is true) to detect generic instructions which don't match any variant.
/// - the prover to route the instructions to the corresponding algebraic constraints.s
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StwoOpcodeKind {
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
    /// `[ap/fp + off0] = [ap/fp + off1] + imm/[ap/fp + off2]`.
    Add,
    /// `[ap/fp + off0] = [ap/fp + off1] * imm/[ap/fp + off2]`.
    Mul,
    /// Blake2s compress / finalize.
    Blake,
    /// QM31 add / mul.
    QM31AddMul,
}

/// Classifies `instruction`. Returns `None` if it is a "generic" instruction, i.e.
/// one that doesn't match any variant of [`StwoOpcodeKind`].
pub const fn classify_instruction(instruction: &Instruction) -> Option<StwoOpcodeKind> {
    /// An immediate operand must directly follow the instruction.
    const fn is_valid_op1_imm(op1_addr: Op1Addr, off2: isize) -> bool {
        !matches!(op1_addr, Op1Addr::Imm) || off2 == 1
    }

    match *instruction {
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
        } => Some(StwoOpcodeKind::Ret),

        Instruction {
            off0: -1,
            off1: -1,
            off2,
            dst_register: Register::FP,
            op0_register: Register::FP,
            op1_addr: op1_addr @ (Op1Addr::Imm | Op1Addr::FP | Op1Addr::AP),
            res: Res::Op1,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Add,
            fp_update: _,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        } => {
            if is_valid_op1_imm(op1_addr, off2) {
                Some(StwoOpcodeKind::AddAp)
            } else {
                None
            }
        }

        // Relative jumps.
        Instruction {
            off0: -1,
            off1,
            off2,
            dst_register: Register::FP,
            op0_register,
            op1_addr,
            res: Res::Op1,
            pc_update: PcUpdate::JumpRel,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        } => {
            let op0_fp = matches!(op0_register, Register::FP);
            match op1_addr {
                Op1Addr::Imm => {
                    if op0_fp && off1 == -1 && off2 == 1 {
                        Some(StwoOpcodeKind::JumpRelImm)
                    } else {
                        None
                    }
                }
                Op1Addr::FP | Op1Addr::AP => {
                    if op0_fp && off1 == -1 {
                        Some(StwoOpcodeKind::JumpRel)
                    } else {
                        None
                    }
                }
                Op1Addr::Op0 => None,
            }
        }

        // Absolute jumps.
        Instruction {
            off0: -1,
            off1,
            off2: _,
            dst_register: Register::FP,
            op0_register,
            op1_addr,
            res: Res::Op1,
            pc_update: PcUpdate::Jump,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::NOp,
            opcode_extension: OpcodeExtension::Stone,
        } => match op1_addr {
            Op1Addr::Op0 => Some(StwoOpcodeKind::JumpDoubleDeref),
            Op1Addr::FP | Op1Addr::AP => {
                if matches!(op0_register, Register::FP) && off1 == -1 {
                    Some(StwoOpcodeKind::JumpAbs)
                } else {
                    None
                }
            }
            Op1Addr::Imm => None,
        },

        // Calls.
        Instruction {
            off0: 0,
            off1: 1,
            off2,
            dst_register: Register::AP,
            op0_register: Register::AP,
            op1_addr,
            res: Res::Op1,
            pc_update,
            ap_update: ApUpdate::Add2,
            fp_update: _,
            opcode: Opcode::Call,
            opcode_extension: OpcodeExtension::Stone,
        } => match (op1_addr, pc_update) {
            (Op1Addr::Imm, PcUpdate::JumpRel) => {
                if off2 == 1 {
                    Some(StwoOpcodeKind::CallRelImm)
                } else {
                    None
                }
            }
            (Op1Addr::FP | Op1Addr::AP, PcUpdate::Jump) => Some(StwoOpcodeKind::CallAbs),
            _ => None,
        },

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
        } => Some(StwoOpcodeKind::Jnz),

        // Assert equal.
        Instruction {
            off0: _,
            off1,
            off2,
            dst_register: _,
            op0_register,
            op1_addr,
            res: Res::Op1,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        } => {
            let op0_fp = matches!(op0_register, Register::FP);
            match op1_addr {
                Op1Addr::Imm => {
                    if op0_fp && off1 == -1 && off2 == 1 {
                        Some(StwoOpcodeKind::AssertEqImm)
                    } else {
                        None
                    }
                }
                Op1Addr::Op0 => Some(StwoOpcodeKind::AssertEqDoubleDeref),
                Op1Addr::FP | Op1Addr::AP => {
                    if op0_fp && off1 == -1 {
                        Some(StwoOpcodeKind::AssertEq)
                    } else {
                        None
                    }
                }
            }
        }

        // Add / mul.
        Instruction {
            off0: _,
            off1: _,
            off2,
            dst_register: _,
            op0_register: _,
            op1_addr: op1_addr @ (Op1Addr::Imm | Op1Addr::FP | Op1Addr::AP),
            res: res @ (Res::Add | Res::Mul),
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::Stone,
        } => {
            if !is_valid_op1_imm(op1_addr, off2) {
                None
            } else if matches!(res, Res::Add) {
                Some(StwoOpcodeKind::Add)
            } else {
                Some(StwoOpcodeKind::Mul)
            }
        }

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
        } => Some(StwoOpcodeKind::Blake),

        Instruction {
            off0: _,
            off1: _,
            off2,
            dst_register: _,
            op0_register: _,
            op1_addr: op1_addr @ (Op1Addr::Imm | Op1Addr::FP | Op1Addr::AP),
            res: Res::Add | Res::Mul,
            pc_update: PcUpdate::Regular,
            ap_update: ApUpdate::Regular | ApUpdate::Add1,
            fp_update: _,
            opcode: Opcode::AssertEq,
            opcode_extension: OpcodeExtension::QM31Operation,
        } => {
            if is_valid_op1_imm(op1_addr, off2) {
                Some(StwoOpcodeKind::QM31AddMul)
            } else {
                None
            }
        }

        _ => None,
    }
}

/// Returns `true` if `instruction` is a "generic" instruction, i.e. one that is not covered by any
/// of the dedicated opcode components of the Stwo prover.
pub const fn is_generic(instruction: &Instruction) -> bool {
    classify_instruction(instruction).is_none()
}

#[cfg(test)]
mod decoder_test {
    use super::*;
    use assert_matches::assert_matches;
    use rstest::rstest;

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

    fn classify(encoded_instr: u128) -> Option<StwoOpcodeKind> {
        classify_instruction(&decode_instruction(encoded_instr).unwrap())
    }

    #[rstest]
    // ret.
    #[case::ret(encode([-2, -1, -1], DST_FP | OP0_FP | OP1_FP | PC_JUMP | RET, STONE), StwoOpcodeKind::Ret)]
    // ap += imm.
    #[case::add_ap_imm(encode([-1, -1, 1], DST_FP | OP0_FP | OP1_IMM | AP_ADD, STONE), StwoOpcodeKind::AddAp)]
    // ap += [fp].
    #[case::add_ap_fp(encode([-1, -1, 0], DST_FP | OP0_FP | OP1_FP | AP_ADD, STONE), StwoOpcodeKind::AddAp)]
    // jmp rel imm.
    #[case::jump_rel_imm(encode([-1, -1, 1], DST_FP | OP0_FP | OP1_IMM | PC_JUMP_REL, STONE), StwoOpcodeKind::JumpRelImm)]
    // jmp rel [ap - 2], ap++.
    #[case::jump_rel(encode([-1, -1, -2], DST_FP | OP0_FP | OP1_AP | PC_JUMP_REL | AP_ADD1, STONE), StwoOpcodeKind::JumpRel)]
    // jmp abs [[ap + 3] + 4].
    #[case::jump_double_deref(encode([-1, 3, 4], DST_FP | PC_JUMP, STONE), StwoOpcodeKind::JumpDoubleDeref)]
    // jmp abs [fp].
    #[case::jump_abs(encode([-1, -1, 0], DST_FP | OP0_FP | OP1_FP | PC_JUMP, STONE), StwoOpcodeKind::JumpAbs)]
    // call rel imm.
    #[case::call_rel_imm(encode([0, 1, 1], OP1_IMM | PC_JUMP_REL | CALL, STONE), StwoOpcodeKind::CallRelImm)]
    // call abs [fp - 1].
    #[case::call_abs(encode([0, 1, -1], OP1_FP | PC_JUMP | CALL, STONE), StwoOpcodeKind::CallAbs)]
    // jmp rel imm if [ap + 5] != 0.
    #[case::jnz(encode([5, -1, 1], OP0_FP | OP1_IMM | PC_JNZ, STONE), StwoOpcodeKind::Jnz)]
    // [ap] = imm, ap++.
    #[case::assert_eq_imm(encode([0, -1, 1], OP0_FP | OP1_IMM | AP_ADD1 | ASSERT_EQ, STONE), StwoOpcodeKind::AssertEqImm)]
    // [fp + 2] = [[ap - 2] + 2].
    #[case::assert_eq_double_deref(encode([2, -2, 2], DST_FP | ASSERT_EQ, STONE), StwoOpcodeKind::AssertEqDoubleDeref)]
    // [ap + 2] = [fp + 1].
    #[case::assert_eq(encode([2, -1, 1], OP0_FP | OP1_FP | ASSERT_EQ, STONE), StwoOpcodeKind::AssertEq)]
    // [fp + 2] = [fp] + imm.
    #[case::add_imm(encode([2, 0, 1], DST_FP | OP0_FP | OP1_IMM | RES_ADD | ASSERT_EQ, STONE), StwoOpcodeKind::Add)]
    // [ap] = [ap - 1] + [fp - 3], ap++.
    #[case::add(encode([0, -1, -3], OP1_FP | RES_ADD | AP_ADD1 | ASSERT_EQ, STONE), StwoOpcodeKind::Add)]
    // [ap] = [ap] * [ap], ap++.
    #[case::mul(encode([0, 0, 0], OP1_AP | RES_MUL | AP_ADD1 | ASSERT_EQ, STONE), StwoOpcodeKind::Mul)]
    #[case::blake(encode([1, 1, 1], DST_FP | OP0_FP | OP1_AP | AP_ADD1, BLAKE), StwoOpcodeKind::Blake)]
    #[case::blake_finalize(encode([-5, -4, -3], OP0_FP | OP1_FP, BLAKE_FINALIZE), StwoOpcodeKind::Blake)]
    #[case::qm31_mul(encode([1, 1, 1], OP1_FP | RES_MUL | ASSERT_EQ, QM31), StwoOpcodeKind::QM31AddMul)]
    #[case::qm31_add_imm(encode([1, 1, 1], OP1_IMM | RES_ADD | AP_ADD1 | ASSERT_EQ, QM31), StwoOpcodeKind::QM31AddMul)]
    fn classify_supported_instruction(
        #[case] encoded_instr: u128,
        #[case] expected: StwoOpcodeKind,
    ) {
        assert_eq!(classify(encoded_instr), Some(expected));
        assert!(!is_generic(&decode_instruction(encoded_instr).unwrap()));
    }

    #[rstest]
    // Regular nop.
    #[case::nop(encode([0, 0, 0], 0, STONE))]
    // A single instruction doing an add, a jump and an ap update.
    #[case::nop_add_jump_add_ap(encode([0, 0, 0], DST_FP | OP0_FP | OP1_IMM | RES_ADD | PC_JUMP | AP_ADD, STONE))]
    // ap += imm with a non-adjacent immediate.
    #[case::add_ap_imm_bad_off2(encode([-1, -1, 2], DST_FP | OP0_FP | OP1_IMM | AP_ADD, STONE))]
    // ap += [[fp] + 0].
    #[case::add_ap_double_deref(encode([-1, -1, 0], DST_FP | OP0_FP | AP_ADD, STONE))]
    // ap += [fp] with dst off0 != -1.
    #[case::add_ap_bad_off0(encode([0, -1, 0], DST_FP | OP0_FP | OP1_FP | AP_ADD, STONE))]
    // jmp abs imm.
    #[case::jump_abs_imm(encode([-1, -1, 1], DST_FP | OP0_FP | OP1_IMM | PC_JUMP, STONE))]
    // jmp rel [[fp - 1] + 0].
    #[case::jump_rel_double_deref(encode([-1, -1, 0], DST_FP | OP0_FP | PC_JUMP_REL, STONE))]
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
    // call rel imm with a non-adjacent immediate.
    #[case::call_rel_imm_bad_off2(encode([0, 1, 2], OP1_IMM | PC_JUMP_REL | CALL, STONE))]
    // jnz with an immediate that is not adjacent.
    #[case::jnz_bad_off2(encode([0, -1, 2], OP0_FP | OP1_IMM | PC_JNZ, STONE))]
    // jnz with ap += imm.
    #[case::jnz_with_ap_add(encode([0, -1, 1], OP0_FP | OP1_IMM | PC_JNZ | AP_ADD, STONE))]
    // jmp rel [fp] if [ap] != 0.
    #[case::jnz_not_imm(encode([0, -1, 0], OP0_FP | OP1_FP | PC_JNZ, STONE))]
    // [ap] = imm with a non-adjacent immediate.
    #[case::assert_eq_imm_bad_off2(encode([0, -1, 2], OP0_FP | OP1_IMM | ASSERT_EQ, STONE))]
    // [ap] = [fp] with op0 based on ap.
    #[case::assert_eq_op0_ap(encode([0, -1, 0], OP1_FP | ASSERT_EQ, STONE))]
    // [ap] = [fp] with off1 != -1.
    #[case::assert_eq_bad_off1(encode([0, 0, 0], OP0_FP | OP1_FP | ASSERT_EQ, STONE))]
    // [ap] = [fp], with a jump.
    #[case::assert_eq_with_jump(encode([0, -1, 0], OP0_FP | OP1_FP | PC_JUMP | ASSERT_EQ, STONE))]
    // [ap] = [ap] + imm with a non-adjacent immediate.
    #[case::add_imm_bad_off2(encode([0, 0, 2], OP1_IMM | RES_ADD | ASSERT_EQ, STONE))]
    // [ap] = [ap] + [[ap] + 0].
    #[case::add_double_deref(encode([0, 0, 0], RES_ADD | ASSERT_EQ, STONE))]
    // [ap] * [ap] without an assert_eq.
    #[case::mul_nop(encode([0, 0, 0], OP1_AP | RES_MUL, STONE))]
    // [ap] = [ap] * [ap], ap += [ap].
    #[case::mul_with_ap_add(encode([0, 0, 0], OP1_AP | RES_MUL | AP_ADD | ASSERT_EQ, STONE))]
    // QM31 with a non-adjacent immediate.
    #[case::qm31_imm_bad_off2(encode([1, 1, 2], OP1_IMM | RES_ADD | ASSERT_EQ, QM31))]
    fn classify_generic_instruction(#[case] encoded_instr: u128) {
        assert_eq!(classify(encoded_instr), None);
        assert!(is_generic(&decode_instruction(encoded_instr).unwrap()));
    }
}
