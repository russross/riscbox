Release-based demo
==================

The [published demo](https://russross.github.io/riscbox/) updates after successful
releases or manual **Demo** runs. The separate Demo workflow builds this directory
against the published workspace version and runs Chrome acceptance before
Pages deployment. Successful builds provide `riscbox-demo.tar.gz`; see
[release automation](../BUILDING.md#release-automation).

To redeploy demo changes without releasing the core, open **Actions → Demo →
Run workflow** and select **main**. The workspace version must already have a
published release, which supplies the unchanged runtime archive.

    make
    python3 -m http.server --directory dist 8000

Open <http://localhost:8000/>. From the repository root, `make demo` performs the
same build. Install QEMU's RISC-V system emulator, fakeroot, cpio, e2fsprogs,
`curl`, Node.js, and `uv`, plus the core prerequisites in [BUILDING.md](../BUILDING.md).
No sudo or host filesystem mounts are needed.

The default build runs `make -C ..`, extracts the resulting release archive
unchanged under `build/release/riscbox-VERSION/`, and copies that complete tree
to `dist/riscbox/`. It adds the app, source examples, configuration, and split
disk beside it. To use a downloaded archive without building the parent repo:

    make RELEASE_ARCHIVE=/absolute/path/riscbox-VERSION.tar.gz

The image builder consumes only the release's compressed Linux and OpenSBI
payloads. Assembly invokes its packaged `splitimg.py`. No application code or
image helper reads the repository's runtime source or internal ABI.
Assembly links guides and provenance to GitHub at the archive version tag.
Local source builds use their source commit; override with
`DOCUMENTATION_REF=TAG_OR_COMMIT` when using an unpublished archive.

Guest image
-----------

QEMU boots a verified Alpine 3.24.2 riscv64 minirootfs as an initramfs, installs
TinyCC (including its runtime archive), musl headers, make, doas, ncurses
development files and terminal data, and the CrackLib word list. BusyBox supplies
`less`; nano is omitted. Hangman's dictionary is `/usr/share/dict/words`.
Preparation compiles all 18 vendored BSD games and runs text-mode smoke checks.
It exports a root-owned tar over 9p. A single fakeroot session extracts the tar
and populates an 80 MiB writable ext4 image with `mkfs.ext4 -d`, retaining numeric
ownership and setuid doas without mounting the disk.

Package resolution uses the pinned Alpine branch; rebuilding can pick up updates
within that branch. Setup output is `build/image-setup.log`. Linux's
`riscv_isa_fallback` accepts legacy CPU descriptions from older QEMU versions,
including Ubuntu 24.04's QEMU 8.2.

Browser startup mounts the resident `shared` share at `/shared` with `cache=none`
and logs in UID 1000 (`riscbox`) without a password. The prompt uses Alpine's
`hostname:directory$` format. Home, temporary files, and system writes use ext4;
there are no tmpfs overlays. Disk writes remain session-local in the runtime's
HTTP overlay and survive reboot until image reset or VM destruction.
`doas` permits password-free root commands. BusyBox acpid handles soft power
events. Networking is used only by QEMU during image preparation.

Playground workflow
-------------------

1. Select a **Source tree** to clear and load 9p immediately, while running or
    halted. Arithmetic is loaded before the first boot. **<clear>** empties 9p.
2. Run `make` and `make run` under `/shared`. All 18 self-contained
    [BSD games](bsd-games-3.3/README.md) retain their upstream licenses.
    Assembly excludes binaries, objects, dependency files, and saved state.
3. Select a file to open it immediately. The selected row stays highlighted.
    Edits sync on blur or after 30 seconds of inactivity. External file changes
    replace the editor immediately, discarding buffered edits; deletion clears
    the editor and makes it read-only. Binary files are read-only.
4. Use the lifecycle controls for soft power requests, forced halt/reboot,
    image reset, and destruction/preparation. Image reset preserves 9p.
    Destroy invalidates both storage facades; Prepare creates a new VM.

Source selection recursively unlinks content while preserving the mounted root
and active fids. A guest shell in a removed directory can return with `cd /shared`.
A newer source selection supersedes any pending source download. This playground
does not protect edited files against data loss.

The three draggable panes begin at 10/45/45 percent for files, editor, and terminal.
A horizontal split separates them from the collapsed storage and event details.
Changed or created files pulse in the tree; deleted rows pulse before disappearing.
External changes to the open file pulse the editor. Editor writes pulse the file
row and terminal pane.

The HTML loads pinned xterm.js, fit/WebGL addons, and CodeMirror from CDNs.
There is no transpiler or bundler. The terminal fits its container and reports
row/column changes to the guest. It uses risclet's typography, colors, bracketed
paste handling, and WebGL rendering with a DOM fallback. Boot output stays in
terminal history across guest resets.

`make test` compiles all games on resident 9p with the emulated RV64 TinyCC,
checks terminal startup and live editing, and runs lifecycle and disk-persistence
acceptance in real Chrome using only the assembled app. It is separate from
core `make check` and owns a temporary Chrome profile and process.
