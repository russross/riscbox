C reference implementation
==========================

This directory contains the complete reference C implementation of Riscbox.
It is independent of the Rust implementation and the repository's image build
pipeline.

Build and test it from this directory:

    make
    make debug
    make wasm
    make test

`run-native.sh` runs the Risclet image with the native C emulator after the
root-owned kernel and image have been built.
