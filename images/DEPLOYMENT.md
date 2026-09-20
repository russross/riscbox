Riscbox image deployment
========================

This directory is a complete browser-deployable Riscbox VM.

Files
-----

*   `index.html` is the supplied browser integration.
*   `riscbox.js` and `riscbox.wasm` are the emulator runtime.
*   `riscbox.cfg` describes the virtual machine. Boot paths are relative to
    this file.
*   `fw_jump.bin-HASH` is OpenSBI firmware and `linux-HASH` is the guest kernel.
*   `drive-HASH/blk.txt` describes the HTTP disk; its
    `blkNNNNNNNNN.bin` files are 256 KiB blocks. The hashes identify source
    content, so assets shared by image generations retain stable URLs.
*   `p9/` contains the generated browser-backed 9p server modules and
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
    bios: "fw_jump.bin-81ceef21",
    kernel: "linux-a837bc72",
    cmdline: "root=/dev/vda rw rootfstype=ext4 console=hvc0",
    drive0: { file: "drive-827a7b2f/blk.txt" },
    console: "virtio",
}
```

The important options are:

*   `memory_size` sets RAM in MiB. `bios`, `kernel`, and optional `initrd`
    select boot payloads.
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
    fs0: { js9p: true, tag: "shared" },
    eth0: { driver: "user" },
    display0: { device: "simplefb", width: 1024, height: 768 },
    input_device: "virtio",
}
```

`display0` needs a `graphic_display` object in the page, and VirtIO input needs
the page to forward keyboard, pointer, and wheel events. `ethN` needs a browser
network frontend; setting `net_state = null` deliberately leaves networking
unavailable. Native Riscbox supports `driver: "user"` directly and also a TAP
backend with `driver: "tap"` and `ifname`.

9p file sharing
---------------

A browser-backed filesystem uses `js9p: true`. When instantiating the runtime,
pass a `p9Servers` map. Each registered server creates an independent session
with asynchronous `request(request, replyCapacity)` and synchronous `close()`
methods. The generated `build/js/p9/index.js` module provides
`Memory9PServer`:

```js
import { Memory9PServer } from "./p9/index.js";

const server = new Memory9PServer({
    "hello.txt": "shared with the guest\n",
});

const runtime = await Riscbox.instantiate(wasmBytes, {
    p9Servers: new Map([["default", server]]),
});
```

Application operations return explicit results rather than throwing for normal
filesystem errors:

```js
const result = server.readFile("hello.txt");
if (result.kind === "ok") {
    console.log(new TextDecoder().decode(result.value));
}
```

Large static trees may instead use `SeedBuilder` with a single typed loader.
`readFile()` reports `not-loaded`; `readFileAsync()` and guest reads start and
share the load. The supplied module includes HTTPS-manifest and pre-downloaded
tar plugin examples. Deployment manifests, archives, loader interpretation,
and content-addressed URLs should be immutable for a filesystem instance;
publish a new plugin and server instance to refresh them.

Mount it in Linux with the same tag used by the configuration:

    mount -t 9p -o trans=virtio,version=9p2000.L shared /mnt/shared

Browser integration
-------------------

The supplied page loads `riscbox.js`, instantiates `riscbox.wasm`, and calls
`runtime.start()` with the configuration URL. Custom integrations use the same
small adapter:

*   Pass `consoleWrite`, `onVmStarted`, `onError`, `networkWrite`, and optional
    scheduling callbacks to `Riscbox.instantiate()`.
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

The cleaner only considers `drive-HASH`, `linux-HASH`, and `fw_jump.bin-HASH`
assets. It leaves the active generation and unrelated deployment files intact.
