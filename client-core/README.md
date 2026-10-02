Shared browser client source
============================

This directory is an application-independent editing environment, compiled into
its consumers. Riscbox owns it while developing the Risclet demo here. Ownership
can move to Risclet with the demo. Codegrinder vendors the complete directory.
It is not a standalone application or a separately published runtime package.

Modules and ownership
---------------------

*   `editor-session.ts` owns CodeMirror, buffered revisions, access control,
    thirty-second debounce, blur synchronization, and filesystem conflicts.
    `editor-text.ts` supplies language, soft-tab, and newline conventions.
*   `workspace.ts` copies and restores complete namespaces, including links and
    metadata, and identifies paths affected by filesystem notifications.
*   `vm-session.ts` owns a prepared VM, its live filesystem handles, terminal,
    lifecycle transitions, and retired input. Application targets own snapshots.
*   `terminal.ts`, `terminal.css`, and `terminal_input.ts` own Wterm rendering,
    selection/paste, and FIFO backpressure. `build/` contains guarded patches.
*   `workspace-view.ts` and its CSS render an accessible file tree.
    `instructions.ts` renders Markdown and resolves workspace image bytes.

Applications own navigation, data loading, server persistence, grading,
clipboard policy, controls, outer layout, and deployment. There are no exam/demo
feature flags. The core has no RPC, authentication, or example-manifest imports.

Dependencies and build
----------------------

Run `npm ci` in this directory. Its package and lock file own the shared
CodeMirror, CommonMark, Wterm, and build-tool versions. Wterm is pinned at 0.5.4;
its two source patches deliberately reject incompatible upstream changes.
Consumers should not install a second CodeMirror/Wterm dependency graph for
shared widgets. Import editor types from the same installation when extending
their configuration.

All Riscbox imports are type-only aliases: `@riscbox/runtime` and
`@riscbox/storage`. Map both to the latest deployed `riscbox.d.ts` in the host
TypeScript configuration. The repository default is `../build/js/riscbox.d.ts`.
An external consumer overrides paths in a configuration outside this subtree.
The JavaScript constructor is supplied by `VmSessionOptions.loadRuntime`.

Webpack consumers import `clientCoreRules()` and `clientCoreModules` from
`build/webpack.mjs`, apply the rules, and put the shared dependency directory in
`resolve.modules`. They also supply TypeScript and CSS loaders. Include
`commonmark.d.ts` in the host TypeScript program. Deploy every generated JS
chunk and WASM asset alongside the main bundle, preserving webpack public-path
behavior. Copying only `bundle.js` is insufficient.

Runtime contract
----------------

Use one matching build of `riscbox.js`, `riscbox.d.ts`, and `riscbox.wasm` with:

*   `prepareResolved`, `boot`, `halt`, `destroy`, `coldReset`, `requestShutdown`,
    `requestReboot`, console callbacks, and `started` lifecycle state.
*   Synchronous VM-owned `filesystem(name)` and `block(index)` facades.
*   Filesystem subscriptions with source, numeric origin, aliases, and old path;
    file/directory/link operations; and `setAttributes` with nanosecond times.
*   Powered-off `clear()` and disk `discardChanges()`.

The old standalone filesystem/source/seed/bind APIs are unsupported. The exam
migration guide describes the required upgrade and integration ordering.
Client VM images use HTTP-backed disks, as both current clients do. Array disks
write through and cannot discard changes; preparation rejects them rather than
allowing Reset to fail after retirement. This is a client-layer constraint,
not a restriction on the Riscbox runtime's array-storage API.

Synchronization and lifecycle
-----------------------------

`EditorSession.flush(trigger)` serializes writes and acknowledges only the
submitted revision after success. Its returned promise covers editor-to-9p
synchronization. `onSynced` is a synchronous notification, including no-op
flushes; it must not throw or start a recursive flush. Server-save completion
belongs to a separate application coordinator. Failed writes retain dirty text
and schedule retry. `clear()` intentionally discards the buffer; call it only
after a successful flush or an explicit reset/discard decision.

Callers serialize `VmSession.setTarget(target, reset, isCurrent)` and disable
editor access and interactive input during transitions. Downloads occur before
retiring the outgoing VM. Same-image switches request orderly poweroff and wait
for the halt callback, snapshot the outgoing namespace, await `afterSnapshot`,
replace the namespace, and boot while retaining disks. Different images force
halt, snapshot, await that same hook, destroy the old VM, prepare the new one,
restore the target, and boot. No filesystem facade survives destruction.

Reset forces halt, cold-resets, discards every configured disk's changes,
discards the incoming target's snapshot, restores `loadFiles()` originals, and
boots. `forceHalt()` can be called outside the selection queue to release a
stalled shutdown; namespace replacement remains serialized. Ordinary reboot
requests guest reboot and retains the namespace and disk state. Reboot input
delivery is not proof that guest shutdown/restart has finished.

Image identity includes absolute configuration, adapter and WASM URLs, memory,
and share name. Keep immutable image descriptors; use versioned URLs when
replacing image contents. Application target identity determines which snapshot
is restored. Do not rebuild target objects on every selection.

Tests
-----

`npm test` runs portable FIFO, subtree-sync, editor/lifecycle, and fractional
pixel rendering tests. The real-WASM snapshot check uses the repository build
or `RISCBOX_CLIENT_RUNTIME` and `RISCBOX_CLIENT_WASM` paths; it reports a skip
when neither matching artifact pair is available. It requires Node with experimental WebSocket support
and `google-chrome`; headed Chrome is used when DISPLAY exists, otherwise
headless Chrome. Profiles and successful test output are temporary. Browser
fixture compilation transpiles TS; run the host type check separately.

In this repository, `npm run check` checks against the generated adapter types;
`make check` includes it and Risclet's host type check. Risclet's real-guest
integration test additionally covers actual guest shutdown, retained disks,
snapshots, reset recovery, downloads, and real thirty-second debounce timing.
External consumers must retain their own server and guest integration tests.

Vendoring and updates
---------------------

From the authoritative source directory:

```sh
node client-core/sync.mjs --check /path/to/codegrinder/client-core
node client-core/sync.mjs /path/to/codegrinder/client-core
```

The tool copies source, tests, documentation, build patches, and lock files;
excludes dependencies/test output; records source-file SHA-256 hashes; and
refuses local modifications, deletions, and unmanaged files before writing.
An empty destination or an identical manually copied subtree can be adopted.
Keep adapters, local TypeScript configs, and application tests outside this
directory. Resolve a shared fix in the authoritative tree before copying it.

After an update, run `npm ci`, the host type check, `npm test`, the application
build, and its integration tests. Review this contract and `EXAM-MIGRATION.md`
when callbacks or runtime requirements change. The provenance source URL is
descriptive; the tree hash and file hashes identify the exact copied content.
