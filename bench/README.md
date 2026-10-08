Manual browser benchmarks
=========================

Run from the repository root:

```sh
make bench
make bench-profile
node bench/compare.mjs bench/results/baseline.json bench/results/candidate.json
```

These targets are manual only. They are not prerequisites of ordinary builds,
tests, release checks, demo acceptance, or GitHub workflows. Generated images,
downloaded sources, split assets, results, and profiles are ignored by Git and
excluded from release archives.

Workloads
---------

| Workload | Fixed work | Coverage |
|----------|------------|----------|
| `loop` | Integer recurrence with arithmetic, shifts, branches, and sixteen stack values | Best-case interpreter throughput with tiny code and data |
| `sqlite` | SQLite 3.50.4 speedtest1's main test set, with an in-memory database and memory-only temporary storage | Larger program text, indexes, joins, sorting, allocations, and pointer chasing |
| `compile` | Serial clean TinyCC builds and links of Adventure, ATC, Robots, Snake, and Spirhunt | Process startup, ext4 metadata, header/source reads, object writes, compilation, linking, and final disk synchronization |

The image uses Alpine 3.24.2, one hart, 256 MiB RAM, the repository's OpenSBI and
Linux payloads, and a writable 96 MiB ext4 HTTP disk split into 512 KiB chunks.
There is no guest network device. Preparation uses QEMU networking to install
guest packages and validate the programs. The existing RISC-V cross GCC builds
standalone static RV64GC/lp64d loop and SQLite binaries at `-O2`; measured
compilations use TinyCC. SQLite is built with `SQLITE_TEMP_STORE=3`,
`SQLITE_THREADSAFE=0`, and `SQLITE_OMIT_LOAD_EXTENSION`, and its benchmark requests
zero worker threads. The static binaries include their own libc; the compiled
games use Alpine's musl and ncurses.
Guest programs and inputs stay identical when changing the emulator build.

Timing and cache policy
-----------------------

The page downloads all immutable disk chunks with two concurrent transfers
before constructing the VM. Timed disk transport uses native HTTP-cache-only
fetches. A cache miss fails the run, and the server independently rejects disk
chunk requests after preload. There are no background transfers during samples.
The temporary Chrome profile has a 256 MiB disk-cache budget and is removed
afterward.

Preloading does not populate Rust's clean block cache or convert the drive to
an array disk. Rust keeps its ordinary initial 16 MiB cache, requests, eviction,
copied responses, and sector overlays. Writes remain session-local. Each
measured sample starts in a fresh VM, so only boot populates its Rust cache and
overlays. Workloads start after guest `sync` and a Linux page/dentry/inode-cache drop.
Additional clean builds within a compilation sample also synchronize and drop
guest caches. Rust cache misses can therefore still return to the browser cache.
Rust's cache is never explicitly flushed or enlarged within a VM.

One complete untimed warm-up precedes each workload's measured repetitions.
Warm-up uses a separate VM to warm browser execution without retaining its
device cache or disk overlays. Each measured sample uses fixed work counts and browser `performance.now()`
between guest console markers. Preparation, initial cleanup/cache dropping,
and output reporting are outside that interval. Compilation includes cleanup
between additional builds and its final `sync` and executable hashes. Loop and
SQLite output is small but includes its write to ext4. Guest-reported SQLite
times are retained as diagnostics, not used to rank runtimes: the emulator
calibrates guest time.

The host requires repeatable loop checksums, SQLite verification hashes, and
all five executable hashes. Image preparation independently checks a known
loop result and builds every game. These checks detect changed results; they
do not replace architectural or guest acceptance tests.

Run options and calibration
---------------------------

```sh
make bench BENCH_ARGS='--label baseline --output results/baseline.json'
make bench BENCH_ARGS='--loop-count 150000000 --sqlite-size 10 --compile-count 1'
node bench/run.mjs --help
```

Counts set loop iterations, SQLite's relative test size, and complete five-game
builds per sample. Defaults are starting counts, not a runtime-adaptive score.
Use one exploratory run to select counts that give roughly 10–20 host seconds
per workload on your machine, then freeze them for the series. Three measured
repetitions are the default. The runner prints medians and ranges and flags
samples shorter than ten seconds. Each command has a configurable timeout;
errors fail cleanly and close Chrome and the server.

Initial headed Chrome measurements on the development host put these defaults
near 14 seconds for the loop, 13 seconds for SQLite, and 14 seconds for one
five-game build. SQLite size 10 peaks near 7 MiB of SQLite-managed memory.
These are calibration guides, not promised performance on other machines.

Console markers are delivered at CPU-run/quantum boundaries. With the default
20 ms quantum, very short samples have coarse timing; the 10–20 second target
keeps that boundary error small relative to useful build comparisons.

Keep Chrome foregrounded during headed measurements and minimize unrelated
host load. The runner disables background timer throttling and renderer
backgrounding, uses headed Chrome when `DISPLAY` exists, and otherwise uses
headless Chrome. `CHROME` overrides its executable. Compare like browser and
host configurations and alternate build order to reduce drift.

Arbitrary runtime builds
------------------------

```sh
make bench-image
node bench/run.mjs --runtime /path/to/extracted-release --label candidate \
    --output bench/results/candidate.json
```

The directory must contain `riscbox.js` and `riscbox.wasm` with the current
public client API. The runner copies both into temporary storage before
starting, so subsequent builds cannot change an active run. Boot payloads and
the image remain the benchmark fixture; selecting a runtime does not replace
them. Without `--runtime`, it uses `build/js/riscbox.js` and the current local
release WASM. The runner works from any directory; explicit relative option
paths resolve against that invocation's working directory. Through Make,
`BENCH_ARGS` paths resolve from `bench/`.

