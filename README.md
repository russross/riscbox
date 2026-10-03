Riscbox
========

Riscbox is a small RV64 virtual platform for teaching, grading, and running
purpose-built Linux systems in a web page. The platform is written in Rust
around a TinyEMU C CPU and memory core, compiled directly to WebAssembly,
and integrated with a dependency-free
JavaScript adapter. It is designed for static hosting and controlled guest
images rather than broad hardware compatibility.

Typical uses include:

*   Browser-hosted student kernels and operating-system exercises.
*   Small Alpine Linux applications distributed as static web assets.
*   Sandboxed grading or lab VMs with a serial console and session-local disk
    writes.
*   A live filesystem shared between a guest, its host page, and optionally
    other VMs through VirtIO 9p.

Riscbox supports one little-endian RV64 hart, M/S/U privilege modes, Sv39,
OpenSBI, current xv6, and a focused QEMU `virt`-style device set. It intentionally
does not support RV32, SMP, vectors, hypervisor mode, PCIe, or general
device emulation. Native Rust builds support testing; the browser is the
deployment target.

Release archive
---------------

Each GitHub release has one `riscbox-VERSION.tar.gz` archive. Extract it into
an application directory. Its `riscbox.js`, `riscbox.d.ts`, `riscbox.wasm`,
and `network/` files are ready to serve as static browser assets. The configured
Linux Image, OpenSBI firmware, and U-Boot binary are gzip-compressed under
content-hash names (`linux-HASH.gz`, `fw_dynamic.bin-HASH.gz`, and
`u-boot.bin-HASH.gz`). The archive also contains this API
README, module documentation, the changelog, and the license. It contains no
guest root filesystem, disk blocks, sample VM, or image build scripts.

