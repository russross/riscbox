#ifndef RISCV_MACHINE_TEST_H
#define RISCV_MACHINE_TEST_H

#include <stddef.h>
#include <stdint.h>

typedef struct RISCVMachineTest RISCVMachineTest;

RISCVMachineTest *riscv_machine_test_init(void);
void riscv_machine_test_end(RISCVMachineTest *test);
uint32_t riscv_machine_test_clint_read(RISCVMachineTest *test,
                                       uint32_t offset);
void riscv_machine_test_clint_write(RISCVMachineTest *test, uint32_t offset,
                                    uint32_t value);
uint32_t riscv_machine_test_plic_read(RISCVMachineTest *test,
                                      uint32_t offset);
void riscv_machine_test_plic_write(RISCVMachineTest *test, uint32_t offset,
                                   uint32_t value);
void riscv_machine_test_plic_set_irq(RISCVMachineTest *test, int irq,
                                     int level);
uint32_t riscv_machine_test_mip(RISCVMachineTest *test);
void riscv_machine_test_finisher_write(uint32_t offset, uint32_t value);
uint8_t *riscv_machine_test_fdt(RISCVMachineTest *test, size_t *size);

#endif
