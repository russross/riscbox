Rust 9P2000.L endpoint
=====================

This is the guest protocol implementation reference. Host applications use
[API.md](API.md) and [HOWTO.md](HOWTO.md) for the JavaScript embedding boundary.

The [session and endpoint](https://github.com/russross/riscbox/blob/main/src/ninep_protocol.rs) implement the resident Rust
server over [namespace state](https://github.com/russross/riscbox/blob/main/src/ninep.rs). VirtIO owns a concrete endpoint
with one independent session over a shared VM namespace. The synchronous
browser facade uses the same Rust server. The [storage ABI](STORAGE-ABI.md)
exposes copied host operations.

Wire behavior follows the [9P2000.L reference](https://github.com/chaos/diod/blob/master/protocol.md)
and its linked Plan 9 operation specifications.

Supported profile
-----------------

| Operations                                                | Behavior                                    |
| --------------------------------------------------------- | ------------------------------------------- |
| version, flush, attach, walk                              | Independent session fids, tags, and msize   |
| lopen, lcreate, read, write, clunk                        | Inode lifetime, open modes, append/truncate |
| statfs, getattr, setattr, readdir, fsync                  | Quotas, nanoseconds, masks, stable cookies  |
| mkdir, symlink, readlink, link, renameat, unlinkat        | Literal entries and shared inode identity   |
| lock, getlock                                             | Process/client owners and range conversion  |
| auth, mknod, xattrs, legacy rename/remove, other requests | EOPNOTSUPP                                  |

The profile is a trusted in-memory share, without authentication or server-side
Unix DAC checks. Attach preserves numeric user identity for new inode ownership;
an omitted numeric UID uses the root inode's UID. The export name does not select
a subtree. Open access modes are enforced. The client resolves symlink traversal;
opening a symlink inode, including an existing symlink in `Tlcreate`, returns
`ELOOP`. Writes and fsync commit to the session's in-memory namespace, without external persistence.

Message size is negotiated up to 64 KiB. Each session permits 65,536 fids. Names are UTF-8, at
most 255 bytes, with slash and NUL excluded. Numeric fields retain their full
wire widths. Attributes advertise only the supported basic mask; birth time,
inode-generation, and data-version extension bits remain unclaimed.

Calling convention
------------------

1. Create `Filesystem::new(limits, epoch_seconds)` and
    `NinePSession::new(&mut filesystem)`. A session belongs to that tree.
2. Set epoch time explicitly before activation. Call
    `submit(&mut filesystem, bytes, reply_capacity)` for a complete synchronous
    reply. The server never starts external work or retains pending requests.
3. Call `reset(&mut filesystem)` when a device resets, and `close(...)` before
    discarding a session. Both release fids, inode references, and locks while
    preserving namespace bytes. Namespace clear invalidates prior fids.

Resident file I/O borrows or writes only the requested range. Complete requests
are parsed before dispatch, and reply space is checked before mutation.
`Twalk` installs a fid only after full success; partial success returns visited
QIDs without changing either fid. `Tsetattr` validates selected fields and
quota before changing metadata; current timestamps resolve at commit time.
`Tlcreate` may open an existing regular file unless `O_EXCL` is set, and
`O_TRUNC` clears resident bytes. Blocking lock requests
return blocked status for client retry rather than waiting inside Rust.

Protocol errors encode Linux errno with the parsed tag. Wrong namespace
identity, closed sessions, missing envelopes, and unusably small reply buffers
are typed transport errors. Valid flush messages always receive `Rflush`:
earlier requests have already completed. No function calls JavaScript.

Runtime and transport ownership
-------------------------------

Preparation creates one resident namespace per configured server name and one
independent session per mount tag. The VM owns all trees; reboot retains them,
and destroy releases them. `BrowserRuntime::with_filesystem(key, operation)`
permits host access between CPU activations, before boot, and while halted.
The raw host clear operation requires a powered-off VM.

Resident guest requests finish within the notifying CPU run, without a browser
service exit. Generic VirtIO descriptor handling remains separate from protocol
and namespace semantics. Quantum entry supplies filesystem epoch time; standalone
operations use `Filesystem::set_time`. Notifications are copied and delivered
only after the Rust namespace borrow has ended.

Validation
----------

`cargo test ninep --lib` covers the namespace and protocol through independent
packet builders: malformed/truncated requests, bounded replies, partial walks,
open modes, ownership, nanoseconds, cookies, locks, and reset lifetimes.
Executable WASM probes exercise host packets, real guest rings, synchronous
shared reads/writes, notifications, reboot, shutdown, and destroy invalidation.
The opt-in `make test-demo` additionally exercises Linux mounts and explicit
editor/guest sharing in Chrome.
