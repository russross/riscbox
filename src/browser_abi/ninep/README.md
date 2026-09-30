Filesystem raw WASM ABI
=======================

The scalar `riscbox_fs_*` exports expose independently owned Rust filesystems.
The [protocol guide](../../ninep_protocol/README.md) describes the guest profile
and reset contract. The JavaScript promise facade is a separate migration step;
existing browser applications still use the TypeScript server.

Buffer and execution contract
-----------------------------

Input buffers must start at an allocation returned by `riscbox_alloc`, and their
length must fit that allocation. Interior addresses and unallocated pointers are
rejected. Inputs are copied before dispatch; free them after the call returns.
All integers in packets are little-endian. `str` and `blob` mean a `u32` byte
length followed by exactly that many bytes; strings must be UTF-8 without NUL.
Every packet must end after its documented fields. `u64` values must be encoded
and decoded with `BigInt`/`DataView`, rather than rounded JavaScript numbers.

`riscbox_fs_data_address()` and `riscbox_fs_data_length()` describe one shared
output snapshot. Copy it immediately. The next filesystem operation, create,
bind, close, load poll/completion, or change poll replaces it. Do not retain a
JavaScript memory view across another allocating activation or an `await`.
Empty results have length zero; their address must not be dereferenced.

No operation calls JavaScript. Source promises and change listeners run after
WASM returns. A host operation or source completion polls guest sessions after
releasing the namespace borrow. This applies even when a source reports an
incorrect byte length: its pending guest requests receive `EIO`.

Exports and status
------------------

| Export                                   | Result and input                                       |
| ---------------------------------------- | ------------------------------------------------------ |
| `riscbox_fs_create(address, length)`     | Nonzero handle; input is the configuration below       |
| `riscbox_fs_bind(handle, address, len)`  | Bind a UTF-8 server key, without a length prefix       |
| `riscbox_fs_call(handle, address, len)`  | Run a host command packet                              |
| `riscbox_fs_close(handle)`               | Close an unattached handle and remove its bindings     |
| `riscbox_fs_next_load()`                 | Handle with queued work, or zero; output is a ticket   |
| `riscbox_fs_complete_load(h, addr, len)` | Complete the ticket using the packet below             |
| `riscbox_fs_next_change(handle)`         | One event available: 1; empty queue: 0                 |
| `riscbox_fs_status()`                    | Status of the last mutating filesystem export          |
| `riscbox_fs_data_address()`              | Current output address                                 |
| `riscbox_fs_data_length()`               | Current output length                                  |

Ordinary commands return 0 on completion. Read and finish-read return 1 while
pending, with the `u32` host request ID in the output. Load completion returns 1
when an otherwise valid ticket is obsolete or duplicated and is ignored.
Negative statuses are Linux errno values: `EBADF` for invalid handles/request
IDs, `EINVAL` for malformed packets, `EBUSY` for forbidden lifecycle actions,
`ESTALE` for reads invalidated by namespace replacement, and filesystem errors
such as `ENOENT`, `ENOSPC`, and `EIO`. Create returns zero on error; inspect
`riscbox_fs_status()`. Queue polls returning zero also expose their error status.
An error clears the output snapshot.

Configuration and binding
-------------------------

Create input is exactly:

```text
u32 max_file_bytes
u32 max_tree_bytes
u32 max_inodes
u32 max_directory_entries
u64 epoch_seconds
```

At least one inode is required for the root; file capacity cannot exceed tree
capacity. Change tracking starts disabled. Handles and host request IDs are
monotonic and never recycled within a WASM instance; zero is not a handle.

Bind before VM startup. Configured `{ server, tag }` endpoints matching the key
receive independent Rust sessions. Several endpoints or keys may share a handle
in one VM. A namespace attachment guard rejects a second live VM. Failed startup
releases acquired guards; reset, reboot, shutdown, and host halt retain them.
Destroy releases them, while the filesystem and key bindings remain available
for later VM creation. Closing a bound handle requires VM teardown, including
teardown of a halted VM. Closing an unbound standalone handle is independent of
another VM. Transitional unregistered keys still use JavaScript servers.

Host commands
-------------

Each call input starts with `u32 operation`, `u64 epoch_seconds`, and
`u64 host_origin`, followed by the body in this table. Epoch seconds are supplied
explicitly; raw WASM has no OS clock. The origin is recorded in change events.

