Riscbox changelog
=================

Unreleased
----------

*   Added `guestClockSkew` (default `0.20`) to slow guest-clock advancement
    within each quantum while keeping its cycle budget unchanged. Timer
    deadlines use the same integer mapping. Timing diagnostics now report the
    interval maximum and boot-to-current 50th, 90th, and 99th percentiles of
    the skew needed to avoid potential zero-skew catch-up waits.
*   Scheduled runnable browser quanta through `MessageChannel` tasks instead of
    repeated zero-delay timers. A full headless Chrome xv6 compile run fell from
    156.1 to 121.9 seconds in the same environment, with negligible WFI time
    during compilation. The profile harness can enable timing logs with
    `RISCBOX_PROFILE_TIMING=1`.
*   Excluded disabled CLINT and supervisor timer compares from future-deadline
    selection, so browser timing diagnostics do not report near-infinite timer
    intervals while those timers are disabled.
*   Enabled browser timing diagnostics in the Risclet demo and added a dedicated
    xv6 compile benchmark page with diagnostics enabled. Both static
    distributions can be served from `images/` for browser testing.
*   Standardized browser scheduling names across C, Rust, the raw WASM ABI,
    JavaScript, tests, and documentation. An execution quantum contains one or
    more CPU runs and may cross host-service boundaries. Renamed the public
    `timesliceMs` option to `targetQuantumMs` and the raw WASM turn exports to
    quantum exports. The browser adapter exposes `runQuantum()` in place of
    the ambiguous `run()` wrapper and rejects the old duration option with a
    migration error. Removed the unused direct-run scheduling policy, its
    duplicate machine and RTC delay helpers, and an obsolete host-scheduling
    callback in the example UI. Timing diagnostics now report emulated
    Mcycles/s instead of approximate MIPS. The C callback interface is now
    named `PlatformCallbacks`, and queued browser input lives in
    `browser_input.rs`.
*   Moved quantum cycle budgets, cycle-rate calibration, precise timer deadline
    arithmetic, and repeated CPU runs into Rust. JavaScript supplies host epoch
    time and whole-quantum monotonic elapsed time, dispatches host actions
    between WASM activations, and schedules browser wakeups.
*   Calibrated quantum cycle budgets from complete-quantum cycle and elapsed-time
    samples with a five-second active-time half-life. Guest time advances in
    10 MHz ticks from cycles at a fixed rate within each quantum; timer writes
    exit the core and subsequent CPU runs target the next precise deadline.
    The target quantum duration defaults to 10 ms, WFI wakeups cap at 100 ms,
    and a guest-clock lead delays the next quantum until host epoch time catches
    up. Opt-in timing diagnostics report rate, CPU runs per quantum, timer
    intervals, WFI sleep time, and catch-up waits.
*   Added an early host-service exit after guest MMIO requests for 9p and HTTP
    block work. The browser adapter resumes unused quantum cycles after
    resident 9p replies and replaces WFI wakeups when asynchronous replies
    arrive. 9p sessions can hint that a request will settle through microtasks;
    the adapter logs 20 consecutive empty yields and recovers. Removed the
    custom `schedule(milliseconds)` option and changed the custom 9p request
    interface to include `expectResponse`.
*   Load raw or gzip-compressed kernels at the existing boot address, with
    decompressed size bounded by the RAM and initrd layout. Image deployments
    now publish `gzip -9` kernels under names derived from the uncompressed
    kernel hash; QEMU image preparation continues to use the raw kernel.
*   Migrated architectural CPU, privileged, memory-translation, atomic,
    compressed, scalar-extension, and floating-point probes to guest programs
    executed by `tinyemu-core` in `tests/tinyemu_architecture.rs`. Added TinyEMU-owned RAM tests for bounds,
    read-only regions, and dirty tracking. Removed the parallel Rust CPU,
    SoftFP, physical-memory implementations and their reference-only tests;
    VirtIO transport tests now adapt the production C RAM interface. No
    production C execution or memory path changed.
