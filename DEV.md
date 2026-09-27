Riscbox development
===================

This file contains only current and future work. Completed implementation
history belongs in `CHANGELOG.md`; durable project constraints and current
architecture belong in `AGENTS.md`; user-visible behavior belongs in
`README.md`.

Development priorities
----------------------

1.  Keep current xv6 and deliberately prepared current Alpine images booting
    without emulator-specific guest patches. Let focused guest failures select
    compatibility work.
2.  Fill small, broadly useful RVA23 gaps when architectural probes and real
    guests can validate them. Vectors, hypervisor support, and multiple harts
    remain out of scope.
3.  Tighten QEMU `virt` device-tree and platform compatibility where firmware
    or Linux depends on it. Correct the existing platform instead of adding
    compatibility modes.
4.  Improve image preparation only when a small loader or boot feature removes
    material deployment friction. Raw kernels remain the QEMU preparation
    baseline; browser deployments use gzip-compressed kernels. Initrds remain
    opaque.
5.  Investigate the remaining active CPU time difference between the Rust
    platform with the TinyEMU core and the archived C platform using paired
    profiles of the same prepared guest image.

CPU core validation
-------------------

CPU test migration is complete. Architectural probes in
`tests/tinyemu_architecture.rs` run real guest instruction streams on
`tinyemu_core::Core`, with expected results checked through architectural
registers, trap state, or guest RAM. The former standalone Rust CPU, SoftFP,
and physical-memory implementations and their tests have been removed. Rust
interpreter fast-path comparisons were retired because their cache and polling
assertions described implementation details rather than guest behavior.

`tests/tinyemu_core.rs` covers C-owned RAM bounds, read-only mappings, dirty
page snapshots and clearing, instruction execution, and MMIO callbacks.
`tests/virtio_transport.rs` uses a small `MemoryAccess` adapter backed by
TinyEMU-owned RAM; it does not allocate a separate guest memory model. The
VirtIO device tests continue to use an unbooted `Machine` and fake host
backends.
The saved xv6 compile profiles in `images/xv6-profile/build/profiles/` show
105.332 seconds for `riscbox-scheduler` versus 97.400 seconds for `tinyemu`, an
8.1% longer sampled profile in the current Riscbox WASM build. Both logs reach
`XV6_PROFILE_BUILD_COMPLETE`; their guest timestamps at the following ext4
read-only remount are 104.075 and 96.130 seconds, respectively. The guest
workload and platform identification in the logs match. This is one paired
capture, so the size of the difference still needs repeated runs to establish
run-to-run variance.

The dominant sampled function is the TinyEMU `riscv_cpu_interp_x64` loop in
both profiles: 84.5% of Riscbox samples and 81.7% of TinyEMU samples. Riscbox
also attributes 4.4% to `pmp_access_ok`, 1.9% to `get_phys_addr`, and 0.7% to
`riscv64_read_slow`; together with other non-interpreter C frames these appear
to be costs around the same core rather than Rust platform or JavaScript
overhead. The archived TinyEMU profile reports most C work under the opaque
`wasm-function[275]` frame, so it cannot support a direct function-by-function
comparison. The idle share is similar (6.7% Riscbox, 7.8% TinyEMU), and file
buffer callbacks account for less than 0.5% in TinyEMU. Follow up by repeating
the paired capture and resolving/minifying symbol attribution for the archived
module before choosing an optimization target. Keep the guest workload fixed.

Candidate work
--------------

### Interrupt-controller modernization

The platform now describes its machine-local interrupt registers as ACLINT
MSWI and MTIMER. The PLIC remains the QEMU `virt` default and is used by the
current Linux and xv6 guests. Replacing it with APLIC or APLIC plus IMSIC would
add controller state, CPU interrupt-architecture support, and firmware and
guest compatibility work without a demonstrated one-hart benefit. Revisit only
if a target guest requires AIA. ACLINT SSWI similarly adds no useful IPI target
for the current single-hart machine.

### Framebuffer demonstration

