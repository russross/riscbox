#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define FLEN 64
#define CONFIG_EXT_C
#define CONFIG_CPU_TEST_SINGLE_STEP
#include "../riscv_cpu.c"

#define CHECK(condition) do {                                                \
        if (!(condition)) {                                                  \
            fprintf(stderr, "%s:%d: check failed: %s\n",                    \
                    __FILE__, __LINE__, #condition);                         \
            exit(1);                                                         \
        }                                                                    \
    } while (0)

typedef struct {
    PhysMemoryMap *map;
    PhysMemoryRange *ram;
    RISCVCPUState *cpu;
} Machine;

static Machine machine_new(void)
{
    Machine machine;

    machine.map = phys_mem_map_init();
    machine.ram = cpu_register_ram(machine.map, 0, 0x10000, 0);
    machine.cpu = riscv_cpu_init(machine.map);
    return machine;
}

static void machine_end(Machine *machine)
{
    riscv_cpu_end(machine->cpu);
    phys_mem_map_end(machine->map);
}

static uint32_t encode_r(uint32_t funct7, uint32_t rs2, uint32_t rs1,
                         uint32_t funct3, uint32_t rd, uint32_t opcode)
{
    return funct7 << 25 | rs2 << 20 | rs1 << 15 | funct3 << 12 |
        rd << 7 | opcode;
}

static uint32_t encode_i(int32_t immediate, uint32_t rs1, uint32_t funct3,
                         uint32_t rd, uint32_t opcode)
{
    return ((uint32_t)immediate & 0xfff) << 20 | rs1 << 15 | funct3 << 12 |
        rd << 7 | opcode;
}

static void write_le32(uint8_t *destination, uint32_t value)
{
    destination[0] = value;
    destination[1] = value >> 8;
    destination[2] = value >> 16;
    destination[3] = value >> 24;
}

static void write_le64(uint8_t *destination, uint64_t value)
{
    int index;

    for (index = 0; index < 8; index++)
        destination[index] = value >> (index * 8);
}

static uint64_t read_le64(const uint8_t *source)
{
    uint64_t value = 0;
    int index;

    for (index = 0; index < 8; index++)
        value |= (uint64_t)source[index] << (index * 8);
    return value;
}

static void run_one(Machine *machine, uint32_t instruction)
{
    write_le32(machine->ram->phys_mem + machine->cpu->pc, instruction);
    riscv_cpu_interp(machine->cpu, 1);
}

static void run_one16(Machine *machine, uint16_t instruction)
{
    machine->ram->phys_mem[machine->cpu->pc] = instruction;
    machine->ram->phys_mem[machine->cpu->pc + 1] = instruction >> 8;
    riscv_cpu_interp(machine->cpu, 1);
}

static uint32_t encode_amo(uint32_t operation, uint32_t rs2, uint32_t rs1,
                           uint32_t width, uint32_t rd)
{
    return operation << 27 | rs2 << 20 | rs1 << 15 | width << 12 |
        rd << 7 | 0x2f;
}

static uint32_t encode_fp(uint32_t funct7, uint32_t rs2, uint32_t rs1,
                          uint32_t rounding, uint32_t rd)
{
    return encode_r(funct7, rs2, rs1, rounding, rd, 0x53);
}

static uint32_t encode_fp_load(int32_t immediate, uint32_t rs1,
                               uint32_t width, uint32_t rd)
{
    return encode_i(immediate, rs1, width, rd, 0x07);
}

static uint32_t encode_fp_store(int32_t immediate, uint32_t rs2,
                                uint32_t rs1, uint32_t width)
{
    uint32_t value = immediate;

    return ((value >> 5) & 0x7f) << 25 | rs2 << 20 | rs1 << 15 |
        width << 12 | (value & 0x1f) << 7 | 0x27;
}

static uint32_t encode_fma(uint32_t format, uint32_t rs3, uint32_t rs2,
                           uint32_t rs1, uint32_t rounding, uint32_t rd)
{
    return rs3 << 27 | format << 25 | rs2 << 20 | rs1 << 15 |
        rounding << 12 | rd << 7 | 0x43;
}

