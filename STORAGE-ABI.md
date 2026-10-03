Platform storage raw WASM ABI
=============================

The scalar exports expose storage owned by the prepared VM. The single browser
adapter wraps copied packets with synchronous filesystem operations and disk
reads that return either resident copies or promises for HTTP misses.

Ownership and buffers
---------------------

`riscbox_prepare_resolved` constructs a powered-off VM and emits action 15
(Prepared). `riscbox_fs_get(address, length)` finds the configured UTF-8 server
name and returns a nonzero handle, or zero with `riscbox_fs_status()` set.
Names are raw UTF-8 without a length prefix. Handles are monotonic within the
WASM instance. Endpoints sharing a name receive independent protocol sessions.
Reboot, halt, and reset retain stores. Destroy invalidates every handle.

Input buffers must begin at an allocation returned by `riscbox_alloc`; their
length must fit that allocation. Inputs are copied before dispatch. Packet
integers are little-endian; `str` and `blob` are a `u32` byte length followed by
bytes. Strings are UTF-8 without NUL. Trailing packet bytes are invalid.
Encode full-width `u64` values with BigInt/DataView.

`riscbox_fs_data_address()` and `riscbox_fs_data_length()` identify the copied
output snapshot. Copy it before any subsequent filesystem activation; never
retain a WASM memory view across an await or an allocating call. Empty output
has length zero. No operation calls JavaScript while Rust is borrowed.

`riscbox_fs_call(handle, address, length)` returns zero on success or negative
Linux errno. `riscbox_fs_next_change(handle)` returns one for a copied event or
zero for no event; inspect `riscbox_fs_status()` for poll errors. Invalid handles
return EBADF, malformed packets EINVAL, forbidden lifecycle operations EBUSY.
All filesystem operations complete synchronously; no source tickets exist.

Host commands
-------------

Each call input starts with `u32 operation`, `u64 epoch_seconds`, and
`u64 host_origin`, followed by the body in this table. Epoch seconds are supplied
explicitly; raw WASM has no OS clock. The origin is recorded in change events.

| Code | Operation       | Body                         | Successful output        |
| ---- | --------------- | ---------------------------- | ------------------------ |
| 1    | Read whole file | `str path`                   | File bytes               |
| 2    | Write/create    | `str path, blob bytes`       | Empty                    |
| 3    | Mkdir           | `str path`                   | Empty                    |
| 4    | Remove          | `str path`                   | Empty                    |
| 5    | Rename          | `str old_path, str new_path` | Empty                    |
| 6    | List directory  | `str path`                   | Directory records        |
| 7    | List file paths | Empty                        | Count and strings        |
| 8    | Stat            | `str path`                   | Metadata record          |
| 9    | Symlink         | `str path, str target`       | Empty                    |
| 10   | Readlink        | `str path`                   | Raw UTF-8 target bytes   |
| 11   | Hard link       | `str existing, str new_path` | Empty                    |
| 12   | Clear namespace | Empty                        | Empty                    |
| 13   | Change tracking | `u32 enabled` (0 or 1)       | Empty                    |
| 14   | Set attributes  | Attribute body below        | Empty                    |

The attribute body is `str path`, `u32 mode`, `u32 uid`, `u32 gid`,
`u64 atime_seconds`, `u32 atime_nanoseconds`, `u64 mtime_seconds`, and
`u32 mtime_nanoseconds`. Mode is limited to `07777`; nanoseconds must be below
one billion. Decode and validation finish before any mutation. The operation
changes ctime and QID version and emits a metadata event carrying the origin.
The synchronous facade exposes `setAttributes(path, FileAttributes, origin?)`.

Operations work before boot, during execution boundaries, and while halted.
Clear is permitted only while powered off.
Writes require existing parent directories and replace the whole regular file.
Remove deletes a file, symlink, or empty directory. The root directory path is
the empty string. The host API uses literal namespace paths; readlink exposes
symlink targets rather than silently traversing them.

List-directory output is `u32 count`, then repeated `str name, u64 inode,
u32 kind, u64 cookie`. List-file-path output is `u32 count` and repeated `str`.
Kinds are directory=1, regular file=2, symlink=3. Stat output is:

```text
u64 inode
u32 kind, mode, uid, gid, version, link_count
u64 size
u64 atime_seconds; u32 atime_nanoseconds
u64 mtime_seconds; u32 mtime_nanoseconds
u64 ctime_seconds; u32 ctime_nanoseconds
```

Change events
-------------

Tracking enablement yields a rescan invalidation. Next-change output is:

```text
u32 kind
u64 inode                       # 0 means absent
u32 source                      # 0=host, 1=guest
u64 host_origin                 # 0 for guest
str path
u32 has_old_path                # 0 or 1
str old_path                    # present only when has_old_path=1
u32 alias_count
str aliases[alias_count]
```

Kinds are create=1, write=2, remove=3, rename=4, metadata=5, reset=8, rescan=9.
Rescan means queued detail was lost or tracking
was enabled; reset means namespace replacement. Directory rename invalidates
old/new path prefixes. The event queue retains at most 1024 detailed records;
overflow yields rescan. Copy an event before invoking host listeners.

Disk operations
---------------

Disk IDs are zero-based configured drive positions. Host access requires the
VM to be powered off. Sector addresses use `low | (high << 32)`; lengths must
be nonzero whole 512-byte sectors and fit the disk.

| Export                                      | Result                                  |
| ------------------------------------------- | --------------------------------------- |
| `riscbox_disk_read(disk, low, high, length)`  | 0 with bytes, positive read ID, or errno |
| `riscbox_disk_finish(read_id)`               | 0 with bytes, 1 pending, or errno        |
| `riscbox_disk_write(disk, low, high, p, len)` | 0 or errno; copied synchronous write     |
| `riscbox_disk_discard(disk)`                 | 0 or errno; retire reads, discard CoW    |
| `riscbox_disk_capacity(disk, high)`          | Low/high capacity half, 0 on error       |
| `riscbox_disk_data_address()`               | Current disk output snapshot address     |
| `riscbox_disk_data_length()`                | Current disk output snapshot length      |

Read IDs are monotonic; pending reads own their destination bytes. Resident
reads create no request ID. HTTP errors return EIO and retired IDs return
ESTALE. Boot is rejected while host reads are pending. `riscbox_cold_reset`
requires a powered-off VM, retires reads and fetches, clears RAM, and reloads
boot images without replacing storage. Discard removes HTTP overlays while
retaining clean cache; it fails for array disks.

HTTP actions carry `riscbox_action_disk()`: zero means a startup asset;
otherwise subtract one for the disk index. Fetch completions are copied through
`riscbox_http_complete`; HTTP errors allow an empty response buffer. The adapter
retires obsolete completions after lifecycle generation changes.

Validation
----------

`make test-unit` exercises namespace/protocol behavior, malformed packets,
invalid pointers, notifications, array copies, sparse HTTP overlays, joined
misses, failures/retries, powered-off restrictions, and lifecycle retirement
through executable WASM operations. `make test` repeats these probes in Chrome.