Add guest framebuffer and input demonstration programs, configure those devices
in an image, and connect dirty-region callbacks to a canvas in the Risclet page.
Validate the complete guest-to-page path together. The current Risclet demo is
intentionally terminal-only.

### Boot and image loading

Evaluate a standard bootloader path and other image
features only against a concrete Alpine deployment problem. Linux already
handles compressed initrds after Riscbox loads them opaquely. Any new loader
must justify its code size and failure surface relative to image preparation.

### Non-CPU performance

Measure HTTP block request latency, 9p request/reply copy volume, concurrent
request latency, resident and logical 9p tree sizes, and peak lazy-load memory
only when those paths become a demonstrated bottleneck. Keep that work separate
from the CPU interpreter optimization baseline.

### WASM build rules

Apply and test various standard tweaks to the Rust WASM build rules to shrink
the binary and check for performance differences (including optimizing for
size).

### Bootloader and installation media

ISO boot remains deferred until a concrete guest image requires it. The
trimmed U-Boot build lacks ISO9660, El Torito, EFI, and FAT support, and the
custom Linux kernel lacks ISO9660. Revisit the required firmware, boot image,
filesystem, and storage path together against an actual ISO.

Supporting installation media also needs a separate storage milestone. Extend
the resolved drive configuration and host block providers described below for
plain image assets or installation media. Select read-only, session CoW, or
mutable behavior explicitly. Validate multiple attached drives, failed reads,
and write behavior before documenting ISO or installer boot as supported.

### Host controls and block providers

Problem and evidence: the current runtime reads a configuration file and HTTP
drive manifests in Rust, and `HttpBlockStore` owns both the clean-block cache
and the session CoW overlay. A host byte array cannot receive guest writes
through that store. Guest poweroff currently makes the VM inactive; the
finisher does not handle the QEMU `virt` reset value. The host has no power
request, forced halt, in-place reset, or destroy control. A block transport
that crosses the WASM boundary on every guest request may materially affect
HTTP-backed guest performance, so retain the existing HTTP path in parallel
until a measured decision about consolidation.

Data and ownership:

*   Rust owns validated machine configuration, machine construction, guest RAM,
    VirtIO descriptor validation, device ordering, run state, and reset of CPU
    and platform interface state. JavaScript resolves configuration defaults,
    asset URLs, and host provider references before passing a resolved startup
    structure across the raw WASM ABI. A configuration URL remains a JavaScript
    convenience; callers may also provide the resolved structure directly.
*   Distinguish `running`, `halted`, and `destroyed` lifetimes. Guest poweroff
    and forced host halt retain the machine and host connections. Guest reboot,
    forced reset, and boot after halt run one machine reset path. Destroy
    releases the machine and connectors. Report lifecycle cause separately
    from state; a soft request completes when delivered, not when the guest
    acts on it. Only an observed guest shutdown or reboot establishes that the
    guest had an opportunity to flush and unmount.
*   Separate each device's resettable guest-facing interface from its durable
    host resource. Reset clears queues, interrupts, pending requests, and old
    completion generations. Block contents, HTTP CoW overlays, and 9p server
    data survive reset. A 9p session may close and reopen against the same
    server. Console connectors survive and receive terminal reset and screen
    clear notifications. Destroy closes host resources. Specify reset and
    close methods for applicable provider and connector APIs.
*   Add a generic asynchronous block transport alongside `HttpBlockStore`.
    Rust validates sector alignment, capacity, descriptor shape, and returned
    read length, then passes read/write sector requests to JavaScript and
    completes VirtIO status. The new TypeScript HTTP provider owns its cache
    and CoW overlay. The array provider reads and writes a host-supplied byte
    array directly, without another cache or overlay. Provider failure must
    complete the guest request with an I/O error; reset retires late replies.
    Do not promise a consistent host export until the guest has shut down or
    otherwise completed its own filesystem synchronization.

Milestones and acceptance, in dependency order:

