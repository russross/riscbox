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

### Support more boot options

Stop using `fw_jump` and switch to `fw_dynamic` with appropriate platform setup changes. Fill out boot-time config options to include optional memory layout controls (kernel load location but also device placement). Make sure common configs are all supported:

* OpenSBI firmware included or not (see xv6 on qemu), compressed or not
* kernel at default/custom load location, compressed or not
* initrd included or not, compressed or not
* U-Boot to support booting ISO images or other plain install media with kernel/initrd in the image (OpenSBI -> U-Boot, which finds the attached image and finds its GRUB or whatever and boots)
* multiple attached block devices: http drive or plain supplied image, the latter with readonly, CoW, or mutable options

### More controls from the host

Audit and improve hooks for the host app to control the guest:

* Hard reboot with same initial boot setup (discard CoW block device changes, 9p server is outside the machine so it is unaffected)
* Reboot with CoW block device changes intact
* Suspend/resume
* Shutdown request (signals OS so it can flush buffers and shut down gracefully), then frees up resources
* Hard kill
* Lower CPU use mode (sleep delays between execution quanta), with ability for the host to adjust it

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
