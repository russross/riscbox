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
*   `riscbox-wasm/` supplies the small internal raw WASM export surface. Keep
    unsafe ABI code isolated there. Main-crate unsafe code is confined to the
    TinyEMU FFI module.
*   The workspace version in root `Cargo.toml` is inherited by both Rust
    crates. Every push to `main` runs Release, which stops if the current version
    tag exists. Otherwise it checks and builds the core before creating the tag
    and publishing one GitHub
    release archive with the WASM runtime, browser modules, canonical Linux
    Image, OpenSBI firmware, U-Boot binary, and API documentation. The release
    archive excludes guest images and image build scripts.
    Failed checks leave the version untagged for the next main push to retry.
    Publication calls the separate reusable Demo workflow, which also accepts
    manual runs on main. Demo requires the published release matching its
    checked-out workspace version, then builds and Chrome-tests the demo against
    that unchanged archive before deploying Pages. Release and Demo have
    separate serialized queues. Deployment skips superseded runtime versions;
    manual Demo runs also check that their source commit remains current.
    Pages uses the GitHub Actions source and
    the `github-pages` environment. Builds have read-only repository access;
    separate publication and deployment jobs receive their write permissions.
*   `js/riscbox.js` owns the dependency-free client adapter and its private
    runtime. The client factory exposes checked lifecycle/input calls and owned
    storage facades; raw exports, scheduling, and buffer helpers are internal.
*   `js/network/` is the typed WebSocket Ethernet frontend and protocol.
*   `js/storage.ts` supplies synchronous filesystem and copied disk facades.
    `tools/build_adapter.mjs` combines it with `js/riscbox.js` in one private
    scope for deployable `build/js/riscbox.js` and declarations. Public storage
    interfaces omit creation/polling/invalidation; internal `FilesystemHandle`
    and `DiskHandle` own those operations. Development probes use the separate
    `build/js/riscbox-internal.js`, which is never packaged. `src/block_storage.rs`
    unifies Rust-owned array and split HTTP stores, HTTP requests, bounded
    clean cache, and sparse sector overlays.
*   `src/ninep.rs` owns the resident namespace and `SharedFilesystem` ownership;
    `src/ninep_protocol.rs` owns wire parsing, 9P2000.L sessions, and concrete
    `NinePEndpoint` device ownership. VirtIO 9p holds that endpoint directly:
    no backend trait, asynchronous completions, or host transport actions.
    Copied host storage APIs live in `src/filesystem_abi.rs` and
    `src/block_abi.rs`; their guide is `STORAGE-ABI.md`. The protocol contract
    is in `NINEP.md`; active coordination belongs in `DEV.md`.
    Keep Rust source modules flat and consolidate helpers with their owning
    architectural boundary instead of adding nested implementation modules.
*   `demo/` is a release-only embedding example with its own Alpine image and
    plain browser app. Its Makefile stages the unchanged release tree and uses
    only packaged runtime, boot payloads, and splitter assets. Historical image
    projects and the shared application client live outside this repository.
    Assembly links to GitHub-rendered guides and example provenance at the
    archive version tag; local source builds use their source commit.
*   Root `package.json` owns the pinned build-only TypeScript compiler.
    Core tests own their Chrome harness and require no mounted client source.
*   `bench/` owns a separate manual-only Alpine benchmark image and public-client
    Chrome runner: integer loop, in-memory SQLite, and serial TinyCC game builds
    on ext4. It warms immutable chunks in Chrome's HTTP cache, requires cache-only
    transport during execution, retains the normal Rust block cache/overlays,
    uses a fresh VM per measured sample after a separate warm-up, and drops
    Linux caches before workloads. Runtime selection and saved JSON
    comparisons are independent of compiler settings. Generated fixtures,
    results, and optional per-workload CPU profiles are ignored and never
    packaged. No benchmark target belongs to ordinary builds, tests, acceptance,
    or CI.
*   `kernel/` owns the canonical custom Linux kernel consumed by image builds.
*   `opensbi/` and `uboot/` own pinned firmware and bootloader builds. Each
    tracks its Makefile, version, and config; downloads, sources, and outputs
    are ignored. The release packages both firmware and the Linux next stage. OpenSBI's
    `defconfig` selects the one-hart Riscbox SBI services and FDT drivers.