For a Rust optimization comparison, build isolated target directories:

```sh
CARGO_TARGET_DIR=bench/build/rust-o3 CARGO_PROFILE_RELEASE_OPT_LEVEL=3 \
    cargo build --release -p riscbox-wasm --target wasm32-unknown-unknown
wasm-opt -O3 -o bench/build/rust-o3/wasm32-unknown-unknown/release/riscbox_wasm.wasm \
    bench/build/rust-o3/wasm32-unknown-unknown/release/riscbox_wasm.wasm
mkdir -p bench/build/runtime-o3
cp build/js/riscbox.js bench/build/runtime-o3/riscbox.js
cp bench/build/rust-o3/wasm32-unknown-unknown/release/riscbox_wasm.wasm \
    bench/build/runtime-o3/riscbox.wasm
node bench/run.mjs --runtime bench/build/runtime-o3 --label rust-o3
```

C remains `-O3`, LTO remains enabled, and the same `wasm-opt -O3` pass is applied.
The suite itself has no special knowledge of compiler variants and also
supports interpreter, platform, scheduling, and storage comparisons.

Rust optimization comparison
----------------------------

On 2026-10-07, source `f2cb4ce5ee02cbc63f8843140cc6afe5398d6f9c` was
measured on an Intel Core i9-9900K with Linux 6.1.0-42-amd64 and headed
Chrome 152.0.7977.82. Isolated builds used Rust 1.98.1, Clang 19.1.7,
and wasm-opt 120. Only Rust release optimization changed: `s` versus `3`.
C stayed at `-O3` with byte-identical core archives; LTO and `wasm-opt -O3`
were identical, as were the browser adapter and guest image.

Four default, unprofiled suite runs used the order `Os, O3, O3, Os`, giving
six measured samples per workload per variant. All correctness signatures
matched and all timed disk fetches came from the browser cache.

| Workload       | Rust Os median | Rust O3 median | O3 time change |
| -------------- | -------------- | -------------- | -------------- |
| Integer loop   | 13.609 s       | 13.397 s       | -1.6%          |
| SQLite memory  | 13.072 s       | 13.117 s       | +0.3%          |
| TinyCC builds  | 14.351 s       | 14.319 s       | -0.2%          |

| WASM size      | Rust Os        | Rust O3        | O3 increase    |
| -------------- | -------------- | -------------- | -------------- |
| Raw bytes      | 374,730        | 487,264        | 30.0%          |
| Gzip bytes     | 166,095        | 200,268        | 20.6%          |

The aggregate loop difference did not repeat in the reverse-order pair:
the final Os batch median was 13.417 s versus 13.427 s for the preceding
O3 batch. SQLite and compilation showed no consistent meaningful gain.
These measurements support retaining the default Rust `-Os` for its size
advantage; they do not establish a repeatable performance benefit from `-O3`.
Local raw batches, combined records, build metadata, and summary are saved
under `bench/results/rust-opt-*.json`; generated results remain untracked.

Results and profiles
--------------------

JSON records contain the full fixture hash/configuration, Alpine package
versions, workload counts/version, cache policy, repetitions, quantum duration,
browser version, host OS/CPU, runtime label and exact adapter/WASM SHA-256
hashes, raw/gzipped WASM sizes, individual elapsed times, disk fetch requests
and bytes, and correctness output. Names and output paths are customizable;
default files live in `bench/results/`.

Each sample also retains the public `speed()` snapshot as a diagnostic. Its
rolling active-time windows can include setup or preceding work, so comparison
uses the sample's browser elapsed time rather than these averages.

Comparison uses median elapsed time and reports time changes and speedup for
each workload, plus WASM size changes. It rejects different fixtures, counts,
cache policies, browser/host configurations, quantum durations, profiling
modes, malformed samples, or correctness mismatches. Repetition counts may
differ. No combined score hides workload-specific changes.

`--profile` or `make bench-profile` exports one `.cpuprofile` per measured
repetition beside the JSON file, named by workload and repetition. Guest boot
and the separate warm-up VM are excluded. Import
these into Chrome DevTools for inspection. Profiled timing records are
marked and cannot be compared against ordinary timing runs. Sampling uses
the [DevTools Profiler protocol](https://chromedevtools.github.io/devtools-protocol/tot/Profiler/)
at a one-millisecond interval and needs no automation dependencies.

Preparation and validation
--------------------------

The image builder needs the existing boot-build prerequisites plus QEMU,
fakeroot, cpio, e2fsprogs, unzip, curl, tar, and the cross compiler's static libc
development files (`libc6-dev-riscv64-cross` on Debian/Ubuntu).
`CROSS_COMPILE` overrides the compiler prefix. SQLite and Alpine source archives
are hash-pinned. Alpine package repository updates can change a rebuilt image;
retain the same generated fixture for comparisons. Package versions and the
full image hash make that distinction explicit. Delete
`bench/build/rootfs.ext4` to deliberately rebuild it.

```sh
node --test bench/report.test.mjs
node bench/run.mjs --loop-count 1000 --sqlite-size 1 --compile-count 1 \
    --repetitions 1 --output bench/results/smoke.json
```

Reporting checks and short browser smoke runs are also manual. The smoke run
validates execution and result handling; its counts are too short for useful
performance conclusions. `make -C bench clean` removes generated image and
assembly files while retaining saved results.
