Trace Comparison After Dispatcher Inlining
==========================================

Scope and conclusion
--------------------

This report compares the optimized production Riscbox WASM at commit
`cde7cf0bf6ed369a9fb5195056fcc6900a49bc02` with the repository's archived
TinyEMU WASM. It examines the page-local interpreter path after moving
`Cpu::run` and `Cpu::execute` into one module and applying WASM-specific
`#[inline(always)]` to `execute`.

The optimization succeeded at its immediate goal. `Cpu::execute` is fully
inlined into `Cpu::run`: the production module has no `Cpu::execute` function,
no ordinary dispatch call, and the integer and compressed opcode dispatch is
lexically present in the generated `Cpu::run` body. The typed
`Result<InstructionOutcome, Trap>` abstraction also no longer exists as a
material returned object or caller/callee transfer in WASM.

It did not make the complete page-local loop equivalent to source-level
flattening. PC reconstruction, the fetch-tail test, the SYSTEM precheck,
per-instruction counter accumulation, the page comparison, and the budget
comparison remain in the ordinary generated loop. Several subordinate execute
helpers and all normal guest data loads and stores also remain calls. Thus the
11.3% profile-time improvement previously measured is consistent with removal
of one important boundary, but the generated loop still differs materially
from TinyEMU.

Artifacts
---------

| Artifact | SHA-256 | Size |
|---|---|---:|
| `target/wasm32-unknown-unknown/release/riscbox_wasm.wasm` | `45b83ff87f00aadee229c8fc0dd7213778c787dc85445145ea164b08e44fa612` | 320,007 bytes |
| `c/js/riscbox-wasm.wasm` | `5d4f9ae952f9b0bf57137455fb3461e9d1ffa90aba78241128b0421e03151676` | 168,515 bytes |

The checked-in TinyEMU module is stripped. A line-aware `-O3 -g3` rebuild from
the same archived C source was used only to identify
`riscv_cpu_interp_x64`; conclusions about its loop were checked against both
the archived production module and `c/riscv_cpu_template.h`. The Riscbox
conclusions below use the production release module, not a debug build.

The toolchain was Rust 1.92.0 (`ded5c06cf`, LLVM 21.1.3), Cargo 1.92.0, and the
WABT `wasm-objdump` and `wasm2wat` installed in `/usr/bin`.

Inlining result
---------------

Production Riscbox contains:

```text
func[88] Cpu::run        size=7,635 bytes
func[87] execute_system  size=1,972 bytes
```

It contains no symbol or function body for `Cpu::execute`. Within `Cpu::run`,
the disassembly directly contains compressed decode, the 32-bit opcode
dispatch, direct register updates and sequential-PC calculation, and branches
to shared trap and chunk-exit handling. Calls remain only to helpers below
`execute`, such as `load`, `store`, `execute_immediate`, compressed ALU/jump
helpers, `execute_system`, and floating-point execution.

Before this change the release module had a 1,757-byte `Cpu::run`, a
5,560-byte `Cpu::execute`, and an ordinary `call Cpu::execute`. The new
7,635-byte body and absence of that function and call prove actual inlining;
function size is corroborating evidence, not the test.

Outcome abstraction
-------------------

The `Result<InstructionOutcome, Trap>` representation is optimized across the
inlined boundary:

*   There is no outcome return area, packed result copy, or discriminant passed
    between functions.
*   Sequential cases assign the next PC directly to an `i64` local and branch
    to shared loop continuation code.
*   Exceptional cases branch directly to shared trap handling.
*   Retirement and exit information become locals and control-flow edges.

This is equivalent to what lexical flattening should accomplish for result
transfer. It is not equivalent for the computations that produced the fields:
the generated continuation still consumes a next-PC local, a retirement
accumulator, and distinct sequential/exit edges.

Ordinary-path overhead audit
----------------------------

| Item | Result after inlining | WASM evidence |
|---|---|---|
| `Cpu::execute` call | Eliminated | No function or call exists; dispatch is in `Cpu::run`. |
| `Result`/outcome transfer | Eliminated | No material result aggregate or cross-function tag transfer. |
| PC reconstruction | Remains | Each cached iteration forms `virtual_page | next_offset` as an `i64` instruction PC. |
| Page comparison | Remains | Continuation computes `next_pc & -4096`, compares it with the cached virtual page, and combines that result with the cached-fetch flag. |
| Fetch-tail test | Remains | Each cached fetch compares the page offset with 4093 before choosing the four-byte or page-tail path. |
| Budget check | Remains | Continuation increments the cycle local and compares it with the budget before looping. LLVM changes the source `<` test to an equivalent `!=` under established invariants, but does not remove it. |
| SYSTEM precheck | Remains | Before opcode dispatch, generated code compares `instruction & 127` with 115, commits PC/counters, and checks the host interrupt-state byte. |
| Counter bookkeeping | Remains, partly improved | Two local accumulators are incremented on an ordinary retired instruction, and the total cycle count is incremented for the budget test. Architectural counters are updated at SYSTEM/chunk/run exits, not on every ordinary instruction. |

