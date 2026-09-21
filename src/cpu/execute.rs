use super::{
    AccessWidth, CSR_MINSTRET, Cpu, CpuBus, CsrError, ENVCFG_CBCFE, ENVCFG_CBIE, ENVCFG_CBZE,
    Exception, InstructionFlow, InstructionOutcome, MSTATUS_MIE, MSTATUS_MPIE, MSTATUS_MPP,
    MSTATUS_MPRV, MSTATUS_SIE, MSTATUS_SPIE, MSTATUS_SPP, MSTATUS_TSR, MSTATUS_TVM, MSTATUS_TW,
    Privilege, Trap, low_u32, mmu::Access,
};

impl Cpu {
    pub(super) fn execute<B: CpuBus>(
        &mut self,
        bus: &mut B,
        pc: u64,
        instruction: u32,
        length: u64,
    ) -> Result<InstructionOutcome, Trap> {
        if length == 2 {
            return self.execute_compressed(
                bus,
                pc,
                u16::try_from(instruction).expect("a compressed instruction fits u16"),
            );
        }
        let opcode = instruction & 0x7f;
        let rd = register_index(instruction, 7);
        let funct3 = (instruction >> 12) & 7;
        let rs1 = register_index(instruction, 15);
        let rs2 = register_index(instruction, 20);
        let mut next_pc = pc.wrapping_add(4);
        let mut retired = true;
        let mut flow = InstructionFlow::Sequential;

        match opcode {
            0x37 => self.write_register(rd, sign_extend(u64::from(instruction & 0xffff_f000), 32)),
            0x17 => self.write_register(
                rd,
                pc.wrapping_add(sign_extend(u64::from(instruction & 0xffff_f000), 32)),
            ),
            0x6f => {
                let immediate = decode_j_immediate(instruction);
                let target = checked_target(pc.wrapping_add(immediate), instruction)?;
                self.write_register(rd, next_pc);
                next_pc = target;
                flow = InstructionFlow::Exit;
            }
            0x67 if funct3 == 0 => {
                let target = self.registers[rs1]
                    .wrapping_add(sign_extend(u64::from(instruction >> 20), 12))
                    & !1;
                let target = checked_target(target, instruction)?;
                self.write_register(rd, next_pc);
                next_pc = target;
                flow = InstructionFlow::Exit;
            }
            0x63 => {
                let left = self.registers[rs1];
                let right = self.registers[rs2];
                let taken = match funct3 {
                    0 => left == right,
                    1 => left != right,
                    4 => signed(left) < signed(right),
                    5 => signed(left) >= signed(right),
                    6 => left < right,
                    7 => left >= right,
                    _ => return Err(illegal(instruction)),
                };
                if taken {
                    next_pc = checked_target(
                        pc.wrapping_add(decode_b_immediate(instruction)),
                        instruction,
                    )?;
                    flow = InstructionFlow::Exit;
                }
            }
            0x03 | 0x23 => self.execute_memory(bus, instruction, opcode)?,
            0x07 | 0x27 => self.execute_fp_memory(bus, instruction, opcode)?,
            0x43 | 0x47 | 0x4b | 0x4f | 0x53 => {
                self.execute_floating(instruction, opcode)?;
            }
            0x2f => self.execute_atomic(bus, instruction, rd, rs1, rs2, funct3)?,
            0x13 | 0x1b | 0x33 | 0x3b => {
                self.execute_arithmetic(instruction, opcode, rd, rs1, rs2, funct3)?;
            }
            0x0f => match funct3 {
                0 if instruction & 0xf00f_ff80 == 0 => {}
                1 if instruction == 0x0000_100f => {}
                2 => self.execute_cache_block(bus, instruction, rd, rs1)?,
                _ => return Err(illegal(instruction)),
            },
            0x73 => {
                (next_pc, retired) = self.execute_system(pc, instruction, rd, rs1, funct3)?;
                flow = InstructionFlow::Exit;
            }
            _ => return Err(illegal(instruction)),
        }
        Ok(InstructionOutcome {
            next_pc,
            retired,
            flow,
        })
    }

