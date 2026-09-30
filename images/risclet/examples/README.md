Risclet examples
================

These files are the source inputs for the standalone browser demo. The image
distribution publishes these files at `examples/` and adds file sizes to the
deployed manifest.

`examples.json` lists each example's display name, editable source file, and
complete guest workspace. Source manifests list paths; deployed manifests use
`{ path, size }` records. The browser creates a Rust namespace for each workspace
before boot. HTTP bodies load only on host or guest reads, including editor and
instruction reads; there is no preload mechanism or remote RPC service.

Each workspace also runs independently with the installed Risclet command:

    cd sort
    make

The reduction example intentionally contains an incomplete student function.
