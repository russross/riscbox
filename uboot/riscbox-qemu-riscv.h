/* SPDX-License-Identifier: GPL-2.0+ */
/*
 * Copyright (C) 2018 Bin Meng <bmeng.cn@gmail.com>
 */

#ifndef __CONFIG_H
#define __CONFIG_H

#include <linux/sizes.h>

#define CFG_SYS_SDRAM_BASE 0x80000000

/* OpenSBI supplies the S-mode timer; these addresses describe the board. */
#define RISCV_MMODE_TIMERBASE 0x2000000
#define RISCV_MMODE_TIMEROFF 0xbff8
#define RISCV_MMODE_TIMER_FREQ 1000000
#define RISCV_SMODE_TIMER_FREQ 1000000

/* The browser host exposes a serial console and VirtIO MMIO disks. */
#define CFG_STD_DEVICES_SETTINGS "stdin=serial\0" \
				 "stdout=serial\0" \
				 "stderr=serial\0"

#define CFG_EXTRA_ENV_SETTINGS \
	CFG_STD_DEVICES_SETTINGS \
	"fdt_high=0xffffffffffffffff\0" \
	"initrd_high=0xffffffffffffffff\0" \
	"kernel_addr_r=0x84000000\0" \
	"kernel_comp_addr_r=0x88000000\0" \
	"kernel_comp_size=0x4000000\0" \
	"fdt_addr_r=0x8c000000\0" \
	"scriptaddr=0x8c100000\0" \
	"pxefile_addr_r=0x8c200000\0" \
	"ramdisk_addr_r=0x8c300000\0"

#endif /* __CONFIG_H */
