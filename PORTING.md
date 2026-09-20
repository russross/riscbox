Riscbox Rust port
=================

Goal
----

Build an independent Rust implementation of the browser Riscbox platform. The
C implementation remains isolated under `c/` as a behavioral reference, but
no C source, object code, or build tool is required by Rust or image production.

The production target is `wasm32-unknown-unknown`. The port covers the RV64
CPU, Sv39 memory system, machine, guest-visible devices, configuration loader,
HTTP block and filesystem storage, JavaScript 9p bridge, and current browser
entry points. A native Rust emulator, SDL, SLIRP, native storage backends, and
image-building tools are outside the port.

Architecture
------------

The Rust package lives at the repository root with source under `src/`.
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
prompt. Browser VMs seed the guest through `/chosen/rng-seed` and expose a
VirtIO entropy device; both receive cryptographic bytes synchronously from Web
Crypto through one narrow host import. The standalone Risclet page boots its
in-memory JavaScript 9p service,
loads either tracked example without RPC, mirrors guest-created and deleted
files, and updates optional instructions from `doc/doc.md`. Its interface
retains the deployed CodeGrinder editor, terminal, draggable panes, and sizing
behavior while removing the RPC and grading workflows. Clean C release,
sanitizer, and reference WASM builds from `c/` remain compatibility checks;
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
    Put stable unmangled exports in the small `riscbox-wasm/` companion crate so
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
*   2026-09-19: Use one explicit entropy source for the FDT `/chosen/rng-seed`
    property and VirtIO RNG. Native runs read the operating-system entropy
    source; the raw browser WASM adapter fills bounded buffers synchronously
    with Web Crypto rather than adding asynchronous device state or a
    guest-visible PRNG.
*   2026-09-19: Make Rust and its browser adapters the root implementation.
    Isolate the complete C reference under `c/`, replace its image splitter in
    production with a standalone Python tool, and make the root-owned custom
    kernel a distribution artifact consumed by image definitions.
*   2026-09-19: Use the conventional `src/` directory for the root emulator
    crate and name the raw browser WebAssembly export crate `riscbox-wasm/`.
*   2026-09-19: Keep legacy HTTP 9p reads asynchronous like HTTP block reads.
    Retain the guest descriptor while metadata or a file body is fetched, then
    retry it after the matching browser completion. Store guest mutations in
    the Rust inode arena and expose the supplied password through
    `.fscmd_pwd`; retain the synchronous JavaScript backend for `js9p` trees.
*   2026-09-19: A fresh C-to-Rust path audit added focused regressions for
    SoftFP cancellation and subnormal division, in-slice interrupt changes,
    browser input backpressure, VirtIO receive wakeups and block completion,
    large HTTP disks and requests, HTTP 9p loading, scalar decode/discovery,
    and tablet coordinate scaling.
*   2026-09-19: Deploy boot payloads and split disks under abbreviated SHA-256
    names, retain old generations until explicit cleanup, and use the config
    replacement as the atomic image rollover. Fetch the config with `no-store`
    and revalidate the programmatically loaded WASM runtime at each VM start.

Proposed 9p transport and filesystem design
------------------------------------------

Status: implementation in progress. Milestones 1 through 3 are complete through
the TypeScript server build and filesystem/session ownership split. Inode-backed
namespace semantics, quotas, and shared locks are the next milestone. The
completed milestones and decision log above describe the pre-project
implementation where this section has not yet superseded it.

### Scope and ownership

Make 9p-over-VirtIO a generic bridge to host-provided 9P2000.L servers. The
Rust emulator owns VirtIO descriptor validation, bounded pending descriptors,
request delivery, response copying, used-ring completion, resets, and
interrupts. It validates the protocol envelope at the guest-memory boundary:
one complete message, an exact encoded size, a request direction, a bounded
reply with a response direction, and a matching reply tag. It does not parse
operations, negotiate features, track fids, translate errors, or synthesize
filesystem replies.