static void test_integer_memory_and_multiply_divide(void)
{
    Machine machine = machine_new();
    RISCVCPUState *cpu = machine.cpu;
    uint32_t store;

    run_one(&machine, encode_i(-8, 0, 0, 1, 0x13));
    run_one(&machine, encode_i(3, 0, 0, 2, 0x13));
    run_one(&machine, encode_r(0, 2, 1, 0, 3, 0x33));
    run_one(&machine, encode_r(0x20, 2, 1, 0, 4, 0x33));
    run_one(&machine, encode_r(1, 2, 1, 0, 5, 0x33));
    run_one(&machine, encode_r(1, 2, 1, 4, 6, 0x33));
    CHECK(cpu->reg[1] == UINT64_MAX - 7);
    CHECK(cpu->reg[3] == UINT64_MAX - 4);
    CHECK(cpu->reg[4] == UINT64_MAX - 10);
    CHECK(cpu->reg[5] == UINT64_MAX - 23);
    CHECK(cpu->reg[6] == UINT64_MAX - 1);

    cpu->reg[10] = 0x8000;
    run_one(&machine, encode_i(-1, 0, 0, 11, 0x13));
    store = 11 << 20 | 10 << 15 | 3 << 12 | 0x23;
    run_one(&machine, store);
    run_one(&machine, encode_i(0, 10, 3, 12, 0x03));
    CHECK(cpu->reg[12] == UINT64_MAX);
    CHECK(cpu->minstret_counter == 9);
    machine_end(&machine);
}

static void test_trap_return_and_pmp(void)
{
    Machine machine = machine_new();
    RISCVCPUState *cpu = machine.cpu;

    cpu->pmpaddr[0] = 0x4000;
    cpu->pmpcfg[0] = 0x0f;
    cpu->mepc = 0x2000;
    cpu->mstatus = (uint64_t)PRV_S << MSTATUS_MPP_SHIFT;
    run_one(&machine, 0x30200073);
    CHECK(cpu->pc == 0x2000);
    CHECK(cpu->priv == PRV_S);
    run_one(&machine, 0xffffffff);
    CHECK(cpu->priv == PRV_M);
    CHECK(cpu->mcause == CAUSE_ILLEGAL_INSTRUCTION);
    CHECK(cpu->mepc == 0x2000);
    CHECK(cpu->mtval == 0xffffffff);

    cpu->pmpcfg[0] = 0x0d;
    cpu->mepc = 0x3000;
    cpu->mstatus = (uint64_t)PRV_S << MSTATUS_MPP_SHIFT;
    cpu->pc = 0x1000;
    run_one(&machine, 0x30200073);
    cpu->reg[1] = 0x8000;
    cpu->reg[2] = 7;
    run_one(&machine, encode_r(0, 2, 1, 3, 0, 0x23));
    CHECK(cpu->mcause == CAUSE_FAULT_STORE);
    CHECK(cpu->mtval == 0x8000);
    machine_end(&machine);
}

static void test_sv39_accessed_update(void)
{
    Machine machine = machine_new();
    RISCVCPUState *cpu = machine.cpu;

    cpu->pmpaddr[0] = 0x4000;
    cpu->pmpcfg[0] = 0x0f;
    write_le64(machine.ram->phys_mem + 0x4008, ((uint64_t)5 << 10) | 1);
    write_le64(machine.ram->phys_mem + 0x5000, ((uint64_t)6 << 10) | 1);
    write_le64(machine.ram->phys_mem + 0x6000, ((uint64_t)8 << 10) | 7);
    write_le64(machine.ram->phys_mem + 0x8000,
               UINT64_C(0x1122334455667788));
    cpu->satp = (UINT64_C(8) << 60) | 4;
    cpu->menvcfg = MENVCFG_ADUE;
    cpu->mstatus = MSTATUS_MPRV | ((uint64_t)PRV_S << MSTATUS_MPP_SHIFT);
    cpu->reg[1] = 0x40000000;
    run_one(&machine, encode_i(0, 1, 3, 2, 0x03));
    CHECK(cpu->reg[2] == UINT64_C(0x1122334455667788));
    CHECK(read_le64(machine.ram->phys_mem + 0x6000) ==
          (((uint64_t)8 << 10) | 0x47));
    machine_end(&machine);
}

static void test_atomic_compressed_and_scalar_extensions(void)
{
    Machine machine = machine_new();
    RISCVCPUState *cpu = machine.cpu;

    cpu->reg[1] = 0x8000;
    cpu->reg[2] = 5;
    write_le32(machine.ram->phys_mem + 0x8000, 0xfffffffe);
    run_one(&machine, encode_amo(0, 2, 1, 2, 3));
    CHECK(cpu->reg[3] == UINT64_MAX - 1);
    CHECK(read_le64(machine.ram->phys_mem + 0x8000) == 3);

    cpu->reg[1] = UINT64_C(0x80000000000000f1);
    cpu->reg[2] = 3;
    run_one(&machine, encode_r(0x10, 2, 1, 2, 3, 0x33));
    CHECK(cpu->reg[3] == 0x1e5);
    run_one(&machine, encode_i(0x600, 1, 1, 4, 0x13));
    CHECK(cpu->reg[4] == 0);

    cpu->reg[1] = 0;
    run_one16(&machine, 0x50fd); /* c.li x1,-1 */
    run_one16(&machine, 0x0085); /* c.addi x1,1 */
    CHECK(cpu->reg[1] == 0);
    CHECK(cpu->pc == 0x1010);

    cpu->reg[8] = UINT64_C(0xffffffffffffff80);
    run_one16(&machine, 0x9c61); /* c.zext.b x8 */
    CHECK(cpu->reg[8] == 0x80);
    machine_end(&machine);
}

