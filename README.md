Riscbox virtual platform
========================

Riscbox is an emulator that runs a RISC-V 64 VM in a browser. The runtime is
WebAssembly (WASM) with a small JavaScript adapter.

This is a fork of Fabrice Bellard's TinyEMU, but there are so many forks out
there that a name change seemed like a good idea. The core emulation loop is
written in C and is an evolution of TinyEMU, but the surrounding platform is
written in Rust and adds more device support and some modernization.

Start with [HOWTO.md](HOWTO.md) for narrated embedding, storage, shutdown,
recovery, and deployment workflows. [API.md](API.md) defines the current
JavaScript calls, options, types, and enforced VM-state contracts. The
[live demo](https://russross.github.io/riscbox/) runs a plain Alpine app around
exactly the assets in a release; its
[source and build instructions](https://github.com/russross/riscbox/tree/main/demo)
are in `demo/`. Contributor setup and
explicit local validation commands are in
[BUILDING.md](https://github.com/russross/riscbox/blob/main/BUILDING.md).

Release contents
----------------

Each release has one `riscbox-VERSION.tar.gz` archive containing `riscbox.js`,
`riscbox.wasm`, `riscbox.d.ts`, the optional `network/` modules, the standalone
`splitimg.py`, and hash-named gzip Linux/OpenSBI/U-Boot payloads. It includes this
README, API/HOWTO guides, implementation storage/protocol references, changelog,
and license. Guest root filesystems, example apps, and image build scripts are
separate application assets.

Embedding applications use the JavaScript adapter. Raw WASM exports, scheduling,
packet buffers, and internal storage handles are implementation details. The
source's native configuration parser remains for development tests. Documentation
and declarations describe the current release; no legacy entry points or
cross-release stability guarantees are provided.

Supported platform
------------------

The target is one little-endian RV64 hart with M/S/U modes and Sv39, following
QEMU `virt` sufficiently to boot current xv6 and deliberately prepared Alpine.
Devices include a 16550A UART, optional VirtIO console, Goldfish RTC, PLIC,
ACLINT MSWI/MTIMER, SiFive test finisher, simple framebuffer, and VirtIO MMIO
block, resident 9p, Ethernet, entropy, keyboard, and tablet devices.

Rust owns platform devices, VM storage, boot loading, and browser requests.
HTTP disks combine bounded clean caching with sparse session-local overlays;
resident 9p trees support synchronous copied host operations while the guest
runs. Applications own persistence. Destroy or page reload loses VM storage.

RV32, multiple harts, vectors, hypervisor support, PCIe, AIA, a native UI,
SLIRP/TAP, and native filesystem/socket backends are outside scope. Networking
requires a host-selected WebSocket origin; no production origin is supplied.

Platform summary
----------------

The compatibility references are the
[RVA23 profiles](https://docs.riscv.org/reference/rva23/rva23-profiles.html) and
QEMU's [`virt` machine](https://www.qemu.org/docs/master/system/riscv/virt.html).
Riscbox implements RV64 I, M, A, F, D, C, the advertised Zba/Zbb/Zbs subsets,
and selected current supervisor and scalar extensions used by its guests. It
uses TinyEMU's bit-exact integer SoftFP lineage. Riscbox is not RVA23 compliant;
vectors and other deliberately omitted requirements are never advertised.
PMP CSRs retain masks and locks for firmware compatibility, but PMP permissions
do not restrict memory accesses.

The platform follows QEMU `virt` addresses for RAM, reset, UART, VirtIO MMIO,
ACLINT MSWI and MTIMER, PLIC, and the test finisher, and adds the QEMU-compatible Goldfish RTC.
The generated device tree describes only configured devices.

TinyEMU relationship and license
---------------------------------

Riscbox began as a focused fork of Fabrice Bellard's
[TinyEMU](https://bellard.org/tinyemu/) and retains its MIT license and copyright
notices.

The active `tinyemu-core/` contains a freestanding subset of TinyEMU's C
CPU, SoftFP, and physical memory implementation. Rust owns the platform,
devices, browser requests, and C allocations. An execution quantum may enter C
multiple times to inject guest time at a timer deadline or process host work;
C calls Rust for device accesses.
Project history is recorded in [CHANGELOG.md](CHANGELOG.md).
