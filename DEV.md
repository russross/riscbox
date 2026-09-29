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
The saved xv6 compile profiles in `images/xv6-profile/profiles/` show
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

Active work
-----------

### Rust 9p server migration (active)

This section is the coordination point for the migration. Update its status,
decisions, and validation record as each milestone completes. Move the lasting
contract to `AGENTS.md`, `README.md`, and `CHANGELOG.md` at completion, then
remove this active plan.

#### Status and scope

The TypeScript server in `js/p9/session.ts` and `js/p9/filesystem.ts` still
serves production guests. The committed VirtIO seam, `c1f1b45` (`Allow
immediate 9p backend replies`), permits `NinePBackend::submit` to return an
immediate `NinePOutcome`; `NinePDevice::notify` completes its descriptor before
returning in that case. The existing browser backend remains asynchronous.
Milestone 1's corrective pass is complete in `src/ninep.rs`. Its validation
includes 22 namespace tests, five VirtIO 9p transport tests, `make check`, and
a release WASM build in a fresh target directory. The standalone namespace
probe executes in both Node and Chrome with no operating-system imports.
The worktree's `src/ninep_protocol.rs` is an unfinished
milestone 2 slice that currently handles only version, attach, and walk; it
is not part of the milestone 1 commit. Neither module is attached to the VM,
raw ABI, or browser adapter. Previous build-only WASM validation missed the
clock trap because the namespace was not called. The corrective gate executes
a standalone exported namespace operation in Node and Chrome. Do not treat
the Rust server as supported yet.

The goal is a Rust-owned 9P2000.L filesystem and protocol engine with a small
browser facade. A filesystem exists before VM boot and remains usable while
the VM is halted. It may attach to one VM at a time; multiple 9p endpoints in
that VM may share its tree but have independent fids, tags, locks, and protocol
generations. The host API always returns promises. Seed plugins declare the
namespace and supply asynchronous bodies only when a host or guest reads an
unloaded file. There is no explicit or automatic preload mechanism.

#### Core request and reset contract

`NinePBackend::submit` returns an immediate outcome for resident work or
retains a request that needs an external load. An immediate reply completes
the VirtIO descriptor within the same CPU run. A pending load emits a host
action only after the exclusive CPU-run borrow ends; JavaScript copies its
source key and starts the plugin promise after returning from WASM. Its
completion reenters through a raw ABI export carrying a filesystem generation,
endpoint generation, and load/request ID. Rust checks all identities before
using the bytes or completing retained descriptors. Guest and host reads of
one unloaded inode share one load. A write, unlink, reset, flush, or retry
cannot let an older completion restore stale content or complete a descriptor
twice. JavaScript never calls back into borrowed Rust state.

VM reset and reboot retain the namespace while retiring active protocol state
and pending guest requests. Halt and orderly shutdown preserve machine/device
state and protocol state; they do not reset VirtIO queues. Filesystem reset replaces
the namespace, retires every endpoint session and load, and emits a reset
notification even if the VM is running. The running guest may need to remount
because its fids are invalidated. VM destroy releases the attachment but need
not destroy the independently held filesystem. These operations need separate
generations and focused tests; the current `Filesystem::reset` alone does not
implement the complete lifecycle contract.

#### Specification and browser boundaries

