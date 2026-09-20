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
    material deployment friction. Prepared raw kernels and initrds remain the
    baseline.
5.  Benchmark before optimizing the interpreter, WASM boundary, storage, or 9p
    implementation. Correctness remains the first gate.

The CPU optimization investigation below is the active planning milestone. Do
not begin an implementation stage until its benchmark method and correctness
gate are fixed, and retain the previous stage as the comparison and rollback
point.

CPU interpreter optimization
----------------------------

### Problem and evidence

The saved V8 profiles in `images/xv6-profile/build/profiles/` run the same xv6
kernel build through the TinyEMU-derived WASM and Riscbox WASM implementations.
Both logs reach `XV6_PROFILE_BUILD_COMPLETE` and poweroff. The last guest
timestamp is 93.695 seconds for TinyEMU and 520.185 seconds for Riscbox, a
5.55x difference. The CPU profiles span 95.974 and 529.722 wall-clock seconds,
respectively, a consistent 5.52x difference. These are single captured runs,
so they establish direction and a baseline rather than confidence intervals.

TinyEMU lacks useful function names, but 80.06% of its samples are in one WASM
function, with most remaining execution samples in a small group called below
it. This is consistent with a compact interpreter loop and helper set. Riscbox
symbols identify a more distributed hot path:

*   `Cpu::run` accounts for 38.09% of all samples as self time and `Cpu::load`
    for 38.94%. Together they account for about 77% of the profile.
*   Arithmetic execution accounts for 2.75%, stores 2.09%, PMP checks 1.35%,
    immediate arithmetic 1.01%, and compressed jumps 0.96%.
*   `PhysicalMemory::read`, address translation, and platform bus reads are
    individually small. The fast TLB-hit load path, instruction loop, and
    dispatch overhead are therefore higher-priority than page walks or MMIO.
*   Idle samples are 8.96% for TinyEMU and 11.39% for Riscbox. Host scheduling,
    block I/O, and JavaScript are not the primary explanation for the gap.

Stage 1 reduced the same Riscbox workload to a 362.416-second guest timestamp
and a 371.725-second CPU profile, 30.3% and 29.8% below the original baseline.
Fixed-width safe arena accesses produced nearly all of that gain; arithmetic
dispatch inlining contributed a further 1.8%. Removing the redundant `x0`
restore had no measurable effect beyond single-run variance. The deployed
module shrank from 319,145 to 318,844 bytes overall. These remain single runs
and establish the comparison point for Stage 2 rather than confidence
intervals.

The remaining hot path is
`BrowserRuntime::run -> Machine::run -> Cpu::run -> fetch/load -> execute`.
TLB hits resolve an `ArenaOffset` and use checked fixed-width scalar arena
accesses. Every instruction still checks interrupt state and updates its
architecturally visible counters. TLB misses and device accesses fall through
`CpuBus` to `PlatformBus` and `PhysicalMemory`; those cold paths remain explicit
and checked.

### Scope and measurement discipline

Optimize the RV64 interpreter and its RAM fast path. Do not expand device,
network, storage, or guest scope. Keep guest virtual and physical addresses as
`u64`, fixed-arena offsets as validated `u32`, and all MMIO, TLB-fill, page-walk,
trap, and host boundaries checked. Unsafe code, if justified by a later stage,
must remain isolated from the platform and raw WASM ABI.

Use WABT tools to disassemble both baseline WASM builds and trace the full CPU
emulation cycle for a hot-path instruction. Repeat for each optimization stage.
Use insights and measurements of the hot path along with profiler data to guide
each stage and suggest revisions to the plan as you go.

Every stage must remain separately revertible and must pass `make check`, a
clean `make wasm`, focused CPU/MMU tests, current xv6 boot and build completion,
and prepared Alpine boot through login and clean shutdown. Add differential
instruction tests against the last accepted stage for normal execution, traps,
misaligned accesses, TLB invalidation, PMP, counters, interrupts, atomics, and
self-modifying or externally modified RAM. Reject a stage that merely moves
samples between symbols without improving end-to-end throughput.

### Stage 2: release build interventions

Compare release settings independently: thin and fat LTO, one codegen unit,
WASM-oriented optimization levels, panic strategy, and a post-link optimizer if
one is already available in the build environment. Track speed and deployed
module size; do not assume the smallest module is fastest.

