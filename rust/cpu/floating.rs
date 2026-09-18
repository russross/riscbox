use crate::softfp::{
    RoundingMode, add_f32, add_f64, class_f32, class_f64, div_f32, div_f64, eq_f32, eq_f64,
    f32_to_f64, f32_to_i32, f32_to_i64, f32_to_u32, f32_to_u64, f64_to_f32, f64_to_i32, f64_to_i64,
    f64_to_u32, f64_to_u64, fma_f32, fma_f64, i32_to_f32, i32_to_f64, i64_to_f32, i64_to_f64,
    le_f32, le_f64, lt_f32, lt_f64, max_f32, max_f64, min_f32, min_f64, mul_f32, mul_f64, sqrt_f32,
    sqrt_f64, sub_f32, sub_f64, u32_to_f32, u32_to_f64, u64_to_f32, u64_to_f64,
};

use super::{
    AccessWidth, Cpu, CpuBus, CsrError, Exception, MSTATUS_FS, Trap, low_u32, mmu::Access,
};

const F32_BOX: u64 = 0xffff_ffff_0000_0000;
const F32_QNAN: u32 = 0x7fc0_0000;

impl Cpu {
    pub(super) const fn fp_enabled(&self) -> bool {
        self.mstatus & MSTATUS_FS != 0
    }

    pub(super) fn require_fp(&self) -> Result<(), CsrError> {
        if self.fp_enabled() {
            Ok(())
        } else {
            Err(CsrError::IllegalAccess)
        }
    }

    pub(super) fn require_fp_trap(&self, instruction: u16) -> Result<(), Trap> {
        if self.fp_enabled() {
            Ok(())
        } else {
            Err(illegal(u32::from(instruction)))
        }
    }

    pub(super) fn mark_fp_dirty(&mut self) {
        self.mstatus |= MSTATUS_FS;
    }

    pub(super) fn write_fp64(&mut self, register: usize, value: u64) {
        self.fp_registers[register] = value;
        self.mark_fp_dirty();
    }

    fn write_fp32(&mut self, register: usize, value: u32) {
        self.write_fp64(register, F32_BOX | u64::from(value));
    }

    fn read_fp32(&self, register: usize) -> u32 {
        let value = self.fp_registers[register];
        if value >> 32 == u64::from(u32::MAX) {
            low_u32(value)
        } else {
            F32_QNAN
        }
    }

    fn rounding_mode(&self, encoded: u32, instruction: u32) -> Result<RoundingMode, Trap> {
        let encoded = if encoded == 7 {
            u32::from(self.frm)
        } else {
            encoded
        };
        match encoded {
            0 => Ok(RoundingMode::NearEven),
            1 => Ok(RoundingMode::Zero),
            2 => Ok(RoundingMode::Down),
            3 => Ok(RoundingMode::Up),
            4 => Ok(RoundingMode::NearMaxMag),
            _ => Err(illegal(instruction)),
        }
    }

    fn merge_flags(&mut self, flags: u32) {
        let old = self.fflags;
        self.fflags |= u8::try_from(flags & 0x1f).expect("masked flags fit u8");
        if self.fflags != old {
            self.mark_fp_dirty();
        }
    }

    pub(super) fn execute_fp_memory<B: CpuBus>(
        &mut self,
        bus: &mut B,
        instruction: u32,
        opcode: u32,
    ) -> Result<(), Trap> {
        if !self.fp_enabled() {
            return Err(illegal(instruction));
        }
        let fp = register(instruction, if opcode == 0x07 { 7 } else { 20 });
        let base = register(instruction, 15);
        let funct3 = instruction >> 12 & 7;
        let (width, is_single) = match funct3 {
            2 => (AccessWidth::Word, true),
            3 => (AccessWidth::DoubleWord, false),
            _ => return Err(illegal(instruction)),
        };
        let immediate = if opcode == 0x07 {
            sign_extend(u64::from(instruction >> 20), 12)
        } else {
            sign_extend(
                u64::from(instruction >> 25) << 5 | u64::from(instruction >> 7 & 0x1f),
                12,
            )
        };
        let address = self.registers[base].wrapping_add(immediate);
        if opcode == 0x07 {
            let value = self.load(bus, address, width, Access::Read)?;
            if is_single {
                self.write_fp32(fp, low_u32(value));
            } else {
                self.write_fp64(fp, value);
            }
        } else {
            let value = if is_single {
                u64::from(low_u32(self.fp_registers[fp]))
            } else {
                self.fp_registers[fp]
            };
            self.store(bus, address, width, value)?;
        }
        Ok(())
    }