1.  Establish lifecycle semantics and baseline measurements. Trace guest
    shutdown and reboot through OpenSBI, the SiFive test register, machine
    status, raw ABI, and adapter. Record HTTP block request counts, bytes,
    copies, host elapsed time, and guest workload time for fixed xv6 and
    prepared Alpine workloads. Define reset retention for each attached device
    and identify the guest OS mechanism for soft power and reboot requests.
2.  Implement reset and halt without changing block transport. Add the QEMU
    `virt` reset value and device-tree reset binding, a scheduled machine reset
    outside MMIO callbacks, explicit device interface resets, lifecycle
    events, host forced halt/reset, boot after halt, and destroy. Add guest
    poweroff and reboot coverage, including late 9p/HTTP completions and
    retained disk data. Add soft power and reboot requests only after a guest
    probe confirms that the prepared userspace handles the chosen signal;
    document that request acceptance does not imply shutdown.
3.  Refactor startup configuration at the JavaScript/Rust boundary. Define a
    typed resolved configuration in `src/config.rs` and an ABI transfer in
    `src/browser_abi.rs` and `riscbox-wasm/src/lib.rs`. Move file parsing,
    defaults, and relative URL resolution to JavaScript while Rust validates
    the received values and builds the machine. Preserve existing deployed
    configuration behavior and direct native Rust machine construction. Test
    missing/invalid values, device ordering, and browser asset loading.
4.  Add the parallel generic VirtIO block connector in `src/virtio_devices.rs`,
    `src/machine.rs`, `src/browser_runtime.rs`, the raw ABI, and
    `js/riscbox.js`. Define typed request ID, device ID, generation, sector,
    length, result, reset, and close operations. Keep the old HTTP store and
    configuration route available. Test reads, writes, malformed replies,
    failures, multiple drives, reset during I/O, and WASM memory ownership.
5.  Add a TypeScript HTTP provider that parses existing split-image manifests,
    fetches clean blocks, retains bounded cache and session CoW semantics,
    and follows current cache headers. Compare actual guest results and the
    baseline from milestone 1 in Chrome. Keep both HTTP implementations until
    performance and memory behavior justify a separate consolidation decision.
6.  Add an array-backed TypeScript provider with direct write-through to a
    host-supplied byte array. Define alignment, capacity, partial final block,
    and ownership of the array at construction. Test host-visible writes,
    preserved bytes across halt/reboot, and teardown. Validate orderly guest
    shutdown before exporting a writable filesystem image.

Cross-cutting acceptance: run `make check`, build clean WASM, exercise headed
or headless Chrome with a temporary profile, and boot the relevant real guests
at each platform milestone. Update `AGENTS.md`, `README.md`, and `CHANGELOG.md`
as behavior lands; remove completed steps from this plan. Keep the QEMU-like
distinction between soft guest requests and forced host actions. Defer a block
flush feature until a guest or persistence contract requires it; kernel write
back is the guest's responsibility. Suspend/resume and adjustable CPU idle
policy remain separate future work.

### WASI integration

Examine what the WASI platform offers and see if deeper support/integration is attractive or not.

### Debugger support

Add qemu-like debugger hooks. This would probably require a lot of planning and work, but a potential use case would be debugging an xv6 kernel, maybe even from another riscbox instance running linux on the same page.

Design and milestone format
---------------------------

Add an active design here before implementing work that spans subsystems. Keep
it short and current, using these sections as needed:

*   Problem and evidence: the guest failure, deployment need, or measurement.
*   Scope and exclusions: the bounded behavior and explicit non-goals.
*   Data and ownership: types, state, lifetimes, storage, and concurrency.
*   Flow and interfaces: files, function signatures, ABI or configuration
    changes, and error handling.
*   Milestones and acceptance: ordered increments with focused tests, WASM
    checks, guest validation, and measurements.

Remove completed milestones rather than accumulating checked-off plans. Move
lasting decisions and results into the appropriate current or historical
document during the same commit.

Deferred findings
-----------------

Record an out-of-scope issue here when development uncovers it. Include the
observed behavior, affected guest or interface, evidence or reproduction, why
it is deferred, and the condition that should trigger reconsideration. Remove
the entry when it becomes an active design or is resolved.

No deferred findings are currently recorded.
