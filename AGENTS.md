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
*   A 16550A UART, optional VirtIO console, Goldfish RTC, PLIC, legacy CLINT,
    SiFive test finisher, simple framebuffer, and VirtIO MMIO block, 9p,
    network, entropy, keyboard, and tablet devices.
*   Browser delivery through raw WebAssembly, a handwritten JavaScript adapter,
    HTTP-backed disks, and host-provided 9P2000.L servers.

RV32, multiple harts, vectors, the hypervisor extension, PCIe, AIA, UEFI,
general device emulation, a native UI, SDL, SLIRP, and native filesystem or
socket backends are outside the current scope. Networking exists but is not a
near-term expansion area. Add scope only when a target guest or deployment has
a concrete need.

Repository map and terminology
------------------------------

*   `src/` is the authoritative emulator library: CPU, SoftFP, memory, machine,
    devices, configuration, storage, and browser runtime.
*   `riscbox-wasm/` supplies the small stable raw WASM export surface. Keep
    unsafe boundary code isolated there; the main crate forbids unsafe code.
*   `js/riscbox.js` is the dependency-free browser adapter for the raw ABI.
*   `js/p9/` is the authoritative TypeScript 9P2000.L server, shared in-memory
    filesystem, and optional seed plugins. Generated JavaScript and declarations
    go under `build/js/p9/`.
*   `images/` contains reproducible image definitions and deployment tooling.
    Generated downloads, images, boot assets, and distributions are not source.
*   `kernel/` owns the canonical custom Linux kernel consumed by image builds.
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

The generated device tree follows standard libfdt layout and QEMU `virt`
bindings. The platform boots current xv6 over UART and VirtIO block and boots a
prepared Alpine system through OpenSBI to login and clean shutdown. The browser
adapter loads configuration, firmware, kernels, initrds, and split HTTP disks
relative to the configuration URL. HTTP disk writes are session-local.

VirtIO 9p is a generic concurrent transport. Rust validates descriptors and
message envelopes but does not implement filesystem semantics. Each configured
`{ server, tag }` endpoint gets an independent asynchronous session from the
host registry. The supplied TypeScript server provides shared inode state,
independent sessions, hard links, stable directory cookies, quotas, byte-range
locks, explicit application results, and optional lazy seed loading. Do not
restore the removed `file`, `socket`, or `js9p` configuration forms.

Architecture rules
------------------

*   Specifications are authoritative. Use the relevant RISC-V specification
    first and QEMU `virt` behavior second. Historical C behavior is evidence
    only when investigating lineage.
*   Keep guest virtual and physical addresses as `u64`. Store validated `u32`
    offsets into the fixed WASM memory arena. Keep allocation, JavaScript calls,
    and uncommon checks out of cached CPU and RAM paths.
*   Start with safe Rust. Add a small unsafe fast path only after a WASM
    benchmark shows a material benefit, and document its invariants.
*   Keep architectural state and host interfaces strongly typed. Favor concrete
    device ownership, shallow control flow, explicit dependencies, and immutable
    values. Avoid shared ownership and interior mutability in the interpreter.
*   Keep the raw WASM ABI and adapter small. Do not add Emscripten, WASI,
    `wasm-bindgen`, an async Rust runtime, or adapter runtime dependencies.
*   Keep dependencies exceptional. Inspect the complete resolved graph before
    adding one. Prefer direct implementations for the interpreter, memory,
    SoftFP, and configuration parser. The narrowly configured RustCrypto crates
    remain only for encrypted split HTTP block images.
*   Keep browser I/O explicit through request/completion and event queues.
    Never retain a JavaScript view across an await or reenter borrowed Rust
    runtime state from a host callback.
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

*   `make test` runs Rust, Python tool, and JavaScript adapter/server tests.
*   `make check` adds strict Clippy and Python type checks.
*   `make wasm` builds the deployed Rust WebAssembly artifact.
*   `make kernel` builds the canonical custom kernel; `make dist` builds the
    core WASM, JavaScript, and kernel artifacts.

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
