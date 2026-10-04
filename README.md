Riscbox
=======

Riscbox is an emulator that runs a RISC-V 64 VM in a browser. The runtime is WebAssembly (WASM) with a small JavaScript adapter.

This is a fork of Fabrice Bellard's TinyEMU, but there are so many forks out there that a name change seemed like a good idea. The inner emulation loop is from TinyEMU with some bug fixes and added instructions, while the surrounding platform is written in Rust and adds more device support and modernization.

I recommend starting here with a [live demo](https://russross.github.io/riscbox/)

It runs a small Alpine Linux instance and exposes some of the main VM lifecycle controls. The demo's [source and build instructions](https://github.com/russross/riscbox/tree/main/demo) are part of the repo and show how to embed a VM in your page.

The docs for using riscbox are split into two parts:

* [HOWTO.md](HOWTO.md) that walks through common setup and workflow scenarios
* [API.md](API.md) defines the current JavaScript calls, options, types, and enforced VM-state contracts

Most users will just a [packaged release from github](https://github.com/russross/riscbox/releases) that includes riscbox assets, plus a Linux kernel image, OpenSBI firmware, and a U-boot bootloader, all customized and pre-built.

See [BUILDING.md](https://github.com/russross/riscbox/blob/main/BUILDING.md) if you want to build from source, run tests, etc.


The platform
------------

Riscbox emulates:

* A single little-endian RISC-V 64 core with MMU and privilege modes for full OS support
* A useful set of Hardware that (mostly based on QEMU's virt platform):
    * VirtIO console and a 16550A UART
    * A block device with a couple main modes:
        * Stream blocks from a read-only web server on demand, local copy-on-write for changes as it runs
        * Host app supplies an entire memory-based read-write image
    * A file server that is entirely in host app memory:
        * The guest interacts with it using Linux's standard 9p2000.L driver over VirtIO
        * The host app can also read and write through a simplified, synchronous API, and it can watch files and be notified of changes
    * An ethernet device: packets are streamed to the server, which relays them to the internet (requires a server-side adapter that is not yet provided)
    * A simple framebuffer device (no JavaScript adapter for this yet, so this is a work in progress)
    * A Goldfish realtime clock (RTC)
    * PLIC, ACLINT MSWI/MTIMER (interrupt and timer support)
    * SiFive test finisher (for initiating shutdowns and reboots)
    * An entropy source for randomness

The block device is set up as a simple way to distribute stable base images that are generic for a wide set of users. The 9p file system is great for sharing state with the host and customizing a guest image on the fly for each user.

The block device and 9p file system can be reset while the guess is halted, or they can persist across guest reboots.

Riscbox does NOT implement:

* Multiple cores
* PMP (mainly used by embedded systems where virtual memory is overkill)
* Vector instructions


License
-------

Riscbox retains the MIT license and copyright notices from [TinyEMU](https://bellard.org/tinyemu/)
