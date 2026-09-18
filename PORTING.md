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
| Next | Platform foundation | Reset path, FDT, CLINT, PLIC, UART, RTC, finisher, and framebuffer |
| Pending | VirtIO devices | MMIO transport plus block, console, 9p, network, and input |
| Pending | Browser services | Configuration, HTTP storage, encrypted filesystem support, JS adapter, and browser entry points |
| Pending | Complete-platform acceptance | Shared native/WASM suite, Chrome validation, xv6 user tests, and Alpine login/shutdown |

Current status
--------------

The SoftFP milestone is complete. Rust now implements binary32 and binary64
with integer arithmetic, all five RISC-V rounding modes, sticky exception
flags, fused multiply-add, square root, comparisons, classification, and all
RV64 integer and format conversions. CPU integration covers F/D state and
CSRs, NaN boxing, scalar and compressed memory operations, and every F/D
instruction family. Matching C and Rust tests cover arithmetic, rounding,
flags, NaNs, conversions, moves, memory access, and disabled floating-point
state. Random differential checks cover the arithmetic core, FMA, and format
conversion. The tests also corrected C reference handling for single-precision
moves, NaN-boxed operands, reserved dynamic rounding modes, and dirty FS state.

Rust 1.92, the `wasm32-unknown-unknown` standard library, Clang 19, Emscripten
3.1.69, Node 20, and Chrome 152 were present when the port was initialized. The
next milestone is the platform foundation: machine reset and device tree,
interrupt controllers, UART, RTC, finisher, and framebuffer.

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