    fn execute_atomic<B: CpuBus>(
        &mut self,
        bus: &mut B,
        instruction: u32,
        rd: usize,
        rs1: usize,
        rs2: usize,
        width_code: u32,
    ) -> Result<(), Trap> {
        let width = match width_code {
            2 => AccessWidth::Word,
            3 => AccessWidth::DoubleWord,
            _ => return Err(illegal(instruction)),
        };
        let address = self.registers[rs1];
        if address & (width.bytes() as u64 - 1) != 0 {
            return Err(Trap {
                exception: if instruction >> 27 == 2 {
                    Exception::LoadAddressMisaligned
                } else {
                    Exception::StoreAddressMisaligned
                },
                value: address,
            });
        }
        let operation = instruction >> 27;
        if operation == 2 {
            if rs2 != 0 {
                return Err(illegal(instruction));
            }
            let old = self.load(bus, address, width, Access::Read)?;
            self.reservation = Some((address, width));
            self.write_register(rd, atomic_result(old, width));
            return Ok(());
        }
        if operation == 3 {
            let succeeds = self.reservation == Some((address, width));
            self.reservation = None;
            if succeeds {
                self.store(bus, address, width, self.registers[rs2])?;
                self.write_register(rd, 0);
            } else {
                self.check_store(bus, address, width)?;
                self.write_register(rd, 1);
            }
            return Ok(());
        }
        let old = self.load_for_store(bus, address, width)?;
        let source = truncate_atomic(self.registers[rs2], width);
        let old_truncated = truncate_atomic(old, width);
        let value = match operation {
            0 => old_truncated.wrapping_add(source),
            1 => source,
            4 => old_truncated ^ source,
            8 => old_truncated | source,
            12 => old_truncated & source,
            16 => signed_atomic_min(old_truncated, source, width),
            20 => signed_atomic_max(old_truncated, source, width),
            24 => old_truncated.min(source),
            28 => old_truncated.max(source),
            _ => return Err(illegal(instruction)),
        };
        self.reservation = None;
        self.store(bus, address, width, value)?;
        self.write_register(rd, atomic_result(old, width));
        Ok(())
    }

    fn execute_memory<B: CpuBus>(
        &mut self,
        bus: &mut B,
        instruction: u32,
        opcode: u32,
    ) -> Result<(), Trap> {
        let rd = register_index(instruction, 7);
        let funct3 = (instruction >> 12) & 7;
        let rs1 = register_index(instruction, 15);
        let rs2 = register_index(instruction, 20);
        if opcode == 0x03 {
            let address =
                self.registers[rs1].wrapping_add(sign_extend(u64::from(instruction >> 20), 12));
            let (width, signed_bits) = match funct3 {
                0 => (AccessWidth::Byte, Some(8)),
                1 => (AccessWidth::HalfWord, Some(16)),
                2 => (AccessWidth::Word, Some(32)),
                3 => (AccessWidth::DoubleWord, None),
                4 => (AccessWidth::Byte, None),
                5 => (AccessWidth::HalfWord, None),
                6 => (AccessWidth::Word, None),
                _ => return Err(illegal(instruction)),
            };
            let value = self.load(bus, address, width, Access::Read)?;
            self.write_register(
                rd,
                signed_bits.map_or(value, |bits| sign_extend(value, bits)),
            );
        } else {
            let immediate =
                u64::from((instruction >> 7) & 0x1f) | u64::from((instruction >> 25) & 0x7f) << 5;
            let address = self.registers[rs1].wrapping_add(sign_extend(immediate, 12));
            let width = match funct3 {
                0 => AccessWidth::Byte,
                1 => AccessWidth::HalfWord,
                2 => AccessWidth::Word,
                3 => AccessWidth::DoubleWord,
                _ => return Err(illegal(instruction)),
            };
            self.store(bus, address, width, self.registers[rs2])?;
            self.reservation = None;
        }
        Ok(())
    }

    #[inline]
    fn execute_arithmetic(
        &mut self,
        instruction: u32,
        opcode: u32,
        rd: usize,
        rs1: usize,
        rs2: usize,
        funct3: u32,
    ) -> Result<(), Trap> {
        let left = self.registers[rs1];
        let right = self.registers[rs2];
        let value = match opcode {
            0x13 => execute_immediate(instruction, funct3, left)?,
            0x1b if funct3 == 1 && instruction >> 26 == 2 => {
                u64::from(low_u32(left)) << (instruction >> 20 & 0x3f)
            }
            0x1b => sign_extend(
                u64::from(execute_word_immediate(instruction, funct3, low_u32(left))?),
                32,
            ),
            0x33 => execute_register(instruction, funct3, left, right)?,
            0x3b if instruction >> 25 == 0x04 && funct3 == 0 => {
                u64::from(low_u32(left)).wrapping_add(right)
            }
            0x3b if instruction >> 25 == 0x04 && funct3 == 4 && rs2 == 0 => left & 0xffff,
            0x3b if instruction >> 25 == 0x10 && matches!(funct3, 2 | 4 | 6) => {
                right.wrapping_add(u64::from(low_u32(left)) << (funct3 >> 1))
            }
            0x3b => sign_extend(
                u64::from(execute_word_register(
                    instruction,
                    funct3,
                    low_u32(left),
                    low_u32(right),
                )?),
                32,
            ),
            _ => unreachable!(),
        };
        self.write_register(rd, value);
        Ok(())
    }