The generated ordinary continuation is structurally equivalent to:

```text
pending_retired += retired
pending_cycles += 1
if cached_fetch && (next_pc & !0xfff) == virtual_page:
    next_offset = next_pc & 0xfff
    cycles += 1
    if cycles != budget:
        continue inner_loop
exit chunk
```

The typed source model has therefore compiled away where it was merely a data
transfer mechanism. The explicit source operations after `execute` have not
compiled away because their values remain observably necessary under the
current loop contract.

Hot-loop comparison with TinyEMU
--------------------------------

TinyEMU's line-aware optimized interpreter is one 17,915-byte
`riscv_cpu_interp_x64` function. Its page-local organization remains the
reference difference:

| Stage | TinyEMU | Current Riscbox |
|---|---|---|
| Cached fetch cursor | Linear WASM address (`code_ptr`) | Virtual page, arena page, and page offset; reconstructs PC |
| Ordinary fetch | One `i32.load align=2` | Four `i32.load8_u` operations and shifts/ORs |
| Tail handling | `code_ptr >= code_end` at block refill boundary | `page_offset < 4093` test before every cached fetch |
| Dispatch | In the interpreter body | Now also in `Cpu::run` |
| Sequential advance | Add 2 or 4 to `code_ptr` | Produce `next_pc`, compare its page, mask it back to an offset |
| Guest data fast path | Inline TLB check and scalar WASM load/store | Call `Cpu::load`/`Cpu::store`; cached RAM copy is byte-oriented |
| Budget | Decrement every instruction, test at block boundary | Increment and test every instruction |
| SYSTEM recognition | Opcode dispatch | Separate precheck, then opcode dispatch |
| Retirement | Direct update in instruction paths | Local retirement and cycle accumulators, committed at exits |

The current `Cpu::run` has 19 static call sites to `Cpu::load` and nine to
`Cpu::store`, representing the decoded memory operations. TinyEMU's normal
aligned RAM cases are inline; calls from its memory cases are slow paths.
Riscbox's calls are statically dispatched monomorphized calls, not trait-object
dispatch, but they remain ordinary guest load/store call boundaries.

The scalar-load difference is concrete. `memory::unchecked::read_u32` is
written as four unchecked byte reads followed by `u32::from_le_bytes`; release
WASM preserves four `i32.load8_u` instructions. `read_width` and `write_width`
similarly copy supported widths byte by byte on cached aligned RAM accesses.
TinyEMU emits an `i32.load align=2` for its ordinary instruction fetch and
scalar WASM loads/stores for aligned cached guest data. These operations are
unaligned-safe in WASM; their alignment immediate is an optimization hint, not
a trapping precondition.

Profile evidence available from the completed milestone run supports looking
at the data-memory path next. In
`images/xv6-profile/build/profiles-inline/riscbox.cpuprofile`, self samples were:

```text
Cpu::run       129,710 / 237,308 total samples
Cpu::load       22,865 / 237,308
Cpu::store      10,784 / 237,308
```

`Cpu::load` and `Cpu::store` therefore account for 14.2% of all samples and
17.8% of non-idle samples before counting any inlined address/decode work in
`Cpu::run`. This does not measure the cost of byte-wise instruction fetch,
which is charged to `Cpu::run`.

Ranked remaining differences
----------------------------

### 1. Scalar arena access and guest-memory call boundaries

Evidence: ordinary instruction fetch is four byte loads versus TinyEMU's one
`i32.load`; cached aligned guest reads and writes copy bytes; `Cpu::load` and
`Cpu::store` retain call boundaries and together have 17.8% of non-idle profile
samples.

Inference: this is the strongest isolated next optimization candidate. Narrow
typed unaligned scalar arena helpers can preserve the existing full-page and
fixed-arena proof while allowing LLVM to emit `i32.load`, `i64.load`, and the
matching stores. After that, compiler-directed inlining or a page-hit-specific
fast helper can be evaluated separately for the normal load/store call
boundary. This need not broaden unsafe code beyond the arena module.