    pub(super) fn execute_floating(&mut self, instruction: u32, opcode: u32) -> Result<(), Trap> {
        if !self.fp_enabled() {
            return Err(illegal(instruction));
        }
        if opcode != 0x53 {
            return self.execute_fused(instruction, opcode);
        }
        let rd = register(instruction, 7);
        let rm = instruction >> 12 & 7;
        let rs1 = register(instruction, 15);
        let rs2 = register(instruction, 20);
        let funct7 = instruction >> 25;
        let format = funct7 & 3;
        let operation = funct7 >> 2;
        if format > 1 {
            return Err(illegal(instruction));
        }
        match operation {
            0..=3 => self.execute_basic_fp(instruction, (operation, format), rd, (rs1, rs2), rm),
            4 => self.execute_sign(format, rd, rs1, rs2, rm, instruction),
            5 => self.execute_min_max(format, rd, rs1, rs2, rm, instruction),
            8 => self.execute_format_conversion(instruction, format, rd, rs1, rs2, rm),
            11 if rs2 == 0 => self.execute_sqrt(instruction, format, rd, rs1, rm),
            20 => self.execute_compare(format, rd, rs1, rs2, rm, instruction),
            24 => self.execute_float_to_int(instruction, format, rd, rs1, rs2, rm),
            26 => self.execute_int_to_float(instruction, format, rd, rs1, rs2, rm),
            28 if rs2 == 0 => self.execute_move_or_class(format, rd, rs1, rm, instruction),
            30 if rs2 == 0 && rm == 0 => {
                if format == 0 {
                    self.write_fp32(rd, low_u32(self.registers[rs1]));
                } else {
                    self.write_fp64(rd, self.registers[rs1]);
                }
                Ok(())
            }
            _ => Err(illegal(instruction)),
        }
    }

    fn execute_fused(&mut self, instruction: u32, opcode: u32) -> Result<(), Trap> {
        let rd = register(instruction, 7);
        let rs1 = register(instruction, 15);
        let rs2 = register(instruction, 20);
        let rs3 = register(instruction, 27);
        let format = instruction >> 25 & 3;
        if format > 1 {
            return Err(illegal(instruction));
        }
        let rm = self.rounding_mode(instruction >> 12 & 7, instruction)?;
        let negate_product = matches!(opcode, 0x4b | 0x4f);
        let negate_addend = matches!(opcode, 0x47 | 0x4f);
        let mut flags = 0;
        if format == 0 {
            let mut left = self.read_fp32(rs1);
            let mut addend = self.read_fp32(rs3);
            if negate_product {
                left ^= 1 << 31;
            }
            if negate_addend {
                addend ^= 1 << 31;
            }
            let value = fma_f32(left, self.read_fp32(rs2), addend, rm, &mut flags);
            self.write_fp32(rd, value);
        } else {
            let mut left = self.fp_registers[rs1];
            let mut addend = self.fp_registers[rs3];
            if negate_product {
                left ^= 1 << 63;
            }
            if negate_addend {
                addend ^= 1 << 63;
            }
            let value = fma_f64(left, self.fp_registers[rs2], addend, rm, &mut flags);
            self.write_fp64(rd, value);
        }
        self.merge_flags(flags);
        Ok(())
    }

    fn execute_basic_fp(
        &mut self,
        instruction: u32,
        operation_format: (u32, u32),
        rd: usize,
        sources: (usize, usize),
        encoded_rm: u32,
    ) -> Result<(), Trap> {
        let (operation, format) = operation_format;
        let (rs1, rs2) = sources;
        let rm = self.rounding_mode(encoded_rm, instruction)?;
        let mut flags = 0;
        if format == 0 {
            let (a, b) = (self.read_fp32(rs1), self.read_fp32(rs2));
            let value = match operation {
                0 => add_f32(a, b, rm, &mut flags),
                1 => sub_f32(a, b, rm, &mut flags),
                2 => mul_f32(a, b, rm, &mut flags),
                3 => div_f32(a, b, rm, &mut flags),
                _ => unreachable!(),
            };
            self.write_fp32(rd, value);
        } else {
            let (a, b) = (self.fp_registers[rs1], self.fp_registers[rs2]);
            let value = match operation {
                0 => add_f64(a, b, rm, &mut flags),
                1 => sub_f64(a, b, rm, &mut flags),
                2 => mul_f64(a, b, rm, &mut flags),
                3 => div_f64(a, b, rm, &mut flags),
                _ => unreachable!(),
            };
            self.write_fp64(rd, value);
        }
        self.merge_flags(flags);
        Ok(())
    }

