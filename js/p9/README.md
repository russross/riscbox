9P2000.L server profile
=======================

`MemoryFilesystem` owns the shared namespace and application operations.
`Memory9PServer` is its registration-compatible name. Each `connect()` creates
an independent `P9Session` with its own negotiated message size, fid table,
active tags, cancellation state, and lifetime. Sessions share namespace data
and change subscriptions only.

Operation matrix
----------------

| Operations | Status |
| ---------- | ------ |
| `version`, `flush`, `attach`, `walk`, `lopen`, `lcreate` | Implemented |
| `read`, `write`, `clunk`, `statfs`, `getattr`, `setattr` | Implemented |
| `readdir`, `fsync`, `symlink`, `readlink`, `mkdir` | Implemented |
| `link`, `renameat`, `unlinkat` | Implemented |
| `lock`, `getlock` | Shared POSIX byte-range locks |
| `auth`, `mknod`, `xattrwalk`, and unknown operations | `EOPNOTSUPP` |

Directory entries refer to stable inode/QID identities. Regular-file hard links
share contents and metadata, unlinked inodes remain live through open fids, and
directory cookies are monotonic within each directory. A session close or
`Tversion` releases its fids and byte-range locks.

`MemoryFilesystem` accepts optional `maxFileBytes`, `maxTreeBytes`, `maxInodes`,
and `maxDirectoryEntries` limits. Defaults are 256 MiB per file, 1 GiB of
logical regular-file data, and 2^20 inodes and directory entries. File bytes are
counted once per inode, including an open inode after its last link is removed.

Application API and lazy seeds
------------------------------

Application operations return `SyncResult` values. A successful result has
`kind: "ok"`; expected filesystem failures have `kind: "error"`. Reading a
lazy seed before loading it returns `kind: "not-loaded"` with every path for
that inode. Use `load(paths, retry)` or `readFileAsync(path, retry)` to request
content explicitly. A failed load is retained until a call sets `retry`.

`SeedBuilder<Key>` constructs and freezes a validated namespace with optional
metadata and shared regular-file inode keys. Pass its entries and one
`SeedLoader<Key>` to the third `MemoryFilesystem` constructor argument, or use
`MemoryFilesystem.fromSeed(plugin)`. Concurrent application and 9P reads share
one load. Whole-file application writes replace an unloaded or loading seed
without waiting, and stale loader completions cannot restore old content.

`createHttpsSeedPlugin()` is a small manifest-backed example whose opaque keys
are resolved URLs. `createTarSeedPlugin()` inspects an already downloaded tar
archive and lazily copies regular-file ranges. A filesystem pins the entry list
and loader objects stored in its inodes; refreshes require a new filesystem.
