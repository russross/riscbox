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

Quick start
-----------

The root build requires Rust, Clang, `ar`, GNU Make, `uv`, Node.js, and the
`wasm32-unknown-unknown` Rust target. Building the supplied Linux images also
requires a RISC-V cross compiler, QEMU, ext4 tools, `curl`, and OpenSBI:

    sudo apt install curl e2fsprogs gcc-riscv64-linux-gnu opensbi qemu-system-misc
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

| Target         | Result |
| -------------- | ------ |
| `make release` | Optimized Rust workspace for development and native tests |
| `make test`    | Rust, Python tool, and JavaScript tests |
| `make check`   | Tests, strict Clippy, TypeScript, and Python type checks |
| `make wasm`    | `target/wasm32-unknown-unknown/release/riscbox_wasm.wasm` |
| `make kernel`  | Canonical custom kernel at `kernel/linux` |
| `make dist`    | WASM, generated JavaScript, and kernel artifacts |

Browser library
---------------

`js/riscbox.js` installs a global `Riscbox` class. Instantiate it with the WASM
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

runtime.start(new URL("./riscbox.cfg", location.href).href, 256);
</script>
```

The adapter schedules execution automatically unless a `schedule(milliseconds)`
callback is supplied. Runnable guests request an immediate next slice; waiting
guests request a bounded timer delay. Integrations can also provide `networkWrite`,
`framebufferRefresh`, and `p9Servers`. Host input methods are
`consoleInput(bytes)`, `consoleResize(columns, rows)`, `keyEvent()`,
`pointerEvent()`, `wheelEvent()`, `networkInput()`, and `networkCarrier()`.
Framebuffer callbacks receive a zero-copy WASM view plus `x`, `y`, `width`,
`height`, and full-frame `stride`; consume the view synchronously.

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
runtime.start(new URL("./riscbox.cfg", location.href).href, 256, "", 0, 0, true);
```

The endpoint uses the protocol documented in `network/README.md`: each binary
WebSocket message is one Ethernet frame without a VirtIO header or frame-check
sequence. Riscbox provides the browser client but no production origin service.
The origin must supply authentication, isolation, rate limiting, routing,
filtering, and any required NAT, DNS, or DHCP.

The root Rust crate exposes the CPU, memory, machine, device, configuration,
storage, and browser-runtime modules for focused testing and custom Rust-side
integration. It is not published on crates.io, and the stable deployment
boundary is the raw WASM ABI wrapped by `js/riscbox.js`.

VM configuration
----------------

Riscbox accepts JSON with comments, unquoted property names, and trailing
commas. Asset paths are resolved relative to the configuration file. A minimal
disk-backed Linux VM is:

```js
{
    version: 1,
    machine: "riscv64",
    memory_size: 256,
    bios: "fw_jump.bin",
    kernel: "linux",
    cmdline: "root=/dev/vda rw rootfstype=ext4 console=hvc0",
    drive0: { file: "drive/blk.txt" },
    console: "virtio",
    uart_output: true,
}
```

The main options are:

*   `bios`, `kernel`, and optional `initrd` select raw boot payloads.
    `memory_size` is in MiB, and `cmdline` is passed to Linux.
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

Boot payloads are explicit. Riscbox directly loads raw OpenSBI `fw_jump.bin`, a
raw uncompressed Linux `Image`, an optional opaque initramfs, or a flat
bare-metal image. It does not parse ELF, PE/COFF, FIT, qcow2, or compressed
kernel images and does not bundle OpenSBI or U-Boot.

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
`images/bin/build-distribution` to produce the browser deployment. The existing
build scripts show the exact call order. Keep downloads and generated files
under `build/`; the final ignored output belongs in `dist/`.

The distribution builder splits the disk into HTTP-loadable blocks and gives
boot and disk assets content-derived names. Publish new assets first and
`riscbox.cfg` last so each VM start sees one complete generation. Static hosting
needs ordinary `GET` requests, the `application/wasm` MIME type, and CORS when
assets cross origins; range requests and a server application are unnecessary.
See [images/README.md](images/README.md) for image layout and
[images/DEPLOYMENT.md](images/DEPLOYMENT.md) for deployment details.

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
    ): Promise<{ kind: "reply"; bytes: Uint8Array } | { kind: "suppressed" }>;
    close(): void;
}
```

The server owns 9P2000.L negotiation, fids, tags, flush ordering, errors, and
filesystem semantics. Rejecting a request promise means the endpoint failed;
normal filesystem errors must be encoded as 9p replies. Do not retain request
or WASM-backed buffers after their documented lifetime.

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
[js/p9/README.md](js/p9/README.md) for the supported operation profile.

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
CLINT, PLIC, and the test finisher, and adds the QEMU-compatible Goldfish RTC.
The generated device tree describes only configured devices.

TinyEMU relationship and license
---------------------------------

Riscbox began as a focused fork of Fabrice Bellard's
[TinyEMU](https://bellard.org/tinyemu/) and retains its MIT license and copyright
notices. The repository preserves the former C fork under `c/` for historical
reference.

The active `tinyemu-core/` contains a freestanding subset of the archived C
CPU, SoftFP, and physical memory implementation. Rust owns the platform,
devices, browser requests, and C allocations. A timeslice enters C once and
calls Rust for device accesses. The historical `c/` tree is not built.
Project history is recorded in [CHANGELOG.md](CHANGELOG.md).
