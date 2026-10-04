Building Riscbox
================

These instructions are for contributors working from a source checkout.
Distribution users should start with [README.md](README.md) and
[HOWTO.md](HOWTO.md). The independent historical image projects and
shared browser client are not inputs to any core build or test.

Prerequisites
-------------

Install Rust with the `wasm32-unknown-unknown` target, Clang, `ar`, Binaryen
(`wasm-opt`), GNU Make, `uv`, and Node.js 20.19 or later with npm. The packaged
Linux and firmware builds also need the RISC-V GNU toolchain, `curl`, `bc`,
Bison, Flex, and the usual kernel development libraries. On Debian/Ubuntu:

    sudo apt install clang llvm lld binaryen gcc-riscv64-linux-gnu \
        binutils-riscv64-linux-gnu make curl bc bison flex libssl-dev libelf-dev dwarves
    rustup target add wasm32-unknown-unknown

`make js` installs the root manifest's pinned TypeScript compiler with `npm ci`.
There are no browser adapter runtime dependencies. Rust dependencies are locked
in `Cargo.lock`; native Rust builds are development and test environments.

Targets
-------

| Target               | Result                                                     |
| -------------------- | ---------------------------------------------------------- |
| `make`               | Runtime, boot assets, and archive in `build/releases/`     |
| `make wasm js`       | WASM runtime and generated browser module/declarations     |
| `make test-unit`     | Rust, splitter, adapter, network, and Node raw-WASM probes |
| `make test`          | Unit checks plus real Chrome/WASM device and ABI probes    |
| `make check`         | Full core checks, TypeScript, Clippy, and Python types     |
| `make check-release` | Core checks, package build, archive validation/loading     |
| `make demo`          | Build the release-based Alpine embedding app               |
| `make test-demo`     | Opt-in Chrome acceptance of the assembled demo             |

`make kernel`, `make opensbi`, and `make uboot` build their pinned components.
`make release` builds the optimized Rust workspace for development.
`make release-path` prints the current archive path for consumers.
`make clean` removes generated core and boot outputs; `make -C demo clean`
removes only demo outputs.

Browser validation uses temporary profiles, headed Chrome when `DISPLAY` is
available, and headless Chrome otherwise. Validation is run explicitly with the
targets above. GitHub runs checks only as part of the release cycle. Core tests
use small firmware probes
and require no guest root filesystem, QEMU, mounted source, or UI framework.
The optional demo build needs QEMU, fakeroot, cpio, and erofs-utils; see
[demo/README.md](demo/README.md).

Release automation
------------------

A version increase in the workspace `Cargo.toml` pushed to `main` runs the
Release workflow. It validates the core, builds the archive, prepares the demo
image in QEMU, and runs Chrome demo acceptance before creating the version tag
and GitHub release. A separate deployment job publishes `demo/dist/` to
<https://russross.github.io/riscbox/>. Generated assets remain workflow artifacts
and Pages content, outside Git; the release archive still excludes the demo.

For manual runs, open **Actions → Release → Run workflow**:

1. Select a branch and leave **mode** as **test** to exercise the same build and
    validation without publishing. Download the runtime archive and
    `riscbox-demo.tar.gz` from the run's artifacts. Extract the demo archive and
    serve its directory with `python3 -m http.server` to try it locally.
2. Select **main** and **publish** to release its current version and deploy the
    demo. An existing version tag must point to the selected commit. When the
    release already exists, the workflow downloads its original runtime archive
    for demo assembly, skips release creation, and retries Pages deployment.

Enable **Settings → Pages → Build and deployment → Source → GitHub Actions**
once in the repository. The `github-pages` environment must allow deployment
from `main`. GitHub's built-in workflow token supplies publication permissions;
manual test builds have read-only repository access. Publication and deployment
receive their required write permissions in separate jobs. Only one workflow
run publishes at a time.

Repository boundaries
---------------------

README, API, and HOWTO are packaged for embedding applications. Storage and
protocol guides document implementation boundaries. `AGENTS.md`, `DEV.md`, and
this file describe contributor workflow and architecture. `demo/` is an example
consumer, excluded from the archive and core checks. Its `dist/riscbox/` directory is copied from the release
archive, and its browser code accesses only that directory and demo-owned assets.

Guest image definitions and their full-system acceptance tests belong to the
separate image project. Shared editor, terminal, and application lifecycle code
belongs to its owning application. Neither is mounted or installed by Riscbox CI.