    fn execute_cache_block<B: CpuBus>(
        &mut self,
        bus: &mut B,
        instruction: u32,
        rd: usize,
        rs1: usize,
    ) -> Result<(), Trap> {
        let operation = instruction >> 20;
        if rd != 0 || !matches!(operation, 0 | 1 | 2 | 4) {
            return Err(illegal(instruction));
        }
        if self.privilege != Privilege::Machine {
            let required = match operation {
                0 => ENVCFG_CBIE,
                1 | 2 => ENVCFG_CBCFE,
                4 => ENVCFG_CBZE,
                _ => unreachable!(),
            };
            if self.menvcfg & required == 0
                || self.privilege == Privilege::User && self.senvcfg & required == 0
            {
                return Err(illegal(instruction));
            }
        }
        self.cache_block(bus, self.registers[rs1], operation == 4)
    }

    fn execute_system(
        &mut self,
        pc: u64,
        instruction: u32,
        rd: usize,
        rs1: usize,
        funct3: u32,
    ) -> Result<(u64, bool), Trap> {
        if funct3 == 4 {
            if instruction & 0xb3c0_707f == 0x81c0_4073 || instruction & 0xb200_707f == 0x8200_4073
            {
                self.write_register(rd, 0);
                return Ok((pc.wrapping_add(4), true));
            }
            return Err(illegal(instruction));
        }
        if funct3 == 0 {
            match instruction {
                0x0000_0073 => {
                    return Err(Trap {
                        exception: match self.privilege {
                            Privilege::User => Exception::UserEnvironmentCall,
                            Privilege::Supervisor => Exception::SupervisorEnvironmentCall,
                            Privilege::Machine => Exception::MachineEnvironmentCall,
                        },
                        value: 0,
                    });
                }
                0x0010_0073 => {
                    return Err(Trap {
                        exception: Exception::Breakpoint,
                        value: 0,
                    });
                }
                0x1020_0073 => return Ok((self.return_from_supervisor(instruction)?, true)),
                0x3020_0073 => return Ok((self.return_from_machine(instruction)?, true)),
                0x1050_0073 => {
                    if self.privilege < Privilege::Machine && self.mstatus & MSTATUS_TW != 0 {
                        return Err(illegal(instruction));
                    }
                    if self.mip & self.mie == 0 {
                        self.power_down = true;
                    }
                }
                0x00d0_0073 | 0x01d0_0073 => {}
                0x1800_0073 | 0x1810_0073 => {
                    if self.privilege == Privilege::User {
                        return Err(illegal(instruction));
                    }
                }
                _ if instruction & 0xfe00_7fff == 0x1200_0073
                    || instruction & 0xfe00_7fff == 0x1600_0073 =>
                {
                    if self.privilege == Privilege::User
                        || self.privilege == Privilege::Supervisor
                            && self.mstatus & MSTATUS_TVM != 0
                    {
                        return Err(illegal(instruction));
                    }
                    self.flush_tlb();
                }
                _ => return Err(illegal(instruction)),
            }
            return Ok((pc.wrapping_add(4), true));
        }

        let csr = u16::try_from(instruction >> 20).expect("a CSR field is 12 bits");
        let immediate = funct3 & 4 != 0;
        let source = if immediate {
            u64::try_from(rs1).expect("a register index fits u64")
        } else {
            self.registers[rs1]
        };
        let operation = funct3 & 3;
        if operation == 0 {
            return Err(illegal(instruction));
        }
        let will_write = operation == 1 || rs1 != 0;
        let old = self
            .csr_read(csr, will_write)
            .map_err(|CsrError::IllegalAccess| illegal(instruction))?;
        if will_write {
            let value = match operation {
                1 => source,
                2 => old | source,
                3 => old & !source,
                _ => unreachable!(),
            };
            self.csr_write(csr, value)
                .map_err(|CsrError::IllegalAccess| illegal(instruction))?;
        }
        self.write_register(rd, old);
        Ok((pc.wrapping_add(4), !(will_write && csr == CSR_MINSTRET)))
    }

