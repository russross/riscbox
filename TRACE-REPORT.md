Trace comparison after hot-loop flattening
=========================================

Scope
-----

This report compares Riscbox at `bd55e35` with the archived TinyEMU WASM and
interpreter source. It uses the production release modules for generated-code
conclusions and the prepared xv6 kernel compile workload for timing. The C
archive is a performance reference, not a compatibility target.

Measured result
---------------

| Interpreter | Previous profile | Current profile | Guest timestamp, current |
|---|---:|---:|---:|
| Riscbox | 264.566 s | 203.275 s | 194.394 s |
| TinyEMU | 94.597 s | 98.558 s | 97.304 s |

Riscbox profile time fell 23.2% from the previous dispatcher-inlining result.
The Riscbox/TinyEMU ratio moved from 2.80 to 2.06, and the absolute profile
gap fell from 170.0 to 104.7 seconds. These are one paired run at each stage,
not a distribution of repeated trials. The current profiles are in
`images/xv6-profile/build/profiles/`; both guest logs contain
`XV6_PROFILE_BUILD_COMPLETE` and a clean poweroff request.

The current Riscbox module is 331,365 bytes, SHA-256
`36c7a6c0a73463707c2227748dfb481fdeec769926909d412fba63b749bb89cc`.
The archived TinyEMU module is 168,515 bytes, SHA-256
`5d4f9ae952f9b0bf57137455fb3461e9d1ffa90aba78241128b0421e03151676`.
Module size is context, not a measure of executed hot-path size.

Loop comparison
---------------

| Stage | Riscbox | TinyEMU |
|---|---|---|
| Page setup | Resolve the execute TLB once per chunk; establish an arena cursor and fast end (`src/cpu/mod.rs:429-453`). | Establish `code_ptr` and `code_end` (`c/riscv_cpu_template.h:289-305`). |
| Fetch | Scalar read from the arena cursor; checked page-tail path (`src/cpu/mod.rs:456-489`). | Scalar read from `code_ptr`; separate page-tail path (`c/riscv_cpu_template.h:306-323`). |
| Dispatch | Inlined in `Cpu::run` (`src/cpu/execute.rs:168-286`). | Directly in `riscv_cpu_interp_x64` (`c/riscv_cpu_template.h:336+`). |
| Cached guest data | Scalar inline arena access after a TLB hit; calls remain for checked slow paths (`src/cpu/mmu.rs:35-60,102-125`). | Inline TLB and scalar data access in instruction cases. |
| Sequential continuation | Advance arena cursor, track PC addend, account for the instruction, and check tail and budget (`src/cpu/execute.rs:88-158`). | Advance `code_ptr` and decrement a cycle counter; check block end (`c/riscv_cpu_template.h:266-324`). |

The production Riscbox WASM contains a 20,946-byte `Cpu::run` body with the
opcode dispatch inside it and no standalone `Cpu::execute`. Normal cached
`Cpu::load` and `Cpu::store` call boundaries are absent. The scalar arena
helpers use unaligned fixed-width operations (`src/memory/unchecked.rs:5-68`),
so the former bytewise instruction and data access concern no longer applies.
Calls from the run body mainly lead to checked slow paths, SYSTEM, traps, and
runtime services. These findings show that the round moved the ordinary
execution path substantially closer to TinyEMU's evaluation loop.

The V8 profile attributes 116,598 of 180,123 Riscbox samples to `Cpu::run`
itself and 47,086 to idle. `pmp_access_ok` has 4,168 samples,
`PhysicalMemory::read` 1,439, and `translate` 963. Sampling does not isolate
the cost of individual operations within `Cpu::run`; the rankings below are
structural hypotheses, not measured per-operation speedups.

Remaining differences, ranked
-----------------------------

1.  **Cursor and PC continuation.** TinyEMU's normal path advances one pointer
    and tests its block end. Riscbox also maintains `arena_fast_end` and
    `pc_addend`, constructs `InstructionAddress` values, and handles
    `InstructionOutcome` flow (`src/cpu/execute.rs:88-158`). The result
    transfer itself has been optimized away, but dependent cursor, PC, and
    continuation operations remain. This is the strongest candidate for the
    large `Cpu::run` self time.

2.  **Page-tail selection.** Riscbox checks `tail_fetch` on each cached
    instruction (`src/cpu/execute.rs:89-102`). TinyEMU establishes `code_end`
    at refill and selects its tail path at the block boundary. The Riscbox
    branch is likely predictable, so its magnitude requires measurement.
    Any change must preserve the 32-bit instruction case beginning at the
    page's final halfword, fixed by `bd55e35`.

3.  **Budget granularity.** Riscbox decrements and checks the remaining budget
    for each instruction (`src/cpu/execute.rs:44-47,135-158`). TinyEMU checks
    primarily at block boundaries (`c/riscv_cpu_template.h:275-279`). This
    may be a meaningful cost, but changing it alters exact `Cpu::run` budgets
    and potentially timer, interrupt, and device-event latency. Measure it as
    a separate experiment with that contract made explicit.

4.  **Slow memory and permission work.** PMP, translation, and physical reads
    remain visible in the profile. They can reflect TLB misses, page walks,
    MMIO, or page-tail paths rather than ordinary cached RAM access. Profile
    those paths separately before optimizing them.

Common integer and compressed helper calls are no longer major standalone
profile frames. The old recommendation to flatten cached memory calls or
replace bytewise arena access has been completed and should not drive the next
round. The current unchecked accesses rely on complete-page TLB validation,
the stable fixed arena during `Cpu::run`, and exclusive mutable access; the
checked paths continue to handle misses, page crossings, and devices.

Next measurement
----------------

First test a cursor-based continuation that reconstructs architectural PCs
only at exits and host-visible boundaries, then inspect its release WASM and
repeat the prepared xv6 profile. Evaluate page-tail selection separately.
Keep block-granular budget checks as an independent contract and performance
experiment. Run three interleaved Riscbox/TinyEMU trials for a final parity
claim.
