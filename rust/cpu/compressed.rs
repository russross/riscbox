use super::{AccessWidth, Cpu, CpuBus, Exception, Trap, mmu::Access};

impl Cpu {
    pub(super) fn execute_compressed<B: CpuBus>(
        &mut self,
        bus: &mut B,
        instruction: u16,
    ) -> Result<bool, Trap> {
        let pc = self.pc;
        let quadrant = instruction & 3;
        let funct3 = instruction >> 13;
        match (quadrant, funct3) {
            (0, 0) => self.execute_addi4spn(instruction)?,
            (0, 2 | 3 | 6 | 7) => {
                let rd = compact_register(instruction, 2);
                let rs1 = compact_register(instruction, 7);
                let (width, immediate, store) = match funct3 {
                    2 | 6 => (
                        AccessWidth::Word,
                        bits(instruction, 10, 3) << 3
                            | bits(instruction, 6, 1) << 2
                            | bits(instruction, 5, 1) << 6,
                        funct3 == 6,
                    ),
                    _ => (
                        AccessWidth::DoubleWord,
                        bits(instruction, 10, 3) << 3 | bits(instruction, 5, 2) << 6,
                        funct3 == 7,
                    ),
                };
                let address = self.registers[rs1].wrapping_add(immediate);
                if store {
                    self.store(bus, address, width, self.registers[rd])?;
                    self.reservation = None;
                } else {
                    let value = self.load(bus, address, width, Access::Read)?;
                    self.write_register(
                        rd,
                        if width == AccessWidth::Word {
                            sext(value, 32)
                        } else {
                            value
                        },
                    );
                }
            }
            (0, 4) => self.execute_zcb_memory(bus, instruction)?,
            (1, 0) => {
                let rd = register(instruction, 7);
                if rd != 0 {
                    self.write_register(
                        rd,
                        self.registers[rd].wrapping_add(ci_immediate(instruction)),
                    );
                }
            }
            (1, 1) => {
                let rd = register(instruction, 7);
                if rd != 0 {
                    self.write_register(
                        rd,
                        sext(
                            self.registers[rd].wrapping_add(ci_immediate(instruction)),
                            32,
                        ),
                    );
                }
            }
            (1, 2) => {
                let rd = register(instruction, 7);
                if rd != 0 {
                    self.write_register(rd, ci_immediate(instruction));
                }
            }
            (1, 3) => self.execute_compressed_lui(instruction)?,
            (1, 4) => self.execute_compressed_alu(instruction)?,
            (1, 5) => {
                self.pc = pc.wrapping_add(cj_immediate(instruction));
                return Ok(true);
            }
            (1, 6 | 7) => {
                let rs1 = compact_register(instruction, 7);
                let condition = self.registers[rs1] == 0;
                if condition == (funct3 == 6) {
                    self.pc = pc.wrapping_add(cb_immediate(instruction));
                    return Ok(true);
                }
            }
            (2, 0) => {
                let rd = register(instruction, 7);
                if rd != 0 {
                    let shift = bits(instruction, 2, 5) | bits(instruction, 12, 1) << 5;
                    self.write_register(rd, self.registers[rd] << shift);
                }
            }
            (2, 2 | 3) => self.execute_compressed_stack_load(bus, instruction, funct3)?,
            (2, 4) => {
                if self.execute_compressed_jump(instruction)? {
                    return Ok(true);
                }
            }
            (2, 6 | 7) => self.execute_compressed_stack_store(bus, instruction, funct3)?,
            _ => return Err(illegal(instruction)),
        }
        if self.pc == pc {
            self.pc = pc.wrapping_add(2);
        }
        Ok(true)
    }

    fn execute_addi4spn(&mut self, instruction: u16) -> Result<(), Trap> {
        let immediate = bits(instruction, 7, 4) << 6
            | bits(instruction, 11, 2) << 4
            | bits(instruction, 5, 1) << 3
            | bits(instruction, 6, 1) << 2;
        if immediate == 0 {
            return Err(illegal(instruction));
        }
        let rd = compact_register(instruction, 2);
        self.write_register(rd, self.registers[2].wrapping_add(immediate));
        Ok(())
    }

