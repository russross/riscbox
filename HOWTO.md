Embedding workflows
===================

These examples use the current JavaScript adapter. [API.md](API.md) defines each
call's arguments, state prerequisites, return values, and errors. The
[release-based demo](https://github.com/russross/riscbox/tree/main/demo) provides
a complete plain JavaScript application with explicit lifecycle and file controls.

Serve an application around a release
-------------------------------------

Extract `riscbox-VERSION.tar.gz` into `dist/riscbox/`. Keep that subtree unchanged:
it contains the matching JavaScript/WASM/declarations, Linux/OpenSBI/U-Boot boot
payloads, splitter, and documentation. Put your HTML, configuration, source
trees, and guest disk beside it. The archive contains no guest root filesystem.

Prepare a raw root filesystem appropriate for your application. EROFS works for
a read-only base with guest tmpfs overlays; ext4 works for a writable disk. Split
the image using the release's executable; `uv` supplies Python 3.13 or later:

```sh
mkdir -p dist
./dist/riscbox/splitimg.py rootfs.erofs dist
```

The command prints a content-named directory and block count. Default chunks
are 512 KiB; an optional third argument selects a power-of-two size in KiB.
Use the printed directory for `drive0.file` in `dist/riscbox.cfg`, replacing the
boot payload hash suffixes with filenames from the archive:

```js
{
    version: 1, machine: "riscv64", memory_size: 128,
    bios: "riscbox/fw_dynamic.bin-HASH.gz",
    kernel: "riscbox/linux-HASH.gz",
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
upload hash-named assets first and replace configuration last. Keep old assets
while old pages can request them. Serve matching JavaScript and WASM together.

Prepare storage before the first boot
------------------------------------

Create the adapter once, prepare a halted machine, populate a share, and boot.
Preparation downloads boot assets without guest execution, so no guest can race
the initial host writes. Load the browser script before your application module:

```html
<script src="riscbox/riscbox.js"></script>
<script type="module" src="app.js"></script>
```

In `app.js`, callbacks connect the VM to your application. Keep the shutdown
resolver for the orderly-shutdown workflow below; your UI can also render state
from these callbacks. Startup means CPU execution began, not that login is ready.

```js
let finishShutdown;
const response = await fetch("riscbox/riscbox.wasm");
if (!response.ok) throw new Error(`WASM HTTP ${response.status}`);
const runtime = await Riscbox.instantiate(await response.arrayBuffer(), {
    consoleWrite: text => terminal.write(text),
    onVmHalted: cause => { finishShutdown?.(cause); },
    onError: error => console.error(error),
});
await runtime.prepareFromUrl(new URL("riscbox.cfg", location.href).href);
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

After halt, `workspace.clear()` and explicit host writes can install another
project. Fetch every source body before clearing if a download failure should
preserve the outgoing project. The disk and share have independent lifetimes;
replacing 9p does not discard block changes. Boot explicitly when ready.

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
runtime.block(0).discardChanges();
await runtime.boot();
```

Add `workspace.clear()` and your source-loading writes before boot only when
you also want to reset the project. Array disks have no discardable overlay;
restore their bytes explicitly or destroy/prepare them again. Every boot
recreates guest tmpfs, so home/tmp contents are distinct from retained disk and
9p storage. Neither page reload nor destroy preserves VM-owned stores.

Cancel preparation or replace the VM
-----------------------------------

Preparation reserves `preparing` before it downloads. `destroy()` can cancel
that operation; its preparation promise rejects and late responses are ignored.
A failed preparation cleans itself up. Handle the rejection even when your UI
deliberately cancelled it.

To replace a live VM, first shut down or halt, then `await runtime.destroy()`.
Existing disk/share objects become invalid. Reuse the same runtime with another
prepare/start call and obtain new storage objects; do not retain the old ones.
Destroy releases the machine but does not remove your application-owned editor
or saved snapshots.

Use a host-supplied disk or configuration
---------------------------------------

For a downloaded raw disk, copy bytes into the VM during preparation. Load a
deployed configuration first when you want its boot URLs/defaults while changing
one storage entry:

```js
const config = await Riscbox.loadResolvedConfig(new URL("riscbox.cfg", location.href).href);
const diskResponse = await fetch("uploaded-disk.img");
if (!diskResponse.ok) throw new Error(`Disk HTTP ${diskResponse.status}`);
const bytes = new Uint8Array(await diskResponse.arrayBuffer());
await runtime.prepareResolved({ ...config, drive0: { bytes } });
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

The packaged `fw_dynamic.bin-HASH.gz` starts the packaged Linux kernel in S-mode
through OpenSBI. Without firmware, a flat kernel starts in M-mode, useful for
bare-metal guests such as xv6. Initrd bytes are passed unchanged; compressed
firmware/kernel output is bounded by the configured RAM layout.

To use U-Boot, select the packaged `u-boot.bin-HASH.gz` as `kernel` with OpenSBI
as `bios`. Boot media must contain a compatible RISC-V EFI loader at
`/EFI/BOOT/BOOTRISCV64.EFI`. Split a raw ISO with the same splitter or use an array
drive; Alpine standard media needs 512 MiB RAM. U-Boot scans FAT EFI partitions
and El Torito FAT boot images. Its EFI application reads ISO9660 and loads the
media's kernel/initramfs. The bundled Linux Image also includes its EFI stub,
FAT/VFAT, ISO9660, loop, and SquashFS support.

Attach the optional network frontend
-----------------------------------

Networking requires both configured `eth0: {driver: "user"}` and an enabled
startup flag. Attach/connect the release's WebSocket frontend before boot; it
can report transport carrier before a VM exists:

```js
import { WebSocketNetwork } from "./riscbox/network/index.js";
const network = new WebSocketNetwork("wss://your-origin.example/ethernet");
const runtime = await Riscbox.instantiate(wasmBytes, { networkWrite: network.transmit });
network.attach(runtime);
network.connect();
await runtime.startFromUrl(configUrl, 0, "", 0, 0, true);
```

The repository supplies no production origin service. Your origin owns routing,
authentication, isolation, filtering, and DHCP/DNS/NAT where needed. Close the
frontend when disposing of its application. The Ethernet/WebSocket wire format
and frontend contract are in the release's `network/README.md`.
