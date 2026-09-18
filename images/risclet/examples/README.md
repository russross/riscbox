Risclet examples
================

These files are the source inputs for the standalone browser demo. The image
distribution publishes this directory unchanged at `examples/`.

`examples.json` lists each example's display name, editable source file, and
complete guest workspace. The browser creates an in-memory 9p filesystem from
the selected workspace; no server or RPC service participates in execution.

Each workspace also runs independently with the installed Risclet command:

    cd sort
    make

The reduction example intentionally contains an incomplete student function.
