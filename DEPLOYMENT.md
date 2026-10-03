Static deployment
=================

Extract `riscbox-VERSION.tar.gz` into an application-owned directory. Its tree
contains the runtime, declarations, network module, hash-named Linux/OpenSBI/
U-Boot payloads, this guide, the API README, storage and protocol guides, and the
standalone `splitimg.py` executable. The archive contains no root filesystem or
application UI. The [embedding demo](https://github.com/russross/riscbox/tree/main/demo)
builds an application around an unchanged copy of this tree.

Preparing a disk
----------------

The splitter requires `uv`, which selects Python 3.13 or later automatically.
Give it a raw disk image and an existing output directory:

    ./riscbox/splitimg.py rootfs.erofs ./dist

It prints the content-named directory and block count. Blocks are 512 KiB by
default; an optional third argument selects a power-of-two size in KiB. The last
block is zero-padded. Point `drive0.file` at `drive-HASH/blk.txt` relative to the
VM configuration URL. The browser uses ordinary GET requests, without ranges.

Boot assets and hosting
-----------------------

Use the archive's `fw_dynamic.bin-HASH.gz` as `bios` and `linux-HASH.gz` as
`kernel`. An EROFS root uses `root=/dev/vda ro rootfstype=erofs` in the command
line. Ext4 uses `root=/dev/vda rw rootfstype=ext4`. See [README.md](README.md)
for configuration, array disks, initrds, and U-Boot/ISO booting.

Serve the application over HTTP or HTTPS with `.wasm` mapped to
`application/wasm`. Cross-origin assets need CORS. A local check can use:

    python3 -m http.server --directory dist 8000

Open <http://localhost:8000/>. The demo's CDN editor and terminal require an
Internet connection; Riscbox itself has no JavaScript runtime dependencies.

Updating a deployment
---------------------

Upload immutable hash-named assets first and replace `riscbox.cfg` last. Keep old
assets while older pages can still request them. The adapter fetches hash-named
assets with `force-cache` and configuration with `no-store`. Serve the matching
JavaScript, declarations, and WASM from one release together.

HTTP disk writes and resident 9p data live in the VM's memory and disappear on
page reload or destroy. Reboot retains them. Cold reset retains disk stores;
call the HTTP disk's `discardChanges()` while powered off to remove its overlay.
Applications own persistence through the copied host APIs. Export writable disks
after orderly guest shutdown; a forced halt does not flush guest filesystems.
