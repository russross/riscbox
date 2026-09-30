Riscbox project guide
=====================

Purpose and scope
-----------------

Riscbox is a focused RV64 virtual platform for small teaching and grading VMs
in a web browser. It is a Rust successor to the repository's historical
TinyEMU fork, not a general-purpose emulator. The production target is
`wasm32-unknown-unknown`; native Rust builds are test runners and development
tools, not a supported native emulator.

The supported machine is deliberately narrow:

*   One little-endian RV64 hart with M/S/U modes and Sv39.
*   A small QEMU `virt`-compatible platform for current xv6 and prepared Alpine
    Linux systems.
*   A 16550A UART, optional VirtIO console, Goldfish RTC, PLIC, ACLINT MSWI and MTIMER,
    SiFive test finisher, simple framebuffer, and VirtIO MMIO block, 9p,
    network, entropy, keyboard, and tablet devices.
*   Browser delivery through raw WebAssembly, a handwritten JavaScript adapter,
    HTTP-backed disks, and host-provided 9P2000.L servers.

The optional TypeScript network adapter carries one Ethernet frame per binary
WebSocket message to a host-selected origin endpoint. Browser network devices
use per-VM locally administered MAC addresses, expose carrier through VirtIO
status and configuration interrupts, and bound pending frames and bytes. The
repository supplies a local Node protocol stub for real WASM/Chrome tests but
no production network origin service, native TAP backend, or SLIRP backend.

RV32, multiple harts, vectors, the hypervisor extension, PCIe, AIA, UEFI,
general device emulation, a native UI, SDL, SLIRP, and native filesystem or
socket backends are outside the current scope. Networking exists but is not a
near-term expansion area.

Repository map and terminology
------------------------------

*   `src/` owns the Rust machine, devices, configuration, storage, browser
    runtime, and the typed boundary to the C core. `browser_input.rs` queues
    host input until a CPU run consumes it. `guest_memory.rs` owns the
    shared memory API and TinyEMU RAM bridge. No parallel Rust CPU, SoftFP, or
    physical-memory implementation remains.
*   `tinyemu-core/` is the active freestanding TinyEMU CPU, SoftFP, and physical
    memory implementation. `build.rs` compiles it with Clang for native and
    raw WASM targets.
*   `riscbox-wasm/` supplies the small stable raw WASM export surface. Keep
    unsafe ABI code isolated there. Main-crate unsafe code is confined to the
    TinyEMU FFI module.
*   The workspace version in root `Cargo.toml` is inherited by both Rust
    crates. A push to `main` that increases it builds and publishes one GitHub
    release archive with the WASM runtime, browser modules, canonical Linux
    Image, OpenSBI firmware, U-Boot binary, and API documentation. The release
    archive excludes guest images and image build scripts.
*   `js/riscbox.js` is the dependency-free browser adapter for the raw ABI.
*   `js/network/` is the typed WebSocket Ethernet frontend and protocol.
*   `js/block/` defines the host block provider interface and supplies
    TypeScript split-HTTP and host-array providers. The browser adapter
    dispatches their requests through the raw WASM ABI; Rust retains VirtIO
    descriptor validation and device ordering.
*   `js/p9/` is the promise-based TypeScript facade for Rust filesystem handles
    and optional on-demand seed plugins. Generated JavaScript and declarations
    go under `build/js/p9/`.
*   `src/ninep.rs` and `src/ninep/` own the standalone Rust namespace;
    `src/ninep_protocol.rs` and `src/ninep_protocol/` own its 9P2000.L session.
    `src/ninep_backend.rs` connects registered Rust trees to VirtIO and the
    browser runtime. `src/browser_abi/ninep.rs` exposes raw filesystem handles,
    copied host-operation packets, source tickets, and change events; its ABI
    guide is `src/browser_abi/ninep/README.md`. The promise facade uses this ABI; Rust owns production protocol semantics. The
    protocol contract is in
    `src/ninep_protocol/README.md`; migration coordination is in `DEV.md`.
