JavaScript API reference
========================

Load the release's `riscbox.js` as a browser script. It installs `Riscbox`;
`riscbox.d.ts` describes the same client surface for TypeScript consumers.
CommonJS consumers can import `Riscbox`, `FilesystemError`, and `BlockError`.
Browser error classes are available as `Riscbox.FilesystemError` and
`Riscbox.BlockError`. Use [HOWTO.md](HOWTO.md) for complete workflows.

The adapter owns scheduling, raw exports, packet buffers, HTTP completions, and
storage handles. These are implementation details, not client APIs. Obtain VM,
filesystem, and disk objects through the factory and accessor methods below;
there are no client constructors for them. This reference describes the current
release without a cross-release compatibility promise.

States and errors
-----------------

`runtime.state` is a read-only `VmState`; `runtime.started` is true while running.

| State       | Meaning                                                        |
| ----------- | -------------------------------------------------------------- |
| `empty`     | No prepared VM; the runtime can prepare a new one.              |
| `preparing` | Configuration or boot assets are loading; destroy cancels them. |
| `halted`    | A prepared VM exists and CPU execution has stopped.             |
| `running`   | The VM can execute, including while its guest sleeps in WFI.    |

Lifecycle calls return `Promise<void>` and reject on invalid states or failed
operations. Queued controls check state when they reach the execution boundary.
Synchronous calls throw on invalid arguments or state. Validation occurs before
integer values are passed to WASM. Filesystem and disk failures use their error
classes with a positive Linux `errno`; argument errors use `TypeError` or
`RangeError`. `onError` reports asynchronous runtime and subscriber failures;
it does not replace rejection/throw handling for explicit calls.

Preparation
-----------

### `Riscbox.instantiate(source, options?)`

Returns `Promise<Riscbox>` in `empty` state. `source` is a `BufferSource` or
compiled `WebAssembly.Module` for the matching release. Instantiation checks the
runtime interface and options; unknown option names are rejected. The runtime
is reusable after destroy. Supply browser Web Crypto for guest entropy.

### `Riscbox.loadResolvedConfig(url, commandLine?, fetchRequest?)`

Returns `Promise<VmConfig>` without creating or changing a VM. `url` must be a
nonempty string; use an absolute URL to resolve relative assets unambiguously.
The default fetch is `globalThis.fetch`. Configuration uses `no-store`, accepts
comments/unquoted keys/trailing commas, supplies defaults, and resolves boot and
drive paths relative to its URL. `commandLine` defaults to `""`; a nonempty value
appends to `cmdline`, or replaces it when beginning with `!`. HTTP and syntax
errors reject. This is useful before replacing a disk entry in a host object.

### `prepareResolved(config, ramMiB?, width?, height?, hasNetwork?)`

Requires `empty`; returns `Promise<void>` after building a `halted` VM.
`config` is a `VmConfig`; asset URLs are used as supplied. Array drive bytes are
copied during the call. The optional overrides default to `0, 0, 0, false`.
RAM and dimensions are unsigned 32-bit integers; zero selects configured values.
`hasNetwork` must be boolean and enables the configured `eth0` device when true.
Boot payload bounds/overlap and platform configuration are validated by Rust.
The call reserves `preparing` before asynchronous work begins. Failed preparation
cleans up to `empty`; successful preparation permits host storage population.

### `prepareFromUrl(url, ramMiB?, commandLine?, width?, height?, hasNetwork?)`

Requires `empty`. Loads configuration through the runtime's fetch option, then
prepares as above. The URL and command-line contracts match
`loadResolvedConfig`; overrides have the same defaults. Returns `Promise<void>`
in `halted` state. Concurrent preparation is rejected. Destroy cancels loading;
late replies cannot replace a newer VM.

### `startResolved(config, ramMiB?, width?, height?, hasNetwork?)`

Requires `empty`. Prepares using `prepareResolved`, then boots. Returns
`Promise<void>` once boot has begun, not when the guest reaches login. A failed
boot can leave a prepared halted VM; inspect `state` and handle the rejection.

### `startFromUrl(url, ramMiB?, commandLine?, width?, height?, hasNetwork?)`

Requires `empty`. Prepares using `prepareFromUrl`, then boots; returns
`Promise<void>` once execution starts. Arguments and failure behavior match the
two component calls. Use preparation separately when storage must be populated
before guest execution.

Lifecycle controls
------------------

All controls return `Promise<void>`. They preserve disk/share identities unless
destroyed. Each callback observes the updated state.

