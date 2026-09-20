Riscbox project direction
=========================

This is a focused fork of TinyEMU for small RISC-V teaching and grading VMs in
a web browser. The implementation stays lean and direct while moving the
machine and CPU toward current standards.

Current Riscbox implementation
-------------------------------

Riscbox is the Rust implementation targeting WebAssembly in the browser. Read
`PORTING.md` before making architectural changes; it records the architecture,
milestone order, validation contract, and current status. Update its status and
decision log at completed milestones or whenever a durable project assumption
changes. The `c/` directory is the historical TinyEMU-derived C fork,
preserved only as a reference archive. It is not the target of development,
compatibility, parity, or new tests.

Riscbox includes the RV64 CPU, memory system, virtual platform, browser
loaders, and browser backends through the existing WASM integration boundary.
It does not include a native Rust emulator, SDL, SLIRP, native filesystem and
socket backends, or image-preparation tools. Native Rust builds are test
runners only. The archived C tree is not a build dependency or validation
surface for current work.

Implement one bounded subsystem at a time. Start from the relevant RISC-V,
device, and QEMU `virt` behavior, add focused Rust tests, and implement Riscbox
until those tests pass. Specifications take precedence over historical behavior
in the archived C tree. Reserve full guest boot validation for the completed
platform, while running component and integration tests at every milestone.

Riscbox implementation guidelines
----------------------------------

*   `wasm32-unknown-unknown` is the production target. Preserve the existing
    browser API with a small handwritten JavaScript adapter; do not introduce
    Emscripten, WASI, `wasm-bindgen`, or an async runtime into the Rust build.
*   Keep guest virtual and physical addresses as `u64`. Use validated `u32`
    offsets for storage in the fixed WASM memory arena. Keep JavaScript calls,
    allocations, and uncommon checks out of cached CPU and RAM paths.
*   Start with safe Rust. Add a small unsafe fast path only after a WASM
    benchmark demonstrates a material benefit, and document its invariants.
*   Keep dependencies few and inspect the complete transitive graph before
    adding one. Prefer direct implementations for the interpreter, memory,
    SoftFP, and configuration parser. The planned exception is narrowly
    configured RustCrypto crates for the legacy encrypted HTTP filesystem.
*   Use strong types for architectural state and host interfaces. Keep device
    dispatch concrete and ownership explicit; avoid shared ownership and
    interior mutability in the interpreter.
*   Test native Rust for fast diagnostics and Rust WASM for deployed behavior.
    Measure WASM throughput, module size, and memory use during milestones, but
    correctness is the initial gate. The archived C tree is not part of this
    validation contract.

Supported scope
---------------

*   RV64 only, with Sv39 virtual memory and one hart.
*   A small QEMU `virt`-compatible platform for current xv6 and small Alpine
    Linux systems.
*   Browser deployment through WebAssembly. Native builds exist for fast image
    preparation, iteration, and deeper correctness testing.
*   A 16550A UART with standard serial-console behavior, plus an optional
    VirtIO console. xv6 must work entirely over the UART. Linux may use the
    VirtIO console while UART output exposes firmware and early kernel logs.
*   VirtIO block, console, 9p, input, and existing network devices. The 9p path
    favors simple browser integration over throughput. Do not expand networking
    without an explicit change in scope.
*   SDL framebuffer output suitable for an HTML canvas integration.

The RISC-V specifications are the primary architectural reference. QEMU `virt`
behavior is the next compatibility reference. RVA23 without vector instructions
is the mid-term CPU goal; full RVA23 is only an eventual possibility. Vector and
hypervisor support are deliberately out of scope.

Prefer standard features that are small, useful to Alpine or xv6, and unlikely
to burden the interpreter. Defer features that are invasive or have little value
for browser-hosted student VMs. Deployed images may be deliberately prepared,
so new image and boot-loader paths must earn their complexity by materially
improving preparation or compatibility. Linux already handles compressed
initrds loaded opaquely by Riscbox; a well-supported boot-loader path remains a
reasonable candidate, not a requirement.