The server owns version and size negotiation, tags, fids, attach identities,
operations, errors, flush ordering, and filesystem state. Linux owns client
behavior and cache policy. A malformed guest message, malformed server reply,
duplicate completion, or completion for an unknown live request is a transport
failure rather than a filesystem error. Ordinary failures are server-generated
`Rlerror` replies. Stop the affected VM cleanly on a transport failure until a
tested VirtIO device-failure and recovery path exists.

Supply a reusable TypeScript server consisting of a shared in-memory
filesystem and independent protocol sessions. It is one server available to
the bridge, not part of the emulator. A custom server can implement the same
session interface without using its inode model or preload support. HTTP block
storage is unchanged by this proposal.

Do not add server-driven cache invalidation, write leases, overlay protocols,
or proprietary control files. Client and server negotiate standard 9P2000.L;
deployment chooses Linux mount options appropriate to its sharing model.

### Concurrent request/completion boundary

The existing single `NinePDevice.pending` entry is an implementation shortcut,
not a restriction of VirtIO, WASM, or 9p. Replace it with a bounded map keyed
by a transport request ID. Each entry retains the descriptor chain, queue,
endpoint generation, request tag, and writable capacity. IDs are independent
of reusable 16-bit 9p tags and are not reused within a live generation.
Use Rust newtypes for endpoint, generation, and request IDs; retire a generation
before counters wrap. The raw ABI represents these IDs with explicit integers.

Drain available chains in submission order, dispatch each once, and accept
completions in server-selected order. A slow file load must not prevent
delivery of a later request, particularly `Tflush`. Bound pending work by the
negotiated virtqueue size and validate descriptor and message lengths before
allocating request copies; do not introduce a smaller serial dispatch gate.
The server may limit concurrent body loads, but that limit must not delay
protocol-only requests behind them.

Use host actions and exported completions through the existing adapter loop:

```text
P9Open(endpoint, generation, server_key)
P9Request(endpoint, generation, request_id, bytes, reply_capacity)
P9Close(endpoint, generation)

riscbox_p9_complete(endpoint, generation, request_id, outcome, ptr, len)
```

The outcome is an explicitly tagged reply, suppressed response, or endpoint
failure. A reply contains a complete 9p response. Suppression releases a
descriptor with no protocol response, as required when a server flushes an
outstanding request. Endpoint failure represents a broken server/connection,
not a filesystem errno. Never leave a failed request pending or fabricate a
successful 9p reply.

The TypeScript contract is conceptually:

```text
interface P9Server {
    connect(): P9Session;
}

interface P9Session {
    request(bytes: Uint8Array, replyCapacity: number): Promise<P9Outcome>;
    close(): void;
}

type P9Outcome =
    | { kind: "reply"; bytes: Uint8Array }
    | { kind: "suppressed" };
```

A rejected request promise is an endpoint failure. Filesystem errors must be
encoded as 9p replies rather than thrown across this boundary.

Register one server per configuration key. Each VirtIO device connection gets
a fresh session, including when two devices select the same shared filesystem.
`connect()` is synchronous so missing registrations fail during VM setup;
requests are always asynchronous. The adapter invokes requests in delivery
order without awaiting earlier promises. It queues settled outcomes and drains
them only after the active WASM call returns, so no callback reenters borrowed
Rust runtime state. Preserve settlement order while batching, especially a
suppression or original reply before its `Rflush`.

Copy request bytes before releasing host-action storage. A session owns each
request copy until its promise settles. Copy a reply into WASM only for the
completion call, and do not retain a WASM view across an await. Custom servers
remain responsible for not mutating a reply after returning it.

Flushing belongs to the server. It tracks old tags, suppresses responses when
appropriate, handles repeated and invalid flushes, and ensures a flushed
response cannot follow its `Rflush`. Aborting a loader call is optional resource
cleanup and does not implement the protocol. A shared load may continue after
one request is flushed if another waiter still needs it. Suppression must
reclaim the original descriptor before exposing `Rflush`, permitting safe tag
and request-buffer reuse. Validate reply-before-flush, suppression-before-flush,
repeated flush, and tag-reuse races against Linux's VirtIO client.