    fn execute_sqrt(
        &mut self,
        instruction: u32,
        format: u32,
        rd: usize,
        rs1: usize,
        encoded_rm: u32,
    ) -> Result<(), Trap> {
        let rm = self.rounding_mode(encoded_rm, instruction)?;
        let mut flags = 0;
        if format == 0 {
            let value = sqrt_f32(self.read_fp32(rs1), rm, &mut flags);
            self.write_fp32(rd, value);
        } else {
            let value = sqrt_f64(self.fp_registers[rs1], rm, &mut flags);
            self.write_fp64(rd, value);
        }
        self.merge_flags(flags);
        Ok(())
    }

    fn execute_sign(
        &mut self,
        format: u32,
        rd: usize,
        rs1: usize,
        rs2: usize,
        operation: u32,
        instruction: u32,
    ) -> Result<(), Trap> {
        if operation > 2 {
            return Err(illegal(instruction));
        }
        let sign = if format == 0 {
            1_u64 << 31
        } else {
            1_u64 << 63
        };
        let a = if format == 0 {
            u64::from(self.read_fp32(rs1))
        } else {
            self.fp_registers[rs1]
        };
        let b = if format == 0 {
            u64::from(self.read_fp32(rs2))
        } else {
            self.fp_registers[rs2]
        };
        let value = match operation {
            0 => a & !sign | b & sign,
            1 => a & !sign | (!b & sign),
            2 => a ^ (b & sign),
            _ => unreachable!(),
        };
        if format == 0 {
            self.write_fp32(rd, low_u32(value));
        } else {
            self.write_fp64(rd, value);
        }
        Ok(())
    }

    fn execute_min_max(
        &mut self,
        format: u32,
        rd: usize,
        rs1: usize,
        rs2: usize,
        operation: u32,
        instruction: u32,
    ) -> Result<(), Trap> {
        if operation > 1 {
            return Err(illegal(instruction));
        }
        let mut flags = 0;
        if format == 0 {
            let (a, b) = (self.read_fp32(rs1), self.read_fp32(rs2));
            let value = if operation == 0 {
                min_f32(a, b, &mut flags)
            } else {
                max_f32(a, b, &mut flags)
            };
            self.write_fp32(rd, value);
        } else {
            let (a, b) = (self.fp_registers[rs1], self.fp_registers[rs2]);
            let value = if operation == 0 {
                min_f64(a, b, &mut flags)
            } else {
                max_f64(a, b, &mut flags)
            };
            self.write_fp64(rd, value);
        }
        self.merge_flags(flags);
        Ok(())
    }

    fn execute_compare(
        &mut self,
        format: u32,
        rd: usize,
        rs1: usize,
        rs2: usize,
        operation: u32,
        instruction: u32,
    ) -> Result<(), Trap> {
        if operation > 2 {
            return Err(illegal(instruction));
        }
        let mut flags = 0;
        let value = if format == 0 {
            let (a, b) = (self.read_fp32(rs1), self.read_fp32(rs2));
            match operation {
                0 => le_f32(a, b, &mut flags),
                1 => lt_f32(a, b, &mut flags),
                2 => eq_f32(a, b, &mut flags),
                _ => unreachable!(),
            }
        } else {
            let (a, b) = (self.fp_registers[rs1], self.fp_registers[rs2]);
            match operation {
                0 => le_f64(a, b, &mut flags),
                1 => lt_f64(a, b, &mut flags),
                2 => eq_f64(a, b, &mut flags),
                _ => unreachable!(),
            }
        };
        self.write_register(rd, u64::from(value));
        self.merge_flags(flags);
        Ok(())
    }