| Code | Operation       | Body                         | Successful output        |
| ---- | --------------- | ---------------------------- | ------------------------ |
| 1    | Read whole file | `str path`                   | File bytes or request ID |
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
| 12   | Reset namespace | Empty                        | Empty                    |
| 13   | Change tracking | `u32 enabled` (0 or 1)       | Empty                    |
| 14   | Install seed    | Seed records below           | Empty                    |
| 15   | Finish read     | `u32 request_id`             | File bytes or request ID |
| 16   | Cancel read     | `u32 request_id`             | Empty                    |
| 17   | Retry source    | `str path`                   | Empty                    |

Operations work before boot, during execution boundaries, and while halted.
Writes require existing parent directories and replace the whole regular file.
Remove deletes a file, symlink, or empty directory. The root directory path is
the empty string. The host API uses literal namespace paths; readlink exposes
symlink targets rather than silently traversing them.

A pending read pins its inode independently of paths and guest fids. Finish
returns the original inode's current bytes after loading, even if its old path
was renamed, unlinked, or reused. A host write to that inode can satisfy the read
before the source finishes. Finish or cancel releases the pin; closing releases
all pins. At most 1024 host reads may be pending per handle. Reset/seed replacement
invalidates their generation; finish then returns `ESTALE` without touching a
replacement inode. VM reset leaves these host reads and source work intact.
Cancel retires the host read but does not cancel namespace source work shared
with another reader. Retry changes a failed source back to on-demand state.

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

Seed installation
-----------------

Seed body is `u32 count` followed by entries. Each entry is `str path`,
`u32 kind`, its kind-specific fields, and metadata:

| Kind | Fields after kind        |
| ---- | ------------------------ |
| 1    | Directory: none          |
| 2    | File: `u32 size, source` |
| 3    | Symlink: `str target`    |
| 4    | Hard link: `str target`  |

Metadata begins with a `u32 mask`. Present fields follow in bit order: mode
(bit 0, `u32`), UID (bit 1, `u32`), GID (bit 2, `u32`), atime (bit 3, `u64`),
mtime (bit 4, `u64`), and ctime (bit 5, `u64`). Other bits are invalid. Seed
timestamps use epoch seconds. Installation validates the complete candidate
tree before replacement, creates implicit parents, and preserves manifest
timestamps. File bodies are never fetched during installation. There is no
preload operation or automatic preloading.

Source tickets and completions
------------------------------

`riscbox_fs_next_load()` drains the started-load queue shared by host reads and
guest requests. Joined loads produce no extra dispatch. Output is exactly:

```text
u64 inode, namespace_generation, load_id
u32 source_id, expected_size
```

Copy this 32-byte ticket and dispatch the source promise outside WASM. Complete
input is the copied ticket, `u64 epoch_seconds`, `u32 status` (0=success,
1=failure), and `blob bytes`. Failure requires an empty blob. Success requires
the exact advertised size; a mismatch settles the source as failed. Every
ticket field is checked. Host overwrite, reset, retry, or deletion can obsolete
a ticket; valid obsolete completions return 1. A closed handle returns `EBADF`.

After source completion or a host mutation, the facade finishes pending host
reads and polls queued changes. Guest completions are published automatically.
Poll source work whenever a quantum returns host-service-required and after a
host call that starts a read. Source polling is separate from the existing
`riscbox_next_action()` queue. Resume the current quantum after source dispatch;
later completions schedule a guest wakeup. Never call finish-quantum while its
outcome still requires host service. This boundary requires no response hint.

Change events
-------------

Tracking enablement yields a rescan invalidation. Next-change output is:

```text
u32 kind
u64 inode                       # 0 means absent
u32 source                      # 0=host, 1=guest, 2=loader
u64 host_origin                 # 0 for guest/loader
str path
u32 has_old_path                # 0 or 1
str old_path                    # present only when has_old_path=1
u32 alias_count
str aliases[alias_count]
```

Kinds are create=1, write=2, remove=3, rename=4, metadata=5, loaded=6,
load-error=7, reset=8, rescan=9. Rescan means queued detail was lost or tracking
was enabled; reset means namespace replacement. Directory rename invalidates
old/new path prefixes. The event queue retains at most 1024 detailed records;
overflow yields rescan. Copy an event before invoking host listeners.

Validation
----------

Native tests cover malformed and truncated packets, atomic seed replacement,
quotas, pinned reads, source sharing, cancellation, failures, and generation
invalidation. Runtime tests exercise shared endpoints, rejected second-VM
attachment, startup failure, reset, halt, destroy, and recreation. The deployed
export probe runs in Node during `make test-unit` and Chrome during `make test`.
It uses copied binary packets, promise continuations, invalid input pointers,
host notifications, and a non-PIE guest firmware that verifies its lazy read
reply after completion through the public scalar ABI.