### Endpoint identity and lifetime

A runtime-scoped endpoint ID plus generation prevents stale asynchronous
completions from reaching reused guest memory. Advance the transport generation
on VirtIO device reset, VM restart, and explicit endpoint close. Invalidate
pending descriptors before closing the old session, then ignore its late
completions without touching guest memory. A duplicate or unknown request ID
in the current generation is an adapter error. Closing one session releases
its fids, locks, and pending operations, not the shared tree or other sessions.

Maintain a second, server-local session generation. A valid `Tversion` starts
a new protocol session, aborts all outstanding protocol work, releases all
fids and locks, and prevents earlier continuations from committing mutations
or producing replies. This is the remount boundary defined by 9p itself.
Linux's VirtIO transport does not send the device a distinct unmount event, so
normal unmount is observed only through flushes and clunks; the next mount's
`Tversion` provides the next reliable generation marker. Do not invent a
private unmount message. An attach selects a tree within a protocol session; it
does not create a new TypeScript filesystem.

Filesystem body loads are shared storage work rather than session work. A load
may populate an unchanged inode after its initiating session closes, but a
stale session may not commit a protocol mutation. Every continuation checks
both its session generation and the inode content revision before committing.

Keep the guest-visible mount tag separate from the host server key. Proposed
configuration is `fsN: { server: "workspace", tag: "shared" }`, with a host
registry mapping `workspace` to a server. Configuration parsing and validation
stay in Rust; host actions carry the selected server key. Reject duplicate
guest mount tags and missing registrations before starting the machine. Linux
allows only one active mount per VirtIO 9p channel, so two simultaneous mounts
of the same shared tree in one VM require two configured devices with distinct
tags and the same server key.

Pass the registry once as
`Riscbox.instantiate(wasm, { p9Servers: ReadonlyMap<string, P9Server> })`.
Do not mutate a registry entry behind a live endpoint. Replacing a server takes
effect only on the next VM start and therefore receives a fresh endpoint
generation and protocol session.

### Shared filesystem and independent sessions

Split the supplied implementation into these concrete owners:

| Owner | State and responsibility |
| ----- | ------------------------ |
| Filesystem | Inodes/QIDs, directory entries, metadata, file contents, quotas, revisions, notifications, and shared byte-range locks |
| 9p session | Negotiated version and `msize`, fids, attach identities, active tags, cancellation, and per-open state |
| Seed loader | Immutable namespace entries plus one asynchronous regular-file body loader |
| Application facade | Synchronous tree operations, explicit loading, and subscriptions over the same filesystem |

The former `Memory9PServer` combined the first and second owners. Milestone 3
separates them: `connect()` creates distinct protocol state over one filesystem,
so two VMs may reuse the same fid and tag values without collisions.
`Tversion` resets only its session. Stable inode identity preserves references
across rename and unlink; storage remains alive while referenced by open fids.
Do not equate inode identity with a pathname. Never reuse a QID path during a
filesystem instance's lifetime; increment its version when observable inode
data or metadata changes. Directory entries refer to inode IDs, which permits
hard links and correct open-unlink lifetime. Count links accurately and count
file bytes once per inode.

Use stable directory cookies rather than array indexes for `Treaddir`, so a
rename or insertion between calls does not silently reinterpret an offset.
Keep open flags and directory iteration state on fids. Implement shared POSIX
byte-range locks across sessions, release a session's locks when it closes, and
return honest `EOPNOTSUPP` errors for unsupported node types or operations.
In-memory `fsync` may succeed as a documented no-op; authentication, devices,
FIFOs, sockets, xattrs, ACLs, and persistence are outside the supplied server's
initial profile.

The initial profile implements version, flush, attach, walk, open/create,
read/write, clunk/remove, statfs, getattr/setattr, readdir, fsync, symlink and
readlink, mkdir, link, rename and renameat, unlinkat, lock, and getlock. Keep an
explicit operation matrix with the protocol tests. `Tauth`, special-node
creation, and extended attributes return standard unsupported errors; do not
return success without implementing their semantics. Version negotiation with
an unknown dialect follows the standard `Rversion("unknown")` behavior.

