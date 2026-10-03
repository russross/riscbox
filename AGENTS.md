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
    HTTP-backed disks, and resident Rust 9P2000.L filesystems shared with the host.

The optional TypeScript network adapter carries one Ethernet frame per binary
WebSocket message to a host-selected origin endpoint. Browser network devices
use per-VM locally administered MAC addresses, expose carrier through VirtIO
status and configuration interrupts, and bound pending frames and bytes. The
repository supplies a local Node protocol stub for real WASM/Chrome tests but
no production network origin service, native TAP backend, or SLIRP backend.

RV32, multiple harts, vectors, the hypervisor extension, PCIe, AIA,
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
*   `js/storage.ts` supplies synchronous filesystem and copied disk facades.
    `tools/build_adapter.mjs` combines it with `js/riscbox.js` into the single
    deployable `build/js/riscbox.js` and declaration file. `src/block_storage.rs`
    unifies Rust-owned array and split HTTP stores; `src/browser_storage.rs`
    owns HTTP requests, bounded clean cache, and sparse sector overlays.
*   `src/ninep.rs` and `src/ninep/` own the standalone Rust namespace;
    `src/ninep_protocol.rs` and `src/ninep_protocol/` own its 9P2000.L session.
    `src/ninep_backend.rs` connects VM-owned trees to VirtIO. The raw copied
    storage APIs live in `src/browser_abi/ninep.rs` and `block.rs`; their guide
    is `src/browser_abi/ninep/README.md`. The protocol contract is in
    `src/ninep_protocol/README.md`; active coordination belongs in `DEV.md`.
*   `images/` contains reproducible Makefile-driven image definitions and deployment tooling.
    Generated downloads, images, boot assets, and distributions are not source.
*   `client-core/` is a read-only sshfs mount of Exam's canonical shared browser
    editor, terminal, file views, namespace snapshots, and VM lifecycle source.
    Risclet compiles it directly. Keep application navigation, downloads,
    submissions, grading, and clipboard policy outside it. Its README defines
    the runtime/build contract; `EXAM-MIGRATION.md` is a self-contained handoff.
    Shared fixes and dependency installation belong in Exam's canonical source.
    Type-only Riscbox aliases resolve through the consumer's build configuration
    to the locally built adapter declarations. Shared tests require a writable copy.
*   `kernel/` owns the canonical custom Linux kernel consumed by image builds.
*   `opensbi/` and `uboot/` own pinned firmware and bootloader builds. Each
    tracks its Makefile, version, and config; downloads, sources, and outputs
    are ignored. The shared image helpers use `opensbi/fw_dynamic.bin`, and
    Risclet uses `kernel/linux` as OpenSBI's S-mode next stage. OpenSBI's
    `defconfig` selects the one-hart Riscbox SBI services and FDT drivers.
*   `README.md` is user-facing documentation. `DEV.md` holds only active plans,
    future work, and deferred findings. `CHANGELOG.md` is the historical record.

In this repository, "native" means a Rust test or image-preparation execution
environment. It does not imply a supported native emulator. "Browser runtime"
means the Rust machine, raw WASM ABI, and JavaScript adapter together. A "9p
server" implements protocol sessions over a wholly resident namespace.
Applications populate their named shares through the synchronous host API.

Current contract
----------------

The CPU implements RV64 I, M, A, F, D, C, and the advertised scalar extensions
needed by the target guests. This includes the implemented B subsets, current
counter and supervisor guarantees, Sstc, Svadu, Svinval, Svnapot, Svpbmt,
cache-block operations, conditional operations, hints, may-be-operations, and
wait-on-reservation. It is moving toward RVA23 where that is useful, but it is
not RVA23 compliant because vectors and several other required extensions are
intentionally absent. Advertise only implemented behavior.

Production CPU runs enter the TinyEMU C instruction loop. C owns CPU state,
TLB, physical mappings, and RAM; its setup and teardown allocations use the
Rust global allocator. Rust owns platform devices and handles MMIO callbacks.
The interpreter accumulates retired instructions locally and publishes the
counter at CSR accesses and CPU-run exits. WASM RV64 high-half multiplication
uses 32-bit limbs and signed corrections; native builds use wide arithmetic.
PMP CSRs retain read/write masks and lock behavior for guest firmware, but
PMP permissions do not restrict memory accesses, page walks, or TLB fills.
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
The supplied U-Boot supports FAT and EFI bootflow scanning, GPT EFI partitions,
and El Torito FAT boot images on 512-byte VirtIO block media. RISC-V ISO boot
uses `/EFI/BOOT/BOOTRISCV64.EFI`; GRUB or another EFI application reads the
ISO9660 tree and loads the media's kernel and initramfs. Alpine standard
3.24.2 riscv64 boots through login and shutdown in the actual browser runtime.
The custom Linux Image includes its EFI stub, compressed initramfs loading,
FAT/VFAT, ISO9660 with Rock Ridge/Joliet, loop devices, and SquashFS with
zlib/XZ/Zstandard. ISO media uses existing split HTTP or VM-owned array disks.
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
Rust owns every disk and share in the VM. `prepareResolved` and `prepareFromUrl`
load and construct the platform without booting; startup helpers prepare then
boot. Resolved drives select HTTP manifests, copied host `Uint8Array` bytes, or
zeroed `capacity_sectors` arrays. HTTP and array disks occupy configured order.
Array writes update Rust bytes directly. HTTP writes use 4 KiB overlays with
per-sector dirty masks and never fetch unwritten sectors. Reads are synchronous
when resident and request/completion based when HTTP chunks are missing. The
optional `fetchBlock` hook replaces immutable chunk transport only; Rust retains
cache, validation, deduplication, errors, ordering, and lifetime ownership.