static void test_float_arithmetic_rounding_and_flags(void)
{
    Machine machine = machine_new();
    RISCVCPUState *cpu = machine.cpu;

    cpu->fs = 1;
    cpu->fp_reg[1] = F32_HIGH | UINT32_C(0x3fc00000); /* 1.5 */
    cpu->fp_reg[2] = F32_HIGH | UINT32_C(0x40100000); /* 2.25 */
    run_one(&machine, encode_fp(0x00, 2, 1, RM_RNE, 3)); /* fadd.s */
    CHECK(cpu->fp_reg[3] == (F32_HIGH | UINT32_C(0x40700000)));
    CHECK(cpu->fflags == 0);
    CHECK(cpu->fs == 3);

    cpu->fs = 1;
    cpu->fflags = 0;
    cpu->fp_reg[2] = F32_HIGH;
    run_one(&machine, encode_fp(0x0c, 2, 1, RM_RNE, 3)); /* fdiv.s */
    CHECK(cpu->fp_reg[3] == (F32_HIGH | UINT32_C(0x7f800000)));
    CHECK(cpu->fflags == FFLAG_DIVIDE_ZERO);
    CHECK(cpu->fs == 3);

    cpu->fflags = 0;
    cpu->fp_reg[1] = F32_HIGH | UINT32_C(0xbf800000);
    run_one(&machine, encode_fp(0x2c, 0, 1, RM_RNE, 3)); /* fsqrt.s */
    CHECK(cpu->fp_reg[3] == (F32_HIGH | UINT32_C(0x7fc00000)));
    CHECK(cpu->fflags == FFLAG_INVALID_OP);

    cpu->fflags = 0;
    cpu->fp_reg[1] = F32_HIGH | UINT32_C(0x3fc00000);
    run_one(&machine, encode_fp(0x60, 0, 1, RM_RTZ, 3)); /* fcvt.w.s */
    CHECK(cpu->reg[3] == 1);
    CHECK(cpu->fflags == FFLAG_INEXACT);
    CHECK(cpu->fs == 3);

    cpu->fflags = 0;
    cpu->frm = RM_RUP;
    run_one(&machine, encode_fp(0x60, 0, 1, 7, 3));
    CHECK(cpu->reg[3] == 2);
    CHECK(cpu->fflags == FFLAG_INEXACT);

    CHECK(csr_write(cpu, 0x002, 5) == CSR_WRITE_OK);
    CHECK(cpu->frm == 5);
    cpu->mtvec = 0x2000;
    run_one(&machine, encode_fp(0x60, 0, 1, 7, 3));
    CHECK(cpu->mcause == CAUSE_ILLEGAL_INSTRUCTION);
    CHECK(cpu->pc == 0x2000);
    machine_end(&machine);
}

static void test_float_nan_boxing_moves_and_conversions(void)
{
    Machine machine = machine_new();
    RISCVCPUState *cpu = machine.cpu;

    cpu->fs = 1;
    cpu->fp_reg[1] = UINT32_C(0x3f800000); /* Unboxed 1.0 is a NaN input. */
    cpu->fp_reg[2] = F32_HIGH | UINT32_C(0x3f800000);
    run_one(&machine, encode_fp(0x00, 2, 1, RM_RNE, 3));
    CHECK(cpu->fp_reg[3] == (F32_HIGH | UINT32_C(0x7fc00000)));

    cpu->fp_reg[3] = F32_HIGH | UINT32_C(0x3f800000);
    run_one(&machine, encode_fma(0, 3, 2, 1, RM_RNE, 4));
    CHECK(cpu->fp_reg[4] == (F32_HIGH | UINT32_C(0x7fc00000)));

    cpu->fp_reg[1] = F32_HIGH | UINT32_C(0x80000001);
    run_one(&machine, encode_fp(0x70, 0, 1, 0, 4)); /* fmv.x.w */
    CHECK(cpu->reg[4] == UINT64_C(0xffffffff80000001));

    cpu->fs = 1;
    cpu->reg[4] = UINT64_C(0x1234567880000001);
    run_one(&machine, encode_fp(0x78, 0, 4, 0, 5)); /* fmv.w.x */
    CHECK(cpu->fp_reg[5] == (F32_HIGH | UINT32_C(0x80000001)));
    CHECK(cpu->fs == 3);

    cpu->fp_reg[1] = F32_HIGH | UINT32_C(0x3fc00000);
    run_one(&machine, encode_fp(0x21, 0, 1, RM_RNE, 6)); /* fcvt.d.s */
    CHECK(cpu->fp_reg[6] == UINT64_C(0x3ff8000000000000));
    run_one(&machine, encode_fp(0x20, 1, 6, RM_RNE, 7)); /* fcvt.s.d */
    CHECK(cpu->fp_reg[7] == (F32_HIGH | UINT32_C(0x3fc00000)));

    cpu->fp_reg[1] = F32_HIGH | UINT32_C(0x7f800001); /* signaling NaN */
    cpu->fp_reg[2] = F32_HIGH | UINT32_C(0x3f800000);
    cpu->fflags = 0;
    run_one(&machine, encode_fp(0x00, 2, 1, RM_RNE, 3));
    CHECK(cpu->fp_reg[3] == (F32_HIGH | UINT32_C(0x7fc00000)));
    CHECK(cpu->fflags == FFLAG_INVALID_OP);
    machine_end(&machine);
}

