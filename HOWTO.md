Embedding workflows
===================

These examples use the current JavaScript adapter. [API.md](API.md) defines each
call's arguments, state prerequisites, return values, and errors. The
[release-based demo](https://github.com/russross/riscbox/tree/main/demo) provides
a complete plain JavaScript application with lifecycle controls and live 9p editing.

Serve an application around a release
-------------------------------------

Extract `riscbox-VERSION.tar.gz` into a working directory. It contains guides,
declarations, the splitter, and one `riscbox-HASH` directory of compressed
client assets. Copy that asset directory into `dist/` and expand each `.br`
file alongside its compressed original:

```sh
mkdir -p dist
cp -R riscbox-VERSION/riscbox-HASH dist/
for file in dist/riscbox-HASH/*.br; do brotli -d -k "$file"; done
```

Configure the server to prefer the `.br` sibling when Brotli is accepted and
fall back to normal content encoding or the original file. For Caddy, use
the following directives in the site block:

```caddyfile
encode zstd gzip
file_server {
    precompressed br
}
```

Verify the response has `Content-Encoding: br` when requested and that WASM has
content type `application/wasm`, for example with:

```sh
curl -I -H 'Accept-Encoding: br' https://example.org/riscbox-HASH/riscbox.wasm
```

URLs always name the original file, without `.br`.
The archive contains no guest root filesystem.

Prepare a raw root filesystem appropriate for your application. EROFS works for
a read-only base with guest tmpfs overlays; ext4 works for a writable disk. Split
the image using the release's executable; `uv` supplies Python 3.13 or later:

```sh
mkdir -p dist/images
./riscbox-VERSION/splitimg.py rootfs.erofs dist/images
```

The command prints a content-named directory and block count. Default chunks
are 512 KiB; an optional third argument selects a power-of-two size in KiB.
When `brotli` is installed, each block also has a quality 11 `.br` sibling;
the original block remains available. Without Brotli the splitter warns and
continues with the original blocks.
Use the printed directory for `drive0.file` in `dist/images/rootfs.cfg`. You can
rename `drive-HASH` to `rootfs-drive-HASH` to distinguish multiple images:

```js
{
    version: 1, machine: "riscv64", memory_size: 128,
    bios: "../riscbox-HASH/fw_dynamic.bin",
    kernel: "../riscbox-HASH/linux",
    cmdline: "root=/dev/vda ro rootfstype=erofs console=hvc0",
    console: "virtio", uart_output: true,
    drive0: { file: "drive-HASH/blk.txt" },
    fs0: { server: "workspace", tag: "workspace" },
}
```

For ext4, use `root=/dev/vda rw rootfstype=ext4`. The supplied kernel supports
both. Guest initialization owns mounts, overlays, users, login, and power-event
handling; these are application image policy, not adapter features.

Serve over HTTP/HTTPS, with `.wasm` mapped to `application/wasm` and CORS for
cross-origin assets. A local server is sufficient:

```sh
python3 -m http.server --directory dist 8000
```

Open <http://localhost:8000/>. Riscbox has no JavaScript runtime dependencies;
the demo's CDN terminal/editor require Internet access. For deployment updates,
upload the new asset directory and image drives first, then replace configuration
and update the HTML script URL. Keep old directories while old pages can request
them. Each image can use a separate named drive directory under `images/`.

Prepare storage before the first boot
------------------------------------

Prepare a halted machine in one call, populate a share, and boot.
Preparation downloads boot assets without guest execution, so no guest can race
the initial host writes. Load the browser script before your application module:

```html
<script src="riscbox-HASH/riscbox.js"></script>
<script type="module" src="app.js"></script>
```

In `app.js`, callbacks connect the VM to your application. Keep the shutdown
resolver for the orderly-shutdown workflow below; your UI can also render state
from these callbacks. Startup means CPU execution began, not that login is ready.

```js
let finishShutdown;
const runtime = await Riscbox.prepare({
    config: { url: "images/rootfs.cfg" },
    consoleWrite: text => terminal.write(text),
    onVmHalted: cause => { finishShutdown?.(cause); },
    onError: error => console.error(error),
});
const workspace = runtime.filesystem("workspace");
workspace.writeFile("hello.c", '#include <stdio.h>\nint main(void) { puts("Hello from 9p"); }\n');
await runtime.boot();
```

Your guest mounts the share by its tag, for example:

```sh
mount -t 9p -o trans=virtio,version=9p2000.L,cache=none workspace /workspace
cd /workspace
tcc -o hello hello.c
./hello
```

Choose ownership/modes that permit the guest user to create files in the share.
Use `setAttributes("", attrs)` for the root directory and explicit attributes
for files when the namespace defaults do not match your guest. The demo shows
UID/GID 1000 ownership. Keep generic tools on the block image and project files
on 9p: a new project then needs no disk rebuild.

Copy files explicitly between host and guest
-------------------------------------------

The share is resident VM-owned storage. The host can read and write it while
the guest runs. With an uncached guest mount, host changes become visible to
guest reads; guest writes become visible to host reads. An editor buffer remains
an independent copy until you save it:

```js
const entries = workspace.listDirectory("");
const path = "hello.c"; // selected explicitly by the host UI
const editorText = new TextDecoder().decode(workspace.readFile(path));
editor.value = editorText; // a host-owned textarea

// On an explicit Save action, replace the file with the current editor text.
workspace.writeFile(path, editor.value);
```

For an automatically updating file view, use `subscribe()` and relist on `rescan`.
Give your writes a distinct bigint origin if you want to ignore their reflected
notifications. Subscriptions report namespace changes; they do not save editor
text or resolve conflicts. An application chooses its own buffering policy.
The demo saves on editor blur or after 30 seconds of inactivity and replaces
buffered edits immediately when an external change arrives. Its source selector
clears and repopulates the live share without halting the VM.

Terminal input also needs explicit buffering. `consoleInput(bytes)` returns the
number accepted, which can be smaller than the input length. Retain and retry
only the unaccepted suffix in a later browser task. Clear your retry queue on
halt/reset/destroy so old keystrokes cannot enter a new boot. Send input only
while `state === "running"`; the adapter schedules execution itself.

Shut down cleanly before exporting or replacing storage
------------------------------------------------------

Soft shutdown lets the guest stop services and flush filesystems. Install a
guest power-event handler, request shutdown, and wait for the guest halt
notification. Register the waiter before requesting so an early halt is observed:

```js
const halted = new Promise(resolve => { finishShutdown = resolve; });
try {
    await runtime.requestShutdown();
    const cause = await halted;
    if (cause !== "guest-poweroff") throw new Error(`Shutdown ended with ${cause}`);
} finally {
    finishShutdown = undefined;
}
const disk = runtime.block(0);
const image = await disk.read(0n, Number(disk.capacitySectors) * 512);
```

Handle request rejection in your UI. A resolved request only confirms event
delivery, and an unresponsive guest may never halt. A separate forced-halt action
can recover control, but it cannot establish that guest writes were flushed.
For large disks, export in whole-sector batches rather than allocating one
whole-disk buffer. Applications own persistence of the copied bytes.

After halt, `workspace.reset()` and explicit host writes can install another
project. Fetch every source body before clearing if a download failure should
preserve the outgoing project. The disk and share have independent lifetimes;
replacing 9p does not discard block changes. Boot explicitly when ready.

To empty a share while the guest runs, call `workspace.clear()`. This recursively
deletes names below the existing root and preserves the mount and active fids.
Open files retain their bytes until closed; a shell in a removed directory must
change to an absolute path such as `/workspace`. The guest can observe the empty
tree and each later host write. Fetching replacement bodies first avoids losing
the outgoing project to a download failure, but does not make loading atomic.
Linux can retain pathname metadata through cached fids even with `cache=none`;
fresh directory enumeration shows the cleared namespace, while existing fids
continue to describe their original objects.

Guest poweroff and forced halt both retire device interaction state, including
9p fids and locks, while preserving RAM and storage bytes. Forced halt does not
flush guest buffers. Boot always runs the guest startup procedure.

Choose an orderly reboot or forced recovery
------------------------------------------

Use `requestReboot()` when you want the guest to flush changes and perform its
normal reboot. The promise resolves after delivering the request. Observe
`onVmReset("guest-reboot")` to know the guest restarted. Disk overlays and 9p
bytes remain, while guest tmpfs is recreated by the new boot.

Use `reset()` when the OS is stuck and you want a fast restart. It requires a
running VM and immediately restarts CPU/devices from the boot payloads. It
retains disk overlays, 9p bytes, and RAM; it does not flush guest buffers or
repair a writable filesystem left midway through an update.

For recovery to a known HTTP-backed base, pair forced halt with cold reset and
explicit disk discard instead. This deliberately abandons session-local disk
writes while retaining the project share:

```js
if (runtime.state === "running") await runtime.halt();
await runtime.coldReset();
runtime.block(0).reset();
await runtime.boot();
```

Add `workspace.reset()` and your source-loading writes before boot only when
you also want to reset the project. Array disks have no discardable overlay;
restore their bytes explicitly or destroy/prepare them again. Every boot
recreates any guest tmpfs configured by the image. The demo uses writable ext4
for home/tmp and retains those files across reboots. Neither page reload nor
destroy preserves VM-owned stores.

Cancel preparation or replace the VM
-----------------------------------

Pass an `AbortSignal` to `Riscbox.prepare()` to cancel downloads and machine
preparation. The promise rejects and late responses are ignored. Failed
preparation releases partially created resources. Handle rejection even when
your UI deliberately cancels the operation.

To replace a live VM, first shut down or halt, then `await runtime.destroy()`.
Existing disk/share objects become invalid. Create a fresh client with
`Riscbox.prepare()` and obtain new storage objects. Destroyed clients cannot be
reused; do not retain their storage objects.
Destroy releases the machine but does not remove your application-owned editor
or saved snapshots.

Use a host-supplied disk or configuration
---------------------------------------

For a downloaded raw disk, copy bytes into the VM during preparation. Use a
deployed configuration and override one storage entry in the preparation call:

```js
const diskResponse = await fetch("uploaded-disk.img");
if (!diskResponse.ok) throw new Error(`Disk HTTP ${diskResponse.status}`);
const bytes = new Uint8Array(await diskResponse.arrayBuffer());
const runtime = await Riscbox.prepare({
    config: { url: "riscbox.cfg" },
    blocks: { drive0: { bytes } },
    consoleWrite: text => terminal.write(text),
});
const disk = runtime.block(0);
disk.write(0n, bootSector); // copied, whole-sector write while halted
await runtime.boot();
```

A capacity-only array drive starts zeroed. HTTP drives instead keep a bounded
clean cache and sparse session-local writes. Provide `fetchBlock` only to change
immutable chunk transport; leave validation, deduplication, write ordering, and
lifetimes with the runtime.

Boot with firmware or installation media
---------------------------------------

The packaged `fw_dynamic.bin` starts the packaged Linux kernel in S-mode
through OpenSBI. Without firmware, a flat kernel starts in M-mode, useful for
bare-metal guests such as xv6. Initrd bytes are passed unchanged; compressed
firmware/kernel output is bounded by the configured RAM layout.

To use U-Boot, select the packaged `u-boot.bin` as `kernel` with OpenSBI
as `bios`. Boot media must contain a compatible RISC-V EFI loader at
`/EFI/BOOT/BOOTRISCV64.EFI`. Split a raw ISO with the same splitter or use an array
drive; Alpine standard media needs 512 MiB RAM. U-Boot scans FAT EFI partitions
and El Torito FAT boot images. Its EFI application reads ISO9660 and loads the
media's kernel/initramfs. The bundled Linux Image also includes its EFI stub,
FAT/VFAT, ISO9660, loop, and SquashFS support.

Attach the optional network frontend
-----------------------------------

Networking requires both configured `eth0: {driver: "user"}` and an enabled
preparation flag. Attach/connect the release's WebSocket frontend before boot;
it can report transport carrier while the prepared VM is halted:

```js
import { WebSocketNetwork } from "./riscbox-HASH/network.js";
const network = new WebSocketNetwork("wss://your-origin.example/ethernet");
const runtime = await Riscbox.prepare({
    config: { url: configUrl }, hasNetwork: true, networkWrite: network.transmit,
});
network.attach(runtime);
network.connect();
await runtime.boot();
```

The repository supplies no production origin service. Your origin owns routing,
authentication, isolation, filtering, and DHCP/DNS/NAT where needed. Close the
frontend when disposing of its application. The Ethernet/WebSocket wire format
and frontend contract are in the release's `NETWORK.md`.
