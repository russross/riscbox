Riscbox image deployment
========================

This directory is a complete browser-deployable Riscbox VM.

Files
-----

*   `index.html` is the supplied browser integration.
*   `riscbox.js` and `riscbox.wasm` are the emulator runtime.
*   `riscbox.cfg` describes the virtual machine. Boot paths are relative to
    this file.
*   `fw_dynamic.bin-HASH.gz` is gzip-compressed OpenSBI firmware.
    `linux-HASH.gz` is the gzip-compressed guest kernel for direct Linux boots.
    The Risclet image also loads `linux-HASH.gz` directly. Payload
    hashes identify the uncompressed images.
*   `drive-HASH/blk.txt` describes the HTTP disk; its
    `blkNNNNNNNNN.bin` files are 512 KiB blocks by default. The drive hash
    identifies source content and block size, so assets with different layouts have
    distinct URLs. Risclet and xv6 profile distribute single EROFS roots;
    Alpine distributes ext4.
*   `p9/` contains the generated Rust filesystem promise facade modules and
    declarations.

Publishing
----------

Copy assets without deleting older hashed generations, then copy the config as
the atomic rollover step. For example:

    rsync -av --exclude=riscbox.cfg ./dist/ server:/srv/www/image/
    rsync -av ./dist/riscbox.cfg server:/srv/www/image/riscbox.cfg

The web server must provide ordinary `GET` access to every file, serve
`.wasm` as `application/wasm`, and leave the block filenames unchanged. Static
hosting is sufficient: Riscbox fetches `blk.txt` first and loads block files as
the guest reads them. Range requests, server-side disk-image support, and a
backend application are not required. Disable transformations that rewrite
binary files. When assets are hosted on another origin, allow them with CORS.

Browsers should load the VM over HTTP or HTTPS, not `file:`. The shared terminal
page loads `riscbox.cfg` by default, accepts another configuration through
`?config=URL`, and uses `?memory=384` to override guest RAM in MiB.
Image-specific pages may intentionally fix these settings in their integration.

Configuration
-------------

Riscbox accepts JSON with comments, unquoted keys, and trailing commas. A
typical disk-backed VM is:

```js
{
    version: 1,
    machine: "riscv64",
    memory_size: 256,
    bios: "fw_dynamic.bin-81ceef21.gz",
    kernel: "linux-a837bc72.gz",
    cmdline: "root=/dev/vda rw rootfstype=ext4 console=hvc0",
    drive0: { file: "drive-827a7b2f/blk.txt" },
    console: "virtio",
}
```

The important options are:

*   `memory_size` sets RAM in MiB. `bios`, `kernel`, and optional `initrd`
    select boot payloads; at least firmware or kernel is required. Firmware
    and kernels may be raw or gzip-compressed. The guest interprets initrd
    compression.
*   `bios_address`, `kernel_address`, `initrd_address`, and `fdt_address`
    override physical load addresses. Quoted hexadecimal strings support
    64-bit addresses. Defaults are RAM base for firmware, RAM base plus 2 MiB
    for the kernel, kernel address plus half of RAM capped at 512 MiB for the
    initrd, and a 2 MiB boundary near the end of RAM for the device tree.
*   `cmdline` is the Linux command line. Use `console=hvc0` with the VirtIO
    console or `console=ttyS0,115200` with the UART.
*   `console` is `virtio` or `uart`. `uart_output: true` also exposes firmware
    and early kernel output while input remains on the VirtIO console.
*   Consecutive `drive0`, `drive1`, and later entries add VirtIO block devices.
    `device` may name the guest-visible device when an integration needs it.
*   `rtc_local_time: true` makes the Goldfish RTC report local rather than UTC
    time.

Optional hardware
-----------------

The image kernel includes the small platform supported by Riscbox. Add devices
to `riscbox.cfg` only when the host integration supplies their frontend:

```js
{
    fs0: { server: "workspace", tag: "shared" },
    eth0: { driver: "user" },
    display0: { device: "simplefb", width: 1024, height: 768 },
    input_device: "virtio",
}
```

