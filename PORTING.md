Riscbox Rust port
=================

Goal
----

Build an independent Rust implementation of the browser Riscbox platform. The
C implementation remains available as the behavioral reference and as one side
of differential tests, but no C object code is linked into Rust.

The production target is `wasm32-unknown-unknown`. The port covers the RV64
CPU, Sv39 memory system, machine, guest-visible devices, configuration loader,
HTTP block and filesystem storage, JavaScript 9p bridge, and current browser
entry points. A native Rust emulator, SDL, SLIRP, native storage backends, and
image-building tools are outside the port.

Architecture
------------

The Rust package lives at the repository root with source under `rust/`.
Production code is split into CPU, SoftFP, memory, devices, machine,
configuration, storage backends, and host integration modules as they are
ported. Native builds exist to run tests; the deployable library is built
directly for bare WASM.

Guest addresses remain `u64`. RAM and other byte storage occupy a fixed arena
whose validated offsets fit in `u32`, matching the WASM memory model. TLBs will
cache virtual-page tags and arena offsets. Ordinary instruction and RAM paths
must not allocate or call JavaScript. Host services supply clocks, scheduling,
network and display output, HTTP completion, JavaScript file buffers, and 9p
requests through explicit interfaces.

Dependencies remain exceptional. Core CPU, memory, SoftFP, and configuration
code use the standard library or direct implementations. The encrypted HTTP
filesystem may use narrowly configured RustCrypto AES, CBC, SHA-256, and PBKDF2
crates after their resolved dependency graph is reviewed and recorded.

Validation workflow
-------------------

Each milestone follows the same order:

1.  Trace the complete affected C path and add focused C behavioral tests.
2.  Express the same cases as Rust tests, including edge and failure cases.
3.  Implement Rust until both suites pass on their native test runners.
4.  Build the Rust WASM target and add deployed-target coverage as its public
    test interface becomes available.
5.  Record the completed behavior and next boundary here, then commit the
    focused milestone.

Specifications define expected architectural behavior. When C differs because
of a defect, add the specification-derived regression test and correct C
narrowly instead of reproducing the defect. Full xv6 and Alpine boot tests begin
after the platform is complete; component tests remain the primary validation
during the port.

Milestones
----------

| State | Milestone | Completion boundary |
| ----- | --------- | ------------------- |
| Complete | Physical memory map | Fixed arena, RAM/device regions, lookup, mapping changes, dirty-page snapshots and invalidation records |
| Complete | CPU foundation | RV64I/M execution, traps, CSRs, privilege, counters, interrupts, PMP, and Sv39 |
| Complete | Remaining integer ISA | Atomics, compressed instructions, and advertised scalar extensions |
| Complete | SoftFP | Exact F/D arithmetic, conversions, rounding, flags, NaN boxing, and floating-point CSRs |
| Complete | Platform foundation | Reset path, FDT, CLINT, PLIC, UART, RTC, finisher, and framebuffer |
| Next | VirtIO devices | MMIO transport plus block, console, 9p, network, and input |
| Pending | Browser services | Configuration, HTTP storage, encrypted filesystem support, JS adapter, and browser entry points |
| Pending | Complete-platform acceptance | Shared native/WASM suite, Chrome validation, xv6 user tests, and Alpine login/shutdown |

Current status
--------------

The platform foundation milestone is complete. Rust now composes the CPU and
physical memory with CLINT, PLIC, 16550A UART, Goldfish RTC, SiFive test
finisher, and optional simple framebuffer devices. Boot loading places
firmware, kernel, initrd, and a version-17 flattened device tree in RAM and
installs the QEMU-compatible reset trampoline. Host-facing methods provide
explicit clock updates, UART input and output, finish status, and consumed
framebuffer redraw spans. Matching sanitizer-backed C and Rust tests cover
device registers, interrupt transitions, FDT contents, boot layout, reset
state, and framebuffer dirty tracking. The tests corrected C PLIC claims below
the configured threshold and made framebuffer refresh consume dirty pages.

Rust 1.92, the `wasm32-unknown-unknown` standard library, Clang 19, Emscripten
3.1.69, Node 20, and Chrome 152 were present when the port was initialized. The
next milestone is VirtIO MMIO transport and the block, console, 9p, network,
and input devices.

Decision log
------------

*   2026-09-18: Use raw Rust WASM with a handwritten compatibility adapter.
*   2026-09-18: Preserve the existing C browser boundary, including loaders and
    browser backends, while excluding the native emulator surface.
*   2026-09-18: Develop one subsystem at a time with matching C and Rust tests.
*   2026-09-18: Begin with safe memory access and optimize only from measured
    WASM results; performance parity is tracked but is not the initial gate.
*   2026-09-18: Represent RAM as one grow-during-construction arena and return
    invalidation records to callers instead of storing callbacks in the memory
    map. Execution will begin only after machine construction fixes the arena.
*   2026-09-18: Keep the CPU interpreter direct and cache guest virtual-page to
    32-bit arena-offset translations. Port atomics, compressed instructions,
    floating point, and optional scalar groups after the base CPU boundary.
*   2026-09-18: Split SoftFP from the remaining integer ISA after inventorying
    their independent state and validation requirements. Implement SoftFP with
    integer algorithms so native and WASM builds have identical rounding,
    exceptions, and NaN behavior.
*   2026-09-18: Represent floating-point values as raw bits throughout the CPU
    and SoftFP boundary. Arithmetic returns sticky flags explicitly; the CPU
    owns architectural state, NaN boxing, and FS dirty transitions.
*   2026-09-18: Keep platform devices as concrete state owned by the machine.
    Device MMIO updates explicit interrupt levels and host-visible event
    buffers; the machine applies CPU interrupt lines at scheduler boundaries.
    Inject both timer ticks and wall-clock nanoseconds so native and WASM tests
    remain deterministic.
