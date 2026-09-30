Rust 9P2000.L endpoint
=====================

The [session](../ninep_protocol.rs) implements the standalone Rust server
profile over [namespace state](../ninep.rs). The production browser still
uses the TypeScript server; runtime and VirtIO attachment is separate work.

Wire behavior follows the [9P2000.L reference](https://github.com/chaos/diod/blob/master/protocol.md)
and its linked Plan 9 operation specifications. TypeScript behavior is useful
for migration comparisons but does not define protocol correctness.

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
`ELOOP`. Writes and fsync commit to the session's in-memory namespace, without a
promise of external-source persistence. Sources provide initial file bytes only.

Message size is negotiated up to 64 KiB. Each session permits 65,536 fids and
1,024 combined pending operations and queued completions. Names are UTF-8, at
most 255 bytes, with slash and NUL excluded. Numeric fields retain their full
wire widths. Attributes advertise only the supported basic mask; birth time,
inode-generation, and data-version extension bits remain unclaimed.

Calling convention
------------------

1. Create `Filesystem::new(limits, epoch_seconds)`, then
   `NinePSession::new(&mut filesystem)`. The namespace has a private identity
   token and a reset generation; a session cannot be passed a different tree.
2. Set the supplied epoch time before each runtime activation. Call
   `submit(&mut filesystem, RequestId, bytes, reply_capacity)`. An immediate
   result contains the complete reply; a pending result identifies a started
   or joined source load. Only started loads dispatch external work.
3. Call `poll(&mut filesystem)` after load success/failure and after mutations
   that can satisfy pending I/O, including host writes and guest truncation.
   Poll every session sharing the tree. Pending operations own write bytes
   and pin their inode independently of clunk, unlink, rename, and fid reuse.
4. Drain `next_completion()` before publishing a current immediate reply.
   Earlier completions carry transport request IDs and either reply bytes or
   suppression. Queued completions continue reserving their protocol tag and
   transport ID until drained. Flush therefore retires an old descriptor
   before `Rflush`; a reply already queued precedes `Rflush` and is honored.
5. Call `reset(&mut filesystem)` when a device resets and `close(...)` before
   discarding a live session. Both release retained inodes and session locks.
   Device reset discards completions because its old queues are no longer
   usable. Filesystem reset instead causes tagged `ESTALE` completions for
   pending requests and invalidates fids when the session next polls/submits.

Resident file I/O borrows or writes only the requested range. Complete requests
are parsed before dispatch, and reply space is checked before mutation.
`Twalk` installs a fid only after full success; partial success returns visited
QIDs without changing either fid. `Tsetattr` validates selected fields and
quota before changing metadata; current timestamps resolve at commit time.
`Tlcreate` may open an existing regular file unless `O_EXCL` is set, and
`O_TRUNC` retires its lazy source without fetching it. Blocking lock requests
return blocked status for client retry rather than waiting inside Rust.

Protocol errors encode Linux errno with the parsed tag. Duplicate live tags or
transport IDs, wrong namespace identity, closed sessions, missing envelopes,
and unusably small reply buffers are typed host/transport errors. Malformed
flush messages cannot generate an `Rlerror`; valid flush messages always get
`Rflush`. No function calls JavaScript or starts a browser promise. The runtime
must dispatch source promises and notification listeners after WASM returns,
and must not keep a JavaScript memory view across an await.

Validation
----------

`cargo test ninep --lib` covers the namespace and protocol through independent
packet builders. The raw-WASM probe in `tests/fixtures/ninep_wasm.rs` executes
both implementations in Node during `make test-unit` and Chrome during
`make test`. Native tests cover malformed/truncated requests, bounded replies,
partial walks, open modes, ownership, nanoseconds, cookies, locks, shared loads,
flush ordering, fid reuse, host writes, and the separate reset lifetimes.