*   `images/` contains reproducible Makefile-driven image definitions and deployment tooling.
    Generated downloads, images, boot assets, and distributions are not source.
*   `kernel/` owns the canonical custom Linux kernel consumed by image builds.
*   `opensbi/` and `uboot/` own pinned firmware and bootloader builds. Each
    tracks its Makefile, version, and config; downloads, sources, and outputs
    are ignored. The shared image helpers use `opensbi/fw_dynamic.bin`, and
    Risclet uses `kernel/linux` as OpenSBI's S-mode next stage. OpenSBI's
    `defconfig` selects the one-hart Riscbox SBI services and FDT drivers.
*   `c/` is a read-only historical TinyEMU-derived archive. It is not an
    implementation source, compatibility target, build dependency, parity
    requirement, or validation surface.
*   `README.md` is user-facing documentation. `DEV.md` holds only active plans,
    future work, and deferred findings. `CHANGELOG.md` is the historical record.

In this repository, "native" means a Rust test or image-preparation execution
environment. It does not imply a supported native emulator. "Browser runtime"
means the Rust machine, raw WASM ABI, and JavaScript adapter together. A "9p
server" implements protocol sessions; a "seed plugin" only supplies an initial
namespace and lazy regular-file bodies to the provided in-memory server.

Current contract
----------------

The CPU implements RV64 I, M, A, F, D, C, and the advertised scalar extensions
needed by the target guests. This includes the implemented B subsets, current
counter and supervisor guarantees, PMP, Sstc, Svadu, Svinval, Svnapot, Svpbmt,
cache-block operations, conditional operations, hints, may-be-operations, and
wait-on-reservation. It is moving toward RVA23 where that is useful, but it is
not RVA23 compliant because vectors and several other required extensions are
intentionally absent. Advertise only implemented behavior.

Production CPU runs enter the TinyEMU C instruction loop. C owns CPU state,
TLB, physical mappings, and RAM; its setup and teardown allocations use the
Rust global allocator. Rust owns platform devices and handles MMIO callbacks.
Shared callback errors and run outcomes live at the TinyEMU boundary, while
interrupt masks live with the platform and memory access types live with the
TinyEMU RAM bridge. The C core is compiled without a C runtime, Emscripten, or
WASI. Rust has no parallel CPU, SoftFP, or physical-memory implementation.

The generated device tree follows standard libfdt layout and QEMU `virt`
bindings. The platform boots current xv6 over UART and VirtIO block and boots a
prepared Alpine system through OpenSBI to login and clean shutdown. The browser
adapter loads configuration, firmware, kernels, initrds, and split HTTP disks
relative to the configuration URL. Raw and gzip-compressed kernels load at the
same guest address; firmware can also be gzip-compressed, and initrds remain
opaque. Boot configuration can override firmware, kernel, initrd, and device
tree physical addresses; the loader validates their RAM bounds and overlap.
The reset ROM passes OpenSBI `fw_dynamic.bin` a dynamic-info block with the
next-stage address. Without firmware, the kernel starts directly in M-mode.
Decompressed output is bounded by the boot layout. Image
deployments gzip firmware and the next-stage payload while naming them from
their uncompressed hashes. HTTP
disk writes are session-local. The browser adapter uses `force-cache` for
content-hash-named boot and disk assets and `no-store` for the configuration.
The browser adapter now parses configuration files, supplies defaults, and
resolves boot and drive URLs through `startFromUrl`; `startResolved` accepts a
host object. Rust validates the resolved configuration and still constructs
the machine. The legacy `start` path and native Rust parser remain available.
HTTP block stores start with a 16 MiB in-memory cache limit that grows to
cover a single request when needed.
The parallel TypeScript HTTP provider reads the same split-image format and
retains its own bounded clean cache and session CoW clusters. The original
Rust HTTP store remains supported while performance and memory behavior are
compared. The TypeScript array provider writes through to the caller's exact
whole-sector `Uint8Array` view and retains it across VM resets. Host image
export requires guest filesystem synchronization or orderly shutdown.
The xv6 profile browser page resolves its deployed config, opens the HTTP
manifest through the TypeScript provider, and attaches it as a host drive.
Resolved `driveN` entries may attach a host block provider by numeric ID and
512-byte sector capacity. HTTP and host drives occupy guest slots in the
configured order. The host provider owns its bytes and handles asynchronous
read/write requests; Rust validates ranges and reply lengths and returns
provider failures as guest I/O errors. Generations retire late replies after
device or VM reset. Provider `reset` retains data, while VM destroy calls
`close`. The Rust HTTP cache and CoW path remain available in parallel.

