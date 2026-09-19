Image builds
============

Each subdirectory is a complete image definition. Its tracked `build.sh`,
`setup.sh`, `riscbox.cfg`, and optional `web/` directory are inputs. Running
`build.sh` creates two ignored outputs:

*   `build/rootfs.ext4` is the writable image used for native testing.
*   `dist/` is the self-contained browser deployment, including another copy
    of `rootfs.ext4` and the chunked HTTP block image.

Build either current image from its directory:

    cd images/risclet
    ./build.sh

    cd images/alpine
    ./build.sh

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