static void test_float_memory_and_fs_state(void)
{
    Machine machine = machine_new();
    RISCVCPUState *cpu = machine.cpu;

    cpu->reg[1] = 0x8000;
    write_le32(machine.ram->phys_mem + 0x8004, UINT32_C(0x3f800000));
    cpu->fs = 1;
    run_one(&machine, encode_fp_load(4, 1, 2, 2)); /* flw */
    CHECK(cpu->fp_reg[2] == (F32_HIGH | UINT32_C(0x3f800000)));
    CHECK(cpu->fs == 3);
    run_one(&machine, encode_fp_store(8, 2, 1, 2)); /* fsw */
    CHECK(read_le64(machine.ram->phys_mem + 0x8008) == UINT32_C(0x3f800000));

    cpu->fs = 0;
    cpu->mtvec = 0x2000;
    run_one(&machine, encode_fp(0x00, 2, 2, RM_RNE, 3));
    CHECK(cpu->mcause == CAUSE_ILLEGAL_INSTRUCTION);
    CHECK(cpu->pc == 0x2000);
    machine_end(&machine);
}

static void test_double_fused_compare_and_classify(void)
{
    Machine machine = machine_new();
    RISCVCPUState *cpu = machine.cpu;

    cpu->fs = 1;
    cpu->fp_reg[1] = UINT64_C(0x3ff8000000000000); /* 1.5 */
    cpu->fp_reg[2] = UINT64_C(0x4000000000000000); /* 2.0 */
    cpu->fp_reg[3] = UINT64_C(0xbff0000000000000); /* -1.0 */
    run_one(&machine, encode_fp(0x01, 2, 1, RM_RNE, 4)); /* fadd.d */
    CHECK(cpu->fp_reg[4] == UINT64_C(0x400c000000000000));
    run_one(&machine, encode_fma(1, 3, 2, 1, RM_RNE, 4)); /* fmadd.d */
    CHECK(cpu->fp_reg[4] == UINT64_C(0x4000000000000000));

    cpu->fs = 1;
    cpu->fflags = 0;
    run_one(&machine, encode_fp(0x51, 2, 1, 1, 5)); /* flt.d */
    CHECK(cpu->reg[5] == 1);
    CHECK(cpu->fflags == 0);
    CHECK(cpu->fs == 3);

    cpu->fs = 1;
    cpu->fp_reg[1] = UINT64_C(0x7ff8000000000000);
    cpu->fflags = 0;
    run_one(&machine, encode_fp(0x51, 2, 1, 2, 5)); /* feq.d qNaN */
    CHECK(cpu->reg[5] == 0);
    CHECK(cpu->fflags == 0);
    CHECK(cpu->fs == 3);
    run_one(&machine, encode_fp(0x71, 0, 1, 1, 5)); /* fclass.d */
    CHECK(cpu->reg[5] == FCLASS_QNAN);

    cpu->fs = 1;
    cpu->fp_reg[1] = UINT64_C(0x4008000000000000); /* 3.0 */
    run_one(&machine, encode_fp(0x61, 2, 1, RM_RNE, 5)); /* fcvt.l.d */
    CHECK(cpu->reg[5] == 3);
    CHECK(cpu->fs == 3);
    machine_end(&machine);
}

int main(void)
{
    test_integer_memory_and_multiply_divide();
    test_trap_return_and_pmp();
    test_sv39_accessed_update();
    test_atomic_compressed_and_scalar_extensions();
    test_float_arithmetic_rounding_and_flags();
    test_float_nan_boxing_moves_and_conversions();
    test_float_memory_and_fs_state();
    test_double_fused_compare_and_classify();
    puts("cpu foundation C tests passed");
    return 0;
}
