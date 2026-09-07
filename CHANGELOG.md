Riscbox changelog
=================

2026-09-07
----------

This release establishes Riscbox as a focused RV64 browser emulator based on
TinyEMU 2019-12-21.

*   Renamed the project, native binaries, WebAssembly artifacts, documentation,
    and user-visible interfaces from TinyEMU/temu to Riscbox.
*   Reduced the CPU to RV64, the MMU to Sv39, and the platform to one hart;
    removed x86, RV32, RV128, Sv32, Sv48, and Windows paths, and made vectors
    and the hypervisor extension explicit non-goals.
*   Moved the machine to the QEMU `virt` memory map and boot protocol, including
    the low reset vector, DRAM/FDT layout, OpenSBI handoff, and standard FDT
    bindings.
*   Added an NS16550A UART, standard PLIC and CLINT layouts, SiFive test
    finisher, QEMU VirtIO vendor identification, and VirtIO block device ID
    requests.
*   Made UART and VirtIO console selection explicit. UART-only guests such as
    xv6 receive input correctly; Linux can use VirtIO console while optionally
    mirroring early UART output.
*   Implemented PMP, supervisor timer compare, Svadu controls, Svinval, Svnapot,
    Svpbmt, and current counter/privilege behavior needed by OpenSBI, Linux, and
    xv6.
*   Added and advertised focused RVA23-era extensions and guarantees, including
    `Zicond`, `Zimop`, `Zawrs`, `Zba`, `Zbb`, `Zbs`, `Zca`, `Zcb`, `Zcmop`,
    cache-block operations, instruction hints, and standard main-memory
    properties.
*   Corrected LR/SC and AMO alignment, reservation, permission, and operand-size
    behavior; Sv39 superpage/PTE validation; MPRV trap return; independent cycle
    and instruction-retirement accounting; high multiplication; FMIN/FMAX; HTTP
    URL bounds; configuration ownership; and writable FDT placement.
*   Replaced the native AES decryption use of obsolete OpenSSL APIs with EVP.
*   Standardized optimized native and sanitizer/debug builds on Clang and the
    browser build on Emscripten. Modernized the Emscripten HTTP bridge and made
    every web asset path relative to its VM configuration.
*   Validated modern xv6 through its complete user test suite and Alpine 3.24.1
    through OpenSBI to a UART login prompt in native, sanitizer, and browser
    builds.

TinyEMU history
===============

2019-12-21
----------

*   Added complete JSLinux demo.
*   RISC-V: added initrd support.
*   RISC-V: fixed FMIN/FMAX instructions.

2018-09-23
----------

*   Added support for separate RISC-V BIOS and kernel.

2018-09-15
----------

*   Renamed to TinyEMU (`temu`).
*   Added a single executable for all emulated machines.

2018-08-29
----------

*   Compilation fixes.

2017-08-06
----------

*   Added a JSON configuration file.
*   Added graphical display with SDL.
*   Added VirtIO input support.
*   Added PCI bus and VirtIO PCI support.
*   Added user-mode networking.

2017-06-10
----------

*   RISC-V: avoided unnecessary kernel patches.

2017-05-25
----------

*   RISC-V: improved emulation performance by 1.4x.
*   Supported user ISA 2.2 and privileged architecture 1.10.
*   Matched the `fs_net` network protocol to the vfsync protocol.
*   Handled console resize.
*   JavaScript emulator: added terminal scrolling, file import/export, and
    copy/paste support.
