BSD source trees
================

These standalone games come from
[BSDGames revision 56b8944](https://github.com/vattam/BSDGames/tree/56b8944332186891cd5c0094bd3991e3c5f5a799).
Each C source retains its Berkeley copyright and BSD license. `wump.info` comes
from that revision's `wump/` directory. The Makefiles build with Alpine's TinyCC.

The small `include/sys/cdefs.h` headers define the BSD source-identification
macros for musl, which has no such header. Wump includes `<fcntl.h>` explicitly
and uses a local instructions file and `/usr/bin/less`. Arithmetic uses its
literal program name in usage text and an explicit return after its fatal
penalty-list error for TinyCC's control-flow analysis.

*   `arithmetic`: interactive arithmetic practice; run `make run`.
*   `wump`: Hunt the Wumpus; run `make run`, answer the instructions prompt,
    and enter `q` to quit.
*   `number`: print numbers as English words; run `make run` or `./number 42`.

Each tree is independent and is loaded at the root of the guest's `/workspace`.
Compiled files live in the same resident 9p namespace and appear in the host
file browser when it is explicitly refreshed.