`display0` needs a framebuffer callback in the page, and VirtIO input needs the
page to forward keyboard, pointer, and wheel events. `eth0` needs a browser
network frontend. Riscbox has no native user-mode or TAP backend.

The distribution contains `network/index.js` and its declaration file. Create
a `WebSocketNetwork`, pass its bound `transmit` method as `networkWrite`, attach
the runtime, connect it, and pass `true` as the final `runtime.startFromUrl()` argument.
The endpoint URL belongs to the page integration rather than `riscbox.cfg`.
See `network/README.md` in the distribution for the binary Ethernet protocol,
limits, reconnect behavior, and production origin-service requirements.

9p file sharing
---------------

A filesystem names a bound Rust namespace and mount tag with `{ server, tag }`.
The former `file`, `socket`, and `js9p` forms and JavaScript protocol-server
registration are not supported. Create and bind the namespace before startup:

```js
import { Filesystem, createHttpsSeedPlugin } from "./p9/index.js";
const runtime = await Riscbox.instantiate(wasmBytes, options);
const workspace = await Filesystem.create(runtime);
await workspace.writeFile("hello.txt", "shared with the guest\n");
await workspace.bind("workspace");
await runtime.startFromUrl(configUrl);
console.log(new TextDecoder().decode(await workspace.readFile("hello.txt")));
```

Host operations always return promises and reject with `FilesystemError`
carrying positive Linux errno. Filesystems remain usable before boot, while
halted, and after VM destroy. Multiple endpoints in one VM may share a handle;
multiple live VMs may not. Bind can replace a key only while the runtime is
inactive. VM reset retains namespace data and retires protocol state.
Filesystem reset replaces data while keeping device queues coherent; the guest
may need to remount. Destroy can also cancel startup and ignores late boot
responses, allowing the runtime to select another namespace.

Large static trees use `SeedBuilder` and an arbitrary typed source loader, or
`createHttpsSeedPlugin({ files: [{ path, size, source? }] }, baseUrl)`.
`installSeed(plugin)` installs metadata without requesting bodies. Host and
guest reads share on-demand loads; no preload mechanism exists. Source failures
return `EIO`, and `retrySource(path)` allows a later read to retry. Hosts supply
credentials and protocol handling in custom loader plugins. See the
[facade guide](https://github.com/russross/riscbox/blob/main/js/p9/README.md)
for subscriptions and source ownership.

Mount using the configured tag:

    mount -t 9p -o trans=virtio,version=9p2000.L,cache=none shared /mnt/shared

Browser integration
-------------------

The supplied page loads `riscbox.js`, instantiates `riscbox.wasm`, and calls
`runtime.startFromUrl()` with the configuration URL. Custom integrations use the same
small adapter:

*   Pass `consoleWrite`, `onVmStarted`, `onError`, and optional scheduling
    callbacks to `Riscbox.instantiate()`. For networking, also pass the bound
    `WebSocketNetwork.transmit` method as `networkWrite`.
*   Pass `framebufferRefresh(bytes, geometry)` to receive a zero-copy view of
    each dirty framebuffer rectangle. `geometry` supplies `x`, `y`, `width`,
    `height`, and the full framebuffer byte stride.
*   Send terminal bytes with `runtime.consoleInput(bytes)` and size changes with
    `runtime.consoleResize(columns, rows)`.
*   Forward keyboard, pointer, wheel, network packet, and carrier events through
    the corresponding runtime methods.

Updates
-------

The browser block backend starts from the published blocks each time; writes
are session-local. A build adds content-addressed boot and disk assets, retains
older generations, and replaces `riscbox.cfg` last. New VM starts fetch that
config without using browser storage and select one complete generation;
running VMs continue using their original URLs.

After the chosen wind-down period, remove generations not referenced by the
current config with:

    ../../tools/image_deployment.py clean ./dist

The cleaner only considers `drive-HASH`, `linux-HASH`, `linux-HASH.gz`,
`u-boot.bin-HASH.gz`, and `fw_dynamic.bin-HASH.gz` assets. It leaves the active
generation and unrelated deployment files intact.
