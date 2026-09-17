Risclet browser demo
====================

The demo boots a small Alpine image and mounts a JavaScript in-memory 9p
filesystem on `/home/student`. The editor and guest share its `sort.s` file:
editor input is immediately visible to the guest, and guest writes update the
editor.

Build the root image and browser assets from the repository root:

    make -j4
    make wasm -j4
    node --test js/p9.test.mjs
    ./examples/risclet/build-rootfs.sh
    ./examples/risclet/build-web-assets.sh

`build-rootfs.sh` expects the pinned Alpine minirootfs and standard ISO under
`image/`. It creates a 64 MiB image with Python 3, Make, the America/Denver time
zone, and Risclet 0.4.8, but does not copy demo source into the disk.
`build-web-assets.sh` expects the configured kernel image at
`image/build/kernel/arch/riscv/boot/Image`; its paths can be overridden through
the environment variables defined at the top of each script. Demo source files
are loaded directly into the JavaScript 9p server rather than converted to the
legacy browser filesystem format.

To run the native demo with the checked-in source directory exported read/write
by diod:

    ./examples/risclet/run-native.sh

The launcher starts a temporary single-user diod instance and cleans it up when
Riscbox exits. Set `RISCBOX=./riscbox-debug` to exercise the debug build.
The guest defaults to `cache=mmap`; append `risclet.cache=none` to the kernel
command line for workloads that do not need executable mappings.

Serve the repository over HTTP:

    python3 -m http.server 8000

Open <http://127.0.0.1:8000/examples/risclet/>. The guest logs in as `student`
automatically. Run:

    risclet start.s sort.s print.s

Routine kernel messages are hidden from the VirtIO console but remain available
from the running guest with `dmesg`. The minimal image does not persist them, so
they are lost when the VM restarts.