*   Changed browser scheduling to request immediate continuation while the
    guest is runnable and a bounded CLINT, supervisor timer, or RTC delay while
    it waits. The existing xv6 profile image completed and shut down in a
    105.333-second V8 profile, down from 161.939 seconds with the previous
    fixed delay; idle sampled time fell from 40.714 to 7.036 seconds. The
    recorded archived C profile is 97.400 seconds. The new trace and console
    log are retained as `riscbox-scheduler` artifacts beside those baselines.
*   Moved machine callback errors and run outcomes to the TinyEMU boundary,
    interrupt bits to the platform module, and shared guest-memory types beside
    the TinyEMU-backed RAM bridge. Production C execution and memory paths are
    unchanged.
*   Moved production CPU instruction execution, CSR/VM/TLB handling, SoftFP,
    and physical RAM into a freestanding TinyEMU C core. Rust retains machine
    setup and devices and entered C for coarse CPU runs, crossing back for
    MMIO. Clang builds the C core directly for native tests and raw WASM;
    Rust's allocator backs C allocations without Emscripten or libc imports.
    Alpine boot through login and shutdown and xv6 full usertests pass.
*   A fresh production-WASM trace and xv6 compile profile found the flattened
    interpreter at 203.275 profile seconds against TinyEMU's 98.558 seconds.
    This is a 23.2% reduction from the previous Riscbox result and moves the
    ratio from 2.80 to 2.06. The remaining likely hot-loop costs are cursor/PC
    continuation, page-tail selection, and exact per-instruction budgets.
*   Flattened the remaining common interpreter hot paths toward TinyEMU's loop:
    validated instruction and data accesses now use one unaligned scalar arena
    operation, page chunks advance a linear arena cursor without repeated page
    masking, architectural PCs are reconstructed only where demanded, and WASM
    aggressively inlines cached memory and common integer/compressed helpers.
    Counter bookkeeping now derives cycle deltas from one remaining-cycle value
    while retaining exact `Cpu::run` budgets and precise SYSTEM visibility.
    Page refill distinguishes a complete instruction ending at the page
    boundary from a 32-bit instruction straddling the final halfword,
    preventing an unchecked fetch beyond the validated page.
*   Directed LLVM to inline the private instruction dispatcher into its single
    WASM call site while retaining the typed Rust execution interface. The
    prepared xv6 workload completed at a 255.576-second guest timestamp and
    264.566 profile seconds, down 11.6% and 11.3% from the previous 289.095 and
    298.194-second results. The paired TinyEMU profile completed in 94.597
    seconds. Generated-code inspection confirmed that LLVM removed the
    dispatcher call and material typed-outcome transfer while retaining the
    structured Rust source interface.
*   Reworked the Rust interpreter around TinyEMU-style page chunks. Sequential
    execution now resolves the execute TLB once per chunk, fetches one aligned
    word with a checked cross-page slow path, keeps PC and counter deltas local,
    and samples interrupts and device events only at explicit boundaries.
    Aligned TLB-hit RAM accesses use isolated unchecked fixed-width helpers
    after complete-page validation; misses, MMIO, page walks, misalignment, and
    mapping changes remain checked.
*   Reduced the prepared xv6 compile workload from the previous 362.416-second
    guest timestamp and 371.725-second V8 profile to 289.095 and 298.194
    seconds, respectively, a further reduction of about 20%. The paired
    TinyEMU run completed at 93.728 profile seconds. The optimized WASM artifact
    is 320,895 bytes.
*   Reduced the prepared xv6 compile workload from 520.185 to 362.416 guest
    seconds with safe fixed-width RAM fast paths and measured arithmetic
    dispatch inlining. Generated WASM now uses direct scalar accesses instead
    of generic copies, retains checked arena bounds, and no longer repairs `x0`
    after every instruction because all register writes already reject it. A
    differential test compares TLB-fast execution with the checked bus path.