The host can deliver soft shutdown and reboot input events, force an immediate
halt or reset, boot a halted machine, and destroy a halted machine. The prepared
Alpine and Risclet guests use BusyBox `acpid` to turn the two input events into
orderly userspace actions. Guest poweroff halts without teardown; guest reboot
uses the QEMU `virt` syscon reset value. In-place reset restarts the C CPU,
reloads boot images, and clears platform and VirtIO interface state while
retaining host backends, 9p servers, HTTP clean cache and CoW data, and guest
RAM mappings. Pending 9p work is retired by generation; pending HTTP requests
are retired without reusing request IDs. Console and framebuffer host callbacks
receive reset notifications. Destroy releases the machine and 9p sessions.

The Risclet demo loads the custom Linux kernel directly through OpenSBI. Its
single EROFS disk is the read-only root filesystem; tmpfs supplies `/tmp` and
the writable overlay layers for `/var` and `/home`. The device tree model
identifies the platform as `riscbox`; QEMU names remain in functional board
bindings and build targets. Linux uses UART early and the VirtIO console for
login.

Rust sizes each execution quantum from a measured emulated cycle rate and a
target duration, twenty milliseconds by default. Each quantum locks its rate
and maps consumed cycles to 10 MHz guest timer ticks using integer arithmetic.
Each quantum extends both its target duration and cycle budget by the
previous guest-clock lead. It starts at the later of host time and the
previous guest time. A decayed P99 estimate of the skew needed by recent
runnable quanta slows guest time proactively. Cycle-rate estimates have a
ten-second half-life; timing diagnostics retain rate-variance samples.
When lead extends a quantum, its cycle budget uses the greater of that
long-term estimate and the previous runnable quantum's observed rate if that
quantum ended ahead of host time. Its guest-time mapping uses the more
conservative of the P99-skewed rate and the rate implied by that previous
quantum's observed cycle throughput. Timing diagnostics report interval
Mcycles/s mean and standard deviation for runnable quanta.
Timer writes exit the C loop so Rust can size the next CPU run to the earliest ACLINT,
supervisor, or RTC deadline. C also exits after MMIO requests that queue external
9p or HTTP block work; Rust resident 9p requests finish within the notifying CPU
run. Rust releases the exclusive CPU-run borrow before JavaScript
dispatches actions. JavaScript resumes the same quantum after resident 9p
replies and wakes a WFI sleeping guest on later completions. JavaScript measures
the complete quantum with a monotonic clock and supplies its elapsed time to
Rust for calibration. WFI sleeping guests wake at the next timer deadline or
a 100-millisecond fallback; asynchronous completions can replace that wakeup.
Runnable quanta continue through `MessageChannel` tasks to avoid browser
clamping of repeated zero-delay timers; delayed wakeups still use `setTimeout`.
The C interpreter may pass a requested cycle limit at a code-block boundary,
and Rust accounts for the actual emulated cycles consumed.
Risclet and xv6 profile image setup boots writable ext4 under QEMU, then
converts the finished filesystem to a read-only EROFS disk.
Their guests mount tmpfs at `/tmp` and session-local tmpfs-backed overlays at
`/var` and `/home`. Split HTTP disks use 512 KiB chunks by default.

Timing and execution lexicon
----------------------------

*   A **browser task** is one event-loop task, such as a timer or input callback.
    A **microtask** is a promise continuation or queued microtask. Use these
    browser terms only for their event-loop meanings.