| Call                | Required state                    | Contract                                                    |
| ------------------- | --------------------------------- | ----------------------------------------------------------- |
| `boot()`            | `halted`                          | Reload boot payloads, reset CPU/devices, and start running.   |
| `requestShutdown()` | `running`                         | Queue a power key; return before the guest shuts down.        |
| `requestReboot()`   | `running`                         | Queue a restart key; return before the guest reboots.         |
| `halt()`            | `running`                         | Force halt without letting guest buffers flush.              |
| `reset()`           | `running`                         | Force reboot from boot payloads, retaining RAM and stores.   |
| `coldReset()`       | `halted`                          | Clear RAM, reload boot payloads, reset devices; remain halted.|
| `destroy()`         | `empty`, `preparing`, or `halted`   | Cancel startup/free VM stores and invalidate storage objects. |

`boot()` and `reset()` reject while host disk reads are pending. `coldReset()`
retires them. Reboot/reset close guest 9p sessions and retain namespace bytes,
clean HTTP cache, and HTTP write overlays. Soft controls require a guest event
handler, such as the demo's BusyBox acpid. Observe `onVmHalted` or `onVmReset` to
know when the requested guest action happens; a resolved request is not evidence
that filesystems were flushed. `destroy()` in `empty` is allowed.

Disk access
-----------

### `block(index)`

Requires `halted` or `running`. Returns the same `BlockDisk` for each configured
zero-based integer index. Unknown indexes throw. Getting a disk while running
does not permit host disk I/O: **every disk operation below requires `halted`**.
Destroy invalidates retained objects; acquiring a later VM's disk returns a new
object. Host byte buffers are copies.

| Member                 | Contract                                                             |
| ---------------------- | -------------------------------------------------------------------- |
| `capacitySectors`      | Read-only `bigint` count of 512-byte sectors.                          |
| `read(sector, length)` | Start at unsigned 64-bit `bigint` sector; positive byte length must be a multiple of 512 and fit unsigned 32 bits and capacity. Returns `Uint8Array` when resident, or `Promise<Uint8Array>` for HTTP misses. |
| `write(sector, bytes)` | Synchronous whole-sector `Uint8Array` write within capacity; returns `void`. Array writes change owned bytes; HTTP writes create sparse overlays without fetching untouched sectors. |
| `discardChanges()`    | Synchronous `void`; discard an HTTP disk's writes while retaining clean cache. Array disks reject this operation. |

HTTP reads are deduplicated and cache bounded. Pending reads reject when retired
by cold reset or destroy. Reboot and halt retain disk bytes/overlays; page reload
and destroy discard all VM storage. Export after orderly guest shutdown for
guest filesystem consistency.

Resident filesystem access
--------------------------

### `filesystem(name)`

Requires `halted` or `running`; `name` must be a nonempty configured server name.
Returns the same `Filesystem` for that resident tree. Multiple tags with the
same server share bytes and have independent guest protocol sessions. Host
operations work while halted or running, except `clear()` requires halt.

All filesystem calls are synchronous; reads return copied bytes, not promises.
Paths are share-relative or begin with one `/`; `""` and `/` identify the root.
Empty components, `.`, `..`, NUL, and overlong names are rejected. Host lookup
does not follow symlinks. Parents must exist; host writes do not create parent
directories. Host mutations bypass guest UID checks; guest access follows the
stored permissions and mount policy. Destroy invalidates every retained facade.

| Call                                | Contract                                                     |
| ----------------------------------- | ------------------------------------------------------------ |
| `readFile(path)`                    | Return a regular file's copied `Uint8Array`.                  |
| `writeFile(path, content, origin?)`  | Create/replace regular-file bytes from `Uint8Array` or UTF-8 string; return `void`. Existing inode metadata is retained. |
| `mkdir(path, origin?)`              | Create a directory; return `void`; existing path is an error. |
| `remove(path, origin?)`             | Remove a file, symlink, or empty directory; return `void`.     |
| `rename(oldPath, newPath, origin?)`  | Rename/move with POSIX-style replacement rules; return `void`.|
| `listDirectory(path?)`              | Return direct `DirectoryEntry` children; default root `""`.   |
| `listFiles()`                       | Return regular-file paths throughout the tree.               |
| `stat(path)`                        | Return `FileStat`, including symlink metadata.                |
| `symlink(path, target, origin?)`     | Create a symlink with its literal target; return `void`.      |
| `readlink(path)`                    | Return a symlink's target string.                            |
| `link(existing, path, origin?)`     | Add a hard link to a file/symlink; return `void`.              |
| `clear()`                           | Require `halted`; empty the entire namespace; return `void`.  |
| `setAttributes(path, attrs, origin?)`| Set mode/UID/GID/atime/mtime; return `void`; update ctime/version.|
| `subscribe(listener)`              | Return an unsubscribe function; deliver copied `P9Change` events in microtasks after WASM borrows end. |