The supplied server is intended for trusted clients. It stores and reports
uid, gid, and mode metadata but is not an authentication or isolation boundary;
`Tauth` is unsupported. Do not claim server-enforced multi-tenant security.
Deployments that need it must provide a different server implementation.

JavaScript's event loop makes individual synchronous commits atomic, but awaits
allow operations to interleave. Do not serialize the filesystem behind one
global promise. After an await, check the session generation and relevant inode
or directory revision, then either commit atomically or retry/fail according to
the operation. Quota checks and their mutations occur in the same synchronous
commit. Append writes select the end offset during that commit.

Keep 64-bit wire values as `bigint` until validation. Convert sizes and offsets
to JavaScript numbers only after proving they are safe integers and within the
configured limits. Filesystem construction accepts these optional limits:

*   `maxFileBytes`, default 256 MiB.
*   `maxTreeBytes`, default 1 GiB, counting logical regular-file sizes once per
    inode and ignoring metadata overhead.
*   `maxInodes`, default 2^20, counting directories, regular files, and
    symlinks; an additional hard link does not consume an inode.
*   `maxDirectoryEntries`, default 2^20, counting every name so repeated hard
    links cannot bypass the metadata resource limit.

Reject an invalid initial namespace atomically. Enforce the same limits on
guest writes, host writes, truncation, links, and loader results. Keep protocol
`msize`, pending-request count, and concurrent-load count as separate transport
or server limits rather than conflating them with filesystem capacity.

### Optional seed loader

Construct the complete initial namespace before constructing the filesystem.
The optional seed is a typed list of directory, symlink, and regular-file
entries. Regular files have a mandatory logical size and a loader key of a
caller-chosen generic type; entries may also provide standard mode, ownership,
and timestamp metadata. A shared regular-file seed inode key can make multiple
directory entries refer to one initial inode. Parent directories may be
explicit or may be created with documented defaults. Reject duplicates,
cycles, dangling hard links, directory hard links, invalid names, inconsistent
shared-inode metadata, and quota overflow.

Expose the preload boundary as a typed plugin value:

```text
interface SeedPlugin<Key> {
    readonly entries: readonly SeedEntry<Key>[];
    readonly loader: SeedLoader<Key>;
}

type SeedEntry<Key> = DirectorySeed | SymlinkSeed | FileSeed<Key>;
```

The concrete entry types carry normalized relative paths and standard
metadata. `FileSeed` carries its logical size, loader key, and optional shared
inode key. Provide a `SeedBuilder<Key>` with `addDirectory`, `addSymlink`,
`addFile`, and `addHardLink` methods so a manifest or archive parser does not
construct internal inode records. `finish()` validates and freezes the entry
list before `MemoryFilesystem` is created. The plugin and all seed arguments
are optional; no plugin means an empty resident filesystem.

The loader has one operation:

```text
interface SeedLoader<Key> {
    load(key: Key, signal: AbortSignal): Promise<Uint8Array>;
}
```

The server gives the loader the opaque key from the selected seed entry and
does not interpret URLs, archive offsets, hashes, encryption, or credentials.
The returned length must equal the declared size. An HTTPS plugin can build the
namespace from its own manifest and use paths or opaque records as keys. A tar
plugin can inspect a pre-downloaded archive first, use byte ranges as keys, and
extract a regular file only when asked. Fetching a manifest or archive is a
preload step owned by the plugin, not a 9p operation or another server type.

Represent regular-file contents as an unloaded seed reference, a shared
in-flight load, or ordinary mutable resident bytes. The loader returns a fresh,
writable `Uint8Array` and transfers its exclusive ownership to the filesystem;
the filesystem does not make a second copy or retain a special seeded state
after a successful load. From then on, the inode is identical to a file created
in memory.

