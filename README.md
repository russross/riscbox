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
an application directory. Its `riscbox.js`, `riscbox.wasm`, `block/`, `p9/`,
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
host block provider. Allocate 512 MiB of RAM for the Alpine standard ISO.
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

`js/riscbox.js` in the source tree (`riscbox.js` in the release archive)
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
then needs a new VM. Its runtime and filesystem handles remain usable.
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
raw WASM ABI wrapped by `js/riscbox.js`.

The raw WASM exports also provide independently owned Rust filesystem handles
for advanced embedding. They support host operations before VM startup,
on-demand source tickets, change events, and retained state across VM lifetimes.
The [filesystem ABI guide](src/browser_abi/ninep/README.md) documents packets,
buffer lifetimes, and attachment rules. The promise facade below wraps these
exports.

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
Both startup methods load boot assets asynchronously, and the `onVmStarted` callback
reports when the machine is ready. The older synchronous `start()` method
remains for integrations using the legacy Rust config fetch path.

Pass `blockProviders: new Map([[1, provider]])` to `Riscbox.instantiate()`
before starting a config with `drive0: { provider: 1, capacity_sectors: "..." }`.
The provider implements `read(sector, length)`, `write(sector, bytes)`,
`reset()`, and `close()` as defined in `js/block/index.ts`. `sector` is a
`bigint`; reads return exactly `length` bytes in a `Uint8Array`, and writes
resolve after the provider accepts the bytes. `reset()` clears pending
interface work but retains stored data; `close()` runs on VM destroy. Errors
and malformed read lengths complete the guest request with an I/O error.
Copy data you need to retain before an asynchronous operation returns; the
adapter copies guest write bytes and provider read replies across WASM memory.
Observe an orderly guest shutdown before treating a writable host image as
synchronized, since the guest kernel may buffer writes.

The parallel TypeScript HTTP provider opens an existing split-image manifest.
Its `capacitySectors` supplies the resolved drive capacity; it keeps a bounded
clean-block cache (16 MiB initially, growing for a single request) and a
session-local 4 KiB copy-on-write overlay. The original `drive0: { file: ... }`
path remains available. Import the generated module and register the provider:

```js
import { openHttpBlockProvider } from "./block/http.js";

const disk = await openHttpBlockProvider(new URL("./drive/blk.txt", location.href).href);
const runtime = await Riscbox.instantiate(wasmBytes, {
    blockProviders: new Map([[1, disk]]),
});
await runtime.startResolved({
    version: 1, machine: "riscv64", memory_size: 256,
    bios: firmwareUrl, kernel: kernelUrl,
    drive0: { provider: 1, capacity_sectors: disk.capacitySectors.toString() },
});
```

For a writable host image, import `ArrayBlockProvider` from
`./block/array.js` and register `new ArrayBlockProvider(bytes)` in the same
`blockProviders` map. Pass its `capacitySectors` as the resolved drive
capacity. The provider uses the caller's exact `Uint8Array` view. The array must
contain a positive whole number of 512-byte sectors; a partial final sector
is rejected. Reads return copies, and writes directly change the supplied
array. `reset()` retains its contents. `close()` makes the provider unusable
but leaves the caller's array intact for export. Use a clone when the source
must remain pristine, and inspect or export a filesystem image after orderly
guest shutdown.

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
*   Consecutive `drive0` through `drive3` add VirtIO block devices. Browser
    HTTP block writes remain in memory and disappear with the VM. A drive may
    instead specify `{ provider: 1, capacity_sectors: "131072" }` to attach a
    host block provider. Drive numbers determine guest device order, including
    mixed HTTP and host drives. Capacity counts 512-byte sectors; quote large
    values to preserve their full width.
*   Consecutive `fs0` through `fs3` use `{ server, tag }` to add host-provided
    VirtIO 9p channels. `server` selects the host registry entry; `tag` is the
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
and `/home` mounts. The xv6 profile browser page attaches its split disk through
the TypeScript HTTP provider for local performance testing. Other image
definitions can continue distributing ext4. The image Makefiles show
the exact call order. Keep downloads and generated files
under `build/`; the final ignored output belongs in `dist/`.

Risclet buffers editor changes and writes them to the shared filesystem when
the editor loses focus, a file or example is selected, the VM is used, or thirty
seconds have passed since the first unflushed edit. Later edits do not postpone
that deadline. Failed writes retain the editor text and retry; switching waits
for a successful flush. If the filesystem changes a file with unflushed edits,
the demo asks whether to discard the editor version. Keeping it replaces the
filesystem version on the next flush. Example files and edits are session-local
and do not persist across a page reload. Instruction images refresh when their
shared files change, and terminal pastes queue until the VM accepts their bytes.

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

