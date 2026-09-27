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
does not support RV32, SMP, vectors, hypervisor mode, PCIe, UEFI, or general
device emulation. Native Rust builds support testing; the browser is the
deployment target.

Release archive
---------------

Each GitHub release has one `riscbox-VERSION.tar.gz` archive. Extract it into
an application directory. Its `riscbox.js`, `riscbox.wasm`, `p9/`, and
`network/` files are ready to serve as static browser assets. `linux` is the
configured RV64 Linux Image, `fw_dynamic.bin` is the Riscbox OpenSBI firmware,
and `u-boot.bin` is the S-mode bootloader. The archive also contains this API
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
versions in this repository. To use U-Boot, set `kernel: "u-boot.bin"` and put
`/boot/Image` and `/boot/extlinux/extlinux.conf` in the guest disk; U-Boot
loads Linux from the disk. The included `linux` file can supply that Image.

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
    ./build.sh
    cd dist
    python3 -m http.server 8000

Open <http://127.0.0.1:8000/>. The generated `dist/` directory is self-contained
and can be copied to any static HTTP server. It contains the VM configuration,
OpenSBI, Linux, a chunked disk, `riscbox.wasm`, `riscbox.js`, and a minimal
console page.

Build targets
-------------

| Target         | Result                                                        |
| -------------- | ------------------------------------------------------------- |
| `make release` | Optimized Rust workspace for development and native tests     |
| `make test`    | Rust, Python tool, and JavaScript tests                       |
| `make check`   | Tests, strict Clippy, TypeScript, and Python type checks       |
| `make wasm`    | `target/wasm32-unknown-unknown/release/riscbox_wasm.wasm`      |
| `make kernel`  | Canonical custom kernel at `kernel/linux`                     |
| `make opensbi` | Riscbox-configured firmware at `opensbi/fw_dynamic.bin`       |
| `make uboot`   | Pinned EROFS-capable S-mode bootloader at `uboot/u-boot.bin`   |
| `make dist`    | WASM, generated JavaScript, kernel, OpenSBI, and U-Boot        |

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
100 milliseconds (default 20); this bounds latency for host input and
completed I/O. The cycle-rate estimate uses a ten-second half-life. The cycle
budget stays based on the target duration; each quantum's guest-time window
extends by any guest-clock lead from the previous quantum. Guest time remains
monotonic without delaying runnable quanta. Set `debugTiming: true`
to log estimated and active emulated
Mcycles/s, CPU runs per quantum, timer intervals, WFI sleep time, and carried
guest time. The variance fields retain the latest interval maximum and the
50th, 90th, and 99th percentiles across runnable quanta since boot.
Integrations can also provide
`networkWrite`, `framebufferRefresh`, and `p9Servers`. Host input methods are
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
in place. `destroy()` releases a halted VM; a later boot then needs a new VM.
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
Both methods load boot assets asynchronously, and the `onVmStarted` callback
reports when the machine is ready. The older synchronous `start()` method
remains for integrations using the legacy Rust config fetch path.

```js
{
    version: 1,
    machine: "riscv64",
    memory_size: 256,
    bios: "fw_dynamic.bin",
    kernel: "linux",
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
    block writes remain in memory and disappear with the VM.
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
├── build.sh       # invokes the shared image helpers
├── setup.sh       # runs as root inside the image under QEMU
├── riscbox.cfg    # paths are relative to the deployed config
└── web/           # optional replacement/additions for the browser page
```

Use `images/bin/create-alpine-ext4` to create the filesystem,
`images/bin/run-image-setup` to customize it under QEMU, and
`images/bin/build-distribution` to produce the browser deployment. Risclet
installs the custom Linux Image and `extlinux.conf` into its root filesystem,
then converts the completed ext4 setup image to one EROFS disk. OpenSBI starts
the pinned U-Boot build with VirtIO MMIO block and ext4/EROFS support. U-Boot
loads Linux from that disk, which Linux then mounts at `/dev/vda`. OpenSBI and
U-Boot boot quietly, and U-Boot has no startup countdown. It first checks the whole
disk for an extlinux configuration, then scans partitions for bootflows. Its
build excludes network, PCI, USB, video, EFI, FAT, and ISO boot support. The
xv6 profile also uses
the builder's `--erofs` mode. Both use session-local writable `/tmp`, `/var`,
and `/home` mounts. Other image definitions can continue distributing ext4. The
existing build scripts show the exact call order. Keep downloads and generated files
under `build/`; the final ignored output belongs in `dist/`.

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
sharing one tree with host UI, and controlled sharing between VMs. It is not an
authentication boundary or a persistent store by itself.