Current platform contract
-------------------------

*   The generated device tree uses the standard header, reservation map,
    structure, and strings layout expected by current libfdt consumers.
*   The CPU exposes 16 standard RV64 PMP entries with 54-bit address registers.
    PMP permissions cover explicit accesses and implicit page-table accesses
    without adding checks to cached TLB hits.
*   Sstc provides `stimecmp` and supervisor timer interrupts. Svadu exposes
    `menvcfg.ADUE` and selects hardware A/D updates or Svade page faults. Both
    extensions are reported through the device tree.
*   The CPU reports Ssccptr, Sscounterenw, Sstvala, Sstvecd, and Ssu64xl. A
    supervisor probe validates hardware Sv39 page-table reads, writable enables
    for every implemented counter, complete trap values, direct trap vectors,
    and hardwired 64-bit U-mode. Exceptions without a defined trap value clear
    the value instead of retaining prior trap state.
*   Svinval implements `SINVAL.VMA` with the existing conservative TLB flush.
    Its two ordering-only fences are no-ops in the in-order one-hart model.
    Privilege and TVM traps follow the standard, and Linux retains the reported
    extension during boot.
*   Svpbmt accepts standard NC and IO leaf PTEs when `menvcfg.PBMTE` is
    enabled and faults on disabled, reserved, or non-leaf uses. The cacheless,
    in-order memory model provides ordering at least as strong as either type;
    PBMT does not add work to cached TLB hits. Linux retains the extension.
*   Svnapot implements the ratified 64 KiB leaf-PTE encoding by substituting
    four physical page-number bits during a page walk. Ordinary 4 KiB TLB
    entries remain unchanged. Reserved encodings and upper-level uses fault,
    and Linux retains the extension.
*   Zicond implements both conditional-zero instructions and is reported
    through the legacy and structured device-tree ISA properties.
*   Zihintpause and Zihintntl are reported because their standard and
    compressed encodings already execute as architectural no-ops.
*   Zimop implements all 40 full-width may-be-operations, and Zcmop implements
    all eight compressed may-be-operations while keeping adjacent reserved
    encodings illegal.
*   Zcb implements its RV64 compressed loads, stores, extensions, NOT, and
    multiply operations. `c.sext.w` uses the standard existing `c.addiw rd,0`
    alias.
*   Zawrs implements both wait-on-reservation instructions as permitted
    immediate completions, without adding scheduler or timing state.
*   Zba implements all nine RV64 address-generation instructions.
*   Zbb implements all 24 RV64 basic bit-manipulation instructions, including
    the RV64 word forms and zero-extension alias.
*   Zbs implements all eight single-bit manipulation instructions. With Zba,
    Zbb, and Zbs complete, the CPU reports the combined `B` extension in `misa`
    and both device-tree ISA properties.
*   Zicbop reports a 64-byte cache-operation block and implements all prefetch
    variants as architectural hints. Linux accepts the discovery data without
    disabling the extension.
*   Zicbom and Zicboz use 64-byte cache blocks. Cache management operations are
    cacheless no-ops after standard translation and permission checks; cache
    zeroing clears the complete block. `menvcfg` and `senvcfg` enforce the
    standard lower-privilege controls, and faults report the original effective
    address. OpenSBI and Linux discover and use the extensions successfully.
*   The one-hart RAM model reports Ziccif, Ziccrse, Ziccamoa, Zicclsm, and
    Za64rs. Linux accepts Ziccrse and uses its queued-spinlock path. Device-tree
    ISA names follow the architecture's category order rather than lexical
    order.
*   Privilege returns restore their own level's interrupt-enable bit, and CSR
    writes that enable an already-pending interrupt end the current interpreter
    block. This is required for standard interrupt delivery between adjacent
    guest instructions without adding work to the normal instruction path.
*   The machine exposes the UART at `0x10000000`. `console: "uart"` connects
    input and output there and omits the VirtIO console. With the default VirtIO
    console, `uart_output: true` mirrors UART output to the host without routing
    input to both devices.