    fn execute_zcb_memory<B: CpuBus>(&mut self, bus: &mut B, instruction: u16) -> Result<(), Trap> {
        let rd = compact_register(instruction, 2);
        let rs1 = compact_register(instruction, 7);
        let operation = bits(instruction, 10, 3);
        let mut immediate = bits(instruction, 5, 2);
        let (width, store, signed) = match operation {
            0 => (AccessWidth::Byte, false, false),
            1 => {
                immediate = bits(instruction, 5, 1) << 1;
                (AccessWidth::HalfWord, false, instruction & (1 << 6) != 0)
            }
            2 => (AccessWidth::Byte, true, false),
            3 if instruction & (1 << 6) == 0 => {
                immediate = bits(instruction, 5, 1) << 1;
                (AccessWidth::HalfWord, true, false)
            }
            _ => return Err(illegal(instruction)),
        };
        let address = self.registers[rs1].wrapping_add(immediate);
        if store {
            self.store(bus, address, width, self.registers[rd])?;
            self.reservation = None;
        } else {
            let value = self.load(bus, address, width, Access::Read)?;
            self.write_register(rd, if signed { sext(value, 16) } else { value });
        }
        Ok(())
    }

    fn execute_compressed_lui(&mut self, instruction: u16) -> Result<(), Trap> {
        let rd = register(instruction, 7);
        if rd == 2 {
            let immediate = bits(instruction, 12, 1) << 9
                | bits(instruction, 6, 1) << 4
                | bits(instruction, 5, 1) << 6
                | bits(instruction, 3, 2) << 7
                | bits(instruction, 2, 1) << 5;
            let immediate = sext(immediate, 10);
            if immediate == 0 {
                return Err(illegal(instruction));
            }
            self.write_register(2, self.registers[2].wrapping_add(immediate));
        } else {
            let immediate = sext(
                (bits(instruction, 12, 1) << 17) | (bits(instruction, 2, 5) << 12),
                18,
            );
            if rd == 0 || immediate == 0 {
                return Err(illegal(instruction));
            }
            self.write_register(rd, immediate);
        }
        Ok(())
    }