*   Added a reproducible Alpine xv6 kernel-only compile image and Node/V8
    profiling harness for comparable Rust Riscbox and archived TinyEMU WASM
    runs. The image uses native guest RISC-V build tools, excludes `fs.img`,
    and shuts down automatically after emitting a completion marker.
*   Completed the browser network path with a dependency-free TypeScript
    WebSocket adapter and a documented binary Ethernet protocol. The adapter
    tracks carrier, bounds browser send buffering, validates messages, and
    reconnects with bounded backoff; distributions include its JavaScript and
    declarations.
*   Hardened VirtIO network handling with per-VM locally administered MAC
    addresses, MAC and status feature negotiation, configuration interrupts,
    bounded FIFO ingress, reset cleanup, nonfatal packet loss, and validation
    of unsupported transmit offload headers.
*   Added focused Rust and TypeScript coverage plus a local Node origin stub
    that boots a non-PIE network firmware probe in temporary-profile headless
    Chrome against the real WASM. The probe exercises device-tree discovery,
    feature negotiation, carrier, and bidirectional Ethernet frames.
*   Simplified VirtIO 9p to one asynchronous request/completion model after the
    last synchronous Rust backend was removed. Request buffers now move into
    host actions while retained descriptors store only their protocol tags.
*   Exposed VirtIO console resize notifications to browser hosts through
    `runtime.consoleResize()` and the raw `riscbox_console_resize` export.
*   Consolidated project documentation around stable agent guidance,
    user-facing documentation, historical changes, and current development
    plans.

2026-09-20
----------

### Generic browser 9p service

*   Replaced the legacy `file`, `socket`, and `js9p` filesystem configuration
    forms with `{ server, tag }` and a host registry of generic 9P2000.L
    servers. Removed the proprietary Rust HTTP 9p backend and command-file
    protocol. The legacy encrypted HTTP filesystem helpers and their crypto
    dependencies were later removed; split HTTP-backed block disks remain.
*   Made VirtIO 9p concurrent. The transport retains bounded descriptor chains
    by typed request ID, accepts out-of-order reply and suppression outcomes,
    validates complete message envelopes and tags, and retires endpoint
    generations on reset without touching stale guest memory.
*   Added explicit open, request, completion, suppression, failure, and close
    actions across the machine, raw WASM ABI, and JavaScript adapter. The
    adapter copies host-owned buffers, prevents WASM reentry, preserves
    settlement order, and isolates independently connected sessions.
*   Added an authoritative TypeScript 9P2000.L server under `js/p9/`. Shared
    filesystem state is separate from session state, so clients reuse fid and
    tag values safely. `Tversion` resets a session; `Tflush` handles response
    suppression and immediate tag reuse.
*   Added stable inode/QID identity, hard links, open-unlink lifetime,
    monotonic directory cookies, logical-byte and metadata quotas, shared
    POSIX byte-range locks, atomic namespace mutations, and explicit unsupported
    operation errors.
*   Added typed lazy seed plugins, shared in-flight loads, retained load
    failures with explicit retry, content-revision race protection, and
    application operations with discriminated results and change events.
    Included HTTPS-manifest and pre-downloaded-tar plugin examples.
*   Validated two Linux mounts backed by one server under `cache=mmap` and
    `cache=none`, cross-client mutation, unmount/remount, one client closing
    while another continued, and VM restart with outstanding asynchronous work.
*   Reduced the optimized WASM artifact from 411,449 bytes before removal of
    the legacy backend to 319,720 bytes. The final adapter was 13,825 bytes;
    generated 9p JavaScript and declarations were 64,896 and 12,816 bytes.

### Supplied 9p server decisions

*   Rust owns VirtIO transport validation and lifecycle only. Servers own
    protocol negotiation, tags, fids, flushes, errors, and filesystem state;
    Linux owns client caching policy.
