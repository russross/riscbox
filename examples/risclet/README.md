Risclet browser demo
====================

The demo boots a small Alpine image, mounts the browser-backed 9p filesystem
on `/home/student`, and snapshots the `sort.s` textarea into that filesystem
after every edit.

Build the root image and browser assets from the repository root:

    make -j4
    make wasm -j4
    ./examples/risclet/build-rootfs.sh
    ./examples/risclet/build-web-assets.sh

`build-rootfs.sh` expects the pinned Alpine minirootfs and standard ISO under
`image/`. It installs Risclet 0.4.7 but does not copy demo source into the disk.
`build-web-assets.sh` expects the configured kernel image at
`image/build/kernel/arch/riscv/boot/Image`; its paths can be overridden through
the environment variables defined at the top of each script.

To run the native demo with the checked-in source directory exported read/write
by diod:

    ./examples/risclet/run-native.sh

The launcher starts a temporary single-user diod instance and cleans it up when
Riscbox exits. Set `RISCBOX=./riscbox-debug` to exercise the debug build.

Serve the repository over HTTP:

    python3 -m http.server 8000

Open <http://127.0.0.1:8000/examples/risclet/>, log in as `student`, and run:

    risclet start.s sort.s print.s
