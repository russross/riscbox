riscbox emulator
================

riscbox is a focused fork of Fabrice Bellard's [TinyEMU](https://bellard.org/tinyemu/). It keeps TinyEMU's small, direct implementation while updating its RISC-V CPU and virtual platform for use in education and specifically in the browser with WASM.

riscbox is intended to run small RISC-V Linux systems and student kernels in a web page, with native builds for image preparation and testing. Its platform generally follows QEMU's RISC-V `virt` machine, and its ISA is moving in the general direction of RVA23.

The scope is deliberately narrow:

*   RV64 only, little-endian, with Sv39 virtual memory only.
*   One hart. There is no SMP model.
*   No vector or hypervisor extension.
*   A small `virt`-style device set, not general device emulation.
*   Browser deployment first; native builds support development and image work.
*   Networking is retained but deprioritized for now.


Dependencies
------------

The native build requires Clang, GNU Make, libcurl, OpenSSL, and SDL 1.2 development files. On Debian or Ubuntu:

    sudo apt install clang make libcurl4-openssl-dev libssl-dev libsdl1.2-dev

Image builds also require the RISC-V cross compiler, QEMU system emulator,
`curl`, ext4 tools, and an OpenSBI generic `fw_jump.bin`:

    sudo apt install curl e2fsprogs gcc-riscv64-linux-gnu opensbi qemu-system-misc

The reference C WASM build requires Emscripten. The deployed Rust WASM build
requires Rust and the `wasm32-unknown-unknown` target:

    sudo apt install emscripten
    rustup target add wasm32-unknown-unknown


Getting started
---------------

Clone and build the native emulator:

    git clone https://github.com/russross/riscbox.git
    cd riscbox
    make -j4
    ./riscbox --help

There are four supported build targets:

| Target            | Output                                            | Use                                           |
| ----------------- | ------------------------------------------------- | --------------------------------------------- |
| `make release`    | `riscbox`, `splitimg`, `build_filelist`           | Fast native iteration; this is the default    |
| `make debug`      | `riscbox-debug`                                   | Strict warnings, debug information, AddressSanitizer, and UndefinedBehaviorSanitizer |
| `make wasm`       | `js/riscbox-wasm.js` and `js/riscbox-wasm.wasm`   | Reference C browser build |
| `make rust-wasm`  | `target/wasm32-unknown-unknown/release/riscbox_wasm.wasm` | Browser deployment |

`make clean` removes all generated outputs. `make install` installs the native release programs under `/usr/local/bin` by default; set `DESTDIR` or `bindir` to stage them elsewhere.


Build and deploy an image
-------------------------

The tracked image definitions live under `images/`; downloads, the single
Linux source tree, intermediate files, disk images, and deployment bundles are
ignored. Build the general Alpine image or the Risclet teaching image from its
own directory:

    cd images/alpine
    ./build.sh

    cd images/risclet
    ./build.sh

Each build downloads verified inputs as needed, incrementally builds the
tracked kernel configuration and WASM runtime, creates an ext4 disk, runs the
image-specific setup under QEMU with networking, and writes a self-contained
`dist/` directory. See [images/README.md](images/README.md) for the build layout.

Serve a completed distribution over HTTP:

    cd images/risclet/dist
    python3 -m http.server 8000

Open <http://127.0.0.1:8000/>. The distribution includes its configuration,
firmware, kernel, unsplit image, chunked browser image, runtime, integration
page, and an all-in-one deployment README. It can be copied directly to a
static server with `rsync`.


Configuration and command line
------------------------------

riscbox reads TinyEMU's small JSON extension: comments, unquoted property names, and trailing commas are accepted. A minimal Linux configuration is:

```js
{
    version: 1,
    machine: "riscv64",
    memory_size: 512,
    bios: "fw_jump.bin",
    kernel: "linux",
    initrd: "initramfs-lts",
    cmdline: "console=ttyS0,115200",
    drive0: { file: "disk.img" },
    console: "uart",
}
```

Paths for boot files, disks, and network filesystems are relative to the configuration file. Supported top-level resources are numbered consecutively: `drive0`, `drive1`, `fs0`, `fs1`, `eth0`, and so on. The most useful options are:

*   `console: "uart"` connects host input and output to the 16550A UART and does not create a VirtIO console. Use this for xv6 and conventional serial Linux.
*   `console: "virtio"` selects the VirtIO console. This is the default.
*   `uart_output: true` mirrors early UART output while input remains attached to the selected VirtIO console.
*   `driveN: { file: "...", device: "..." }` adds a VirtIO block device.
*   `fsN: { file: "...", tag: "..." }` adds a VirtIO 9p filesystem backed by
    a native directory or the browser HTTP filesystem.
*   `fsN: { socket: "...", tag: "..." }` connects the VirtIO device directly
    to a 9P server over a native Unix-domain socket.
*   `fsN: { js9p: true, tag: "..." }` connects the VirtIO device to the
    synchronous `Module.p9Server` endpoint in a browser build.
