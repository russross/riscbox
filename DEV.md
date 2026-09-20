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

### Stage 2: page-chunk interpreter loop

Port the execution structure from `c/riscv_cpu_template.h` while retaining the
Rust decoder and architectural state. TinyEMU keeps `code_ptr`, `code_end`, a
PC addend, remaining cycles, and counter addends in locals. It translates code
only when the current span is exhausted and materializes state at block or CSR
boundaries. Reproduce those properties directly.

Split `Cpu::run` into an outer block loop and an inner page-chunk loop. Add a
private execute span with data equivalent to:

```text
ExecuteChunk {
    virtual_page: u64,
    arena_page: ArenaOffset,
    next_offset: u16,
    end_offset: u16,
}
```

The outer loop owns the remaining `u32` budget, counter deltas, and the
architectural PC at each refill boundary. A refill helper accepts the current
PC, checks its alignment, probes the execute TLB once, and uses the existing
checked translation and bus path on a miss. A successful entry covers the
complete 4 KiB virtual and arena page, so the chunk ends at that page boundary.
A non-RAM instruction mapping remains a one-instruction checked slow path.

The inner loop derives PC from the virtual page and offset, fetches one
little-endian `u32`, passes that local PC into execution, advances by two or
four bytes, and decrements the local budget. Sequential instructions stay in
the loop without writing `self.pc`. A taken branch,
jump, trap, privilege transition, execute-TLB flush, WFI, or other nonsequential
PC installation ends the chunk and returns to the refill boundary, matching
TinyEMU's `JUMP_INSN`. Do not add a second TLB lookup for same-page branch
targets in this milestone.

When fewer than four bytes remain in the page, read the low halfword from the
chunk. Execute it directly if compressed. Otherwise read the high halfword
through the checked execute-load path at `pc + 2`, preserving translation, PMP,
and precise faults for a straddling instruction. The following sequential
instruction forces a refill. This corresponds to TinyEMU's
`code_ptr >= code_end` slow path.

Make PC flow explicit rather than mutating and rereading `self.pc`. Change the
private signatures to the equivalent of `execute(&mut self, bus: &mut B, pc:
u64, instruction: u32, length: u64) -> Result<InstructionOutcome, Trap>` and
`execute_compressed(&mut self, bus: &mut B, pc: u64, instruction: u16) ->
Result<InstructionOutcome, Trap>`. The outcome contains `next_pc`, `retired`,
and an `InstructionFlow` with sequential and exit variants. Thread `pc` through
compressed jump and system helpers that currently read or write `self.pc`.
Trap-return helpers return their target rather than installing it directly.
System instructions that flush translations or change privilege, WFI, taken
control flow, and trap returns report an exit. Arithmetic, memory operations,
untaken branches, and non-flushing CSR operations remain sequential. Only the
outer loop and trap entry commit `self.pc`.

### Stage 3: block-local counters and event sampling

Keep PC and a private `RunCounters { elapsed: u32, cycle: u32, retired: u32 }`
in loop locals and commit their wrapping deltas together on every chunk exit
and final return. The remaining budget derives from `elapsed`, so there is one
per-instruction decrement/increment rather than three architectural stores.
Follow TinyEMU's CSR rule: materialize pending deltas before reading cycle or
instret CSRs; after a write, rebase the corresponding local delta. An
instruction that writes
`minstret` must not also retire into the newly written value. Pass the pending
deltas into CSR execution explicitly or materialize them before calling the
existing CSR helpers; do not let CSR helpers observe stale architectural
counters. Exceptions and interrupts consume one cycle but do not retire;
completed instructions retire once.

At each outer boundary, commit the previous chunk, then stop for an exhausted
budget or WFI. Poll `CpuBus::take_interrupt_state_changed` only there. If it
reports a change, return to `Machine::run`, whose existing pre/post
`sync_interrupts` calls update CPU interrupt state. This bounds device-originated
latency by a page chunk and the browser's 200,000-cycle block budget.

Check interrupts at the same boundary, but gate all privilege and priority work
on `self.mip & self.mie != 0`. Only then call the existing selection logic.
Taking an interrupt consumes one cycle and begins a new chunk. CSR operations
that can make a pending interrupt newly takeable must exit the chunk: writes to
`mip`, `mie`, `mideleg`, interrupt-enable fields in `mstatus` or `sstatus`, and
the state changes performed by `mret` and `sret` need explicit classification.

Do not impose another instruction cap. With compressed instructions, one 4 KiB
page contains at most 2,048 instructions, already well below the host budget.

### Stage 4: validated unchecked arena access

Once the block loop is correct and measured, remove bounds-result plumbing from
TLB hits. Make each entry carry a validated page window rather than an
unqualified offset. Only `fill_tlb`, after a successful
`CpuBus::ram_range(physical_page, PAGE_SIZE, write)`, may construct it. Its
invariant is that the complete page was inside the fixed arena, the arena cannot
resize during `Cpu::run`, and every fast-path offset stays in that page.

Put unsafe operations in one internal memory module. Replace the crate-wide
`unsafe_code = "forbid"` setting with a policy that permits unsafe only there,
and update `AGENTS.md` in the same change. Expose narrow safe wrappers backed by
`get_unchecked` and `get_unchecked_mut`, with fixed-width little-endian helpers.
Document the validated-page, width, arena-stability, and exclusive-borrow
invariants at every unsafe block.

Use unchecked `u32` access for common instruction fetch and `u16` for the
page-end halfword. Use the same helpers for aligned read/write TLB hits after
proving `page_offset + width <= PAGE_SIZE`. Misaligned and cross-page accesses,
misses, MMIO, page walks, A/D updates, PMP checks, dirty marking, and mapping
changes stay checked. Remove `Option` only from proved hit branches.

Audit invalidation before enabling this. `flush_tlb` already covers address
space, PMP, privilege, and relevant status changes, while
`invalidate_write_range` covers writable entries for host memory lifecycle
changes. If a host remap can stale read or execute entries, extend invalidation
to all overlapping TLB kinds; an unchecked entry may not outlive its page proof.

### Tests, measurement, and landing order

Land separately measurable changes: execute chunks and explicit flow results;
local counters and boundary event checks; unchecked instruction fetch; then
unchecked aligned data hits. Do not combine release tuning, pointer-cached CPU
state, decoder changes, or a C core boundary with this milestone.

Test both page-end instruction positions, cross-page 32-bit fetch faults,
same-page and cross-page branches, traps and interrupts at refills, enabling a
pending interrupt by CSR, WFI, mid-chunk counter reads/writes, and budgets zero,
one, and mid-page. Extend `tests/cpu_fast_path.rs` differential coverage for all
widths and alignments, arena/page endpoints, self-modifying code, PMP rejection,
Sv39 remapping, and host invalidation. A counted test bus must prove event polls
occur once per chunk rather than once per instruction.

For each step, run focused CPU, MMU, machine, and browser tests, then
`make check` and a clean `make wasm`. Inspect generated WASM to verify that the
sequential inner loop has no execute-TLB probe, slice-bounds branch, interrupt
priority scan, event poll, or architectural counter store. Measure repeated xv6
build runs, module size, and V8 profiles. Retain a step only if it improves
end-to-end throughput without regressing xv6 or prepared Alpine boot, login,
and clean shutdown.

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