    fn return_from_supervisor(&mut self, instruction: u32) -> Result<u64, Trap> {
        if self.privilege < Privilege::Supervisor
            || self.privilege == Privilege::Supervisor && self.mstatus & MSTATUS_TSR != 0
        {
            return Err(illegal(instruction));
        }
        let privilege = if self.mstatus & MSTATUS_SPP != 0 {
            Privilege::Supervisor
        } else {
            Privilege::User
        };
        self.mstatus = (self.mstatus & !MSTATUS_SIE)
            | if self.mstatus & MSTATUS_SPIE != 0 {
                MSTATUS_SIE
            } else {
                0
            };
        self.mstatus |= MSTATUS_SPIE;
        self.mstatus &= !(MSTATUS_SPP | MSTATUS_MPRV);
        self.set_privilege(privilege);
        Ok(self.sepc)
    }

    fn return_from_machine(&mut self, instruction: u32) -> Result<u64, Trap> {
        if self.privilege != Privilege::Machine {
            return Err(illegal(instruction));
        }
        let encoded = (self.mstatus & MSTATUS_MPP) >> 11;
        let privilege = match encoded {
            0 => Privilege::User,
            1 => Privilege::Supervisor,
            3 => Privilege::Machine,
            _ => return Err(illegal(instruction)),
        };
        self.mstatus = (self.mstatus & !MSTATUS_MIE)
            | if self.mstatus & MSTATUS_MPIE != 0 {
                MSTATUS_MIE
            } else {
                0
            };
        self.mstatus |= MSTATUS_MPIE;
        self.mstatus &= !MSTATUS_MPP;
        if privilege < Privilege::Machine {
            self.mstatus &= !MSTATUS_MPRV;
        }
        self.set_privilege(privilege);
        Ok(self.mepc)
    }
}

fn illegal(instruction: u32) -> Trap {
    Trap {
        exception: Exception::IllegalInstruction,
        value: u64::from(instruction),
    }
}

fn checked_target(target: u64, _instruction: u32) -> Result<u64, Trap> {
    if target.trailing_zeros() >= 1 {
        Ok(target)
    } else {
        Err(Trap {
            exception: Exception::InstructionAddressMisaligned,
            value: target,
        })
    }
}

fn register_index(instruction: u32, shift: u32) -> usize {
    usize::try_from((instruction >> shift) & 0x1f).expect("a register field fits usize")
}

fn decode_j_immediate(instruction: u32) -> u64 {
    let value = u64::from((instruction >> 31) & 1) << 20
        | u64::from((instruction >> 21) & 0x3ff) << 1
        | u64::from((instruction >> 20) & 1) << 11
        | u64::from((instruction >> 12) & 0xff) << 12;
    sign_extend(value, 21)
}

fn decode_b_immediate(instruction: u32) -> u64 {
    let value = u64::from((instruction >> 31) & 1) << 12
        | u64::from((instruction >> 25) & 0x3f) << 5
        | u64::from((instruction >> 8) & 0xf) << 1
        | u64::from((instruction >> 7) & 1) << 11;
    sign_extend(value, 13)
}

fn sign_extend(value: u64, bits: u32) -> u64 {
    let shift = 64 - bits;
    unsigned(signed(value << shift) >> shift)
}

fn signed(value: u64) -> i64 {
    i64::from_ne_bytes(value.to_ne_bytes())
}

fn unsigned(value: i64) -> u64 {
    u64::from_ne_bytes(value.to_ne_bytes())
}

fn signed32(value: u32) -> i32 {
    i32::from_ne_bytes(value.to_ne_bytes())
}

fn unsigned32(value: i32) -> u32 {
    u32::from_ne_bytes(value.to_ne_bytes())
}