`origin` defaults to `0n` and is an unsigned 64-bit bigint used for host echo
filtering. New host files/directories have the namespace's default UID/GID 1000;
use explicit attributes when preparing another ownership policy. List results
and notifications describe current namespace state, not guest mount caches.
Use `cache=none` for host/guest sharing. An unsubscribed listener is skipped even
if an event was already queued. Subscriber errors are reported to `onError`.

Filesystem value types
----------------------

`FileKind` is `"directory"`, `"file"`, or `"symlink"`. `DirectoryEntry` contains
`name: string`, `inode: bigint`, `kind: FileKind`, and `cookie: bigint`.
`FileStat` contains inode/kind, numeric mode/uid/gid/version/linkCount,
`size: bigint`, and atime/mtime/ctime as `FileTime`. A `FileTime` has unsigned
64-bit epoch `seconds: bigint` and integer `nanoseconds` from 0 to 999,999,999.
`FileAttributes` requires mode (0 through `0o7777`), unsigned 32-bit UID/GID,
atime, and mtime. Inode identity, ctime, size, and link count are not assignable.

`P9Change` contains kind (`create`, `write`, `remove`, `rename`, `metadata`,
`reset`, or `rescan`), inode, source (`host` or `guest`), origin, path, optional
oldPath, and hard-link aliases. A `rescan` event requires relisting instead of
assuming every change was individually delivered. Reset retains bytes but
invalidates guest sessions; restoring a snapshot creates new inode IDs/ctime.

Input and transport
-------------------

These methods are synchronous and return numeric acceptance/status. Console
and device input require `running`; keyboard/tablet/wheel additionally require
`input_device: "virtio"`. Device status is `0` on success. Queued input is retired
by lifecycle reset/halt/destroy; the host must retire its own retry buffers too.

| Call                         | Values and result                                                |
| ---------------------------- | ---------------------------------------------------------------- |
| `consoleInput(bytes)`       | Require `Uint8Array`; return count accepted into the bounded FIFO. Retry only the remaining suffix later. |
| `consoleResize(cols, rows)`  | Require VirtIO console; positive unsigned 16-bit integer dimensions; return `0`. |
| `keyEvent(down, code)`      | Boolean transition and unsigned 16-bit Linux input key code; return `0`. |
| `pointerEvent(x, y, buttons)`| Unsigned 32-bit integer coordinates, clamped/scaled by the platform; mask bits 1/2/4 are left/right/middle buttons; return `0`. |
| `wheelEvent(delta)`         | Signed 32-bit integer movement at the last pointer position; return `0`. |
| `networkInput(packet)`      | Require `Uint8Array`; return `0` accepted or `1` dropped. Empty/over-65535-byte frames and non-running VMs drop. A running VM requires configured `eth0` and `hasNetwork: true`; otherwise throw. |
| `networkCarrier(up)`        | Boolean carrier status; accepted in every state so a transport can attach before preparation; return `0`. |

Runtime options and callbacks
-----------------------------

Options are supplied once to `instantiate`; the adapter copies the option
object. Callback functions are optional and are not awaited. Keep them short,
avoid throwing, and use the updated state to choose valid operations.

| Option                 | Contract                                                        |
| ---------------------- | --------------------------------------------------------------- |
| `fetch`               | `typeof fetch`; transport for configuration, boot assets, and disk manifests/chunks; default browser fetch. |
| `fetchBlock`          | `({disk, url, cache}) => Promise<Uint8Array>`; override immutable chunk transport only. Disk is zero-based; Rust validates length and owns cache/write/lifetime policy. |
| `targetQuantumMs`     | Finite duration greater than 0 and at most 100; default 20 ms.     |
| `debugTiming`         | Boolean, default false; periodic console timing diagnostics.      |
| `consoleWrite`        | `(text: string) => void`; streaming UTF-8 across chunks, isolated per VM. Halt flushes incomplete sequences as replacement characters; reset/destroy discard decoder tails. |
| `consoleReset`        | `() => void`; display reset notification on boot/reset/cold reset. The host chooses whether to clear terminal history. |
| `onVmStarted`         | `() => void`; boot/start begins execution, not userspace readiness. |
| `onVmHalted`          | `(cause: HaltCause) => void`; state is halted. Causes: `guest-poweroff`, `host-halt`, `guest-failure`. |
| `onVmReset`           | `(cause: ResetCause) => void`; state is running. Causes: `guest-reboot`, `host-reset`, `host-boot`; host boot also reports started. |
| `onVmDestroyed`       | `() => void`; storage invalidated and state empty.                |
| `onError`             | `(error: unknown) => void`; asynchronous runtime/subscriber error. |
| `networkWrite`        | `(packet: Uint8Array) => void`; one copied Ethernet frame.         |
| `framebufferClear`    | `() => void`; clear notification on boot/reset/cold reset.         |
| `framebufferRefresh`  | `(bytes, {x,y,width,height,stride}) => void`; zero-copy WASM pixel view for a dirty rectangle with full-frame stride. Consume/copy synchronously; never retain across an await or another VM operation. |