Configure `fs0: { server: "workspace", tag: "shared" }`, then create and bind
its filesystem before starting the VM:

```js
import { Filesystem, createHttpsSeedPlugin } from "./p9/index.js";
const runtime = await Riscbox.instantiate(wasmBytes, options);
const workspace = await Filesystem.create(runtime);
await workspace.writeFile("hello.txt", "shared with the guest\n");
await workspace.bind("workspace");
await runtime.startFromUrl(configUrl);
const bytes = await workspace.readFile("hello.txt");
```

Mount it in Linux with:

    mount -t 9p -o trans=virtio,version=9p2000.L,cache=none shared /mnt/shared

Two simultaneous mounts need distinct configured tags; both may select one
filesystem. Only one live VM may attach a filesystem. Reboot/reset retains
files while resetting protocol state. Shutdown/halt preserves device state.
Destroy releases the attachment but retains host access and bindings.
`await workspace.reset()` replaces the namespace even while mounted; pending
host reads receive `ESTALE`, and the guest may need to remount.

Host methods always return promises and reject with `FilesystemError` carrying
positive Linux `errno`. They include whole-file reads/writes, directory and link
operations, metadata, and change subscriptions. `await workspace.subscribe(fn)`
returns an async unsubscribe function. Events include aliases and a numeric
`bigint` host origin for filtering an application's own writes.

9p source plugins
-----------------

Rust owns the 9P2000.L server. Custom plugins supply a namespace and asynchronous
file bodies; generic JavaScript protocol servers are no longer supported:

```ts
interface SeedPlugin<Key> {
    readonly entries: readonly SeedEntry<Key>[];
    readonly loader: {
        load(key: Key, signal: AbortSignal): Promise<Uint8Array>;
    };
}
```

Build entries with `SeedBuilder`, then `await workspace.installSeed(plugin)`.
`createHttpsSeedPlugin()` supports HTTP manifests; `createTarSeedPlugin()`
exposes an already downloaded archive. Hosts can supply credentials and protocol
handling for other sources through the same loader interface. Bodies load only
on host or guest reads; there is no preload mechanism. Concurrent readers join
one source request. Failure returns `EIO`; `retrySource(path)` permits a later
read to retry. Host writes supersede pending source bytes.

The optional limits are `maxFileBytes`, `maxTreeBytes`, `maxInodes`, and
`maxDirectoryEntries`: defaults are 256 MiB per file, 1 GiB of logical file data,
and 2^20 inodes and directory entries. See the [facade guide](js/p9/README.md)
for host methods, notifications, and lifecycle details, and the
[protocol profile](src/ninep_protocol/README.md) for supported guest operations.

Platform summary
----------------

The compatibility references are the
[RVA23 profiles](https://docs.riscv.org/reference/rva23/rva23-profiles.html) and
QEMU's [`virt` machine](https://www.qemu.org/docs/master/system/riscv/virt.html).
Riscbox implements RV64 I, M, A, F, D, C, the advertised Zba/Zbb/Zbs subsets,
and selected current supervisor and scalar extensions used by its guests. It
uses TinyEMU's bit-exact integer SoftFP lineage. Riscbox is not RVA23 compliant;
vectors and other deliberately omitted requirements are never advertised.

The platform follows QEMU `virt` addresses for RAM, reset, UART, VirtIO MMIO,
ACLINT MSWI and MTIMER, PLIC, and the test finisher, and adds the QEMU-compatible Goldfish RTC.
The generated device tree describes only configured devices.

TinyEMU relationship and license
---------------------------------

Riscbox began as a focused fork of Fabrice Bellard's
[TinyEMU](https://bellard.org/tinyemu/) and retains its MIT license and copyright
notices. The repository preserves the former C fork under `c/` for historical
reference.

The active `tinyemu-core/` contains a freestanding subset of the archived C
CPU, SoftFP, and physical memory implementation. Rust owns the platform,
devices, browser requests, and C allocations. An execution quantum may enter C
multiple times to inject guest time at a timer deadline or process host work;
C calls Rust for device accesses. The historical `c/` tree is not built.
Project history is recorded in [CHANGELOG.md](CHANGELOG.md).
