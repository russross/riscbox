//! RISC-V interrupt bits shared by platform devices and the CPU core.

pub const MIP_SSIP: u32 = 1 << 1;
pub const MIP_MSIP: u32 = 1 << 3;
pub const MIP_STIP: u32 = 1 << 5;
pub const MIP_MTIP: u32 = 1 << 7;
pub const MIP_SEIP: u32 = 1 << 9;
pub const MIP_MEIP: u32 = 1 << 11;