*   Each configured endpoint receives a fresh session, while multiple sessions
    may share one filesystem. Runtime endpoint generations and server session
    generations independently prevent stale asynchronous work from committing.
*   JavaScript's synchronous commits provide mutation atomicity, but operations
    revalidate session and inode revisions after awaits instead of serializing
    the filesystem behind one promise.
*   Seed plugins define a complete immutable namespace and one opaque body
    loader. Successful buffers transfer into ordinary mutable inode storage;
    refreshing a deployment creates a new filesystem instance.
*   The supplied server is for trusted clients. It records Unix metadata but
    does not provide authentication, isolation, persistence, cache invalidation,
    leases, or proprietary overlay behavior.

2026-09-19
----------

### Rust becomes the implementation

*   Made the Rust workspace the root implementation and moved the complete
    former C implementation under `c/` as a reference-only archive. The archive
    ceased to be a build dependency, compatibility target, or validation
    surface.
*   Moved the emulator crate to conventional `src/`, added the
    `riscbox-wasm/` raw-export crate, replaced the production image splitter
    with a typed Python tool, and made the root-owned kernel a distribution
    artifact consumed by image definitions.
*   Completed the raw WASM browser runtime with explicit HTTP request/completion
    queues, console and device events, framebuffer dirty rectangles, wall-clock
    injection, and synchronous Web Crypto entropy for both `/chosen/rng-seed`
    and VirtIO RNG.
*   Added regression coverage found by a C-to-Rust workflow audit: SoftFP
    cancellation and subnormal division, in-slice interrupt changes, browser
    input backpressure, VirtIO receive wakeups and block completion, large HTTP
    disks and requests, scalar decode/discovery, and tablet scaling.
*   Changed deployments to content-derived boot and disk names, retained old
    generations until explicit cleanup, replaced configuration last, fetched
    configuration with `no-store`, and revalidated programmatically loaded WASM
    for each VM start.

### Browser-runtime decisions

*   Browser I/O uses explicit queues. HTTP block reads retain their VirtIO
    descriptor until the matching asynchronous completion arrives.
*   Framebuffer events reference bytes in the fixed WASM arena and carry dirty
    rectangle geometry and stride. The host consumes the zero-copy view during
    the callback.
*   Browser epoch milliseconds cross the raw ABI as low and high 32-bit words,
    avoiding an i64/BigInt boundary while preserving exact JavaScript values.
*   Entropy comes from one explicit host source: operating-system entropy in
    native tests and bounded synchronous Web Crypto fills in the browser. No
    guest-visible pseudorandom generator was added.

2026-09-18
----------

### Rust port completion

*   Reimplemented the RV64 CPU, Sv39 memory system, SoftFP, platform, VirtIO
    devices, browser storage, configuration loader, raw WASM ABI, and browser
    integration in Rust.
*   Adopted a grow-during-construction fixed memory arena with validated `u32`
    offsets, explicit invalidation records, and cached guest-page to arena-offset
    translations.
*   Kept floating-point values as raw bits across CPU and SoftFP boundaries.
    Integer algorithms provide identical native/WASM rounding, flags, NaN
    behavior, and subnormal handling; the CPU owns architectural state and NaN
    boxing.
*   Kept devices as concrete machine-owned state. MMIO updates interrupt levels
    and host event buffers; scheduler boundaries apply CPU interrupt lines.
    Injected timer ticks and wall-clock time keep tests deterministic.
*   Ported only the modern VirtIO MMIO transport. Descriptor chains are fully
    validated before dispatch, receive queues wait for explicit ingress, and
    block, network, and 9p services use explicit backend boundaries.
*   Used raw Rust WASM and a handwritten compatibility adapter instead of
    Emscripten, WASI, `wasm-bindgen`, or an async Rust runtime. The main emulator
    crate remained safe Rust; stable unmangled exports live in the boundary
    crate.
*   Added narrowly configured RustCrypto AES, CBC, PBKDF2, and SHA-256 support
    for legacy encrypted HTTP storage. Default features were disabled; the
    resolved graph contained 13 transitive crates and omitted allocator,
    password-format, randomness, and zeroization features.
