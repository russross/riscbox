C historical reference archive
==============================

This directory is the historical C fork of TinyEMU preserved as a reference
archive. It is not the current Riscbox implementation, a development target,
or a compatibility requirement. Do not add new features or maintain parity
here; current Riscbox development and compatibility work belongs at the
repository root and targets Rust running as browser WebAssembly.

Build and test it from this directory:

    make
    make debug
    make wasm
    make test

`run-native.sh` runs the Risclet image with the native C emulator after the
root-owned kernel and image have been built.