*   The **TinyEMU CPU core** is C; the **machine/platform** and **browser runtime**
    are Rust. The **raw WASM ABI** exposes scalar calls, the **browser adapter**
    is JavaScript, and the **host application** embeds the adapter. C calls
    Rust through `PlatformCallbacks`; JavaScript handles queued `HostAction`s.
*   An **execution quantum** is one logical bounded unit of guest work. It can
    contain several synchronous JS-to-WASM activations and several **CPU runs**
    (individual C interpreter calls). A **host-service boundary** returns an
    active quantum to JavaScript for queued device actions and source requests.
*   The **quantum target duration** is the configured nominal host-time interval
    plus any carried guest-clock lead; it determines the **quantum cycle budget**.
    A **CPU-run cycle limit** is
    the smaller request for one C call. **Cycles consumed** are what C actually
    reports, including permitted overshoot. All cycles here are emulated guest
    cycles, never host CPU cycles.
*   **Host epoch time** comes from `Date.now()` in Unix milliseconds. **Host
    elapsed time** comes from `performance.now()` in monotonic milliseconds.
    **Guest timer ticks** are 10 MHz units; **guest RTC nanoseconds** are the
    corresponding nanosecond base passed to Goldfish RTC, which applies its
    device offset. Keep absolute **timer deadlines** in
    guest ticks distinct from remaining guest-tick durations.
*   **Timer reprogramming** changes a future deadline; a **due timer** sets an
    interrupt pending. **WFI sleep** means the guest CPU waits for an interrupt.
    **VM inactive** refers only to the VM lifecycle. A **guest-clock lead** is
    the last presented guest time ahead of host epoch time; the next quantum
    carries that lead into its guest-time window.

VirtIO 9p supports concurrent pending requests and resident synchronous replies.
Rust validates descriptors and message envelopes; its registered filesystem
backend owns protocol semantics and drains earlier retirements before `Rflush`.
Each configured `{ server, tag }` endpoint gets an independent session. Raw
filesystem handles exist before boot and retain namespace state across VM
lifetimes. Bind keys before startup; a VM lifetime guard rejects a second live
VM over one handle while allowing several endpoints in the attached VM. Halt,
shutdown, and reset retain the guard; destroy releases it. Unregistered keys reject VM startup. Host reads pin their inode
through asynchronous loading, independent of rename, unlink, and path reuse.
Source and host-operation completions run between CPU activations and poll
guest sessions after releasing namespace borrows. Namespace reset reports
`ESTALE` for pending host reads; obsolete source completions are ignored.
The TypeScript facade supplies promises, copied packets, source dispatch, and
change subscriptions; Rust owns inode state, sessions, locking, and replies.
Source polling is separate from the generic host-action queue. The adapter
polls it after host calls and every CPU boundary, then wakes the guest after
completion. Filesystem handles belong to their runtime's WASM instance.
Do not restore the removed `file`, `socket`, or `js9p` configuration
forms.

Architecture rules
------------------

*   Specifications are authoritative. Use the relevant RISC-V specification
    first and QEMU `virt` behavior second. Historical C behavior is evidence
    only when investigating lineage.
*   Keep guest virtual and physical addresses as `u64`. Keep allocation,
    JavaScript calls, and uncommon checks out of cached C CPU and RAM paths.
*   Keep unsafe Rust confined to the TinyEMU FFI module. Document ownership, arena stability, and mutable aliasing
    invariants at each unsafe block.
*   Keep architectural state and host interfaces strongly typed. Favor concrete
    device ownership, shallow control flow, explicit dependencies, and immutable
    values. Keep the C CPU-run borrow exclusive of Rust device memory access.
*   Keep the raw WASM ABI and adapter small. Do not add Emscripten, WASI,
    `wasm-bindgen`, an async Rust runtime, or adapter runtime dependencies.
*   Keep dependencies exceptional. Inspect the complete resolved graph before
    adding one. Prefer direct implementations for the configuration parser.
    The narrowly configured RustCrypto crates
    remain only for encrypted split HTTP block images.
