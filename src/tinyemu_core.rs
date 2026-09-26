//! Ownership and ABI boundary for the C execution core.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusError {
    AccessFault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunState {
    Running,
    Waiting,
    HostAttention,
    TimerChanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunOutcome {
    pub cycles: u32,
    pub state: RunState,
}

#[allow(unsafe_code)]
mod ffi {
    use super::BusError;
    use std::alloc::{Layout, alloc_zeroed, dealloc};
    use std::ffi::{CStr, c_char, c_void};
    use std::ptr::NonNull;

    const ALIGN: usize = 16;
    const HEADER: usize = ALIGN;

    #[repr(C)]
    struct CoreState {
        _opaque: [u8; 0],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct RunResult {
        pub cycles: u32,
        pub reason: u32,
    }

    unsafe extern "C" {
        fn tinyemu_core_create() -> *mut CoreState;
        fn tinyemu_core_destroy(core: *mut CoreState);
        fn tinyemu_core_register_ram(core: *mut CoreState, base: u64, len: u64, flags: i32) -> i32;
        fn tinyemu_core_register_device(
            core: *mut CoreState,
            base: u64,
            len: u64,
            widths: i32,
        ) -> i32;
        fn tinyemu_core_ram_range(
            core: *mut CoreState,
            address: u64,
            len: usize,
            write: i32,
        ) -> *mut u8;
        fn tinyemu_core_take_dirty(
            core: *mut CoreState,
            region: i32,
            words: *mut u32,
            count: usize,
        ) -> i32;
        fn tinyemu_core_clear_dirty(core: *mut CoreState, region: i32, offset: u64) -> i32;
        fn tinyemu_core_set_time(core: *mut CoreState, ticks: u64);
        fn tinyemu_core_stimecmp(core: *const CoreState) -> u64;
        fn tinyemu_core_set_interrupts(core: *mut CoreState, mask: u32);
        fn tinyemu_core_run(core: *mut CoreState, budget: u32, host: *mut c_void) -> RunResult;
        fn tinyemu_core_pc(core: *const CoreState) -> u64;
        fn tinyemu_core_register(core: *const CoreState, index: u32) -> u64;
        fn tinyemu_core_mcause(core: *const CoreState) -> u64;
        fn tinyemu_core_mtval(core: *const CoreState) -> u64;
    }

    pub struct Core {
        state: NonNull<CoreState>,
    }

    #[derive(Clone, Copy)]
    pub(crate) struct CoreHandle {
        state: NonNull<CoreState>,
    }

    pub trait HostCallbacks {
        /// Reads one device register.
        ///
        /// # Errors
        /// Returns an access fault for an invalid device operation.
        fn read(&mut self, address: u64, width: u32) -> Result<u32, BusError>;
        /// Writes one device register.
        ///
        /// # Errors
        /// Returns an access fault for an invalid device operation.
        fn write(&mut self, address: u64, width: u32, value: u32) -> Result<(), BusError>;
        fn interrupts(&self) -> u32;
        fn host_attention(&self) -> bool {
            false
        }
    }

    struct HostContext<'a> {
        host: &'a mut dyn HostCallbacks,
    }

    impl CoreHandle {
        pub fn ram_view(&self, address: u64, len: usize) -> Option<&[u8]> {
            // SAFETY: Machine owns this live C core, and an immutable Machine
            // borrow prevents a concurrent interpreter call or RAM write.
            let pointer = unsafe { tinyemu_core_ram_range(self.state.as_ptr(), address, len, 0) };
            let pointer = NonNull::new(pointer)?;
            Some(unsafe { std::slice::from_raw_parts(pointer.as_ptr(), len) })
        }

        pub fn register_ram(&mut self, base: u64, len: u64, flags: i32) -> Option<usize> {
            // SAFETY: The handle points to the live C core owned by Machine.
            let region =
                unsafe { tinyemu_core_register_ram(self.state.as_ptr(), base, len, flags) };
            usize::try_from(region).ok()
        }

        pub fn register_device(&mut self, base: u64, len: u64, widths: i32) -> Option<usize> {
            // SAFETY: The handle points to the live C core owned by Machine.
            let region =
                unsafe { tinyemu_core_register_device(self.state.as_ptr(), base, len, widths) };
            usize::try_from(region).ok()
        }

        pub fn ram_range(&mut self, address: u64, len: usize, write: bool) -> Option<&mut [u8]> {
            // SAFETY: C checks the complete region. The slice is tied to this
            // mutable handle borrow; Machine does not run the CPU concurrently.
            let pointer = unsafe {
                tinyemu_core_ram_range(self.state.as_ptr(), address, len, i32::from(write))
            };
            let pointer = NonNull::new(pointer)?;
            Some(unsafe { std::slice::from_raw_parts_mut(pointer.as_ptr(), len) })
        }

        pub fn take_dirty(&mut self, region: usize, count: usize) -> Option<Vec<u32>> {
            let region = i32::try_from(region).ok()?;
            let mut words = vec![0; count];
            // SAFETY: C writes exactly count words after validating the region.
            let status = unsafe {
                tinyemu_core_take_dirty(self.state.as_ptr(), region, words.as_mut_ptr(), count)
            };
            (status == 0).then_some(words)
        }

        pub fn clear_dirty(&mut self, region: usize, offset: u64) -> bool {
            let Ok(region) = i32::try_from(region) else {
                return false;
            };
            // SAFETY: The C handle is live and validates region and offset.
            unsafe { tinyemu_core_clear_dirty(self.state.as_ptr(), region, offset) == 0 }
        }
    }

    impl Core {
        #[must_use]
        pub fn new() -> Option<Self> {
            // SAFETY: The C constructor returns an owned pointer or null.
            let state = NonNull::new(unsafe { tinyemu_core_create() })?;
            Some(Self { state })
        }

        pub fn register_ram(&mut self, base: u64, len: u64, flags: i32) -> Option<usize> {
            self.handle().register_ram(base, len, flags)
        }

        pub fn register_device(&mut self, base: u64, len: u64, widths: i32) -> Option<usize> {
            self.handle().register_device(base, len, widths)
        }

        /// Returns and clears the dirty-page snapshot for a registered RAM region.
        pub fn take_dirty(&mut self, region: usize, count: usize) -> Option<Vec<u32>> {
            self.handle().take_dirty(region, count)
        }

        /// Clears one dirty-page marker in a registered RAM region.
        pub fn clear_dirty(&mut self, region: usize, offset: u64) -> bool {
            self.handle().clear_dirty(region, offset)
        }

        pub fn ram_range(&mut self, address: u64, len: usize, write: bool) -> Option<&mut [u8]> {
            // SAFETY: C validates the entire RAM range. The returned slice borrows
            // self, so it cannot outlive the core or overlap another Rust access.
            let pointer = unsafe {
                tinyemu_core_ram_range(self.state.as_ptr(), address, len, i32::from(write))
            };
            let pointer = NonNull::new(pointer)?;
            Some(unsafe { std::slice::from_raw_parts_mut(pointer.as_ptr(), len) })
        }

        pub(crate) fn handle(&self) -> CoreHandle {
            CoreHandle { state: self.state }
        }

        pub fn set_time(&mut self, ticks: u64) {
            // SAFETY: self owns a live C core.
            unsafe { tinyemu_core_set_time(self.state.as_ptr(), ticks) };
        }

        #[must_use]
        pub fn stimecmp(&self) -> u64 {
            // SAFETY: this reads the live core while no interpreter call is active.
            unsafe { tinyemu_core_stimecmp(self.state.as_ptr()) }
        }

        pub fn set_interrupts(&mut self, mask: u32) {
            // SAFETY: self owns a live C core.
            unsafe { tinyemu_core_set_interrupts(self.state.as_ptr(), mask) };
        }

        pub fn run(&mut self, budget: u32) -> RunResult {
            // SAFETY: The core and RAM remain stable for the complete call. The
            // callbacks currently reject MMIO and do not reenter the core.
            unsafe {
                tinyemu_core_run(
                    self.state.as_ptr(),
                    budget.min(i32::MAX as u32),
                    core::ptr::null_mut(),
                )
            }
        }

        pub fn run_host(&mut self, budget: u32, host: &mut dyn HostCallbacks) -> RunResult {
            let mut context = HostContext { host };
            // SAFETY: C uses the stack context only during this synchronous call.
            // Its callbacks do not retain slices or reenter the CPU interpreter.
            unsafe {
                tinyemu_core_run(
                    self.state.as_ptr(),
                    budget.min(i32::MAX as u32),
                    (&raw mut context).cast::<c_void>(),
                )
            }
        }

        #[must_use]
        pub fn pc(&self) -> u64 {
            // SAFETY: self owns a live C core.
            unsafe { tinyemu_core_pc(self.state.as_ptr()) }
        }

        #[must_use]
        pub fn register(&self, index: u32) -> u64 {
            // SAFETY: self owns a live C core; C bounds-checks the index.
            unsafe { tinyemu_core_register(self.state.as_ptr(), index) }
        }

        #[must_use]
        pub fn machine_cause(&self) -> u64 {
            // SAFETY: self owns a live C core.
            unsafe { tinyemu_core_mcause(self.state.as_ptr()) }
        }

        #[must_use]
        pub fn machine_trap_value(&self) -> u64 {
            // SAFETY: self owns a live C core.
            unsafe { tinyemu_core_mtval(self.state.as_ptr()) }
        }
    }

    impl Drop for Core {
        fn drop(&mut self) {
            // SAFETY: This is the unique owner of the C core.
            unsafe { tinyemu_core_destroy(self.state.as_ptr()) };
        }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn tinyemu_host_read(
        host: *mut c_void,
        address: u64,
        width: u32,
        value: *mut u32,
    ) -> i32 {
        if host.is_null() || value.is_null() {
            return -1;
        }
        // SAFETY: run_host passes a live HostContext for the call duration.
        let context = unsafe { &mut *host.cast::<HostContext<'_>>() };
        match context.host.read(address, width) {
            Ok(result) => {
                // SAFETY: C passes the address of its local output value.
                unsafe { value.write(result) };
                0
            }
            Err(_) => -1,
        }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn tinyemu_host_write(
        host: *mut c_void,
        address: u64,
        width: u32,
        value: u32,
    ) -> i32 {
        if host.is_null() {
            return -1;
        }
        // SAFETY: run_host passes a live HostContext for the call duration.
        let context = unsafe { &mut *host.cast::<HostContext<'_>>() };
        context.host.write(address, width, value).map_or(-1, |()| 0)
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn tinyemu_host_interrupts(host: *mut c_void) -> u32 {
        if host.is_null() {
            return 0;
        }
        // SAFETY: run_host passes a live HostContext for the call duration.
        let context = unsafe { &*host.cast::<HostContext<'_>>() };
        context.host.interrupts()
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn tinyemu_host_attention(host: *mut c_void) -> u32 {
        if host.is_null() {
            return 0;
        }
        // SAFETY: The interpreter retains this context only during run_host.
        let context = unsafe { &*host.cast::<HostContext<'_>>() };
        u32::from(context.host.host_attention())
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn tinyemu_alloc_zeroed(size: usize) -> *mut c_void {
        let Some(total) = size.checked_add(HEADER) else {
            return core::ptr::null_mut();
        };
        let Ok(layout) = Layout::from_size_align(total, ALIGN) else {
            return core::ptr::null_mut();
        };
        // SAFETY: The layout is valid. The returned block stays owned by C until
        // tinyemu_free receives its payload pointer.
        let base = unsafe { alloc_zeroed(layout) };
        if base.is_null() {
            return core::ptr::null_mut();
        }
        // SAFETY: The allocation contains HEADER bytes followed by size bytes.
        unsafe {
            base.cast::<usize>().write_unaligned(total);
            base.add(HEADER).cast()
        }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn tinyemu_free(ptr: *mut c_void) {
        if ptr.is_null() {
            return;
        }
        // SAFETY: Every non-null pointer passed here came from
        // tinyemu_alloc_zeroed and retains its header and original layout.
        unsafe {
            let base = ptr.cast::<u8>().sub(HEADER);
            let total = base.cast::<usize>().read_unaligned();
            let layout = Layout::from_size_align_unchecked(total, ALIGN);
            dealloc(base, layout);
        }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn tinyemu_abort() -> ! {
        std::process::abort()
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn tinyemu_assert_fail(
        expression: *const c_char,
        file: *const c_char,
        line: i32,
    ) -> ! {
        // SAFETY: The C assert macro passes static NUL-terminated strings.
        let expression = unsafe { CStr::from_ptr(expression) };
        // SAFETY: The C assert macro passes static NUL-terminated strings.
        let file = unsafe { CStr::from_ptr(file) };
        eprintln!(
            "TinyEMU assertion failed at {}:{line}: {}",
            file.to_string_lossy(),
            expression.to_string_lossy()
        );
        std::process::abort()
    }
}

pub(crate) use ffi::CoreHandle;
pub use ffi::{Core, HostCallbacks, RunResult};
