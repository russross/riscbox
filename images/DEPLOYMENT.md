Riscbox image deployment
========================

This directory is a complete browser-deployable Riscbox VM. It also contains
the unsplit `rootfs.ext4` for native use and future image maintenance.

Files
-----

*   `index.html` is the supplied browser integration.
*   `riscbox-wasm.js` and `riscbox-wasm.wasm` are the emulator runtime.
*   `riscbox.cfg` describes the virtual machine. Boot paths are relative to
    this file.
*   `riscbox-native.cfg` is the native equivalent when included. It selects
    the unsplit ext4 disk and native host backends instead of browser resources.
*   `fw_jump.bin` is OpenSBI firmware and `linux` is the guest kernel.
*   `rootfs.ext4` is the complete writable disk image for native tools.
*   `drive/blk.txt` describes the HTTP disk; `drive/blkNNNNNNNNN.bin` files are
    its 256 KiB blocks.
*   `p9.js` and its `p9.d.ts` declarations are included when the integration
    uses the browser-backed 9p server.

Publishing
----------

Copy the directory without changing its internal layout. For example:

    rsync -av --delete ./dist/ server:/srv/www/image/

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
    bios: "fw_jump.bin",
    kernel: "linux",
    cmdline: "root=/dev/vda rw rootfstype=ext4 console=hvc0",
    drive0: { file: "drive/blk.txt" },
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

A browser-backed filesystem uses `js9p: true`. Before starting the VM, set
`Module.p9Server` to a synchronous object with a
`request(request, replyCapacity)` method. The supplied `p9.js` provides
`Memory9PServer`:

```js
import { Memory9PServer } from "./p9.js";

Module.p9Server = new Memory9PServer({
    "hello.txt": "shared with the guest\n",
});
```

Mount it in Linux with the same tag used by the configuration:

    mount -t 9p -o trans=virtio,version=9p2000.L shared /mnt/shared

Native integrations may instead use `fsN: { file: "directory", tag: "shared" }`
for a directory backend, or `fsN: { socket: "server.sock", tag: "shared" }`
for a Unix-domain 9p server. Browser HTTP filesystems use a `file` manifest but
are read-only from the server's perspective.

Browser integration
-------------------

Define the host objects before loading `riscbox-wasm.js`:

*   `Module.onRuntimeInitialized()` calls `Module.ccall("vm_start", ...)` with
    the configuration URL. `Module.onVmStarted()` runs when devices are ready.
*   `term.write(text)` receives console output and `term.getSize()` returns
    `[columns, rows]`. Send input bytes with
    `Module._console_queue_char(byte)` and notify size changes with
    `Module._console_resize()`.
*   `update_downloading(active)` reports asset fetch activity.
*   `graphic_display` and `net_state` may be `null` when those devices are not
    configured.

Native testing and updates
--------------------------

When `riscbox-native.cfg` is included, use it with the unsplit disk for
persistent native writes:

    /path/to/riscbox -rw riscbox-native.cfg

Do not pass the browser `riscbox.cfg` to the native emulator: its
`drive/blk.txt` resource is an HTTP block descriptor, not a raw local disk.

The browser block backend starts from the published blocks each time; writes
are session-local. To make persistent changes, update the image inputs, rebuild,
and publish the complete `dist/` directory. Updating only some block files can
mix image generations in intermediary caches, so deployments should replace
the config, boot files, runtime, `blk.txt`, and all blocks together.
