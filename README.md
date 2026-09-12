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

Preparing the Alpine example also requires `curl`, `gzip`, 7-Zip, and an OpenSBI generic `fw_jump.bin`:

    sudo apt install curl gzip 7zip opensbi

The WASM build requires Emscripten:

    sudo apt install emscripten


Getting started
---------------

Clone and build the native emulator:

    git clone https://github.com/russross/riscbox.git
    cd riscbox
    make -j4
    ./riscbox --help

There are three supported build targets:

| Target            | Output                                            | Use                                           |
| ----------------- | ------------------------------------------------- | --------------------------------------------- |
| `make release`    | `riscbox`, `splitimg`, `build_filelist`           | Fast native iteration; this is the default    |
| `make debug`      | `riscbox-debug`                                   | Strict warnings, debug information, AddressSanitizer, and UndefinedBehaviorSanitizer |
| `make wasm`       | `js/riscbox-wasm.js` and `js/riscbox-wasm.wasm`   | Browser deployment |

`make clean` removes all generated outputs. `make install` installs the native release programs under `/usr/local/bin` by default; set `DESTDIR` or `bindir` to stage them elsewhere.


Boot a basic Alpine system
--------------------------

The simplest supported Alpine profile uses the official RISC-V standard ISO, its kernel and compressed initramfs, and OpenSBI `fw_jump.bin`. riscbox loads raw boot payloads, so the gzip-compressed kernel from the ISO must be decompressed; the initramfs stays compressed for Linux to unpack.

The checked-in example is pinned to Alpine 3.24.1, which is exercised by the project's native, debug, and browser validation. From the repository root:

    RISCBOX_ASSETS=image/alpine
    ALPINE_ISO=alpine-standard-3.24.1-riscv64.iso
    ALPINE_URL=https://dl-cdn.alpinelinux.org/alpine/v3.24/releases/riscv64

    mkdir -p "$RISCBOX_ASSETS"
    curl --fail --location --output "$RISCBOX_ASSETS/$ALPINE_ISO" "$ALPINE_URL/$ALPINE_ISO"
    curl --fail --location --output "$RISCBOX_ASSETS/$ALPINE_ISO.sha256" "$ALPINE_URL/$ALPINE_ISO.sha256"
    (cd "$RISCBOX_ASSETS" && sha256sum --check "$ALPINE_ISO.sha256")

    7z e -y "-o$RISCBOX_ASSETS" "$RISCBOX_ASSETS/$ALPINE_ISO" boot/vmlinuz-lts boot/initramfs-lts
    gzip --decompress --stdout "$RISCBOX_ASSETS/vmlinuz-lts" > "$RISCBOX_ASSETS/linux"
    install -m 644 /usr/lib/riscv64-linux-gnu/opensbi/generic/fw_jump.bin "$RISCBOX_ASSETS/fw_jump.bin"

Boot it using the UART for I/O:

    ./riscbox examples/alpine.cfg

Log in as `root`; the standard live image does not initially require a password. Native block devices use snapshot writes by default, so guest changes disappear when riscbox exits. Pass `-rw` only when the attached image is meant to be modified. Press `Ctrl-A X` to stop the emulator or `Ctrl-A H` for console help.


Embed riscbox in a web page
---------------------------

First build the WebAssembly runtime and convert the Alpine ISO into the chunked HTTP block format:

    make wasm
    mkdir -p image/alpine/drive
    ./splitimg image/alpine/alpine-standard-3.24.1-riscv64.iso image/alpine/drive

Serve the repository over HTTP; browsers cannot load the VM reliably from `file:` URLs:

    python3 -m http.server 8000

Open <http://127.0.0.1:8000/examples/web/?config=alpine.cfg>. The example page is a minimal, dependency-free UART terminal. It demonstrates the complete embedding contract and boots the same Alpine profile entirely in WebAssembly. Production applications can replace its `term` object with a full terminal component while keeping the riscbox API calls.

The [Risclet example](examples/risclet/README.md) adds a browser-backed 9p
filesystem and a live `sort.s` editor.

For another site, copy `riscbox-wasm.js`, `riscbox-wasm.wasm`, the VM configuration, firmware, kernel, initramfs, and split block directory into its static assets. Load the JavaScript after defining these globals:

*   `Module.onRuntimeInitialized()` calls `Module.ccall("vm_start", ...)`.
*   `Module.onVmStarted()` runs after the VM and its devices are ready.
*   `term.write(text)` accepts console output.
*   `term.getSize()` returns `[columns, rows]`.
*   `update_downloading(active)` reports HTTP activity.
*   `graphic_display` and `net_state` may be `null` for a console-only VM.

Console input is queued one byte at a time through `Module._console_queue_char(byte)`. All VM URLs may be relative to the configuration file. Serve `.wasm` files as `application/wasm`; ordinary static servers generally do this already. Cross-origin assets also need the usual CORS headers.

Browser code can replace a file in the first 9p filesystem with
`Module.ccall("fs_import_text", "number", ["string", "string", "string"],
[directory, filename, text])`. Call it from `onVmStarted()` or later; zero
indicates success.

For a `js9p` filesystem, set `Module.p9Server` before starting the VM. The
included server accepts a nested object whose string and `Uint8Array` leaves
are files:

```js
import { Memory9PServer } from "./js/p9.js";

Module.p9Server = new Memory9PServer({
    "Makefile": "all:\n\tcc -o hello hello.c\n",
    "hello.c": "int main(void) { return 0; }\n",
    tests: { "input.txt": new Uint8Array([1, 2, 3]) },
});
```

`readFile`, `writeFile`, `remove`, `rename`, `snapshot`, and `subscribe` let an
application interact with the live tree. Each file is limited to 16 MiB. A
different synchronous object implementing `request(request, replyCapacity)`
can be installed as `Module.p9Server` instead.


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

The machine uses QEMU `virt` addresses and device-tree bindings for RAM at `0x80000000`, the reset vector at `0x1000`, UART0 at `0x10000000`, VirtIO MMIO transports from `0x10001000`, CLINT at `0x02000000`, PLIC at `0x0c000000`, and the SiFive test finisher at `0x00100000`. The generated FDT is passed in `a1` and describes only configured devices.

Available devices are:

*   NS16550A-compatible UART and VirtIO console.
*   VirtIO MMIO block, network, 9p, keyboard, and tablet devices.
*   PLIC and legacy CLINT interrupt/timer controllers.
*   SiFive-compatible poweroff/test device.
*   A simple framebuffer backed by SDL natively or an HTML canvas in a browser.
*   Native raw disks and 9p directories, plus chunked HTTP disks and remote 9p.

Compared with QEMU `virt`, riscbox omits multiple harts, RV32, configurable CPU models, PCIe, flash, fw_cfg, RTC, ACPI, UEFI, AIA/IMSIC/APLIC, IOMMU, NUMA, and the broad device catalogue. Those omissions are intentional unless a small, standard implementation becomes necessary for the target guests.

### Boot and image profiles

The following paths are supported:

| Profile           | Firmware slot                     | Kernel slot                                                                     | Storage |
| ----------------- | --------------------------------- | ------------------------------------------------------------------------------- | ------- |
| Linux direct boot | Raw OpenSBI `fw_jump.bin`         | Raw, uncompressed Linux `Image`; optional initramfs is passed through unchanged | Raw native disk or split HTTP disk |
| xv6 or bare metal | Flat M-mode image at `0x80000000` | Omit | Optional VirtIO block disk |
| S-mode bootloader | Raw OpenSBI `fw_jump.bin`         | Flat bootloader at `0x80200000` | Bootloader-supported VirtIO media |

riscbox does not parse ELF, PE/COFF, FIT, qcow2, or compressed kernel images and does not provide built-in OpenSBI or U-Boot. Keeping boot assets explicit makes browser deployment predictable. Linux may consume a compressed initramfs, as in the Alpine example, because riscbox loads that file opaquely and describes it in the FDT.


License and credits
-------------------

riscbox is distributed under the same MIT license as TinyEMU; see [LICENSE](LICENSE). Copyright notices for Fabrice Bellard and other upstream authors are preserved. The bundled SLIRP-derived files retain their own BSD-style notices in the source.

TinyEMU was created by [Fabrice Bellard](https://bellard.org/) and is available from the [original TinyEMU site](https://bellard.org/tinyemu/). riscbox's changes and release history are recorded in [CHANGELOG.md](CHANGELOG.md).
