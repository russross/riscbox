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
| Complete | VirtIO devices | MMIO transport plus block, console, 9p, network, and input |
| Complete | Browser services | Configuration, HTTP storage, encrypted filesystem support, JS adapter, and browser entry points |
| Complete | Complete-platform acceptance | Shared native/WASM suite, Chrome validation, xv6 user tests, and Alpine login/shutdown |
| Complete | Framebuffer delivery and Risclet demo | Dirty-region WASM callbacks plus a static, editable multi-example 9p application |

Current status
--------------

The planned Rust port is complete through the browser integration boundary.
The runtime loads configuration, boot images, and split HTTP disks through
explicit request/completion queues; pending VirtIO block descriptors resume
after their blocks arrive. The raw WASM ABI and dependency-free JavaScript
adapter provide scheduling, console and device events, framebuffer dirty
regions, and synchronous browser 9p service calls. Framebuffer updates refer
directly to the fixed WASM arena and include their position, dimensions, and
stride. Both supplied pages load the Rust artifact, and the image distribution
script packages it as `riscbox.wasm`.

Focused native tests cover the complete machine and browser runtime. The
release acceptance suite boots Alpine 3.24.2, logs in, and shuts down through
the finisher, and runs current xv6 user tests over its UART and VirtIO block
device. Chrome 152 boots the deployed Rust WASM Alpine image to its login
prompt. The standalone Risclet page boots its in-memory JavaScript 9p service,
loads either tracked example without RPC, mirrors guest-created and deleted
files, and updates optional instructions from `doc/doc.md`. Its interface
retains the deployed CodeGrinder editor, terminal, draggable panes, and sizing
behavior while removing the RPC and grading workflows. Clean
C release, sanitizer, and reference WASM builds remain compatibility checks;
the Rust workspace, strict Clippy checks, WASM build, Node adapter tests, and
the ignored full-guest acceptance tests are the port's validation surfaces.
Run the full guests explicitly with:

    cargo test --release --test platform_acceptance alpine_reaches_login_and_shuts_down -- --ignored
    RISCBOX_XV6_KERNEL=/path/to/kernel.bin RISCBOX_XV6_DISK=/path/to/fs.img \
        cargo test --release --test platform_acceptance xv6_boots_over_uart_and_passes_user_tests -- --ignored

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
*   2026-09-18: Port only the modern VirtIO MMIO transport; PCI remains outside
    the browser platform boundary. Validate every descriptor chain before
    device dispatch, keep host receive queues pending until explicit ingress,
    and use backend traits for block, network, and raw 9p services.
*   2026-09-18: Keep browser I/O as explicit request/completion and event queues.
    Put stable unmangled exports in the small `riscbox-wasm` companion crate so
    the emulator crate continues to forbid unsafe code. The handwritten adapter
    copies all host-owned buffers across the boundary.
*   2026-09-18: Use narrowly configured RustCrypto `aes`, `cbc`, `pbkdf2`, and
    `sha2` crates for legacy filesystem compatibility. Default features are
    disabled. The four direct crates resolve to 13 transitive crates for shared
    cipher/digest traits and buffers, HMAC, constant-time helpers, and fixed
    arrays; allocator, password-format, randomness, and zeroization features
    remain disabled.
*   2026-09-18: Keep HTTP disk requests asynchronous through the machine and
    VirtIO layers so browser fetches never block the interpreter. Resume the
    retained descriptor chain only after the matching completion arrives.
*   2026-09-18: Keep browser 9p synchronous at the existing JavaScript service
    boundary. The WASM wrapper supplies a fixed reply buffer and rejects absent
    servers, oversized replies, and backend errors without adding an executor.
*   2026-09-18: Deliver framebuffer changes as dirty rectangles whose bytes
    remain in the fixed WASM arena. The JavaScript adapter passes a zero-copy
    view with explicit geometry and stride to the host callback.
*   2026-09-18: Package Risclet examples as tracked static files and load them
    into a browser-owned 9p tree. The guest and editor share that live tree;
    `doc/doc.md` alone controls whether the instructions view exists.
*   2026-09-18: Preserve the proven CodeGrinder frontend stack for the Risclet
    demo: CodeMirror, Split.js, and CommonMark. Use ghostty-web's xterm-compatible
    terminal and fit addon in place of xterm.js and its WebGL renderer. The
    locked frontend graph contains 34 production packages; the removed gRPC and
    protobuf stack is absent. Keep the raw emulator adapter dependency-free.
*   2026-09-18: Pass browser epoch milliseconds through the raw WASM ABI as
    explicit low and high 32-bit words. JavaScript numbers represent the full
    millisecond value exactly, while the split avoids an i64/BigInt boundary
    and preserves the host wall clock used by the Goldfish RTC.

Next milestone
--------------

Add framebuffer demonstration programs to the image, configure the guest
display and input devices, and connect the framebuffer callback to a canvas in
the Risclet page. The current demo intentionally remains terminal-only until
that guest-to-page path can be validated together.
