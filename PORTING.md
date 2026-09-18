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
| Next | CPU foundation | Integer execution, traps, CSRs, privilege, counters, PMP, and Sv39 before optional instruction groups |
| Pending | SoftFP and remaining ISA | F/D semantics, compressed and advertised scalar extensions |
| Pending | Platform foundation | Reset path, FDT, CLINT, PLIC, UART, RTC, finisher, and framebuffer |
| Pending | VirtIO devices | MMIO transport plus block, console, 9p, network, and input |
| Pending | Browser services | Configuration, HTTP storage, encrypted filesystem support, JS adapter, and browser entry points |
| Pending | Complete-platform acceptance | Shared native/WASM suite, Chrome validation, xv6 user tests, and Alpine login/shutdown |

Current status
--------------

The physical memory milestone is complete. Rust now provides a fixed byte arena
with typed guest addresses and 32-bit arena offsets, ordered RAM and device
regions, enable/move/disable operations, and double-buffered dirty-page
snapshots that return explicit TLB invalidation ranges. Matching native C and
Rust tests cover mapping boundaries, zero initialization, devices, disabled and
moved RAM, dirty bitmap rollover, and page-level clearing. The sanitizer tests
also found and fixed a null zero-length `memset` and a dirty-bitmap teardown leak
in the C reference.

Rust 1.92, the `wasm32-unknown-unknown` standard library, Clang 19, Emscripten
3.1.69, Node 20, and Chrome 152 were present when the port was initialized. The
next milestone is CPU foundation; no CPU port has started.

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