### 2. PC/cursor and page-exit organization

Evidence: every cached iteration reconstructs a `u64` PC from page plus offset,
then the continuation masks the resulting PC, compares its page, and masks it
again for the next offset. TinyEMU advances one linear cursor and checks its
end at the block boundary.

Inference: these dependent operations are likely a major part of the remaining
`Cpu::run` self time. A page-local arena cursor plus a separately maintained
architectural PC could remove work without changing the safe chunk/TLB proof.
This should be tested in generated WASM rather than assumed from source shape.

### 3. Per-instruction fetch-tail and budget branches

Evidence: both branches remain in the generated inner loop. TinyEMU derives
the fetch bound once per block and checks budget only at block boundaries.

Inference: hoisting the normal fetch region from its final two-byte tail is a
semantics-preserving candidate. Budget granularity is a runtime-contract
choice and should not be mixed into that experiment. These branches may be
well predicted, so their importance relative to scalar loads and cursor work
requires measurement.

### 4. Remaining common helper calls

Evidence: `execute_immediate`, `execute_register`, `execute_word_register`, and
compressed ALU/jump helpers remain calls inside `Cpu::run`. The prior profile
attributes 3,664 samples to `execute_immediate`, 3,783 to compressed jump,
1,468 to compressed ALU, and 1,229 to `execute_register`. TinyEMU keeps the
corresponding integer operations in its interpreter body.

Inference: selected compiler-directed inlining of common integer helpers may
produce another measurable gain. It should follow scalar access work or be
tested independently because indiscriminate inlining can increase dispatch
code pressure. Floating point, SYSTEM, traps, and other uncommon helpers are
reasonable cold calls.

### 5. SYSTEM precheck and counter state

Evidence: the generic opcode comparison remains on every instruction, and the
SYSTEM case commits three architectural counters before dispatch. Ordinary
instructions increment two pending counters plus the budget counter.

Inference: moving the host-state synchronization into a SYSTEM-specific cold
edge and consolidating loop counters may help, but current evidence does not
rank it above the concrete scalar-memory and cursor differences. Counter
visibility semantics must be retained for CSR reads and host exits.

Safety implications
-------------------

The evidence does not justify general unsafe interpreter code. TLB fill already
proves that a complete page is resident in the fixed arena, and page-local
fixed-width access already has narrow unsafe helpers. The immediate opportunity
is to make those helpers perform one unaligned little-endian scalar operation
instead of several bytes, with the same documented bounds and arena-stability
invariants. Cursor restructuring and helper inlining are safe control-flow
changes. Broader unsafe state access would not address the measured call,
scalar-load, PC, page, fetch-tail, or budget differences.

Reproduction
------------

From repository root at the recorded commit:

```sh
make wasm
sha256sum target/wasm32-unknown-unknown/release/riscbox_wasm.wasm \
    c/js/riscbox-wasm.wasm
wasm-objdump -x target/wasm32-unknown-unknown/release/riscbox_wasm.wasm \
    > /tmp/riscbox-current.x
wasm-objdump -d target/wasm32-unknown-unknown/release/riscbox_wasm.wasm \
    > /tmp/riscbox-current.d
wasm2wat target/wasm32-unknown-unknown/release/riscbox_wasm.wasm \
    -o /tmp/riscbox-current.wat
wasm-objdump -d c/js/riscbox-wasm.wasm > /tmp/tinyemu-current.d
```

Locate the production Rust function and prove the old boundary is absent:

```sh
rg 'GT\$3run17h|GT\$7execute17h' /tmp/riscbox-current.x
rg 'call .*GT\$7execute17h' /tmp/riscbox-current.d
```

The first command reports `Cpu::run` but no exact `Cpu::execute` symbol; the
second reports no match. Inspect the complete run body in
the WAT rather than relying on names alone:

```sh
rg -n '^  \(func .*Cpu.*run|^  \(func .*execute_system' \
    /tmp/riscbox-current.wat
```

An optimized Rust artifact with DWARF can be built outside the repository
target directory for source-line investigation. Its code layout can differ
slightly, so use it for mapping only:

```sh
CARGO_TARGET_DIR=/tmp/riscbox-trace-current \
CARGO_PROFILE_RELEASE_DEBUG=2 \
    cargo build --release -p riscbox-wasm --target wasm32-unknown-unknown
```

No new long profiler workload was run for this report. Profile sample counts
come from the milestone artifacts already under
`images/xv6-profile/build/profiles-inline/`.
