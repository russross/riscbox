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

### Block provider migration

Problem and evidence: the current runtime reads a configuration file and HTTP
drive manifests in Rust, and `HttpBlockStore` owns both the clean-block cache
and the session CoW overlay. A host byte array cannot receive guest writes
through that store. A block transport that crosses the WASM boundary on every
guest request may materially affect HTTP-backed guest performance, so retain
the existing HTTP path in parallel until a measured decision about
consolidation.

HTTP baseline from one headless Chrome run per workload, before migration:

*   Risclet login: 1.98 seconds from boot click, 21 block responses totaling
    11,010,048 bytes from 12 distinct block URLs. This run reused a Chrome
    profile after an earlier Risclet boot, so browser cache was warm.
*   Prepared Alpine login: 0.92 seconds from page startup, 10 block responses
    totaling 5,242,880 bytes from 10 distinct block URLs. The current Rust
    HTTP store was unchanged; the image included the new power handlers.
*   xv6 profile compile marker: 99.77 seconds from page startup, 159 block
    responses totaling 83,361,792 bytes from 157 distinct block URLs. The
    guest shell reported 98.80 seconds for the compile. This used a fresh page
    load of the existing xv6 distribution. These single runs do not establish
    run-to-run variance or a performance target by themselves.

The current block response path copies fetched bytes into a WASM allocation
and then into an owned Rust vector before the clean cache takes ownership.
`fetchMs` sums from the three probes were 2.02 seconds, 0.02 seconds, and
0.42 seconds respectively; overlapping requests make those sums unsuitable as
wall-time costs. Repeat fixed image and cache conditions when comparing the
new provider, including browser memory use and complete guest workload time.

Data and ownership:

*   Rust will keep validated machine configuration, machine construction,
    guest RAM, VirtIO descriptor validation, and device ordering. JavaScript
    will resolve configuration defaults,
    asset URLs, and host provider references before passing a resolved startup
    structure across the raw WASM ABI. A configuration URL remains a JavaScript
    convenience; callers may also provide the resolved structure directly.
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

1.  Add a TypeScript HTTP provider that parses existing split-image manifests,
    fetches clean blocks, retains bounded cache and session CoW semantics,
    and follows current cache headers. Compare actual guest results and the
    baseline above in Chrome. Keep both HTTP implementations until
    performance and memory behavior justify a separate consolidation decision.
2.  Add an array-backed TypeScript provider with direct write-through to a
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