Reads of one unloaded inode share a load. A mutation that needs existing
content waits for that load and then treats the transferred buffer as ordinary
mutable resident data. A whole-file replacement of an unloaded inode instead
installs its new resident buffer immediately and discards the seed reference
without fetching it. Unlink and rename operate on the already materialized
namespace and never rediscover seed entries. Replacing or otherwise changing
an unloaded inode bumps its content revision, causing an older load completion
to be discarded rather than restoring stale bytes. Copy-on-write describes the
filesystem's relationship to its seed source, not an immutable resident-data
layer.

Pin one seed list and loader interpretation for the filesystem's lifetime.
Refreshing a remote deployment creates a new filesystem/server instance; it
does not alter unloaded entries underneath live clients. A failed load leaves
an explicit failed state for direct callers and produces an ordinary `EIO` for
9p requests. An explicit retry clears that failure; unrelated operations and
protocol requests remain usable.

### Synchronous application access

Retain a small synchronous application facade for `readFile`, `writeFile`,
`remove`, `rename`, `listFiles`, and `subscribe`. Return a discriminated result
rather than throwing for expected outcomes:

```text
type SyncResult<Value> =
    | { kind: "ok"; value: Value }
    | { kind: "not-loaded"; paths: readonly string[] }
    | { kind: "error"; error: FilesystemError };

interface ApplicationFilesystem {
    readFile(path: string): SyncResult<Uint8Array>;
    writeFile(path: string, bytes: Uint8Array): SyncResult<void>;
    remove(path: string): SyncResult<void>;
    rename(oldPath: string, newPath: string): SyncResult<void>;
    listFiles(): SyncResult<readonly string[]>;
    load(paths: readonly string[], retry?: boolean): Promise<SyncResult<void>>;
    readFileAsync(path: string, retry?: boolean): Promise<SyncResult<Uint8Array>>;
    subscribe(listener: (change: FilesystemChange) => void): () => void;
}
```

`readFile` returns `not-loaded` for an unloaded body; it never returns empty
bytes or starts hidden asynchronous work. `writeFile` is a whole-file replace,
so it may synchronously replace an unloaded file without fetching it. Namespace
listing, rename, unlink, and metadata access remain synchronous because the
complete namespace is resident.

Add `load(paths)` and `readFileAsync(path)` for explicit asynchronous access.
`load` returns a promise and emits a `loaded` or `load-error` subscription event
when it settles; concurrent calls share the in-flight load. A retry option is
required for a retained failure. Memory-only trees remain entirely synchronous.
Both host and 9p operations use the same inode mutations, quota accounting,
revision checks, and notifications. Change events identify the inode and the
affected path or paths so hard-linked content changes are not mistaken for
path identity.

### Multiple clients and deployment cases

The primary use case for shared 9p server trees is simple sharing between VM(s)
and the host app. The emphasis is on correct 9p2000.L semantics, avoiding
performance hacks and added complexity in anticipation of naive clients.

Multiple Linux VMs can mount the same live filesystem, and the host app can
access it through the synchronous facade. Shared inode metadata, hard links,
open-unlinked files, and locks must behave consistently across sessions. Audit
the current success stubs before advertising multi-client correctness.

The server does not attempt to repair Linux cache coherence. Use `cache=none`
when host code or multiple VMs must observe one another's changes promptly.
`cache=mmap` enables read-ahead and writeback so it permits executable mappings,
but host or peer changes may remain stale. `cache=loose` is appropriate only
for an exclusive mount whose tree is not modified behind the client. These are
deployment choices and do not change server semantics.

### Remove legacy `fs_net`

Remove `file`, `socket`, and `js9p` filesystem backends from the active Rust
configuration schema and accept only `{ server, tag }` for browser 9p devices.
Do not retain configuration aliases or translate the legacy HTTP format in
Rust. Remove `src/http_9p.rs`, its proprietary command file and password
plumbing, and all now-unused PBKDF2 support after auditing encrypted HTTP block
storage. Note: the legacy C implementation is archived for reference only. Do
not alter it, and do not target compatibility with it. Development of this
feature set is for Rust and the WASM target only.

There is no migration process and no legacy instances using affected features
to support.