Use the [9P2000.L reference](https://github.com/chaos/diod/blob/master/protocol.md)
and its linked Plan 9 operation specifications as the protocol oracle.
TypeScript differential tests detect migration differences but must not retain
its partial-walk fid installation or its lock ownership/unlock defects.
`Tlock`/`Tgetlock` use process/client ownership and
[fcntl interval semantics](https://man7.org/linux/man-pages/man2/fcntl_locking.2.html);
session IDs identify cleanup provenance. Namespace time is explicit epoch
seconds: `Filesystem::new(limits, now)` and `set_time(now)` take runtime values,
never an operating-system clock import. Loading unchanged bytes preserves
attributes and QID version; mode/ownership changes affect ctime, not mtime.
Manifest timestamps are applied after tree assembly.
Only `LoadStart::Started(ticket)` dispatches a source; joined readers await
the same ticket.

The [VirtIO specification](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html)
defines descriptor ownership, used lengths, and device reset. Same-call
completion is our implementation choice, not a VirtIO ordering requirement.
Protocol flush suppresses the old reply and retires its descriptor with zero
used bytes before reporting `Rflush`. Device reset discards old pending chains;
late replies must never write into a reset queue. Filesystem reset is not a
device reset: its endpoint coordinator must retire still-owned requests and
invalidate fids without pretending the guest reset its queues. Outstanding
unflushed requests receive tagged errors; silent suppression belongs to flush,
not to namespace replacement while the device remains active.

Raw [wasm32-unknown-unknown](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html)
has no implicit host clock. Supply epoch time at
each synchronous activation, separately from monotonic quantum measurements.
Promise results, plugin dispatch, and notification listeners run after the
WASM activation returns; no host callback reenters borrowed Rust state and no
JavaScript memory view survives an await. A resident guest operation does not
cross into JavaScript. A pending result needs an explicit completion/retirement
queue because a flush or version request can finish earlier requests.

The namespace records a typed host origin, guest origin, or loader origin.
Reverse directory links resolve hard-link paths without scanning unrelated
files. The event queue holds at most 1024 detailed records; overflow yields a
`Rescan` invalidation, distinct from filesystem reset. Disable change tracking
when no host subscribes; enabling it invalidates the old view. Directory rename
records invalidate the old/new path prefixes. Host file writes and renames
require existing parents; seed installation creates implicit parents. The
browser migration must preserve editor origin filtering with numeric origin IDs.

#### Remaining milestones

Milestone 1 is complete; milestone 2 is in progress; milestones 3–7 have not
started. Advance these statuses here when a milestone passes its stated tests
and is committed.

2.  **Complete the 9P2000.L session.** Port the operation matrix in
    `js/p9/README.md` to `src/ninep_protocol.rs`: version, flush, attach,
    walk, open/create, read/write, clunk, statfs, attributes, readdir, fsync,
    symlink/readlink, mkdir, link, rename/unlink, and locks. Keep documented
    unsupported operations as `EOPNOTSUPP`. Model an immediate reply,
    suppression, or pending load explicitly; retain tag and fid state per
    endpoint. Parse and validate a complete request before state changes;
    bound replies by the negotiated message size and descriptor capacity.
    Bind each session to filesystem identity/generation and reject stale fids
    before lookup. Release session locks on close/version. Return partial
    `Rwalk` without installing `newfid`, enforce walk limits and directory/open
    rules, and retain the parsed request tag on error responses. Add inode-based
    range read/write/resize operations to the namespace for retained unlinked
    fids; do not route guest I/O through whole-file path operations. Model
    `Tsetattr` validity masks and timestamp precision explicitly rather than
    using seed metadata as the complete protocol update model.
    Test malformed input, duplicate tags, partial walks, flush ordering,
    concurrent endpoints, and session reset/close against the specifications;
    compare TypeScript separately for intentional host compatibility changes.

3.  **Connect Rust sessions to VirtIO and browser runtime state.** Replace
    `BrowserNineP` in `src/browser_runtime.rs` with a backend that references
    a registered Rust filesystem. Update `src/machine.rs` and
    `src/virtio_devices.rs` so resident requests finish before `notify`
    returns and only lazy reads retain descriptors. Add a completion/retirement
    drain keyed by request ID so one submission can finish earlier requests,
    with `Rflush` ordered after retirement. Do not rely solely on the current
    request's `Option<NinePOutcome>`. Keep C CPU-run borrowing exclusive of
    filesystem access by host callbacks. Retire pending work by
    generation on VM and device reset. Test same-call resident completion,
    out-of-order async completion, flush, and reset in native Rust.

4.  **Expose filesystem handles and on-demand loads through raw WASM.** Add
    scalar exports in `src/browser_abi.rs` and `riscbox-wasm/src/lib.rs` for
    creating and configuring a filesystem before VM startup, host operations,
    attaching configured server keys, load actions/completions, change events,
    and reset. Use copied buffers with explicit lifetimes. Reject attaching
    one handle to a second live VM, but allow both Risclet endpoints to share
    its handle. Test bounds, invalid handles, ownership, late completions, and
    VM destroy/recreation in native and real WASM builds.

5.  **Replace the browser 9p implementation.** Make `js/p9/` a typed facade
    for promise-returning host operations, change subscriptions, `SeedBuilder`,
    and HTTPS and tar on-demand source plugins. The host API has one error
    model and no `load`, `readFileAsync`, or `not-loaded` result. Update
    `js/riscbox.js` to start plugin promises after WASM returns, deliver
    completion bytes once, and dispatch change listeners outside WASM calls.
    Remove `p9Sessions`, `expectResponse`, hints, and the microtask-yield loop
    only after same-quantum resident replies and delayed wakeups pass real
    Chrome tests. Remove generic JavaScript 9p server registration in favor
    of filesystem handles and source plugins.

6.  **Migrate images and host applications.** Change
    `images/risclet/ui/index.ts` to create a filesystem for each example from
    its manifest before VM boot, load file bodies through HTTP on demand, and
    await editor reads, writes, file lists, instructions, and change handling.
    Guard late UI results when switching examples. Keep `fs0` and `fs1` in
    `images/risclet/riscbox.cfg` as separate sessions over one filesystem.
    Update `images/bin/build-distribution`, image Makefiles, and deployed
    `p9/` modules; build Risclet, Alpine, and xv6 profile distributions. The
    QEMU build-time 9p setup mount is a separate path and should remain intact.

7.  **Validate and publish the contract.** Run `make test-unit`, `make check`,
    `make wasm`, and real WASM/Chrome tests. Exercise Risclet boot, mount,
    guest and host edits, notifications, lazy HTTP success/failure, host file
    access before boot, VM reboot/shutdown, filesystem reset while mounted,
    and destroy/reattach. Measure resident request latency and copy volume
    against the current server before claiming a performance improvement.
    Update `AGENTS.md`, `README.md`, `images/README.md`,
    `images/DEPLOYMENT.md`, `js/p9/README.md`, and `CHANGELOG.md`; remove stale
    multi-VM, synchronous host API, and `expectResponse` descriptions. Commit
    each validated bounded change and remove this plan when migration finishes.

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
the resolved drive configuration and host block providers for
plain image assets or installation media. Select read-only, session CoW, or
mutable behavior explicitly. Validate multiple attached drives, failed reads,
and write behavior before documenting ISO or installer boot as supported.

### HTTP block consolidation

Keep the Rust and TypeScript HTTP paths in parallel. A single fresh-profile
Alpine Chrome run per path reached login in about 0.72 seconds and fetched the
same 10 blocks (5 MiB), while reported JS heap at login was about 6.1 MiB for
Rust HTTP and 8.2 MiB for TypeScript HTTP. Before removing either path, repeat
matched runs and compare Risclet login, xv6 profile compilation, and browser
memory under controlled cache conditions. The earlier Rust-only baseline used
warm cache for Risclet login (1.98 seconds, 21 responses), and separate runs
for Alpine login (0.92 seconds, 10 responses) and xv6 compilation (99.77
seconds, 159 responses). One TypeScript HTTP xv6 run completed with guest
`time make` at 102.71 seconds. These results do not measure run-to-run
variation.

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