*   `README.md` owns overview/scope, `API.md` client call contracts, and
    `HOWTO.md` narrated application/deployment workflows. Those are packaged
    alongside `STORAGE-ABI.md` and `NINEP.md` implementation references.
    `BUILDING.md` covers contributor setup and explicit local checks. `DEV.md`
    holds active plans/future work; `CHANGELOG.md` is the historical record.

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
content-hash-named boot and disk assets and `no-cache` for WASM and configuration.
The browser adapter parses configuration files, supplies defaults, and
resolves boot and drive URLs through the single `Riscbox.prepare(options)` factory,
which accepts config URL, text, or object sources, optional per-slot block source
overrides, callbacks, and named preparation options. It fetches WASM beside the
adapter by default and uses `WebAssembly.instantiateStreaming`. Rust validates
the resolved configuration and still constructs the machine. Native Rust
configuration loading remains for development tests.
The client has no `start()` alias or raw startup entry points; documentation
describes the current release without cross-release compatibility promises.
Public client state is `halted`, `running`, or terminal `destroyed`. Preparation
returns only after constructing a halted VM and cleans up failures. An optional
`AbortSignal` cancels preparation before a client is returned.
Boot requires halt, forced reset requires running, and cold reset requires halt;
queued controls check prerequisites at dispatch. Device input checks scalar
ranges and device/state prerequisites before WASM conversion. Network carrier
may precede boot, while frames outside execution are dropped.
Each runtime owns a streaming console UTF-8 decoder: halt flushes a truncated
sequence, and reset/destroy discard its tail. Other text decoding is stateless.
HTTP block stores start with a 16 MiB in-memory cache limit that grows to
cover a single request when needed.
Rust owns every disk and share in the VM. `Riscbox.prepare()` loads and constructs
the platform without booting; 9p population and `boot()` remain separate.
Configured drives select HTTP manifests, copied host `Uint8Array` bytes, or
zeroed `capacity_sectors` arrays. HTTP and array disks occupy configured order.
Array writes update Rust bytes directly. HTTP writes use 4 KiB overlays with
per-sector dirty masks and never fetch unwritten sectors. Reads are synchronous
when resident and request/completion based when HTTP chunks are missing. The
optional `fetchBlock` hook replaces immutable chunk transport only; Rust retains
cache, validation, deduplication, errors, ordering, and lifetime ownership.

Host disk access requires halted state, including preboot and forced halt. Host reads
return copied bytes or promises for HTTP misses; writes are synchronous. Boot
rejects pending host reads. Reboot retains bytes and CoW. Halted disk
`reset()` removes HTTP overlays while retaining cache and rejects array disks; `coldReset()`
retires reads/fetches, clears guest RAM, and reloads boot images while preserving
stores. Destroy frees all storage and invalidates every facade and client;
replacement requires a fresh `Riscbox.prepare()` call. Export consistency
requires orderly guest shutdown.

The host can deliver soft shutdown and reboot input events, force an immediate
halt or reset, boot a halted machine, and destroy a halted machine. The prepared
demo guest uses BusyBox `acpid` to turn the two input events into
orderly userspace actions. Guest poweroff, failure, and forced halt reset all
guest device interfaces, queues, interrupts, timers, fids, and locks while
retaining hardware configuration, RAM bytes, and backing stores. Queued guest
input and I/O are retired; final console output precedes the halt notification.
Boot restarts from the reset entry point rather than resuming execution. Guest reboot
uses the QEMU `virt` syscon reset value. In-place reset restarts the C CPU,
reloads boot images, and clears platform and VirtIO interface state while
retaining host backends, 9p servers, HTTP clean cache and CoW data, and guest
RAM mappings. Resident 9p replies complete synchronously; pending HTTP requests
are retired without reusing request IDs. Console and framebuffer host callbacks
receive reset notifications. Destroy releases the machine and 9p sessions.
Preparation cancellation releases partial machines and retires pending HTTP
responses. Destroyed clients cannot be reused. Browser
HTTP completions and errors are guarded by the VM lifecycle generation.