### Files, staging, and acceptance

Proposed Rust file boundaries: `src/virtio_devices.rs` owns pending 9p
descriptors;
`src/machine.rs`, `src/browser_runtime.rs`, `src/browser_abi.rs`, and
`riscbox-wasm/src/lib.rs` carry generic endpoint actions/completions;
`src/config.rs` records server keys and mount tags.

Use `js/riscbox.ts` for endpoint registration and lifecycle,
`js/p9/filesystem.ts` for inodes and the application facade,
`js/p9/session.ts` for wire parsing and protocol state, and `js/p9/seed.ts` for
seed types and loading. Put the HTTPS and tar examples under `js/p9/plugins/`;
they depend on the seed interface, not the server internals. Export the public
surface from `js/p9/index.ts`. Make these TypeScript sources authoritative and
emit browser-consumable JavaScript plus declarations under `build/js/`; do not
hand-maintain a parallel `p9.d.ts`. Keep the standalone adapter free of runtime
dependencies. This milestone converts maintained browser integration code, not
the Python image tools or shell build scripts. Update Risclet imports,
distribution packaging, examples, and deployment documentation to consume the
emitted modules.

1.  **Complete:** implement the concurrent Rust VirtIO transport core. Retain
    each descriptor in a bounded map keyed by a typed request ID, drain chains
    in submission order, accept reply and suppression outcomes out of order,
    validate response envelopes, and retire the generation on device reset.
    Focused tests cover malformed, mismatched, oversized, duplicate, suppressed,
    out-of-order, reset, and stale-generation completions.
2.  **Complete:** carry endpoint and generation identity through machine host
    actions, the raw WASM ABI, and the JavaScript adapter. Test queue saturation, two
    endpoints, VM restart, endpoint failure, adapter completion ordering, and
    late promise settlements with a controllable fake server.
3.  **Complete:** establish the TypeScript build and split shared filesystem
    state from 9p session state. Route the current memory behavior through the concurrent
    boundary. Validate two sessions reusing the same fid and tag values,
    `Tversion` generations, all flush races, and session close.
4.  **Complete:** add inode-backed directories, hard links, open-unlink lifetime, stable
    directory cookies, quotas, and shared byte-range locks. Test atomic rename,
    append, truncation, quota races, lock release, QID stability, and two Linux
    clients mutating one tree.
5.  **Complete:** add typed seeds, the single loader operation, explicit direct-API results,
    and HTTPS and tar example plugins. Exercise shared loads, failure and retry,
    whole-file replacement without a load, load/mutation races, loader length
    validation, deletion, hard-linked seeds, and immutable deployment pinning.
6.  Remove the Rust `fs_net` path and the old configuration forms. Keep the C
    reference unchanged. Run
    focused native transport and TypeScript tests, strict type checks, WASM
    builds, and headed Chrome guest tests. Boot Linux with two configured tags,
    verify sharing under documented cache modes, restart during outstanding
    loads, unmount/remount, and close one client while another continues.

Measure concurrent request latency, request and reply copy volume, resident and
logical tree sizes, peak load memory, generated JavaScript size, and WASM size.
Do not introduce a server WASM module or a more elaborate storage layer without
measurements showing a material benefit.

Protocol references for implementation review:

*   [9p request multiplexing](https://9fans.github.io/plan9port/man/man9/intro.html)
    defines concurrent tagged requests.
*   [Flush semantics](https://9fans.github.io/plan9port/man/man9/flush.html)
    defines response suppression and ordering relative to `Rflush`.
*   [Linux VirtIO 9p transport](https://github.com/torvalds/linux/blob/master/net/9p/trans_virtio.c)
    reclaims zero-length used buffers without delivering a protocol reply.
*   [Linux 9p client documentation](https://docs.kernel.org/filesystems/9p.html)
    describes mount tags, attach options, and cache consistency limitations.
*   [9p version semantics](https://9fans.github.io/plan9port/man/man9/version.html)
    defines the protocol-session boundary and its required cleanup.

Implementation decisions
------------------------

*   2026-09-19: Split the original concurrent-boundary stage at the Rust
    transport boundary. The device owns a `BTreeMap` of retained chains keyed
    by `NinePRequestId`; its size is bounded by the negotiated virtqueue size.
    `NinePGeneration` invalidates all retained chains on reset, and stale
    completions are ignored without accessing guest memory. Current-generation
    unknown or duplicate completions remain transport failures. Browser
    endpoint identity and asynchronous host actions form milestone 2.
*   2026-09-19: Deliver browser 9p work as explicit open, request, and close
    host actions. The raw ABI carries endpoint, generation, request ID, reply
    capacity, and tagged reply/suppression/failure outcomes. The adapter owns
    one independently connected session per endpoint, copies requests before
    promise work, accepts settlement-order completions without WASM reentry,
    closes failed sessions, and ignores closes for retired generations. The
    legacy `js9p` configuration selects the temporary `default` registry key;
    the typed configuration change remains in the removal milestone.
    At this checkpoint the dependency-free adapter is 13,992 bytes and the
    optimized WASM module is 411,449 bytes.
*   2026-09-20: Make `js/p9/*.ts` the authoritative supplied-server source and
    emit JavaScript plus declarations under `build/js/p9/`. A
    `MemoryFilesystem` owns the shared namespace and application facade, while
    every `connect()` creates a `P9Session` with independent `msize`, fids,
    active tags, cancellation, and lifetime. `Tversion` retires earlier session
    work and clears fids; `Tflush` suppresses an active response before its
    `Rflush`, including repeated flush and immediate tag reuse. The standalone
    adapter remains handwritten JavaScript at the raw WASM boundary. The three
    emitted server modules total 37,420 bytes of JavaScript and 6,362 bytes of
    declarations at this checkpoint.
*   2026-09-20: Give the shared TypeScript filesystem stable inode/QID identity,
    per-directory monotonic cookies, link and fid reference counts, and
    synchronous logical-byte, inode, and directory-entry quota accounting.
    Hard links share one inode; unlink and rename detach names without retiring
    open fids, and storage is reclaimed after the last link and fid disappear.
    POSIX byte-range locks are shared across sessions and released by unlock,
    `Tversion`, or session close. Initial and replacement trees validate before
    retiring live fids. Focused protocol tests exercise two independent clients,
    append commits, truncation, quota contention, rename replacement, lock
    release, stable cookies, and QID/open-unlink lifetime. The three emitted
    server modules total 47,018 bytes of JavaScript and 7,713 bytes of
    declarations; the optimized Rust WASM remains 411,449 bytes.
*   2026-09-20: Represent seeded regular-file bodies as unloaded, one shared
    in-flight load, retained failure, or ordinary resident bytes. A successful
    loader result is adopted directly, so peak filesystem load storage is one
    declared file body plus promise state rather than a second copied body.
    Content revisions discard stale completions after replacement or deletion;
    partial 9P mutations await content, while whole-file application writes do
    not. `SeedBuilder` validates and freezes typed entries and hard links, and
    the HTTPS-manifest and pre-downloaded-tar examples use the same single
    `load(key, signal)` boundary. Application methods now return discriminated
    results, and change events carry inode identity and all affected paths.
    Focused tests cover shared application and 9P loads, failure retention and
    retry, length validation, quotas, deletion, hard-linked seeds, load/write
    races, and pinned loader interpretation. The five emitted server modules
    total 64,896 bytes of JavaScript and 12,816 bytes of declarations. Request
    and reply copying at the WASM boundary is unchanged, and the optimized Rust
    WASM remains 411,449 bytes.

Later milestones (out of scope for now)
---------------------------------------

Add framebuffer demonstration programs to the image, configure the guest
display and input devices, and connect the framebuffer callback to a canvas in
the Risclet page. The current demo intentionally remains terminal-only until
that guest-to-page path can be validated together.

Revisit networking support for guests.

Benchmark, profile, and explore performance improvements.

Explore bootloader support, compressed kernel support, compressed initrd support, etc.