fn execute_immediate(instruction: u32, funct3: u32, left: u64) -> Result<u64, Trap> {
    let immediate = sign_extend(u64::from(instruction >> 20), 12);
    let encoded = instruction >> 20;
    let shift = encoded & 0x3f;
    let value = match funct3 {
        0 => left.wrapping_add(immediate),
        2 => u64::from(signed(left) < signed(immediate)),
        3 => u64::from(left < immediate),
        4 => left ^ immediate,
        6 => left | immediate,
        7 => left & immediate,
        1 if instruction >> 26 == 0 => left << shift,
        1 if encoded & !0x3f == 0x280 => left | (1_u64 << shift),
        1 if encoded & !0x3f == 0x480 => left & !(1_u64 << shift),
        1 if encoded & !0x3f == 0x680 => left ^ (1_u64 << shift),
        1 if encoded == 0x600 => u64::from(left.leading_zeros()),
        1 if encoded == 0x601 => u64::from(left.trailing_zeros()),
        1 if encoded == 0x602 => u64::from(left.count_ones()),
        1 if encoded == 0x604 => sign_extend(left, 8),
        1 if encoded == 0x605 => sign_extend(left, 16),
        5 if instruction >> 26 == 0 => left >> shift,
        5 if instruction >> 26 == 0x10 => unsigned(signed(left) >> shift),
        5 if encoded & !0x3f == 0x600 => left.rotate_right(shift),
        5 if encoded & !0x3f == 0x480 => (left >> shift) & 1,
        5 if encoded == 0x287 => left
            .to_le_bytes()
            .map(|byte| if byte == 0 { 0_u8 } else { 0xff_u8 })
            .into_iter()
            .enumerate()
            .fold(0, |result, (index, byte)| {
                result | u64::from(byte) << (index * 8)
            }),
        5 if encoded == 0x6b8 => left.swap_bytes(),
        _ => return Err(illegal(instruction)),
    };
    Ok(value)
}

fn execute_word_immediate(instruction: u32, funct3: u32, left: u32) -> Result<u32, Trap> {
    let encoded = instruction >> 20;
    let value = match funct3 {
        0 => left.wrapping_add(low_u32(sign_extend(u64::from(instruction >> 20), 12))),
        1 if instruction >> 25 == 0 => left << (instruction >> 20 & 0x1f),
        1 if encoded == 0x600 => left.leading_zeros(),
        1 if encoded == 0x601 => left.trailing_zeros(),
        1 if encoded == 0x602 => left.count_ones(),
        5 if instruction >> 25 == 0 => left >> (instruction >> 20 & 0x1f),
        5 if instruction >> 25 == 0x20 => unsigned32(signed32(left) >> (instruction >> 20 & 0x1f)),
        5 if encoded & !0x1f == 0x600 => left.rotate_right(encoded & 0x1f),
        _ => return Err(illegal(instruction)),
    };
    Ok(value)
}

fn execute_register(instruction: u32, funct3: u32, left: u64, right: u64) -> Result<u64, Trap> {
    let funct7 = instruction >> 25;
    if funct7 == 1 {
        return execute_multiply_divide(funct3, left, right);
    }
    let value = match (funct7, funct3) {
        (0, 0) => left.wrapping_add(right),
        (0x20, 0) => left.wrapping_sub(right),
        (0, 1) => left << (right & 0x3f),
        (0, 2) => u64::from(signed(left) < signed(right)),
        (0, 3) => u64::from(left < right),
        (0, 4) => left ^ right,
        (0, 5) => left >> (right & 0x3f),
        (0x20, 5) => unsigned(signed(left) >> (right & 0x3f)),
        (0, 6) => left | right,
        (0, 7) => left & right,
        (0x20, 4) => !(left ^ right),
        (0x20, 6) => left | !right,
        (0x20, 7) => left & !right,
        (0x05, 4) => {
            if signed(left) < signed(right) {
                left
            } else {
                right
            }
        }
        (0x05, 5) => left.min(right),
        (0x05, 6) => {
            if signed(left) > signed(right) {
                left
            } else {
                right
            }
        }
        (0x05, 7) => left.max(right),
        (0x14, 1) => left | (1_u64 << (right & 0x3f)),
        (0x24, 1) => left & !(1_u64 << (right & 0x3f)),
        (0x24, 5) => (left >> (right & 0x3f)) & 1,
        (0x34, 1) => left ^ (1_u64 << (right & 0x3f)),
        (0x10, 2 | 4 | 6) => right.wrapping_add(left << (funct3 >> 1)),
        (0x30, 1) => left.rotate_left((right & 0x3f) as u32),
        (0x30, 5) => left.rotate_right((right & 0x3f) as u32),
        (0x07, 5) => {
            if right == 0 {
                0
            } else {
                left
            }
        }
        (0x07, 7) => {
            if right != 0 {
                0
            } else {
                left
            }
        }
        _ => return Err(illegal(instruction)),
    };
    Ok(value)
}