Host disk access requires poweroff, including preboot and forced halt. Host reads
return copied bytes or promises for HTTP misses; writes are synchronous. Boot
rejects pending host reads. Reboot retains bytes and CoW. Powered-off
`discardChanges()` removes HTTP overlays while retaining cache; `coldReset()`
retires reads/fetches, clears guest RAM, and reloads boot images while preserving
stores. Destroy frees all storage and invalidates every facade; a runtime may
prepare another VM. Export consistency requires orderly guest shutdown.

The host can deliver soft shutdown and reboot input events, force an immediate
halt or reset, boot a halted machine, and destroy a halted machine. The prepared
Alpine and Risclet guests use BusyBox `acpid` to turn the two input events into
orderly userspace actions. Guest poweroff halts without teardown; guest reboot
uses the QEMU `virt` syscon reset value. In-place reset restarts the C CPU,
reloads boot images, and clears platform and VirtIO interface state while
retaining host backends, 9p servers, HTTP clean cache and CoW data, and guest
RAM mappings. Resident 9p replies complete synchronously; pending HTTP requests
are retired without reusing request IDs. Console and framebuffer host callbacks
receive reset notifications. Destroy releases the machine and 9p sessions.
Destroy also cancels config/asset startup, retires its pending HTTP response,
and permits reuse of the runtime after invalidating its storage handles. Browser
HTTP completions and errors are guarded by the VM lifecycle generation.

The Risclet demo loads the custom Linux kernel directly through OpenSBI. Its
single EROFS disk is the read-only root filesystem; tmpfs supplies `/tmp` and
the writable overlay layers for `/var` and `/home`. The device tree model
identifies the platform as `riscbox`; QEMU names remain in functional board
bindings and build targets. Linux uses UART early and the VirtIO console for
login.
Risclet uses one VM and one share across examples. It downloads complete file
bodies before boot and caches original bytes in the application. Switching
flushes the editor, requests orderly guest shutdown, snapshots the outgoing
namespace, restores the incoming example's snapshot or originals, and boots
the same VM with retained disk overlays. Application-memory snapshots retain
bytes, directories, symlinks, hard links, permissions, ownership, and access/
modification times; restored inode identities and ctime are new. Reset forces
halt, cold-resets, discards the HTTP overlay, restores current-example originals,
and boots. Reset can interrupt pending orderly shutdown or reboot.
Reboot requests an orderly guest reboot retaining edits and storage.
Filesystem operations and subscriptions are synchronous with numeric
origin filtering; notifications run after Rust borrows end.
The editor buffers changes until blur, file selection, VM interaction, Sync,
or a thirty-second fallback timer restarted by each edit. Writes acknowledge
only their submitted revision; failures retain dirty text and retry. Conflicting
filesystem changes require a discard decision before replacing dirty text.
Instruction subscriptions track referenced images as well as the document.
Terminal input batches copied bytes, retries partial FIFO acceptance, and
retires queued and pending input on reset, halt, teardown, or runtime failure.
The terminal uses pinned Wterm 0.5.4 with its Ghostty core, DOM rendering, 18px
Latin Modern Mono, and a 64 KiB history budget. Guarded build loaders adapt
viewport clipping and connected box strokes. Browser rendering tests cover
fractional scaling, partial-row clearing, retained history, and idle rendering.

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
supervisor, or RTC deadline. C also exits after MMIO requests that queue HTTP block work; resident 9p requests finish within
the notifying CPU run. Rust releases the exclusive CPU-run borrow before JavaScript
dispatches actions. JavaScript resumes the same quantum after HTTP dispatch and wakes a WFI
sleeping guest on later completions. JavaScript measures
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
    active quantum to JavaScript for queued device actions and HTTP requests.
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

VirtIO 9p completes resident requests synchronously within the CPU run. Rust
validates descriptors and envelopes; protocol sessions own fids and locks.
Preparation creates one tree per configured server name and an independent
session per tag. Host access works before boot, while running, and after halt.
Whole-tree `clear()` requires poweroff. Reset closes protocol state and retains
bytes; destroy releases every tree. The adapter uses copied packets and change
events, with no source loader, independent creation/binding, or promise-based
filesystem interface. Do not restore removed `file`, `socket`, or `js9p` forms.

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

*   `make test-unit` runs Rust, Python tool, JavaScript adapter/facade tests,
    and executable raw-WASM 9p namespace/protocol/transport, CPU, and deployed
    filesystem ABI probes in Node.
*   `make test` adds real WASM/Chrome network integration and 9p server tests
    for development, plus ISO boot coverage when `RISCBOX_ALPINE_ISO` is set.
*   `make check` adds strict Clippy and Python type checks. The GitHub release
    workflow runs unit tests, type checks, Clippy, and builds without Chrome or
    full-guest tests.
*   `make test-images` rebuilds Risclet, Alpine, and xv6 profile distributions,
    runs native Alpine acceptance, and exercises the deployed Risclet UI and
    guest in Chrome. It covers synchronous host/editor/guest changes, application
    download failure/retry, notifications, retained reboot, clean reset, shutdown,
    and switching during example downloads. Browser tests use temporary profiles and normal
    event-loop timing; they use headed Chrome when a display is available.
*   `make wasm` builds the deployed Rust WebAssembly artifact.
*   `RISCBOX_ALPINE_ISO=/path/to/alpine-standard-riscv64.iso cargo test --release
    --test platform_acceptance alpine_iso_boots_through_efi_and_shuts_down --
    --ignored` exercises the ISO's EFI loader, live userspace, ISO9660/FAT
    reads, and shutdown. With the same environment variable, run
    `node --test tests/alpine_iso_browser.test.mjs` for real Chrome/WASM coverage;
    `RISCBOX_ISO_TRANSPORT=http` selects split HTTP rather than host-array media.
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