*   `ethN: { driver: "user" }` adds user-mode networking. Native builds also support `driver: "tap"` with an `ifname`; see `netinit.sh`.
*   `display0: { device: "simplefb", width: 1024, height: 768 }` adds the simple framebuffer. `input_device: "virtio"` adds keyboard and tablet input.

Command-line options:

    usage: riscbox [options] config_file
      -m MB             override RAM size
      -rw               write directly to native disk images
      -ro               reject disk writes
      -ctrlc            let Ctrl-C stop riscbox instead of reaching the guest
      -append TEXT       append to the kernel command line

The default native disk mode is a private in-memory snapshot. The browser HTTP block device also keeps writes in memory. Browser builds currently accept at most one block device and one 9p filesystem.


Technical status
----------------

The compatibility references are the [RVA23 profiles](https://docs.riscv.org/reference/rva23/rva23-profiles.html) and QEMU's [`virt` machine](https://www.qemu.org/docs/master/system/riscv/virt.html).

**CPU and privileged architecture.** riscbox implements RV64 I, M, A, F, D, C, and the B subsets used by the advertised `Zba`, `Zbb`, and `Zbs` extensions. Floating-point operations use TinyEMU's bit-exact SoftFP implementation. The machine reports privileged architecture 1.12, implements M/S/U modes, Sv39, sixteen PMP entries, supervisor timer compare, hardware or faulting A/D updates, fine-grained TLB invalidation, Svnapot, and Svpbmt.

The advertised smaller extensions and profile properties currently include `Zicbom`, `Zicbop`, `Zicboz`, `Zicond`, `Zihintntl`, `Zihintpause`, `Zimop`, `Zawrs`, `Zca`, `Zcb`, `Zcmop`, the counter extensions, and the validated main-memory and supervisor guarantees recorded in the generated device tree. Cache blocks and reservation sets are 64 bytes.

riscbox is not RVA23 compliant. In particular, RVA23 requires vectors and associated vector extensions, which are intentionally out of scope. Other important gaps include `Zfhmin`, `Zfa`, `Zkt`, pointer masking, and the complete privileged-architecture 1.13 profile. The implemented subset is advertised precisely so guest software does not infer unavailable instructions.

### Virtual platform

The machine uses QEMU `virt` addresses and device-tree bindings for RAM at `0x80000000`, the reset vector at `0x1000`, the Goldfish RTC at `0x00101000`, UART0 at `0x10000000`, VirtIO MMIO transports from `0x10001000`, CLINT at `0x02000000`, PLIC at `0x0c000000`, and the SiFive test finisher at `0x00100000`. The generated FDT is passed in `a1` and describes only configured devices.

Available devices are:

*   NS16550A-compatible UART and VirtIO console.
*   Goldfish real-time clock initialized from the host wall clock.
*   VirtIO MMIO block, network, 9p, keyboard, and tablet devices.
*   PLIC and legacy CLINT interrupt/timer controllers.
*   SiFive-compatible poweroff/test device.
*   A simple framebuffer backed by SDL natively or an HTML canvas in a browser.
*   Native raw disks and 9p directories, plus chunked HTTP disks and remote 9p.

Compared with QEMU `virt`, riscbox omits multiple harts, RV32, configurable CPU models, PCIe, flash, fw_cfg, ACPI, UEFI, AIA/IMSIC/APLIC, IOMMU, NUMA, and the broad device catalogue. Those omissions are intentional unless a small, standard implementation becomes necessary for the target guests.

### Boot and image profiles

The following paths are supported:

| Profile           | Firmware slot                     | Kernel slot                                                                     | Storage |
| ----------------- | --------------------------------- | ------------------------------------------------------------------------------- | ------- |
| Linux direct boot | Raw OpenSBI `fw_jump.bin`         | Raw, uncompressed Linux `Image`; optional initramfs is passed through unchanged | Raw native disk or split HTTP disk |
| xv6 or bare metal | Flat M-mode image at `0x80000000` | Omit | Optional VirtIO block disk |
| S-mode bootloader | Raw OpenSBI `fw_jump.bin`         | Flat bootloader at `0x80200000` | Bootloader-supported VirtIO media |

riscbox does not parse ELF, PE/COFF, FIT, qcow2, or compressed kernel images and does not provide built-in OpenSBI or U-Boot. Keeping boot assets explicit makes browser deployment predictable. Linux may consume a compressed initramfs because riscbox loads that file opaquely and describes it in the FDT.


License and credits
-------------------

riscbox is distributed under the same MIT license as TinyEMU; see [LICENSE](LICENSE). Copyright notices for Fabrice Bellard and other upstream authors are preserved. The bundled SLIRP-derived files retain their own BSD-style notices in the source.

TinyEMU was created by [Fabrice Bellard](https://bellard.org/) and is available from the [original TinyEMU site](https://bellard.org/tinyemu/). riscbox's changes and release history are recorded in [CHANGELOG.md](CHANGELOG.md).
