Rust filesystem browser facade
==============================

`Filesystem` exposes the Rust namespace through promises. Rust owns all
9P2000.L sessions and VirtIO replies. The facade owns source plugins, pending
host reads, and subscriptions. See the [protocol profile](../../src/ninep_protocol/README.md)
and [raw ABI guide](../../src/browser_abi/ninep/README.md).

Host operations
---------------

```js
import { Filesystem, createHttpsSeedPlugin } from "./p9/index.js";
const runtime = await Riscbox.instantiate(wasmBytes, options);
const workspace = await Filesystem.create(runtime);
await workspace.writeFile("hello.txt", "hello\n");
await workspace.bind("workspace");
await runtime.startFromUrl(configUrl);
const bytes = await workspace.readFile("hello.txt");
```

Configure `fs0: { server: "workspace", tag: "shared" }` and mount `shared`
with Linux `trans=virtio,version=9p2000.L,cache=none`. Bind before VM startup.
Multiple endpoints in one VM may use the same tree. A filesystem belongs to
its runtime's WASM instance and cannot be attached to a second live VM.

All host operations return promises and reject with `FilesystemError`, whose
`errno` is a positive Linux errno. Operations include `readFile`, `writeFile`,
`mkdir`, `remove`, `rename`, `listDirectory`, `listFiles`, `stat`, `symlink`,
`readlink`, `link`, `installSeed`, `retrySource`, `reset`, `bind`, and `close`.
Writes replace whole regular files and require existing parent directories.
Paths are literal namespace paths; the root is the empty string. Host reads
pin their inode through lazy loading even if its path is renamed or reused.
File identities, sizes, times, and origin values that use u64 are `bigint`.

`Filesystem.create(runtime, limits)` accepts `maxFileBytes`, `maxTreeBytes`,
`maxInodes`, and `maxDirectoryEntries`. Defaults are 256 MiB per file, 1 GiB of
logical regular-file data, and 2^20 inodes and directory entries. Hard links
count their shared data once. Open unlinked inodes remain quota-accounted.

Sources and notifications
-------------------------

`SeedBuilder<Key>` validates and freezes a manifest. `installSeed(plugin)`
atomically replaces the namespace, creates implicit parent directories, and
retains optional metadata and shared inode keys. The loader is arbitrary host
code with signature `load(key: Key, signal: AbortSignal): Promise<Uint8Array>`.
It may implement authenticated S3, Google Drive, GitHub, or another source.
Return exactly the declared file size. Source failures become `EIO`; call
`retrySource(path)` to allow a subsequent read to retry.

`createHttpsSeedPlugin(manifest, baseUrl)` supplies URL-based fetches.
`createTarSeedPlugin(archive)` declares an already downloaded tar archive and
copies file ranges when read. There is no preload API or automatic preload.
Only host or guest reads request unloaded bodies; concurrent readers join one
source load. Whole-file writes can satisfy reads while a source is pending.
Obsolete completions cannot restore overwritten or reset data.

```js
const unsubscribe = await workspace.subscribe(change => {
    if (change.source === "host" && change.origin === 1n) return;
    // Inspect path, oldPath, aliases, kind, inode, source and origin.
});
await workspace.writeFile("hello.txt", "edited\n", 1n);
await unsubscribe();
```

Listeners run in microtasks after copied events leave WASM and the synchronous
service loop. `reset` invalidates the namespace; `rescan` invalidates a cached
view after tracking enablement or queue overflow. Rename invalidates both path
prefixes. Disable tracking by removing the last subscriber. Listener failures
reach the runtime's `onError` callback, or the console when none is supplied.

Lifecycle and browser boundary
------------------------------

Host access works before boot and while halted. VM reset/reboot preserves the
namespace and pending host source reads, but discards protocol state. Halt and
shutdown preserve device and protocol state. Destroy releases the VM attachment
while retaining filesystem handles and bindings. Close requires VM teardown for
bound handles; close aborts outstanding sources and rejects pending host reads.
Filesystem reset can run while the VM runs and invalidates guest fids; the guest
may need to remount. Pending host reads reject with `ESTALE`.

Resident guest requests complete within their notifying CPU run, without a
JavaScript callback or promise. Async source requests leave WASM through the
source queue; the adapter resumes the current quantum after dispatch and wakes
a sleeping VM after completion. No response hint or promise-yield loop exists.
Input and output bytes are copied; no WASM-backed view survives an await.
