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
| `renameat`, `unlinkat` | Implemented |
| `lock`, `getlock` | Compatibility stubs; shared locking is milestone 4 |
| `auth`, `mknod`, `xattrwalk`, `link`, and unknown operations | `EOPNOTSUPP` |

The current namespace retains the pre-project tree model and 16 MiB per-file
limit. Inode identity, hard links, stable directory cookies, quotas, and shared
locks belong to milestone 4.