*   Preserved the CodeGrinder-derived Risclet interface with CodeMirror,
    Split.js, CommonMark, and an xterm-compatible terminal while removing its
    RPC, protobuf, and grading workflows. Static examples and `doc/doc.md` are
    served from a browser-owned 9p tree.
*   Completed native component tests, strict checks, WASM tests, current xv6
    user tests, Alpine 3.24.2 login and clean shutdown, and Chrome 152 browser
    boot validation.

### Architecture decisions

*   Production targets bare `wasm32-unknown-unknown`; native Rust is a test and
    image-preparation environment rather than an emulator product.
*   Guest addresses remain `u64`; fixed-arena storage uses validated `u32`
    offsets. JavaScript calls, allocations, and uncommon checks stay out of hot
    CPU and RAM paths.
*   The interpreter, memory, SoftFP, and configuration parser use direct
    implementations. Dependencies remain exceptional and require review of the
    complete resolved graph.
*   Specifications and QEMU `virt` behavior define compatibility. The archived
    C implementation is historical evidence, not expected behavior.

2026-09-11
----------

*   Added a native raw 9P2000.L endpoint connected to a Unix-domain server and
    a diod-backed Risclet demonstration with a read/write student directory.
*   Raised the VirtIO 9p queue capacity for Linux scatter/gather requests while
    retaining the smaller default for other devices.
*   Added the original browser `js9p` endpoint and in-memory JavaScript server,
    host-side file operations, and change notifications. These interfaces were
    superseded by the generic concurrent service on 2026-09-20.

2026-09-07
----------

This release established the focused RV64 fork based on TinyEMU 2019-12-21.

*   Renamed the project and user-visible interfaces from TinyEMU/temu to
    Riscbox.
*   Reduced the CPU to RV64, the MMU to Sv39, and the platform to one hart;
    removed x86, RV32, RV128, Sv32, Sv48, and Windows paths; and made vectors
    and hypervisor support explicit non-goals.
*   Moved the machine to the QEMU `virt` memory map and boot protocol, including
    the low reset vector, DRAM/FDT layout, OpenSBI handoff, and standard device
    tree bindings.
*   Added the NS16550A UART, standard PLIC and CLINT layouts, SiFive test
    finisher, QEMU VirtIO vendor ID, Goldfish RTC, and VirtIO block ID requests.
*   Implemented PMP, supervisor timer compare, Svadu, Svinval, Svnapot, Svpbmt,
    current counter and privilege behavior, and focused RVA23-era scalar
    extensions used by OpenSBI, Linux, and xv6.
*   Corrected LR/SC and AMO semantics, Sv39 PTE validation, MPRV return,
    counters, high multiplication, FMIN/FMAX, HTTP bounds, configuration
    ownership, and writable FDT placement.
*   Validated current xv6 through its user tests and Alpine 3.24.1 through
    OpenSBI to a login prompt in native, sanitizer, and browser builds.

TinyEMU history
===============

2019-12-21
----------

*   Added the complete JSLinux demo and RISC-V initrd support.
*   Corrected RISC-V FMIN/FMAX instructions.

2018-09-23
----------

*   Added separate RISC-V BIOS and kernel support.

2018-09-15
----------

*   Renamed the project to TinyEMU (`temu`) and added a single executable for
    all emulated machines.

2018-08-29
----------

*   Added compilation fixes.

2017-08-06
----------

*   Added JSON configuration, SDL display, VirtIO input, PCI/VirtIO PCI,
    user-mode networking, and JavaScript terminal and file integration.

2017-06-10
----------

*   Avoided unnecessary RISC-V kernel patches.

2017-05-25
----------

*   Improved RISC-V emulation performance, supported user ISA 2.2 and
    privileged architecture 1.10, matched the `fs_net`/vfsync protocol, and
    handled console resize.