fn execute_word_register(
    instruction: u32,
    funct3: u32,
    left: u32,
    right: u32,
) -> Result<u32, Trap> {
    let funct7 = instruction >> 25;
    if funct7 == 1 {
        return execute_word_multiply_divide(funct3, left, right);
    }
    let value = match (funct7, funct3) {
        (0, 0) => left.wrapping_add(right),
        (0x20, 0) => left.wrapping_sub(right),
        (0, 1) => left << (right & 0x1f),
        (0, 5) => left >> (right & 0x1f),
        (0x20, 5) => unsigned32(signed32(left) >> (right & 0x1f)),
        (0x30, 1) => left.rotate_left(right & 0x1f),
        (0x30, 5) => left.rotate_right(right & 0x1f),
        _ => return Err(illegal(instruction)),
    };
    Ok(value)
}

fn execute_multiply_divide(funct3: u32, left: u64, right: u64) -> Result<u64, Trap> {
    let value = match funct3 {
        0 => left.wrapping_mul(right),
        1 => high_i128(i128::from(signed(left)) * i128::from(signed(right))),
        2 => high_i128(i128::from(signed(left)) * i128::from(right)),
        3 => high_u128(u128::from(left) * u128::from(right)),
        4 => {
            let left = signed(left);
            let right = signed(right);
            if right == 0 {
                u64::MAX
            } else if left == i64::MIN && right == -1 {
                unsigned(left)
            } else {
                unsigned(left / right)
            }
        }
        5 => {
            if right == 0 {
                u64::MAX
            } else {
                left / right
            }
        }
        6 => {
            let left = signed(left);
            let right = signed(right);
            if right == 0 {
                unsigned(left)
            } else if left == i64::MIN && right == -1 {
                0
            } else {
                unsigned(left % right)
            }
        }
        7 => {
            if right == 0 {
                left
            } else {
                left % right
            }
        }
        _ => return Err(illegal(0)),
    };
    Ok(value)
}

fn execute_word_multiply_divide(funct3: u32, left: u32, right: u32) -> Result<u32, Trap> {
    let value = match funct3 {
        0 => left.wrapping_mul(right),
        4 => {
            let left = signed32(left);
            let right = signed32(right);
            if right == 0 {
                u32::MAX
            } else if left == i32::MIN && right == -1 {
                unsigned32(left)
            } else {
                unsigned32(left / right)
            }
        }
        5 => {
            if right == 0 {
                u32::MAX
            } else {
                left / right
            }
        }
        6 => {
            let left = signed32(left);
            let right = signed32(right);
            if right == 0 {
                unsigned32(left)
            } else if left == i32::MIN && right == -1 {
                0
            } else {
                unsigned32(left % right)
            }
        }
        7 => {
            if right == 0 {
                left
            } else {
                left % right
            }
        }
        _ => return Err(illegal(0)),
    };
    Ok(value)
}

fn high_i128(value: i128) -> u64 {
    let bytes = value.to_le_bytes();
    u64::from_le_bytes([
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ])
}

fn high_u128(value: u128) -> u64 {
    let bytes = value.to_le_bytes();
    u64::from_le_bytes([
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ])
}

fn truncate_atomic(value: u64, width: AccessWidth) -> u64 {
    match width {
        AccessWidth::Word => value & u64::from(u32::MAX),
        AccessWidth::DoubleWord => value,
        _ => unreachable!(),
    }
}

fn atomic_result(value: u64, width: AccessWidth) -> u64 {
    match width {
        AccessWidth::Word => sign_extend(value & u64::from(u32::MAX), 32),
        AccessWidth::DoubleWord => value,
        _ => unreachable!(),
    }
}

fn signed_atomic_min(left: u64, right: u64, width: AccessWidth) -> u64 {
    if signed_atomic(left, width) < signed_atomic(right, width) {
        left
    } else {
        right
    }
}

fn signed_atomic_max(left: u64, right: u64, width: AccessWidth) -> u64 {
    if signed_atomic(left, width) > signed_atomic(right, width) {
        left
    } else {
        right
    }
}

fn signed_atomic(value: u64, width: AccessWidth) -> i64 {
    match width {
        AccessWidth::Word => i64::from(low_u32(value).cast_signed()),
        AccessWidth::DoubleWord => signed(value),
        _ => unreachable!(),
    }
}
