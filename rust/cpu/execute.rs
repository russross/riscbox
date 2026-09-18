use super::{
    AccessWidth, CSR_MINSTRET, Cpu, CpuBus, CsrError, Exception, MSTATUS_MIE, MSTATUS_MPIE,
    MSTATUS_MPP, MSTATUS_MPRV, MSTATUS_SIE, MSTATUS_SPIE, MSTATUS_SPP, MSTATUS_TSR, MSTATUS_TVM,
    MSTATUS_TW, Privilege, Trap, low_u32, mmu::Access,
};

impl Cpu {
    pub(super) fn execute<B: CpuBus>(
        &mut self,
        bus: &mut B,
        instruction: u32,
    ) -> Result<bool, Trap> {
        let pc = self.pc;
        let opcode = instruction & 0x7f;
        let rd = register_index(instruction, 7);
        let funct3 = (instruction >> 12) & 7;
        let rs1 = register_index(instruction, 15);
        let rs2 = register_index(instruction, 20);
        let mut next_pc = pc.wrapping_add(4);
        let mut retired = true;

        match opcode {
            0x37 => self.write_register(rd, sign_extend(u64::from(instruction & 0xffff_f000), 32)),
            0x17 => self.write_register(
                rd,
                pc.wrapping_add(sign_extend(u64::from(instruction & 0xffff_f000), 32)),
            ),
            0x6f => {
                let immediate = decode_j_immediate(instruction);
                self.write_register(rd, next_pc);
                next_pc = checked_target(pc.wrapping_add(immediate), instruction)?;
            }
            0x67 if funct3 == 0 => {
                let target = self.registers[rs1]
                    .wrapping_add(sign_extend(u64::from(instruction >> 20), 12))
                    & !1;
                self.write_register(rd, next_pc);
                next_pc = checked_target(target, instruction)?;
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
                }
            }
            0x03 | 0x23 => self.execute_memory(bus, instruction, opcode)?,
            0x13 | 0x1b | 0x33 | 0x3b => {
                self.execute_arithmetic(instruction, opcode, rd, rs1, rs2, funct3)?;
            }
            0x0f => match funct3 {
                0 if instruction & 0xf00f_ff80 == 0 => {}
                1 if instruction == 0x0000_100f => {}
                _ => return Err(illegal(instruction)),
            },
            0x73 => {
                retired = self.execute_system(instruction, rd, rs1, funct3)?;
                next_pc = self.pc;
            }
            _ => return Err(illegal(instruction)),
        }
        self.pc = next_pc;
        Ok(retired)
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
        }
        Ok(())
    }

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
            0x1b => sign_extend(
                u64::from(execute_word_immediate(instruction, funct3, low_u32(left))?),
                32,
            ),
            0x33 => execute_register(instruction, funct3, left, right)?,
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

    fn execute_system(
        &mut self,
        instruction: u32,
        rd: usize,
        rs1: usize,
        funct3: u32,
    ) -> Result<bool, Trap> {
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
                0x1020_0073 => self.return_from_supervisor(instruction)?,
                0x3020_0073 => self.return_from_machine(instruction)?,
                0x1050_0073 => {
                    if self.privilege < Privilege::Machine && self.mstatus & MSTATUS_TW != 0 {
                        return Err(illegal(instruction));
                    }
                    self.pc = self.pc.wrapping_add(4);
                    if self.mip & self.mie == 0 {
                        self.power_down = true;
                    }
                }
                _ if instruction & 0xfe00_7fff == 0x1200_0073 => {
                    if self.privilege == Privilege::User
                        || self.privilege == Privilege::Supervisor
                            && self.mstatus & MSTATUS_TVM != 0
                    {
                        return Err(illegal(instruction));
                    }
                    self.flush_tlb();
                    self.pc = self.pc.wrapping_add(4);
                }
                _ => return Err(illegal(instruction)),
            }
            return Ok(true);
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
        let will_write = operation == 1 || source != 0;
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
        self.pc = self.pc.wrapping_add(4);
        Ok(!(will_write && csr == CSR_MINSTRET))
    }

    fn return_from_supervisor(&mut self, instruction: u32) -> Result<(), Trap> {
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
        self.pc = self.sepc;
        Ok(())
    }

    fn return_from_machine(&mut self, instruction: u32) -> Result<(), Trap> {
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
        self.pc = self.mepc;
        Ok(())
    }
}

fn illegal(instruction: u32) -> Trap {
    Trap {
        exception: Exception::IllegalInstruction,
        value: u64::from(instruction),
    }
}

fn checked_target(target: u64, _instruction: u32) -> Result<u64, Trap> {
    if target.trailing_zeros() >= 2 {
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
    let value = match funct3 {
        0 => left.wrapping_add(immediate),
        2 => u64::from(signed(left) < signed(immediate)),
        3 => u64::from(left < immediate),
        4 => left ^ immediate,
        6 => left | immediate,
        7 => left & immediate,
        1 if instruction >> 26 == 0 => left << (instruction >> 20 & 0x3f),
        5 if instruction >> 26 == 0 => left >> (instruction >> 20 & 0x3f),
        5 if instruction >> 26 == 0x10 => unsigned(signed(left) >> (instruction >> 20 & 0x3f)),
        _ => return Err(illegal(instruction)),
    };
    Ok(value)
}

fn execute_word_immediate(instruction: u32, funct3: u32, left: u32) -> Result<u32, Trap> {
    let value = match funct3 {
        0 => left.wrapping_add(low_u32(sign_extend(u64::from(instruction >> 20), 12))),
        1 if instruction >> 25 == 0 => left << (instruction >> 20 & 0x1f),
        5 if instruction >> 25 == 0 => left >> (instruction >> 20 & 0x1f),
        5 if instruction >> 25 == 0x20 => unsigned32(signed32(left) >> (instruction >> 20 & 0x1f)),
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