The embedding demo loads the packaged Linux kernel through OpenSBI, mounts a
writable 80 MiB ext4 root without tmpfs overlays, and autologins user `riscbox`
on the VirtIO console. BusyBox acpid handles orderly power events. QEMU setup
passes Linux `riscv_isa_fallback` for legacy CPU ISA device-tree bindings from
older QEMU versions, including the release runner's QEMU 8.2.
It installs TinyCC, its static runtime, musl headers, make, doas, and small tools.
Five self-contained BSD-GAMES 3.3 projects (adventure, atc, robots, snake, and
spirhunt) in `demo/bsd-games-3.3/` are
vendored and distributed as selectable source trees. Alpine includes ncurses
development files and terminal data.
QEMU preparation builds every game; Chrome acceptance compiles each on resident
9p and checks terminal startup in the real WASM emulator.
The host populates a resident `shared` share mounted at `/shared`, owned by
UID/GID 1000. The indented file tree subscribes to host and guest changes.
Selecting a file opens it immediately; editor changes sync on blur or after
30 seconds of inactivity. External changes discard buffered edits immediately;
deleted files clear and lock the editor. Change pulses mark affected rows and
the editor pane. Empty and binary-file editor selections are blank and gray.
The VM status legend shows 1/5/15-second active-time speed averages and clock/CPU
uptime, updating once per second while running, freezing at halt, and resetting
on boot. Source selection clears and repopulates the live share, preserving root
identity and active fids, with no halt requirement. Full tree clears remove rows
immediately; new entries pulse when loaded.
The app uses pinned CDN xterm.js, fit/WebGL addons, and CodeMirror without an
application build step. Draggable panes start at 10/45/45 percent for the tree,
editor, and terminal; a horizontal gutter separates collapsed details below.
Image reset preserves the share and source selection leaves the disk alone.
Terminal input retries partial FIFO acceptance and retires on lifecycle changes;
boot/reset messages remain in terminal history. Demo validation is opt-in.
The terminal starts unfocused. Two independent background transfers warm the
browser cache with immutable 256 KiB image chunks. Terminal focus pauses new
prefetch requests without cancelling active transfers; blur resumes remaining chunks.

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
Split HTTP disks use 512 KiB chunks by default.

The public `speed()` API reports 1/5/15-second active-time Mcycles/s averages,
clock uptime, and cycling uptime in seconds. Rust owns one deque of completed
quantum samples and rolling totals for each window, retaining the whole oldest
sample needed to cover a window. Short histories use available samples. The
adapter supplies monotonic quantum durations and measures clock uptime.
Collection is independent of diagnostics. Every boot/reset clears measurements;
quanta spanning guest reboot are omitted. Halt freezes all five values, and
cold reset retains them until boot.

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
Whole-tree `clear()` recursively unlinks root contents while running or halted,
preserving root identity, active fids, and locks. Files and directories use
separate namespace link and fid reference counts; detached inodes and their
quota usage survive until the last fid closes. Filesystem `reset()` requires
halted state and replaces all namespace content and root identity. Device reset
closes protocol state and retains bytes; destroy releases every tree. Both host
filesystem operations preserve facades and subscriptions. The adapter uses
copied packets and change events, with no source loader, independent
creation/binding, or promise-based
filesystem interface. Do not restore removed `file`, `socket`, or `js9p` forms.

Linux can retain pathname fids through dentries even with `cache=none`. Clear
does not revoke those fids: metadata can still describe detached inodes while
fresh directory enumeration describes the cleared namespace. Host change
notifications do not invalidate guest caches.

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
    Inspect `Cargo.lock` before changing the resolved dependency graph.
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
    for development, with small owned firmware probes.
*   `make check` adds strict Clippy and Python type checks. The GitHub Release
    workflow runs unit tests, type checks, Clippy, package validation, and
    builds, plus QEMU demo image preparation and real Chrome demo acceptance.
*   `make check-release` explicitly runs core checks, builds the archive, checks
    named contents and documentation links, excludes development artifacts, and
    instantiates its packaged WASM through its packaged client adapter. Typed
    example consumers compile against deployable declarations in `js-check`.
    GitHub runs these checks for untagged releases; tagged main pushes skip
    checks and ordinary PR checks are not automatic.
*   `make test-demo` builds the optional release-only example and checks real
    Chrome/WASM boot, TinyCC games, automatic host/guest file synchronization, lifecycle
    controls, image/share reset, and destroyed facade invalidation. Browser tests
    use temporary profiles, headed Chrome when a display is available, and
    headless Chrome otherwise. `make demo` builds without running acceptance.
*   `make wasm` builds the deployed Rust WebAssembly artifact.
*   `make bench-image`, `make bench`, and `make bench-profile` explicitly prepare
    or run the manual performance suite. See `bench/README.md` for fixed workload
    counts, arbitrary runtime inputs, cache policy, results, and comparison.
    GitHub workflows must never invoke these benchmarks.
*   `make kernel`, `make opensbi`, and `make uboot` build the pinned guest
    components and their hash-named gzip assets. The default `make` builds
    those with the core WASM and JavaScript and packages the release archive.
    The optional demo has an independent make-driven image preparation workflow.

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
*   Update the owning distribution guide when users gain or lose a feature:
    `README.md` for scope, `API.md` for contracts, `HOWTO.md` for workflows.
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
