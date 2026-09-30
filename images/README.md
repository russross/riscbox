Image builds
============

Each subdirectory is a complete image definition. Its tracked `Makefile`,
`setup.sh`, `riscbox.cfg`, and optional `web/` directory are inputs. Running
`make` creates ignored outputs:

*   `build/rootfs.ext4` is the writable setup image used under QEMU.
    Risclet and xv6 profile also create `build/rootfs.erofs` for distribution.
*   `dist/` is the self-contained browser deployment. It retains hashed boot
    and chunked-disk generations until the operator cleans them.

Build either current image from its directory:

    make -C images/risclet

    make -C images/alpine

The `xv6-profile` definition is an offline boot workload for performance
measurement. It builds the current xv6 kernel, excluding `fs.img`, as the
unprivileged `test` user and powers off. Build it once, then collect matching
Node/V8 profiles for the Rust and
historical C WebAssembly runtimes:

    make -C images/xv6-profile
    make -C images/xv6-profile profile

The profiler writes `.cpuprofile` files and guest console logs under
`images/xv6-profile/profiles/`. Use `make -C images/xv6-profile profile-tinyemu`
for the historical C runtime.
Set `RISCBOX_PROFILE_TIMING=1` to include periodic emulator timing diagnostics
in the Riscbox profile log.

Risclet and xv6 profile finish setup on ext4, then mount it read-only and use
`mkfs.erofs` to create their distribution disks. Install `erofs-utils` and
ensure passwordless `sudo` can mount the ext4 image and run `mkfs.erofs` for
these builds. Their guest roots mount read-only; tmpfs supplies `/tmp` and the
upper layers for `/var` and `/home`. The Alpine definition still distributes
ext4. Risclet loads the custom Linux Image through OpenSBI and mounts its
single EROFS disk as `/dev/vda`. Split HTTP disks use
512 KiB chunks by default.

The shared helpers under `bin/` download and verify the pinned Alpine
minirootfs, consume the root-owned `kernel/linux`, `opensbi/fw_dynamic.bin`,
and Rust WASM build, boot
setup scripts under QEMU with user networking, and assemble the deployment.
Run `make kernel`, `make opensbi`, `make uboot`, or `make wasm` at the
repository root to build those components independently. The firmware and
bootloader source archives, extracted trees, and objects are ignored; their
Makefiles rebuild only when the tracked version or config changes.
The prepared Alpine base image configures BusyBox `acpid` to handle the host's
power and restart input events with orderly `/sbin/poweroff` and `/sbin/reboot`.
Risclet starts the same daemon after mounting its shared filesystem.

The browser Risclet application loads its workspaces from the tracked
`risclet/examples/` directory. `examples.json` names each example and lists
the source paths in each workspace. The distribution builder publishes exact
file sizes in `dist/examples/examples.json` and copies the complete directory,
including dotfiles and documentation, to `dist/examples/`. The browser creates
Rust namespace handles before VM boot and fetches bodies only when the host or
guest reads them. There is no preload mechanism. One runtime retains all example
handles across VM destruction, switching, reboot, and shutdown; asynchronous
UI results are guarded against example switches. The application needs only an HTTP
server; it does not use an RPC service. Its TypeScript source and locked npm
dependencies live under `risclet/ui/`. The distribution build installs them
when needed, runs the type checker, and bundles the CodeMirror, Ghostty,
Split.js, and CommonMark frontend into `dist/bundle.js`.

Browser timing tests
--------------------

After building `images/risclet` and `images/xv6-profile`, serve the repository's
`images/` directory:

    python3 -m http.server 8000 --bind 127.0.0.1 --directory images

Open `http://127.0.0.1:8000/risclet/dist/` and click **Boot VM**. Open
`http://127.0.0.1:8000/xv6-profile/dist/` to run the xv6 compile benchmark;
it starts automatically and shuts down after reporting
`XV6_PROFILE_BUILD_COMPLETE`. Both pages enable timing diagnostics. Read the
periodic `Riscbox timing` entries in the browser developer console.
The skew in each timing report is selected automatically from recent runnable
quanta; the session percentiles remain available for comparison.

Shared image-preparation artifacts are kept under `images/build/`; per-image
work and output are kept under that image's `build/` and `dist/`. None is
tracked. Kernel sources and build products live under `kernel/`, outside the
image definitions. The Alpine release is selected once near the top of
`bin/create-alpine-ext4`.

New image definition
--------------------

Create a directory with these inputs:

*   `Makefile` tracks the shared helpers and inputs in build order.
*   `setup.sh` runs as root inside QEMU with networking enabled. Files passed
    after it to `run-image-setup` appear in `/mnt/setup` by basename.
*   `riscbox.cfg` uses paths relative to the eventual deployment directory.
*   `web/` optionally replaces or adds files in the deployment. Without it,
    the shared terminal page is used.

Keep downloads and generated files under `build/`. A normal build should leave
only intentional image inputs visible to Git.

The distribution builder accepts `--erofs` for completed ext4 setup images,
compresses the next-stage payload with `gzip -9`, and writes
content-addressed boot and disk assets. The payload name uses the uncompressed
payload's hash with a `.gz` suffix. Set `BOOT_PAYLOAD` and
`BOOT_PAYLOAD_NAME` to package a next stage other than the default Linux
kernel. Risclet uses that default. The builder replaces `dist/riscbox.cfg`
last. Remove superseded generations when appropriate
with `../../tools/image_deployment.py clean ./dist` from an image directory.