Configure a channel and register the matching server key:

```js
// riscbox.cfg
fs0: { server: "workspace", tag: "shared" },
```

```js
import { Memory9PServer } from "./p9/index.js";

const workspace = new Memory9PServer({
    "hello.txt": "shared with the guest\n",
});

const runtime = await Riscbox.instantiate(wasmBytes, {
    p9Servers: new Map([["workspace", workspace]]),
});
```

Mount it in Linux with:

    mount -t 9p -o trans=virtio,version=9p2000.L shared /mnt/shared

Use `cache=none` when host code or another VM must see changes promptly,
`cache=mmap` when executable mappings matter and some staleness is acceptable,
and `cache=loose` only for an exclusive guest mount. Two simultaneous mounts
need two configured channels with distinct tags, though both may select the
same server and shared tree.

The supplied `Memory9PServer` implements the common 9P2000.L file, directory,
link, rename, metadata, locking, and lifecycle operations. Host application
methods include `readFile`, `writeFile`, `remove`, `rename`, `listFiles`,
`load`, `readFileAsync`, and `subscribe`. Expected failures are returned as
discriminated results:

```js
const result = workspace.readFile("hello.txt");
if (result.kind === "ok") {
    console.log(new TextDecoder().decode(result.value));
} else if (result.kind === "error") {
    console.error(result.error.message);
}
```

The optional limits are `maxFileBytes`, `maxTreeBytes`, `maxInodes`, and
`maxDirectoryEntries`. Defaults are 256 MiB per file, 1 GiB of logical file
data, and 2^20 inodes and directory entries.

9p servers and seed plugins
---------------------------

A custom server only needs to create an independent session for each VirtIO
endpoint. Requests may complete out of order:

```ts
interface P9Server {
    connect(): P9Session;
}

interface P9Session {
    request(
        bytes: Uint8Array,
        replyCapacity: number,
        expectResponse: () => void,
    ): Promise<{ kind: "reply"; bytes: Uint8Array } | { kind: "suppressed" }>;
    close(): void;
}
```

The server owns 9P2000.L negotiation, fids, tags, flush ordering, errors, and
filesystem semantics. Rejecting a request promise means the endpoint failed;
normal filesystem errors must be encoded as 9p replies. Do not retain request
or WASM-backed buffers after their documented lifetime.
Call `expectResponse()` once, before starting the promise or microtask chain,
when the request can settle without blocking I/O. Call the retained callback
for each earlier request that a flush, reset, or close can settle. Do not call
it for HTTP, local-storage, or other asynchronous work. The callback carries
no result; only the returned promise completes a request. After 20 consecutive
microtask yields without a response, the driver logs unresolved hints and the
guest continues. Each delivered response resets that count.

For large static trees, a seed plugin is usually simpler than a custom server.
It declares the complete namespace and lazily loads regular-file bodies:

```ts
interface SeedPlugin<Key> {
    readonly entries: readonly SeedEntry<Key>[];
    readonly loader: {
        load(key: Key, signal: AbortSignal): Promise<Uint8Array>;
    };
}
```

Build entries with `SeedBuilder`, then call `MemoryFilesystem.fromSeed(plugin)`
or pass the plugin to `Memory9PServer`. `createHttpsSeedPlugin()` and
`createTarSeedPlugin()` demonstrate remote manifests and pre-downloaded tar
archives. One filesystem instance pins its namespace and loader interpretation;
create a new instance to publish a new generation. See
[9p documentation](https://github.com/russross/riscbox/blob/main/js/p9/README.md)
for the supported operation profile.

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
