use super::{
    ArenaOffset, BusError, Cpu, CpuBus, Exception, GuestAddress, MENVCFG_ADUE, MENVCFG_PBMTE,
    MSTATUS_MPP, MSTATUS_MPRV, MSTATUS_MXR, MSTATUS_SUM, PAGE_MASK, PAGE_SHIFT, PAGE_SIZE,
    PMP_CFG_A_MASK, PMP_CFG_A_TOR, PMP_CFG_L, PMP_CFG_R, PMP_CFG_W, PMP_CFG_X, Privilege, TLB_SIZE,
    TlbEntry, Trap,
};
use crate::memory::AccessWidth;

const SATP_MODE_SV39: u64 = 8;
const PTE_VALID: u64 = 1 << 0;
const PTE_USER: u64 = 1 << 4;
const PTE_ACCESSED: u64 = 1 << 6;
const PTE_DIRTY: u64 = 1 << 7;
const PTE_PPN_MASK: u64 = (1 << 44) - 1;
const PTE_PBMT_MASK: u64 = 3 << 61;
const PTE_NAPOT: u64 = 1 << 63;
const PTE_HIGH_RESERVED: u64 = 0x7f << 54;
const PTE_NONLEAF_RESERVED: u64 = PTE_USER | PTE_ACCESSED | PTE_DIRTY | PTE_PBMT_MASK | PTE_NAPOT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Access {
    Read = 0,
    Write = 1,
    Execute = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TranslationFault {
    Page,
    Access,
}

impl Cpu {
    pub(super) fn load<B: CpuBus>(
        &mut self,
        bus: &mut B,
        address: u64,
        width: AccessWidth,
        access: Access,
    ) -> Result<u64, Trap> {
        let len = width.bytes();
        if address & (len as u64 - 1) != 0 {
            let mut value = 0;
            for index in 0..len {
                value |= self.load(
                    bus,
                    address.wrapping_add(index as u64),
                    AccessWidth::Byte,
                    access,
                )? << (index * 8);
            }
            return Ok(value);
        }

        let virtual_page = address & !PAGE_MASK;
        let index = ((address >> PAGE_SHIFT) as usize) & (TLB_SIZE - 1);
        let cached = match access {
            Access::Read => self.tlb_read[index],
            Access::Write => self.tlb_write[index],
            Access::Execute => self.tlb_execute[index],
        };
        if cached.virtual_page == virtual_page {
            let offset = cached.arena_page.0 as usize + (address & PAGE_MASK) as usize;
            return read_arena(bus.arena(), offset, width)
                .ok_or_else(|| Self::access_trap(access, address));
        }

        let physical = self
            .translate(bus, address, len, access)
            .map_err(|fault| Self::translation_trap(access, address, fault))?;
        let result = bus
            .read(GuestAddress(physical), width)
            .map_err(|_| Self::access_trap(access, address))?;
        self.fill_tlb(bus, virtual_page, physical & !PAGE_MASK, access, index);
        Ok(result)
    }

    pub(super) fn store<B: CpuBus>(
        &mut self,
        bus: &mut B,
        address: u64,
        width: AccessWidth,
        value: u64,
    ) -> Result<(), Trap> {
        let len = width.bytes();
        if address & (len as u64 - 1) != 0 {
            for index in 0..len {
                self.store(
                    bus,
                    address.wrapping_add(index as u64),
                    AccessWidth::Byte,
                    value >> (index * 8),
                )?;
            }
            return Ok(());
        }

        let virtual_page = address & !PAGE_MASK;
        let index = ((address >> PAGE_SHIFT) as usize) & (TLB_SIZE - 1);
        let cached = self.tlb_write[index];
        if cached.virtual_page == virtual_page {
            let offset = cached.arena_page.0 as usize + (address & PAGE_MASK) as usize;
            return write_arena(bus.arena_mut(), offset, width, value)
                .ok_or_else(|| Self::access_trap(Access::Write, address));
        }

        let physical = self
            .translate(bus, address, len, Access::Write)
            .map_err(|fault| Self::translation_trap(Access::Write, address, fault))?;
        bus.write(GuestAddress(physical), width, value)
            .map_err(|_| Self::access_trap(Access::Write, address))?;
        self.fill_tlb(
            bus,
            virtual_page,
            physical & !PAGE_MASK,
            Access::Write,
            index,
        );
        Ok(())
    }

    pub fn invalidate_write_range(&mut self, start: ArenaOffset, len: u32) {
        let start = u64::from(start.0);
        let end = start + u64::from(len);
        for entry in &mut self.tlb_write {
            let offset = u64::from(entry.arena_page.0);
            if offset >= start && offset < end {
                *entry = TlbEntry::default();
            }
        }
    }

    fn fill_tlb<B: CpuBus>(
        &mut self,
        bus: &mut B,
        virtual_page: u64,
        physical_page: u64,
        access: Access,
        index: usize,
    ) {
        let privilege = self.effective_privilege(access);
        if !self.pmp_access_ok(physical_page, PAGE_SIZE, access, privilege) {
            return;
        }
        let Ok(arena_page) = bus.ram_range(
            GuestAddress(physical_page),
            usize::try_from(PAGE_SIZE).expect("page size fits usize"),
            access == Access::Write,
        ) else {
            return;
        };
        let entry = TlbEntry {
            virtual_page,
            arena_page,
        };
        match access {
            Access::Read => self.tlb_read[index] = entry,
            Access::Write => self.tlb_write[index] = entry,
            Access::Execute => self.tlb_execute[index] = entry,
        }
    }

    fn translate<B: CpuBus>(
        &mut self,
        bus: &mut B,
        virtual_address: u64,
        len: usize,
        access: Access,
    ) -> Result<u64, TranslationFault> {
        let privilege = self.effective_privilege(access);
        let physical = if privilege == Privilege::Machine || self.satp >> 60 != SATP_MODE_SV39 {
            virtual_address
        } else {
            self.walk_sv39(bus, virtual_address, access, privilege)?
        };
        if self.pmp_access_ok(physical, len as u64, access, privilege) {
            Ok(physical)
        } else {
            Err(TranslationFault::Access)
        }
    }

    fn walk_sv39<B: CpuBus>(
        &mut self,
        bus: &mut B,
        virtual_address: u64,
        access: Access,
        privilege: Privilege,
    ) -> Result<u64, TranslationFault> {
        let high = virtual_address >> 38;
        if high != 0 && high != (1_u64 << 26) - 1 {
            return Err(TranslationFault::Page);
        }

        let mut pte_address = (self.satp & PTE_PPN_MASK) << PAGE_SHIFT;
        for level in (0..3).rev() {
            let shift = PAGE_SHIFT + level * 9;
            let index = (virtual_address >> shift) & 0x1ff;
            pte_address = pte_address.wrapping_add(index * 8);
            let mut pte = self.read_pte(bus, pte_address)?;
            if pte & PTE_VALID == 0 || pte & PTE_HIGH_RESERVED != 0 {
                return Err(TranslationFault::Page);
            }
            let mut physical = ((pte >> 10) & PTE_PPN_MASK) << PAGE_SHIFT;
            let mut permissions = (pte >> 1) & 7;
            if permissions == 0 {
                if pte & PTE_NONLEAF_RESERVED != 0 {
                    return Err(TranslationFault::Page);
                }
                pte_address = physical;
                continue;
            }
            if permissions == 0b010 || permissions == 0b110 {
                return Err(TranslationFault::Page);
            }
            let pbmt = pte & PTE_PBMT_MASK;
            if pbmt == PTE_PBMT_MASK || pbmt != 0 && self.menvcfg & MENVCFG_PBMTE == 0 {
                return Err(TranslationFault::Page);
            }
            if pte & PTE_NAPOT != 0 {
                if level != 0 || (physical >> PAGE_SHIFT) & 0xf != 8 {
                    return Err(TranslationFault::Page);
                }
                physical =
                    (physical & !(0xf << PAGE_SHIFT)) | (virtual_address & (0xf << PAGE_SHIFT));
            }
            let page_offset_mask = (1_u64 << shift) - 1;
            if physical & page_offset_mask != 0 {
                return Err(TranslationFault::Page);
            }
            let user_page = pte & PTE_USER != 0;
            match privilege {
                Privilege::Supervisor if user_page => {
                    if access == Access::Execute || self.mstatus & MSTATUS_SUM == 0 {
                        return Err(TranslationFault::Page);
                    }
                }
                Privilege::User if !user_page => return Err(TranslationFault::Page),
                _ => {}
            }
            if access == Access::Read && self.mstatus & MSTATUS_MXR != 0 {
                permissions |= permissions >> 2;
            }
            if permissions & (1 << access as u8) == 0 {
                return Err(TranslationFault::Page);
            }
            let needs_update =
                pte & PTE_ACCESSED == 0 || access == Access::Write && pte & PTE_DIRTY == 0;
            if needs_update {
                if self.menvcfg & MENVCFG_ADUE == 0 {
                    return Err(TranslationFault::Page);
                }
                pte |= PTE_ACCESSED;
                if access == Access::Write {
                    pte |= PTE_DIRTY;
                }
                self.write_pte(bus, pte_address, pte)?;
            }
            return Ok((virtual_address & page_offset_mask) | (physical & !page_offset_mask));
        }
        Err(TranslationFault::Page)
    }

    fn read_pte<B: CpuBus>(&self, bus: &mut B, address: u64) -> Result<u64, TranslationFault> {
        if !self.pmp_access_ok(address, 8, Access::Read, Privilege::Supervisor) {
            return Err(TranslationFault::Access);
        }
        bus.read(GuestAddress(address), AccessWidth::DoubleWord)
            .map_err(|_| TranslationFault::Access)
    }

    fn write_pte<B: CpuBus>(
        &self,
        bus: &mut B,
        address: u64,
        value: u64,
    ) -> Result<(), TranslationFault> {
        if !self.pmp_access_ok(address, 8, Access::Write, Privilege::Supervisor) {
            return Err(TranslationFault::Access);
        }
        bus.write(GuestAddress(address), AccessWidth::DoubleWord, value)
            .map_err(|_| TranslationFault::Access)
    }

    fn effective_privilege(&self, access: Access) -> Privilege {
        if access != Access::Execute && self.mstatus & MSTATUS_MPRV != 0 {
            return match (self.mstatus & MSTATUS_MPP) >> 11 {
                0 => Privilege::User,
                1 => Privilege::Supervisor,
                _ => Privilege::Machine,
            };
        }
        self.privilege
    }

    fn pmp_access_ok(&self, address: u64, len: u64, access: Access, privilege: Privilege) -> bool {
        let Some(end) = address.checked_add(len) else {
            return false;
        };
        for index in 0..self.pmpcfg.len() {
            let config = self.pmpcfg[index];
            let mode = config & PMP_CFG_A_MASK;
            if mode == 0 {
                continue;
            }
            let encoded = self.pmpaddr[index];
            let (lower, upper) = if mode == PMP_CFG_A_TOR {
                (
                    if index == 0 {
                        0
                    } else {
                        self.pmpaddr[index - 1] << 2
                    },
                    encoded << 2,
                )
            } else if mode == 2 << 3 {
                let lower = encoded << 2;
                (lower, lower.saturating_add(4))
            } else {
                let trailing = (!encoded).trailing_zeros();
                let mask = if trailing == 64 {
                    u64::MAX
                } else {
                    (1_u64 << trailing) - 1
                };
                let lower = (encoded & !mask) << 2;
                let size = 1_u64.checked_shl(trailing + 3).unwrap_or(u64::MAX);
                (lower, lower.saturating_add(size))
            };
            if address >= upper || end <= lower {
                continue;
            }
            if address < lower || end > upper {
                return false;
            }
            if privilege == Privilege::Machine && config & PMP_CFG_L == 0 {
                return true;
            }
            let permission = match access {
                Access::Read => PMP_CFG_R,
                Access::Write => PMP_CFG_W,
                Access::Execute => PMP_CFG_X,
            };
            return config & permission != 0;
        }
        privilege == Privilege::Machine
    }

    fn translation_trap(access: Access, address: u64, fault: TranslationFault) -> Trap {
        let exception = match (access, fault) {
            (Access::Execute, TranslationFault::Page) => Exception::InstructionPageFault,
            (Access::Execute, TranslationFault::Access) => Exception::InstructionAccessFault,
            (Access::Read, TranslationFault::Page) => Exception::LoadPageFault,
            (Access::Read, TranslationFault::Access) => Exception::LoadAccessFault,
            (Access::Write, TranslationFault::Page) => Exception::StorePageFault,
            (Access::Write, TranslationFault::Access) => Exception::StoreAccessFault,
        };
        Trap {
            exception,
            value: address,
        }
    }

    fn access_trap(access: Access, address: u64) -> Trap {
        Self::translation_trap(access, address, TranslationFault::Access)
    }
}

fn read_arena(arena: &[u8], offset: usize, width: AccessWidth) -> Option<u64> {
    let len = width.bytes();
    let bytes = arena.get(offset..offset.checked_add(len)?)?;
    let mut value = [0_u8; 8];
    value[..len].copy_from_slice(bytes);
    Some(u64::from_le_bytes(value))
}

fn write_arena(arena: &mut [u8], offset: usize, width: AccessWidth, value: u64) -> Option<()> {
    let len = width.bytes();
    let destination = arena.get_mut(offset..offset.checked_add(len)?)?;
    destination.copy_from_slice(&value.to_le_bytes()[..len]);
    Some(())
}

impl From<BusError> for TranslationFault {
    fn from(_: BusError) -> Self {
        Self::Access
    }
}
