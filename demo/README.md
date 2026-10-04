Release-based embedding demo
============================

The [published demo](https://russross.github.io/riscbox/) updates after successful
releases. The Release workflow builds this directory against the release archive
and runs Chrome acceptance before publishing. Manual test runs provide a
downloadable `riscbox-demo.tar.gz` without replacing the public site; see
[release automation](../BUILDING.md#release-automation).

    make
    python3 -m http.server --directory dist 8000

Open <http://localhost:8000/>. From the repository root, `make demo` performs the
same build. Install QEMU's RISC-V system emulator, fakeroot, cpio, erofs-utils,
`curl`, Node.js, and `uv`, plus the core build prerequisites in
[BUILDING.md](../BUILDING.md). No sudo or host filesystem mounts are needed.

The default build runs `make -C ..`, extracts the resulting release archive
unchanged under `build/release/riscbox-VERSION/`, and copies that complete tree
to `dist/riscbox/`. It adds the app, source examples, configuration, and split
disk beside it. To build against a downloaded archive without the parent repo:

    make RELEASE_ARCHIVE=/absolute/path/riscbox-VERSION.tar.gz

The image builder consumes only the release's compressed Linux and OpenSBI
payloads. Assembly invokes its packaged `splitimg.py`. No application code or
image helper reads the repository's runtime source or internal ABI.

Guest image
-----------

QEMU boots a verified Alpine 3.24.2 riscv64 minirootfs as an initramfs, installs
TinyCC (including its runtime archive), musl headers, make, doas, nano, and less,
and compiles/runs all three source examples. It exports a root-owned tar over
9p. `mkfs.erofs --tar=f` converts it to the read-only base disk. Package resolution
uses the pinned Alpine branch's repositories; rebuilding can pick up package
updates within that branch. Setup output is in `build/image-setup.log`.
The QEMU setup boot enables Linux's `riscv_isa_fallback` so older QEMU versions,
including Ubuntu 24.04's QEMU 8.2, can describe CPU extensions through their
legacy `riscv,isa` device-tree property.

Browser startup mounts tmpfs on `/tmp` and `/run`, overlays `/var` and `/home`,
mounts the resident host share at `/workspace` with `cache=none`, and logs in
UID 1000 (`demo`) without a password. `doas` permits password-free root commands.
BusyBox acpid handles the soft shutdown/reboot events. The browser VM has no
network device; networking is used only by QEMU to prepare the generic image.

Explicit workflows
------------------

1. Select a source tree and click **Load tree**. Replacement requires a halted
   VM. Arithmetic is loaded by default before the first boot.
2. In the terminal, run `make` and `make run` under `/workspace`. The individual
   [BSD examples](examples/README.md) retain their upstream licenses.
3. Select a file in the automatically updated, indented tree and click **Copy to editor**. Selecting
   a file alone does not read its contents. **Save to 9p** writes the editor's
   entire current text to that path.
4. Try soft shutdown/reboot, forced halt/reboot, boot, image reset, share clear,
   and reloading the current tree. The event log names the API calls and VM
   notifications. Resetting the image preserves 9p; resetting 9p preserves the
   disk. Destroy invalidates both facades; Prepare creates a new VM.

Tree replacement and share reset require poweroff. The controls do not save
editor text or protect unsaved changes. The file browser subscribes to host and
guest filesystem changes; editor reads and writes remain explicit.
The editor is a copied host buffer; disk, share, editor, and guest state have
separate lifetimes.

The HTML loads pinned Wterm and CodeMirror modules from CDNs. The JavaScript has
no transpiler or bundler. The terminal is 80×25 with library-default typography;
the editor has a fixed size and no syntax highlighting. Boot output stays in
terminal history across guest resets; the separate Clear terminal control clears
the display and retained history without touching the VM.

`make test` runs opt-in real Chrome acceptance using only the assembled file tree
as the app's server root. It is separate from core `make check`.
