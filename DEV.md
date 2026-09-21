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
5.  Bring the Rust interpreter hot loop near TinyEMU throughput before pursuing
    non-CPU optimization. Measure generated WASM and end-to-end guest work at
    every stage; preserve checked slow paths while allowing narrowly proved
    unsafe operations where they remove measured overhead.

CPU hot-loop optimization
-------------------------

### Problem and evidence

The page-chunk interpreter reduced the prepared xv6 compile profile from
371.725 to 298.194 seconds, but the paired TinyEMU profile completes in 93.728
seconds. Riscbox therefore remains 3.18 times slower on the current end-to-end
CPU workload.

`TRACE-REPORT.md` aligns the optimized WASM with both source trees. Ordinary
Riscbox instructions still cross an uninlined `Cpu::run -> Cpu::execute` call;
the callee is about 5.5 KiB and returns a `Result<InstructionOutcome, Trap>`.
The caller then updates four loop values, checks flow and the virtual page, and
reconstructs the next page offset. TinyEMU keeps decode, dispatch, cursor
advance, and accounting in one function. Its calls are predominantly slow
paths. Riscbox's TLB-hit instruction and aligned data accesses already use
unchecked fixed-width arena helpers after full-page validation, so additional
unchecked memory access is not the first optimization target.

The next milestone is structural rather than stylistic: use generated-code and
profile evidence to remove costs from the Rust WASM hot loop. Readability and
type boundaries may yield inside this one measured subsystem when a benchmark
justifies the trade, but the implementation should remain idiomatic Rust where
the abstractions compile away. The platform, MMU misses, MMIO, traps, system
operations, and browser ABI remain explicit Rust code.

### Scope and acceptance

Optimize `Cpu::run`, ordinary integer/compressed dispatch, instruction fetch,
and block-local accounting. Preserve the existing architectural state, helper
implementations, `CpuBus` interface, fixed arena, TLB representation, and raw
WASM ABI. Do not optimize floating point, page walks, devices, storage, or the
JavaScript adapter unless a new profile identifies them after the loop work.
Do not import the archived C implementation piecemeal. Take the Rust design as
far as measurements support before considering a different implementation
language as a separate project decision.

Use `tools/profile-xv6` and the prepared image for every performance decision.
Record the optimized WASM size, guest completion timestamp, V8 profile time,
hot function sizes, and whether the ordinary instruction path contains calls.
Run three interleaved Riscbox/TinyEMU measurements for the final Rust candidate
and compare medians; earlier stages may use one paired run to reject a
regression. Use the profile and generated WASM after each stage to decide
whether another bounded Rust change has credible leverage. Performance targets
and any decision to replace the loop with C remain outside this design.

Every landed stage must pass focused CPU fast-path and architectural tests,
`make check`, a clean `make wasm`, the xv6 profiling workload through poweroff,
and prepared Alpine boot through login and clean shutdown. Compare CPU state
and writable RAM against the pre-stage implementation for representative
instruction streams containing sequential 16- and 32-bit instructions,
taken and untaken control flow, loads and stores, atomics, system operations,
exceptions, interrupts, page ends, self-modifying code, and exact counter
observations. Reject changes that only move profile samples or trade a material
guest regression for smaller microbenchmark time.

### Data and control flow

Keep architectural addresses as `u64` and validated arena offsets as `u32`.
At execute-TLB refill, derive scalar page-local state equivalent to:

```text
virtual_page: u64
arena_cursor: u32
arena_fast_end: u32
page_offset: u16
remaining_cycles: i64
retired_delta: u32
```

`arena_fast_end` identifies the last cursor at which an unchecked four-byte
fetch is valid. The full-page TLB proof and fixed-arena lifetime continue to
justify `read_u16` and `read_u32`; no raw pointer or arena borrow may survive a
call that can mutate the bus. Reconstruct a `u64` PC from `virtual_page` and
`page_offset` only for PC-relative instructions and cold exits.