Rust release builds already disable integer overflow checks by default unless
configured otherwise. Cargo has no stable profile switch that globally removes
slice bounds checks. Confirm the actual workspace flags and generated WASM
before attributing cost to either. Prefer source shapes that let LLVM prove a
single safe range check. Treat unchecked indexing as stage 3 unsafe work, not a
build-only setting. Do not disable debug assertions or other checks unless the
generated release artifact contains them and the benchmark shows material cost.

### Stage 3: targeted unsafe RAM fast paths

Permit unsafe code only in a small internal module owned by memory/MMU code;
keep the main CPU logic and platform safe. Introduce a validated RAM view such
as `RamWindow { base: ArenaOffset, len: u32 }`, created only by checked
`PhysicalMemory::ram_range`, and narrow helpers with signatures equivalent to
`unsafe fn read_unchecked(arena: &[u8], offset: ArenaOffset, width:
AccessWidth) -> u64` and the corresponding write. Document invariants at each
unsafe definition: arena allocation is stable during execution, the entire
access lies in the validated window, offsets fit the WASM memory arena,
alignment or unaligned operation semantics are explicit, and mutable access is
not aliased.

Apply these helpers only to measured TLB-hit instruction fetches and aligned
RAM loads/stores. Keep misses, MMIO, misalignment, dirty tracking, page-table
updates, and TLB fill safe. Test offsets at zero, page and arena ends, every
width and alignment, stale entries after invalidation, and malformed guest
addresses. Measure targeted unchecked indexing separately from pointer caching
so its value is known.

### Stage 4: pointer-based interpreter loop

If stage 3 leaves a material gap, prototype a broader loop that caches raw
pointers or equivalent WASM linear-memory offsets for CPU registers, the RAM
arena, and read/write/execute TLB entries across a bounded run block. Keep a
single checked slow-path boundary for TLB misses, traps, interrupts, MMIO, and
host-visible exits. Define a `RunContext` containing the arena base and length,
TLB tables, counter deltas, and an explicit exit reason; construct it only while
the machine and arena cannot be reentered or resized.

This stage may fuse fetch, decode, execute, PC advance, and counter accumulation
in a style closer to TinyEMU. Its unsafe module must state pointer provenance,
aliasing, arena-stability, and reentry invariants. No pointer or JavaScript view
may survive a run call, allocation, device callback, or await. Compare the
result both with stage 3 and TinyEMU; retain it only if the additional gain is
large enough to justify the larger audit surface and duplicated slow exits.

### Stage 5: TinyEMU-derived CPU core boundary

Use this as an architectural alternative, not the automatic conclusion. Adapt
only TinyEMU's RV64 CPU execution core into a separately owned module while the
Rust platform retains memory allocation, devices, configuration, browser
runtime, and host I/O. Define a narrow boundary around operations equivalent to
`run(budget) -> RunOutcome`, checked RAM window acquisition, physical/MMIO
reads and writes, interrupt sampling, timer/counter state, and TLB invalidation.
Use typed request and exit enums rather than exposing platform objects or loose
callbacks. The core must implement Riscbox's current ISA, privilege, PMP, Sv39,
counter, trap, and invalidation contract; historical TinyEMU behavior is not a
compatibility authority.

Prototype the boundary first with the existing Rust CPU so its overhead and
ownership model can be measured independently. Then compare two core options:
a close Rust adaptation using isolated unsafe pointers, and a C-derived core
compiled into WASM behind a minimal C ABI. A C core adds a second toolchain,
cross-language state representation, and a much larger validation burden; take
it only if stage 4 still leaves a consequential gap and the prototype produces
a clear additional gain. The final decision should report performance, WASM
size, unsafe or C line count, duplicated architectural logic, differential-test
results, and maintenance cost for each accepted stage.

Candidate work
--------------

### Framebuffer demonstration

Add guest framebuffer and input demonstration programs, configure those devices
in an image, and connect dirty-region callbacks to a canvas in the Risclet page.
Validate the complete guest-to-page path together. The current Risclet demo is
intentionally terminal-only.

### Boot and image loading

Evaluate a standard bootloader path, compressed kernel loading, and other image
features only against a concrete Alpine deployment problem. Linux already
handles compressed initrds after Riscbox loads them opaquely. Any new loader
must justify its code size and failure surface relative to image preparation.

### Non-CPU performance

Measure HTTP block request latency, 9p request/reply copy volume, concurrent
request latency, resident and logical 9p tree sizes, and peak lazy-load memory
only when those paths become a demonstrated bottleneck. Keep that work separate
from the CPU interpreter optimization baseline.

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
