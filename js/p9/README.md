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