Put the ordinary opcode match lexically in the page-local loop, likely by
moving `Cpu::run` into `src/cpu/execute.rs` beside its private execution
helpers. Sequential arms update the cursor directly and do not construct an
outcome. Taken control flow, traps, system instructions, host-visible events,
and page-tail fetches leave through a small private `HotLoopExit` enum or a
dedicated cold helper. This enum exists only at block exits; it must not be a
per-instruction return value. Keep a one-instruction checked executor for
non-RAM mappings and tests, even if limited dispatch is duplicated to keep the
RAM loop flat.

The intended private boundary is equivalent to
`run_page_chunk<B: CpuBus>(&mut self, bus: &mut B, pc: u64,
remaining_cycles: i64, retired_delta: u32) -> HotLoopExit`. `HotLoopExit`
carries the next architectural PC, consumed cycles and retired delta, plus one
of sequential-page-end, control-flow, trap, system/event, or budget reasons.
If passing those values prevents LLVM from retaining them in locals, fold this
boundary into `Cpu::run`; the generated-code requirement takes precedence over
the helper signature. The existing checked `execute` signature remains for
the uncommon non-RAM path until measurements justify removing it.

### Milestones

1.  Establish the generated-code gate. Add a reproducible inspection command
    or script that builds release WASM, reports the relevant function sizes,
    and fails or clearly reports if the ordinary page-local path still calls a
    full instruction dispatcher. Preserve the current profile artifacts as the
    comparison point; do not treat WASM size alone as a performance result.

2.  Flatten 32-bit integer dispatch into the page-local loop. Remove
    `InstructionOutcome` construction and the `Cpu::execute` call from the
    ordinary RV64 integer path. Keep memory, atomic, floating-point, and other
    substantial opcode bodies as helpers where measurements show their calls
    are not dominant. Sequential instructions advance scalar cursors; taken
    branches and exceptional results use cold exits. Measure before expanding
    more helpers inline, since instruction-cache growth can erase dispatch
    gains.

3.  Integrate compressed dispatch and cursor fetch. Give compressed sequential
    instructions the same direct cursor contract instead of returning an
    outcome. Split the normal four-byte fetch region from the final page
    halfword so the common path has no per-instruction tail test. A 32-bit
    instruction at the final halfword continues through the checked execute
    load and reports a precise second-page fault.

4.  Consolidate accounting and cold boundaries. Use one local remaining-cycle
    value and one retired delta, committing architectural PC, cycle, elapsed,
    and `instret` only at exits. Move the generic SYSTEM opcode precheck into
    system dispatch while preserving the requirement to materialize counters
    before CSR reads/writes and to return immediately when host interrupt state
    changes. Verify `minstret` writes, non-retiring traps, WFI, and interrupt
    priority explicitly.

5.  Evaluate block-granular budgets independently. First retain exact budget
    checks so the flattened-loop gain is measured without a semantic change.
    Then benchmark a TinyEMU-style signed remaining count checked only at page
    or control-flow boundaries. If retained, document that `Cpu::run` may
    overshoot by at most one page-local block, update `RunOutcome` and browser
    scheduling tests, and verify timer and device-event latency. Do not combine
    this contract change with dispatch flattening.

6.  Reprofile before adding unsafe code. Inspect the final Rust WASM for calls,
    spills, bounds checks, indirect dispatch, and oversized functions. Add a
    narrower unsafe helper or raw arena pointer only when a specific remaining
    instruction in the generated hot path and a paired benchmark justify it.
    Keep each unsafe block in `src/memory/unchecked.rs` with explicit page,
    width, arena-stability, and aliasing invariants. Do not translate C pointer
    patterns mechanically into pervasive Rust `unsafe`: keep ownership,
    architectural state, control-flow exits, and slow-path interfaces typed and
    safe, and use unsafe only at the narrow memory operations whose proofs
    cannot be expressed without it.

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