    fn execute_move_or_class(
        &mut self,
        format: u32,
        rd: usize,
        rs1: usize,
        operation: u32,
        instruction: u32,
    ) -> Result<(), Trap> {
        let value = match operation {
            0 if format == 0 => sign_extend(u64::from(low_u32(self.fp_registers[rs1])), 32),
            0 => self.fp_registers[rs1],
            1 if format == 0 => u64::from(class_f32(self.read_fp32(rs1))),
            1 => u64::from(class_f64(self.fp_registers[rs1])),
            _ => return Err(illegal(instruction)),
        };
        self.write_register(rd, value);
        Ok(())
    }

    fn execute_format_conversion(
        &mut self,
        instruction: u32,
        format: u32,
        rd: usize,
        rs1: usize,
        source_format: usize,
        encoded_rm: u32,
    ) -> Result<(), Trap> {
        let rm = self.rounding_mode(encoded_rm, instruction)?;
        let mut flags = 0;
        match (format, source_format) {
            (0, 1) => {
                let value = f64_to_f32(self.fp_registers[rs1], rm, &mut flags);
                self.write_fp32(rd, value);
            }
            (1, 0) => {
                let value = f32_to_f64(self.read_fp32(rs1), &mut flags);
                self.write_fp64(rd, value);
            }
            _ => return Err(illegal(instruction)),
        }
        self.merge_flags(flags);
        Ok(())
    }

    fn execute_float_to_int(
        &mut self,
        instruction: u32,
        format: u32,
        rd: usize,
        rs1: usize,
        kind: usize,
        encoded_rm: u32,
    ) -> Result<(), Trap> {
        let rm = self.rounding_mode(encoded_rm, instruction)?;
        let mut flags = 0;
        let value = if format == 0 {
            let a = self.read_fp32(rs1);
            match kind {
                0 => sign_extend(u64::from(f32_to_i32(a, rm, &mut flags).cast_unsigned()), 32),
                1 => sign_extend(u64::from(f32_to_u32(a, rm, &mut flags)), 32),
                2 => f32_to_i64(a, rm, &mut flags).cast_unsigned(),
                3 => f32_to_u64(a, rm, &mut flags),
                _ => return Err(illegal(instruction)),
            }
        } else {
            let a = self.fp_registers[rs1];
            match kind {
                0 => sign_extend(u64::from(f64_to_i32(a, rm, &mut flags).cast_unsigned()), 32),
                1 => sign_extend(u64::from(f64_to_u32(a, rm, &mut flags)), 32),
                2 => f64_to_i64(a, rm, &mut flags).cast_unsigned(),
                3 => f64_to_u64(a, rm, &mut flags),
                _ => return Err(illegal(instruction)),
            }
        };
        self.write_register(rd, value);
        self.merge_flags(flags);
        Ok(())
    }

    fn execute_int_to_float(
        &mut self,
        instruction: u32,
        format: u32,
        rd: usize,
        rs1: usize,
        kind: usize,
        encoded_rm: u32,
    ) -> Result<(), Trap> {
        let rm = self.rounding_mode(encoded_rm, instruction)?;
        let source = self.registers[rs1];
        let mut flags = 0;
        if format == 0 {
            let value = match kind {
                0 => i32_to_f32(low_u32(source).cast_signed(), rm, &mut flags),
                1 => u32_to_f32(low_u32(source), rm, &mut flags),
                2 => i64_to_f32(source.cast_signed(), rm, &mut flags),
                3 => u64_to_f32(source, rm, &mut flags),
                _ => return Err(illegal(instruction)),
            };
            self.write_fp32(rd, value);
        } else {
            let value = match kind {
                0 => i32_to_f64(low_u32(source).cast_signed(), rm, &mut flags),
                1 => u32_to_f64(low_u32(source), rm, &mut flags),
                2 => i64_to_f64(source.cast_signed(), rm, &mut flags),
                3 => u64_to_f64(source, rm, &mut flags),
                _ => return Err(illegal(instruction)),
            };
            self.write_fp64(rd, value);
        }
        self.merge_flags(flags);
        Ok(())
    }
}

fn register(instruction: u32, start: u32) -> usize {
    usize::try_from(instruction >> start & 0x1f).expect("register field fits usize")
}

fn sign_extend(value: u64, bits: u32) -> u64 {
    ((value << (64 - bits)).cast_signed() >> (64 - bits)).cast_unsigned()
}

fn illegal(instruction: u32) -> Trap {
    Trap {
        exception: Exception::IllegalInstruction,
        value: u64::from(instruction),
    }
}