To boot Linux directly, prepare a compatible RISC-V root filesystem image,
split it into HTTP blocks using the repository's
[`tools/splitimg.py`](https://github.com/russross/riscbox/blob/main/tools/splitimg.py),
and place its block directory beside the runtime. Create `riscbox.cfg` there
using the example in “VM configuration” below; replace `drive/blk.txt` with
the path reported by the splitter. Serve the directory over HTTP or HTTPS and
load `riscbox.js` and `riscbox.wasm` as shown in “Browser library”. The
configuration URL is the base for its boot and disk paths. For an EROFS root,
use `rootfstype=erofs` and `ro` in `cmdline`; for a prepared ext4 root, use
`rootfstype=ext4` and `rw`. Serve `.wasm` as `application/wasm`.

The archive's OpenSBI and U-Boot binaries are compiled from the pinned source
versions in this repository. To use U-Boot, set `kernel` to the archive's
`u-boot.bin-HASH.gz` name and put
`/boot/Image` and `/boot/extlinux/extlinux.conf` in the guest disk; U-Boot
loads Linux from the disk. Decompress the included `linux-HASH.gz` asset to
supply that Image.

To boot a RISC-V installation ISO, use OpenSBI as `bios`, U-Boot as `kernel`,
and attach the ISO as `drive0`. Split the `.iso` with `tools/splitimg.py` and
set `drive0.file` to its `drive-HASH/blk.txt`, or attach its bytes through a
VM-owned byte array. Allocate 512 MiB of RAM for the Alpine standard ISO.
U-Boot scans FAT EFI boot partitions, including El Torito boot images, for
`/EFI/BOOT/BOOTRISCV64.EFI`. The ISO's EFI loader supplies its own kernel and
initramfs; the included Linux Image is not required for this boot path.
Alpine standard 3.24.2 riscv64 has been validated through login, filesystem
access, and shutdown in Chrome. HTTP disk writes remain session-local.

The supplied Linux Image also includes its RISC-V EFI stub, compressed
initramfs support, FAT/VFAT, ISO9660 with Rock Ridge and Joliet, loop devices,
and SquashFS with zlib, XZ, and Zstandard decompression. U-Boot reads the FAT
boot image inside an ISO; an EFI application such as GRUB reads the ISO9660
tree. Boot media must contain a RISC-V EFI loader compatible with this platform.

To run the optional ISO acceptance tests after building OpenSBI, U-Boot,
WASM, and JavaScript:

```sh
RISCBOX_ALPINE_ISO=/path/to/alpine-standard-riscv64.iso \
    cargo test --release --test platform_acceptance alpine_iso_boots_through_efi_and_shuts_down -- --ignored
RISCBOX_ALPINE_ISO=/path/to/alpine-standard-riscv64.iso \
    node --test tests/alpine_iso_browser.test.mjs
```

Set `RISCBOX_ISO_TRANSPORT=http` for the browser test to exercise split HTTP
media instead of a host array. Both paths check ISO9660 and the embedded FAT
image and require guest poweroff.

Quick start
-----------

The root build requires Rust, Clang, `ar`, GNU Make, `uv`, Node.js, and the
`wasm32-unknown-unknown` Rust target. Building the supplied Linux images also
requires a RISC-V cross compiler, QEMU, ext4 tools, and `curl`. The image
scripts build their pinned OpenSBI firmware from source:

    sudo apt install curl e2fsprogs gcc-riscv64-linux-gnu qemu-system-misc
    rustup target add wasm32-unknown-unknown

Clone, build, and test the emulator:

    git clone https://github.com/russross/riscbox.git
    cd riscbox
    make
    make test

The supplied Alpine definition is a complete example project:

    cd images/alpine
    make
    cd dist
    python3 -m http.server 8000

Open <http://127.0.0.1:8000/>. The generated `dist/` directory is self-contained
and can be copied to any static HTTP server. It contains the VM configuration,
OpenSBI, Linux, a chunked disk, `riscbox.wasm`, `riscbox.js`, and a minimal
console page.

Build targets
-------------

| Target              | Result                                                                 |
| ------------------- | ---------------------------------------------------------------------- |
| `make release`      | Optimized Rust workspace for development and native tests              |
| `make test-unit`    | Rust, Python, JavaScript, and raw-WASM 9p server tests                 |
| `make test`         | Unit tests and Chrome/WASM network and 9p server tests                 |
| `make check`        | Full tests, strict Clippy, TypeScript, and Python type checks          |
| `make test-images`  | Build image distributions; native Alpine and Chrome Risclet acceptance |
| `make wasm`         | `target/wasm32-unknown-unknown/release/riscbox_wasm.wasm`              |
| `make` / `make all` | Build the complete release archive in `build/releases/`                |
| `make kernel`       | Canonical kernel and its `kernel/linux-HASH.gz` asset                  |
| `make opensbi`      | OpenSBI and its `opensbi/fw_dynamic.bin-HASH.gz` asset                 |
| `make uboot`        | U-Boot and its `uboot/u-boot.bin-HASH.gz` asset                        |
| `make dist`         | Update the release archive from all build components                   |

Browser library
---------------

`build/js/riscbox.js` built by `make js` (`riscbox.js` in the release archive)
installs a global `Riscbox` class. Instantiate it with the WASM
bytes and callbacks, then start it with a configuration URL and RAM size:

```html
<script src="./riscbox.js"></script>
<script type="module">
const terminal = document.querySelector("#terminal");
const response = await fetch("./riscbox.wasm");
const runtime = await Riscbox.instantiate(await response.arrayBuffer(), {
    consoleWrite: (text) => terminal.append(document.createTextNode(text)),
    onVmStarted: () => console.log("VM started"),
    onError: (error) => console.error(error),
});

await runtime.startFromUrl(new URL("./riscbox.cfg", location.href).href, 256);
</script>
```

The adapter schedules execution automatically. Runnable guests request an
immediate next execution quantum through a browser task; WFI sleeping guests
wake at the nearest guest timer deadline or after at most 100 milliseconds.
A completed asynchronous device request can wake a WFI sleeping guest. Rust calibrates the quantum cycle
budget from complete quanta timed by JavaScript. Set `targetQuantumMs` in the
instantiate options to choose a nominal duration greater than zero and at most
100 milliseconds (default 20); carried guest-clock lead can extend an
individual quantum beyond that nominal duration. The cycle-rate estimate
uses a ten-second half-life. The cycle
budget and target duration both extend by any guest-clock lead from the
previous quantum. A decayed P99 estimate of recent clock variation slows
guest time proactively. When lead remains, the next quantum uses the more
conservative of that skewed rate and the rate implied by the previous quantum's
measured cycle throughput. Guest time remains monotonic without delaying
runnable quanta. Set `debugTiming: true` to log the smoothed estimated rate,
interval mean and standard deviation of runnable quantum rates, active
emulated Mcycles/s, CPU runs per quantum, timer intervals, WFI sleep time, and
the latest guest-clock lead sampled at the start of a quantum. The variance fields retain the latest interval maximum and the
50th, 90th, and 99th percentiles across runnable quanta since boot. Timing
reports stop when the VM powers off.
Integrations can also provide
`networkWrite`, `framebufferRefresh`, and `onError`. Host input methods are
`consoleInput(bytes)`, `consoleResize(columns, rows)`, `keyEvent()`,
`pointerEvent()`, `wheelEvent()`, `networkInput()`, and `networkCarrier()`.
`runQuantum()` explicitly requests a quantum when a host integration needs to
resume a VM; normal wakeups are scheduled by the browser adapter.
Framebuffer callbacks receive a zero-copy WASM view plus `x`, `y`, `width`,
`height`, and full-frame `stride`; consume the view synchronously.

Lifecycle methods return promises. `requestShutdown()` and `requestReboot()`
deliver guest input events and return before the guest has acted; the guest OS
must handle those events. The prepared Alpine and Risclet images run BusyBox
`acpid` for them. `halt()` immediately stops CPU execution and retains the
machine, attached devices, 9p servers, and disk contents. `boot()` starts a
halted VM from its boot images; `reset()` immediately resets a running VM. A
guest-initiated poweroff halts the VM, and a guest-initiated reboot resets it
in place. `destroy()` releases a halted VM or cancels startup; a later boot
then needs a new VM. Its runtime can prepare another VM; all filesystem and disk objects become invalid.
`onVmHalted(cause)` and `onVmReset(cause)` report `guest-poweroff`,
`guest-reboot`, `host-halt`, `host-reset`, `host-boot`, or `guest-failure` as
applicable. `onVmDestroyed()` reports teardown. `consoleReset()` and
`framebufferClear()` let host displays clear themselves on reset. A forced
halt or reset does not let the guest flush filesystem buffers. Wait for an
observed guest halt or reboot before treating a writable disk as synchronized.

For the supplied WebSocket network frontend, import the generated TypeScript
module and attach it before starting a network-enabled VM:

```js
import { WebSocketNetwork } from "./network/index.js";

const network = new WebSocketNetwork(
    new URL("./network", location.href).href.replace(/^http/, "ws"),
    { onError: (error) => console.error(error) },
);
const runtime = await Riscbox.instantiate(await response.arrayBuffer(), {
    consoleWrite: (text) => terminal.append(document.createTextNode(text)),
    networkWrite: network.transmit,
});
network.attach(runtime);
network.connect();
await runtime.startFromUrl(new URL("./riscbox.cfg", location.href).href, 256, "", 0, 0, true);
```

The endpoint uses the protocol documented in `network/README.md`: each binary
WebSocket message is one Ethernet frame without a VirtIO header or frame-check
sequence. Riscbox provides the browser client but no production origin service.
The origin must supply authentication, isolation, rate limiting, routing,
filtering, and any required NAT, DNS, or DHCP.

The root Rust crate exposes the machine, device, configuration, storage, and
browser-runtime modules for focused testing and custom Rust-side integration.
`guest_memory` contains the shared guest-memory types and TinyEMU RAM bridge.
The crate is not published on crates.io. Its stable deployment boundary is the
raw WASM ABI wrapped by the generated `build/js/riscbox.js` adapter.

The raw WASM exports provide VM-owned filesystem and disk handles for advanced
embedding. The [storage ABI guide](STORAGE-ABI.md) documents
copied packets, buffer lifetimes, and powered-off disk access.

VM configuration
----------------

Riscbox accepts JSON with comments, unquoted property names, and trailing
commas. Asset paths are resolved relative to the configuration file. A minimal
disk-backed Linux VM is:

`startFromUrl(url, ramMiB?, commandLine?, width?, height?, hasNetwork?)`
fetches the file with `no-store`, resolves defaults and asset URLs in
JavaScript, then passes the result to Rust for validation and machine setup.
`startResolved(config, ramMiB?, width?, height?, hasNetwork?)` accepts a host
object directly. Its asset paths are used as given; callers supply absolute
URLs when needed. `ramMiB` of zero uses the configuration's `memory_size`.
`Riscbox.loadResolvedConfig(url, commandLine?, fetch?)` fetches a deployed
configuration with `no-store` and returns the resolved object, so an embedding
page can replace a drive entry before calling `startResolved`.
Startup methods load boot assets asynchronously and boot the machine.
`prepareResolved(config, ...)` and `prepareFromUrl(url, ...)` load and construct
the same platform but leave it powered off. Populate storage before calling
`await runtime.boot()`. The legacy `start()` also returns a promise.

Block storage
-------------

Rust owns disk bytes, the clean HTTP cache, and copy-on-write overlays.
`drive0: { file: manifestUrl }` selects a split HTTP image.
`drive0: { bytes: imageBytes }` copies a whole-sector `Uint8Array` into Rust;
`drive0: { capacity_sectors: "131072" }` allocates a zeroed writable disk.
Consecutive drive numbers preserve guest order for mixed HTTP and array disks.

```js
await runtime.prepareResolved({
    version: 1, machine: "riscv64", memory_size: 256,
    bios: firmwareUrl, kernel: kernelUrl,
    drive0: { bytes: imageBytes },
});
const disk = runtime.block(0);
disk.write(0n, bootSector);             // synchronous, whole 512-byte sectors
const image = await disk.read(0n, Number(disk.capacitySectors) * 512);
await runtime.boot();
```

All host disk operations require a powered-off VM: before boot, after
`halt()`, or after observed guest poweroff. Reads return copied bytes directly
when resident, otherwise a promise while HTTP chunks load. Writes are always
synchronous. HTTP writes record only changed sectors and never fetch a chunk
to preserve its unwritten sectors. Array writes update Rust's store directly.
The caller's original array and exported snapshots remain independent copies.
Pending host reads must finish or be retired by `coldReset()` before boot.

Halt, reboot, and reset retain bytes and HTTP overlays. While powered off,
`disk.discardChanges()` removes an HTTP disk's overlay while retaining its
clean cache; array disks reject overlay discard. `coldReset()` resets
CPU/devices and clears RAM while preserving storage. Use both operations for a
clean HTTP-backed boot. Destroy invalidates every disk and share and frees their
contents; page reload also loses all runtime data. Observe orderly guest
shutdown before exporting an image that requires guest filesystem consistency.
Disk operations throw or reject with `BlockError` carrying positive Linux errno.

For alternate immutable chunk transport, supply
`fetchBlock: ({ disk, url, cache }) => Promise<Uint8Array>` to
`Riscbox.instantiate()`. Rust still owns caching, deduplication, writes, errors,
and lifecycle retirement. The hook receives zero-based disk indexes and chunk
URLs; configuration, boot assets, and manifests use the regular `fetch` option.
Failures and incorrectly sized chunks become guest I/O errors or rejected host
reads. Obsolete replies after reset/destroy are ignored.

Replace `HASH` in the following example with each asset's eight-character
suffix, and replace the drive path with the directory reported by `splitimg.py`.

```js
{
    version: 1,
    machine: "riscv64",
    memory_size: 256,
    bios: "fw_dynamic.bin-HASH.gz",
    kernel: "linux-HASH.gz",
    cmdline: "root=/dev/vda rw rootfstype=ext4 console=hvc0",
    drive0: { file: "drive/blk.txt" },
    console: "virtio",
    uart_output: true,
}
```

The main options are:

*   `bios`, `kernel`, and optional `initrd` select boot payloads. At least one
    of `bios` and `kernel` is required. Firmware and kernels may be raw or
    gzip-compressed; initrds are passed to the guest unchanged. `memory_size`
    is in MiB, and `cmdline` is passed to Linux.
*   `bios_address`, `kernel_address`, `initrd_address`, and `fdt_address`
    optionally set physical load addresses. Use quoted hexadecimal strings for
    addresses above the config parser's signed 32-bit integer range, such as
    `kernel_address: "0x100000000"`.
*   `console` is `virtio` by default or `uart`. `uart_output: true` mirrors
    firmware and early-kernel UART output while input stays on VirtIO.
*   Consecutive `drive0` through `drive3` add VirtIO block devices.
    Use `{ file: "drive-HASH/blk.txt" }` for HTTP, `{ bytes: imageBytes }`
    in a host object, or `{ capacity_sectors: "131072" }` for a zeroed array.
    Capacity counts 512-byte sectors; quote large values to preserve full width.
*   Consecutive `fs0` through `fs3` use `{ server, tag }` to add shared
    VirtIO 9p channels. `server` names an automatic resident tree; `tag` is the
    guest-visible mount tag.
*   `display0: { device: "simplefb", width, height }` adds a framebuffer, and
    `input_device: "virtio"` adds keyboard and tablet devices. `eth0` adds the
    single supported network interface as `{ driver: "user" }` when the host
    installs a frontend. Native TAP and SLIRP backends are not supported.

Boot payloads are explicit. By default, firmware loads at `0x80000000`, a
kernel at `0x80200000`, and an initrd at the kernel address plus half of RAM
(capped at 512 MiB). The device tree is placed near the end of RAM at a 2 MiB
boundary. Riscbox passes OpenSBI `fw_dynamic.bin` the kernel entry through
its dynamic-info block in the reset ROM. Omitting `bios` starts the kernel in
M-mode; this supports bare-metal guests such as xv6. An S-mode U-Boot binary
can be supplied as `kernel` after OpenSBI. Riscbox does not parse ELF,
PE/COFF, FIT, qcow2, or other compressed kernel formats. The runtime loads
firmware and bootloader binaries supplied by the image; the repository builds
its pinned OpenSBI and U-Boot binaries separately.

Creating an image project
-------------------------

Each directory under `images/` is an independent image definition. Start from
`images/alpine/` for a general Linux VM or `images/risclet/` for a VM with a
browser-backed workspace:

```text
images/my-image/
├── Makefile       # tracks the shared image helpers and build inputs
├── setup.sh       # runs as root inside the image under QEMU
├── riscbox.cfg    # paths are relative to the deployed config
└── web/           # optional replacement/additions for the browser page
```

Use `images/bin/create-alpine-ext4` to create the filesystem,
`images/bin/run-image-setup` to customize it under QEMU, and
`images/bin/build-distribution` to produce the browser deployment. Risclet
converts the completed ext4 setup image to one EROFS disk. Its configuration
loads the custom Linux Image directly through OpenSBI; Linux mounts the disk
read-only at `/dev/vda`. The xv6 profile also uses the builder's `--erofs`
mode. Both use session-local writable `/tmp`, `/var`,
and `/home` mounts. Both browser pages attach their split disks through Rust. Other image
definitions can continue distributing ext4. The image Makefiles show
the exact call order. Keep downloads and generated files
under `build/`; the final ignored output belongs in `dist/`.

Risclet compiles the shared editor, terminal, file views, snapshots, and VM
lifecycle from [`client-core/`](client-core/README.md), a read-only sshfs mount
of Exam's canonical source and dependency installation. Update shared source
and dependencies in Exam; Risclet builds consume the mount without writing to
it. Its TypeScript configuration maps runtime types to the locally built
`build/js/riscbox.d.ts`. Review the [shared changelog](client-core/CHANGELOG.md)
when updating the demo. The Makefile's shared test targets create and remove a
temporary writable copy of the mount through `tools/test_client_core.mjs`.

Risclet uses one VM and one resident share. It downloads complete example
files before boot and caches their original bytes in the application.
Selecting an example flushes the editor, requests orderly guest shutdown,
snapshots the outgoing share, restores the incoming example, and boots the same
VM with retained disk changes. Snapshots preserve file bytes, empty directories,
symlinks, hard links, permissions, ownership, and access/modification times.
They live in application memory and are lost when the page reloads. Reboot
requests an orderly guest reboot and retains the current share and disk changes.
Reset forces halt, clears RAM and the HTTP overlay, restores the current
example's original files, and boots. Reset remains available while orderly
shutdown or reboot is pending.

Editor changes flush on blur, file selection, VM interaction, Sync, or thirty
seconds after the latest edit. Each edit restarts this fallback timer. Sync is
enabled while the editor is dirty; it writes to 9p without server persistence.
Failed writes retain text for retry. External
changes to dirty files require a discard decision. Instruction images refresh
when their shared files change; terminal pastes queue until accepted.

The terminal uses Wterm's DOM renderer with its Ghostty core, 18px Latin Modern
Mono, and a 64 KiB history budget. Screen clearing clips retained history at
the live-screen boundary; Reset removes history and selection. Browser checks
cover fractional scaling, partial-row viewport heights, connected box drawing,
bracketed paste, idle rendering, and container resizing.

The distribution builder splits the disk into HTTP-loadable blocks, compresses
the next-stage payload with `gzip -9`, and gives boot and disk assets
content-derived names. The `.gz` payload name uses the uncompressed payload's
hash. Publish new assets
first and `riscbox.cfg` last so each VM start sees one complete generation.
The adapter fetches hash-named boot assets and disk chunks with `force-cache`
and the configuration with `no-store`. Static hosting needs ordinary `GET`
requests, the `application/wasm` MIME type,
and CORS when assets cross origins; range requests and a server application are
unnecessary.
See the repository's [image build guide](https://github.com/russross/riscbox/blob/main/images/README.md)
and [deployment guide](https://github.com/russross/riscbox/blob/main/images/DEPLOYMENT.md)
for image layout and publishing details.

9p file sharing
---------------

VirtIO 9p lets a guest mount a filesystem owned by the host page. It is suited
to editable student workspaces, importing starter files, exporting results,
sharing one tree with host UI. It is not an authentication boundary or a
persistent store by itself.

Configure `fs0: { server: "workspace", tag: "shared" }`. Preparation creates
the named resident share automatically:

```js
const runtime = await Riscbox.instantiate(wasmBytes, options);
await runtime.prepareFromUrl(configUrl);
const workspace = runtime.filesystem("workspace");
workspace.writeFile("hello.txt", "shared with the guest\n");
await runtime.boot();
const bytes = workspace.readFile("hello.txt");
```

Mount it in Linux with:

    mount -t 9p -o trans=virtio,version=9p2000.L,cache=none shared /mnt/shared

Host methods are synchronous and throw `FilesystemError` with positive Linux
`errno`. They include `readFile`, `writeFile`, `mkdir`, `remove`, `rename`,
`listFiles`, `listDirectory`, `stat`, `symlink`, `readlink`, `link`, and
`setAttributes`. Attributes specify permissions, ownership, and access/
modification times with nanosecond precision; changing them updates ctime.
Writes replace whole files and require existing parent directories. Paths are
literal namespace paths; the empty path names the root directory.

`workspace.subscribe(listener)` returns a synchronous unsubscribe function.
Copied change events are delivered after WASM returns, with aliases, host/guest
source, and a `bigint` host origin for filtering application writes. Supply the
origin as the final argument to mutations. Reads return copied byte arrays.

Several configured tags may share one server name and independent protocol
sessions. Host access works before boot, while running, and after poweroff.
`workspace.clear()` replaces the tree only while powered off; reboot and reset
retain it. Destroy invalidates all shares. Loaders, seed plugins, independent
filesystem creation/binding, and source tickets are removed: applications fetch
their own content and insert it through the regular API.

Default quotas are 256 MiB per file, 1 GiB of logical file data, and 2^20 inodes
and directory entries. The [storage ABI](STORAGE-ABI.md) and
[protocol profile](NINEP.md) describe the native contracts.

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