*   Keep browser I/O explicit through request/completion and event queues.
    Never retain a JavaScript view across an await or reenter borrowed Rust
    runtime state from a host callback.
*   Supply filesystem epoch time explicitly at the runtime boundary; raw
    `wasm32-unknown-unknown` has no operating-system clock. Validate new Rust
    browser subsystems by calling their WASM operations, since an unused
    implementation can build successfully while retaining unsupported calls.
*   Deploy boot payloads and split disks under content-derived names. Replace
    the configuration last as the atomic rollout and retain old assets until an
    explicit cleanup.

Development workflow
--------------------

1.  Trace the affected path end to end before editing: guest-visible behavior,
    Rust state and dispatch, raw ABI, JavaScript host integration, and image or
    guest configuration where applicable.
2.  Define one bounded subsystem or compatibility failure. Start from the
    specification and add focused tests for meaningful behavior, edge cases,
    malformed input, reset/lifetime behavior, and failure reporting.
3.  Implement the smallest direct change. Preserve the interpreter and TLB hit
    paths; place uncommon checks on writes, misses, or device paths when the
    architecture permits it.
4.  Validate native Rust first, then the WASM and JavaScript surfaces that can
    differ. For guest compatibility work, also exercise the real guest or a
    focused non-PIE firmware probe.
5.  Update documentation in the same change, then make one focused commit with
    an imperative one-line message.

Do not convert a PIE ELF with sections near zero and `0x80000000` directly to a
flat probe image: `objcopy` preserves the address gap and can create a
multi-gigabyte sparse file. Link probes as non-PIE firmware or extract the
intended loadable section explicitly.

Validation
----------

*   `make test-unit` runs Rust, Python tool, JavaScript adapter/server tests,
    and executable raw-WASM 9p namespace/protocol/transport, CPU, and deployed
    filesystem ABI probes in Node.
*   `make test` adds real WASM/Chrome network integration and 9p server tests
    for development.
*   `make check` adds strict Clippy and Python type checks. The GitHub release
    workflow runs unit tests, type checks, Clippy, and builds without Chrome or
    full-guest tests.
*   `make wasm` builds the deployed Rust WebAssembly artifact.
*   `make kernel`, `make opensbi`, and `make uboot` build the pinned guest
    components and their hash-named gzip assets. The default `make` builds
    those with the core WASM and JavaScript and packages the release archive.
    Image Makefiles build deployments by default and run profiles explicitly.

For CPU or platform milestones, run `make check` and rebuild WASM from a clean
tree. Current xv6 is the primary UART and supervisor-mode integration guest.
Prepared Alpine through OpenSBI is the primary Linux and platform guest. Run it
through discovery, root-media access, userspace startup, login, and shutdown.
U-Boot is additional coverage only when its boot path is relevant.

Documentation lifecycle
-----------------------

Documentation is part of every development milestone, not a later cleanup:

*   Update `AGENTS.md` when project scope, terminology, repository ownership,
    durable architecture decisions, workflow, or the current platform contract
    changes. Keep it sufficient to start a fresh task without rediscovery.
*   Update `README.md` when users gain or lose a feature, option, public API,
    setup step, image workflow, or supported use case.
*   Add completed user-visible work, compatibility changes, migrations,
    measurements worth preserving, and durable implementation decisions to the
    current `CHANGELOG.md` release section.
*   Keep active designs, milestones, TODOs, and out-of-scope discoveries in
    `DEV.md`. When work completes, remove its plan and move only lasting results
    to `AGENTS.md`, `README.md`, or `CHANGELOG.md` as appropriate.
*   Do not duplicate the same status narrative across files. Verify internal
    links and search for stale terminology and removed interfaces before the
    milestone commit.

Repository policy
-----------------

Track source and durable documentation only. Keep VM images, generated boot
assets, temporary probes, test configurations, and build outputs out of Git.
This repository is an explicit exception to the general read-only Git rule:
commit focused, high-confidence changes automatically after validation. Never
use `git checkout` or `git reset` to discard work.