*   The QEMU-compatible Goldfish RTC at `0x00101000` exposes the host wall clock
    and alarm interrupts on PLIC IRQ 11. Linux initializes its system clock from
    the device in native and browser builds.
*   VirtIO MMIO devices report QEMU's standard vendor ID. Current upstream xv6
    boots from the UART, mounts its VirtIO block device, and passes its complete
    user test suite.
*   VirtIO block implements the standard 20-byte device identification request
    and completes unsupported requests with `VIRTIO_BLK_S_UNSUPP`. A prepared
    Alpine 3.24.2 system boots through OpenSBI, mounts its installation ISO,
    reaches a UART login prompt, and shuts down cleanly.
*   Native, debug, and browser release surfaces use the Riscbox name. Web assets
    resolve firmware, kernel, initrd, disk, and 9p paths relative to the VM
    configuration. The dependency-free example page boots Alpine to a UART
    login prompt in current Chrome.

Near-term priorities
--------------------

*   Keep current xv6 and a deliberately prepared current Alpine image booting
    without emulator-specific guest patches. Use new guest failures to select
    the next platform or CPU compatibility work.
*   Fill small, broadly useful RVA23 gaps when focused architectural probes and
    real guests can validate them. Continue to omit vectors, hypervisor support,
    multiple harts, and features whose complexity does not serve the target VM.
*   Tighten QEMU `virt` device-tree and platform parity where firmware or Linux
    depends on it. Prefer correcting the existing platform over adding parallel
    compatibility modes.
*   Improve image preparation only when a small loader feature removes material
    friction. Keep specially prepared uncompressed kernels and initrds as the
    baseline; evaluate compressed initrds or a standard boot loader from actual
    Alpine deployment needs.

Build and validation
--------------------

The root build is Rust-focused:

*   `make` or `make release` builds the optimized Rust workspace.
*   `make test` runs Rust, Python tool, and JavaScript adapter tests; `make
    check` also runs strict Clippy and Python type checks.
*   `make wasm` builds the deployed Rust WebAssembly artifact, `make kernel`
    builds the canonical custom kernel, and `make dist` builds both core
    distribution artifacts.
For CPU or platform changes, rebuild the Rust checks and WASM target from a
clean tree when the milestone is ready. Exercise the affected behavior with a
guest or focused firmware probe rather than relying on compilation alone. Current xv6 is the
primary UART and supervisor-mode integration guest. Stock Alpine through OpenSBI
is the primary Linux/platform compatibility path; U-Boot is useful compatibility
coverage when its extra boot path is relevant.

Bare-metal probe images must be linked as non-PIE firmware or extracted from
the intended loadable section explicitly. Do not convert a PIE ELF with sections
near zero and `0x80000000` directly to a flat binary: `objcopy` preserves the
address gap and creates a multi-gigabyte sparse image.

Engineering guidelines
----------------------

*   Preserve the short, predictable interpreter and TLB hit paths. Put uncommon
    architectural checks on CSR writes, translation misses, or device paths when
    the standard permits it. Do not add speculative performance abstractions;
    measure before tuning.
*   Prefer direct data structures and shallow control flow. Platform drivers
    prioritize simple, standard, correct behavior over cleverness.
*   Keep changes incremental and focused. Test and commit each completed
    milestone, then re-evaluate the next priority using what the guest exposed.
*   For guest compatibility failures, trace the complete firmware, kernel, and
    device transaction before changing code. Validate the fix with the real
    guest plus a focused architectural probe when register semantics are at
    issue. Run Linux through the debug build far enough to cover device
    discovery, root-media access, userspace startup, login, and shutdown.
*   All commit messages are one line and should match the existing imperative
    style.
*   Track source and durable project documentation only. Keep images, generated
    boot assets, temporary firmware probes, and test configuration files out of
    git.
*   This repository is an exception to the system-wide read-only git rule.
    Focused, high-confidence fixes should be committed automatically after
    validation. Never use git checkout or reset to discard work.