    fn execute_compressed_alu(&mut self, instruction: u16) -> Result<(), Trap> {
        let rd = compact_register(instruction, 7);
        match bits(instruction, 10, 2) {
            0 | 1 => {
                let shift = bits(instruction, 2, 5) | bits(instruction, 12, 1) << 5;
                let value = if bits(instruction, 10, 2) == 0 {
                    self.registers[rd] >> shift
                } else {
                    (self.registers[rd].cast_signed() >> shift).cast_unsigned()
                };
                self.write_register(rd, value);
            }
            2 => self.write_register(rd, self.registers[rd] & ci_immediate(instruction)),
            3 => {
                let rs2 = compact_register(instruction, 2);
                let operation = bits(instruction, 5, 2) | bits(instruction, 12, 1) << 2;
                let left = self.registers[rd];
                let right = self.registers[rs2];
                let value = match operation {
                    0 => left.wrapping_sub(right),
                    1 => left ^ right,
                    2 => left | right,
                    3 => left & right,
                    4 => sext(left.wrapping_sub(right), 32),
                    5 => sext(left.wrapping_add(right), 32),
                    6 => left.wrapping_mul(right),
                    7 => match bits(instruction, 2, 3) {
                        0 => left & 0xff,
                        1 => sext(left, 8),
                        2 => left & 0xffff,
                        3 => sext(left, 16),
                        4 => left & u64::from(u32::MAX),
                        5 => !left,
                        _ => return Err(illegal(instruction)),
                    },
                    _ => unreachable!(),
                };
                self.write_register(rd, value);
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    fn execute_compressed_stack_load<B: CpuBus>(
        &mut self,
        bus: &mut B,
        instruction: u16,
        funct3: u16,
    ) -> Result<(), Trap> {
        let rd = register(instruction, 7);
        if rd == 0 {
            return Err(illegal(instruction));
        }
        let (width, immediate) = if funct3 == 2 {
            (
                AccessWidth::Word,
                bits(instruction, 12, 1) << 5
                    | bits(instruction, 4, 3) << 2
                    | bits(instruction, 2, 2) << 6,
            )
        } else {
            (
                AccessWidth::DoubleWord,
                bits(instruction, 12, 1) << 5
                    | bits(instruction, 5, 2) << 3
                    | bits(instruction, 2, 3) << 6,
            )
        };
        let value = self.load(
            bus,
            self.registers[2].wrapping_add(immediate),
            width,
            Access::Read,
        )?;
        self.write_register(
            rd,
            if width == AccessWidth::Word {
                sext(value, 32)
            } else {
                value
            },
        );
        Ok(())
    }

    fn execute_compressed_stack_store<B: CpuBus>(
        &mut self,
        bus: &mut B,
        instruction: u16,
        funct3: u16,
    ) -> Result<(), Trap> {
        let rs2 = register(instruction, 2);
        let (width, immediate) = if funct3 == 6 {
            (
                AccessWidth::Word,
                bits(instruction, 9, 4) << 2 | bits(instruction, 7, 2) << 6,
            )
        } else {
            (
                AccessWidth::DoubleWord,
                bits(instruction, 10, 3) << 3 | bits(instruction, 7, 3) << 6,
            )
        };
        self.store(
            bus,
            self.registers[2].wrapping_add(immediate),
            width,
            self.registers[rs2],
        )?;
        self.reservation = None;
        Ok(())
    }

    fn execute_compressed_jump(&mut self, instruction: u16) -> Result<bool, Trap> {
        let rd = register(instruction, 7);
        let rs2 = register(instruction, 2);
        let high = instruction & (1 << 12) != 0;
        if !high && rs2 == 0 {
            if rd == 0 {
                return Err(illegal(instruction));
            }
            self.pc = self.registers[rd] & !1;
            return Ok(true);
        }
        if high && rs2 == 0 {
            if rd == 0 {
                return Err(Trap {
                    exception: Exception::Breakpoint,
                    value: 0,
                });
            }
            let target = self.registers[rd] & !1;
            self.write_register(1, self.pc.wrapping_add(2));
            self.pc = target;
            return Ok(true);
        }
        if rd != 0 {
            self.write_register(
                rd,
                if high {
                    self.registers[rd].wrapping_add(self.registers[rs2])
                } else {
                    self.registers[rs2]
                },
            );
        }
        Ok(false)
    }
}

fn bits(instruction: u16, start: u32, width: u32) -> u64 {
    u64::from(instruction >> start) & ((1_u64 << width) - 1)
}

fn compact_register(instruction: u16, start: u32) -> usize {
    usize::try_from(bits(instruction, start, 3) + 8).expect("compressed register fits usize")
}

fn register(instruction: u16, start: u32) -> usize {
    usize::try_from(bits(instruction, start, 5)).expect("register fits usize")
}

fn ci_immediate(instruction: u16) -> u64 {
    sext(bits(instruction, 2, 5) | bits(instruction, 12, 1) << 5, 6)
}

fn cj_immediate(instruction: u16) -> u64 {
    sext(
        bits(instruction, 12, 1) << 11
            | bits(instruction, 11, 1) << 4
            | bits(instruction, 9, 2) << 8
            | bits(instruction, 8, 1) << 10
            | bits(instruction, 7, 1) << 6
            | bits(instruction, 6, 1) << 7
            | bits(instruction, 3, 3) << 1
            | bits(instruction, 2, 1) << 5,
        12,
    )
}

fn cb_immediate(instruction: u16) -> u64 {
    sext(
        bits(instruction, 12, 1) << 8
            | bits(instruction, 10, 2) << 3
            | bits(instruction, 5, 2) << 6
            | bits(instruction, 3, 2) << 1
            | bits(instruction, 2, 1) << 5,
        9,
    )
}

fn sext(value: u64, width: u32) -> u64 {
    ((value << (64 - width)).cast_signed() >> (64 - width)).cast_unsigned()
}

fn illegal(instruction: u16) -> Trap {
    Trap {
        exception: Exception::IllegalInstruction,
        value: u64::from(instruction),
    }
}
