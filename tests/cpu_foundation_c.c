#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define FLEN 64
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

int main(void)
{
    test_integer_memory_and_multiply_divide();
    test_trap_return_and_pmp();
    test_sv39_accessed_update();
    puts("cpu foundation C tests passed");
    return 0;
}