Configuration reference
-----------------------

`VmConfig` describes one little-endian RV64 VM. The adapter supplies defaults;
Rust validates the resolved machine and boot layout. Configuration-file drive
and boot paths resolve relative to the configuration URL. Host objects use URLs
as given. JSON files cannot contain `Uint8Array` or bigint values; use quoted
integers for large capacities/physical addresses.

| Field                              | Contract                                                   |
| ---------------------------------- | ---------------------------------------------------------- |
| `version`, `machine`               | Required `1` and `"riscv64"`.                               |
| `memory_size`                      | Required positive signed 32-bit integer in MiB; subject to WASM allocation limits. |
| `bios`, `kernel`, `initrd`          | Asset URL strings; at least bios or kernel required. Firmware/kernel can be raw or gzip; initrd is opaque. |
| `cmdline`                          | Guest command line; default empty.                          |
| `console`                         | `"virtio"` (default) or `"uart"`.                           |
| `uart_output`, `rtc_local_time`    | Booleans, default false; UART output mirroring and local RTC policy. |
| `bios_address`, `kernel_address`, `initrd_address`, `fdt_address` | Optional numeric or quoted integer physical addresses; RAM bounds and overlap checked. |
| `drive0` through `drive3`          | Consecutive HTTP `{file, device?}` or array `{bytes?, capacity_sectors?}` drives. Bytes must be nonempty whole sectors; explicit capacity must match bytes. Capacity accepts safe number, bigint, or integer string; capacity-only arrays start zeroed. |
| `fs0` through `fs3`               | Consecutive `{server, tag}` named resident 9p channels.        |
| `display0`                        | `{device: "simplefb", width, height}` framebuffer.           |
| `input_device`                    | `"virtio"` keyboard/tablet pair.                            |
| `eth0`                            | `{driver: "user"}`; one Ethernet interface enabled by the startup network flag. |

Default firmware/kernel addresses are `0x80000000`/`0x80200000`. Default initrd
placement is kernel address plus half of RAM, capped at 512 MiB; FDT is near the
end of RAM on a 2 MiB boundary. With OpenSBI dynamic firmware, the reset ROM
passes the kernel entry through its dynamic-info block. Without firmware, a
kernel starts in M-mode. ELF, PE/COFF, FIT, and qcow2 are not loader formats;
use flat boot binaries and raw block media.

Implementation references
-------------------------

The resident storage packets are described in [STORAGE-ABI.md](STORAGE-ABI.md)
and guest 9P2000.L in [NINEP.md](NINEP.md). Those documents describe internal
implementation boundaries; embedding applications use the JavaScript API above.
The optional WebSocket wire protocol is described in the release's
`network/README.md` (source: `js/network/README.md`).

WebSocket frontend
------------------

Import `WebSocketNetwork` from the release's `network/index.js`. It implements
transport only; the VM's startup configuration must separately enable networking.
Its `NetworkRuntime` dependency consists of `networkInput` and `networkCarrier`,
which the public `Riscbox` facade implements.

| Call                              | Contract                                                     |
| --------------------------------- | ------------------------------------------------------------ |
| `new WebSocketNetwork(url, opts?)` | Select a host-owned WebSocket endpoint; no guest-config URL.  |
| `attach(runtime)`                | Attach once and initially report carrier down; repeated attachment throws. |
| `connect()`                      | Require attachment; start connecting. Repeated calls while connecting/connected do nothing. |
| `transmit(packet)`               | Bound callback suitable for `networkWrite`; return boolean acceptance. Disconnected, invalid-size, or backpressured packets return false. |
| `close()`                        | Stop reconnects, close the socket, and report carrier down; a later connect can reopen it. |

All calls are synchronous. Options are `onError: (error: Error) => void`,
`webSocketFactory: (url: string) => WebSocket`, and injectable `setTimeout` and
`clearTimeout` functions. Defaults use browser WebSocket/timers. Protocol/error
and reconnect behavior are in the wire reference. Frames received outside VM
execution are dropped by the adapter.
