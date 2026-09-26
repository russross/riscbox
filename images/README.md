Image builds
============

Each subdirectory is a complete image definition. Its tracked `build.sh`,
`setup.sh`, `riscbox.cfg`, and optional `web/` directory are inputs. Running
`build.sh` creates two ignored outputs:

*   `build/rootfs.ext4` is the writable image used for native testing.
*   `dist/` is the self-contained browser deployment. It retains hashed boot
    and chunked-disk generations until the operator cleans them.

Build either current image from its directory:

    cd images/risclet
    ./build.sh

    cd images/alpine
    ./build.sh

The `xv6-profile` definition is an offline boot workload for performance
measurement. It builds the current xv6 kernel, excluding `fs.img`, as the
unprivileged `test` user and powers off. Build it once, then collect matching
Node/V8 profiles for the Rust and
historical C WebAssembly runtimes:

    images/xv6-profile/build.sh
    tools/profile-xv6

The profiler writes `.cpuprofile` files and guest console logs under
`images/xv6-profile/build/profiles/` by default. Pass another directory as
the profiler's sole argument when profiles should be retained elsewhere.
Set `RISCBOX_PROFILE_TIMING=1` to include periodic emulator timing diagnostics
in the Riscbox profile log.

The shared helpers under `bin/` download and verify the pinned Alpine
minirootfs, consume the root-owned `kernel/linux` and Rust WASM build, boot
setup scripts under QEMU with user networking, and assemble the deployment.
Run `make kernel` or `make wasm` at the repository root to build those core
artifacts independently.

The browser Risclet application loads its workspaces from the tracked
`risclet/examples/` directory. `examples.json` names each example and lists
the files copied into its in-memory 9p tree. The distribution builder copies
the complete directory, including dotfiles and documentation, to
`dist/examples/`. The application is entirely static and needs only an HTTP
server; it does not use an RPC service. Its TypeScript source and locked npm
dependencies live under `risclet/ui/`. The distribution build installs them
when needed, runs the type checker, and bundles the CodeMirror, xterm.js,
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

Shared image-preparation artifacts are kept under `images/build/`; per-image
work and output are kept under that image's `build/` and `dist/`. None is
tracked. Kernel sources and build products live under `kernel/`, outside the
image definitions. The Alpine release is selected once near the top of
`bin/create-alpine-ext4`.

New image definition
--------------------

Create a directory with these inputs:

*   `build.sh` calls the shared helpers in order.
*   `setup.sh` runs as root inside QEMU with networking enabled. Files passed
    after it to `run-image-setup` appear in `/mnt/setup` by basename.
*   `riscbox.cfg` uses paths relative to the eventual deployment directory.
*   `web/` optionally replaces or adds files in the deployment. Without it,
    the shared terminal page is used.

Keep downloads and generated files under `build/`. A normal build should leave
only intentional image inputs visible to Git.

The distribution builder compresses the kernel with `gzip -9` and writes
content-addressed boot and disk assets. The kernel name uses the uncompressed
kernel's hash with a `.gz` suffix. It replaces `dist/riscbox.cfg` last. Remove
superseded generations when appropriate
with `../../tools/image_deployment.py clean ./dist` from an image directory.
