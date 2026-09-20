Riscbox development
===================

This file contains only current and future work. Completed implementation
history belongs in `CHANGELOG.md`; durable project constraints and current
architecture belong in `AGENTS.md`; user-visible behavior belongs in
`README.md`.

Development priorities
----------------------

1.  Keep current xv6 and deliberately prepared current Alpine images booting
    without emulator-specific guest patches. Let focused guest failures select
    compatibility work.
2.  Fill small, broadly useful RVA23 gaps when architectural probes and real
    guests can validate them. Vectors, hypervisor support, and multiple harts
    remain out of scope.
3.  Tighten QEMU `virt` device-tree and platform compatibility where firmware
    or Linux depends on it. Correct the existing platform instead of adding
    compatibility modes.
4.  Improve image preparation only when a small loader or boot feature removes
    material deployment friction. Prepared raw kernels and initrds remain the
    baseline.
5.  Benchmark before optimizing the interpreter, WASM boundary, storage, or 9p
    implementation. Correctness remains the first gate.

Active design: browser networking reliability
---------------------------------------------

### Problem and evidence

The Rust platform has a minimal two-queue VirtIO network device and the raw
WASM ABI carries Ethernet frames in both directions, but networking currently
stops at callbacks in `js/riscbox.js`. There is no distributed network
frontend, documented wire protocol, or end-to-end browser test. The current
device also regresses from or leaves robustness gaps in the historical TinyEMU
path:

*   Browser VMs use the fixed `02:00:00:00:00:01` address instead of TinyEMU's
    per-VM locally administered address. Two VMs on one layer-2 service collide.
*   `NetworkDevice::receive` is an unbounded `Vec`, removes from the front, and
    turns an undersized posted receive chain into a fatal machine error.
*   Device reset preserves pending receive frames. Guest transmit accepts
    nonzero checksum or segmentation fields even though no offload feature is
    advertised.
*   Carrier events reach `BrowserRuntime` but are discarded. The device does
    not advertise `VIRTIO_NET_F_STATUS`, expose `virtio_net_config.status`, or
    raise a configuration-change interrupt.
*   FDT generation already describes every instantiated device with standard
    `virtio,mmio` nodes, as QEMU `virt` does. Existing tests do not trace a
    configured network device through slot assignment and the boot tree.

TinyEMU's browser frontend is useful as a behavioral baseline: one binary
WebSocket message carries one Ethernet frame, socket open/close drives carrier,
and guest frames are sent as binary messages. Its optional text `ping:` reply
is relay-specific and is not part of the Riscbox protocol.

### Scope and exclusions

Complete the supported single-interface browser path from VirtIO through the
raw WASM ABI and a dependency-free TypeScript WebSocket adapter to a same-origin
endpoint. Preserve the small, non-offloading VirtIO device while matching QEMU
and the VirtIO specification where that is useful: MAC and link status,
configuration interrupts, ordinary `virtio,mmio` discovery, and reset-safe
queues.

The production origin-side bridge, NAT, routing, filtering, authentication,
rate limiting, TLS termination, and browser-to-remote-server deployment tests
remain out of scope. Do not restore SLIRP, TAP, native sockets, or the C archive
as runtime dependencies. The only server is a local Node test stub; it is not
copied into distributions. Do not add multiqueue, control queues, checksum or
segmentation offload, merged receive buffers, or a second interface without a
guest requirement.

### Data and ownership

*   Replace `NetworkConfig.driver: String` with a closed `NetworkDriver::User`
    value for the existing `driver: "user"` browser configuration. Reject
    native-only `tap`/`ifname` forms in the active Rust schema. The WebSocket
    URL is a host integration setting, not guest configuration.
*   `NetworkDevice<B>` owns `carrier_up: bool` and a `VecDeque<Vec<u8>>` receive
    FIFO. Accept nonempty frames through 65,535 bytes and cap pending ingress at
    256 frames or 1 MiB. Drop newest ingress when any limit is exceeded so
    already accepted ordering is stable; report invalid host frames and
    capacity drops as nonfatal rejection rather than machine failure.
*   Generate six MAC bytes from the existing machine entropy source at browser
    machine creation, then set the local bit and clear the multicast bit. The
    address remains stable across VirtIO reset and WebSocket reconnect, and a
    new `Riscbox.start()` creates a new address.
*   Add `js/network.ts` with `WebSocketNetwork`, typed options, and a narrow
    structural runtime interface containing `networkInput(Uint8Array)` and
    `networkCarrier(boolean)`. Inject a WebSocket factory for Node unit tests;
    browsers use the native constructor. The adapter owns the socket and
    reconnect timer. It never retains WASM-backed views.
*   The adapter drops guest output while disconnected and when
    `WebSocket.bufferedAmount` exceeds 1 MiB. It accepts only binary inbound
    messages within the documented frame limit. Socket open is carrier-up;
    error, close, explicit `close()`, and reconnect delay are carrier-down.
    Reconnect starts at 250 ms, doubles through a 10-second ceiling, resets
    after open, and is canceled by `close()`.

### Flow and interfaces

1.  In `src/virtio_devices.rs`, advertise `VIRTIO_NET_F_MAC` and
    `VIRTIO_NET_F_STATUS`; expose the six-byte MAC followed by the little-endian
    link-status field. Add `set_carrier(&mut self, &mut VirtioTransport, bool)`
    and raise a configuration interrupt only on a transition. Reset clears RX
    frames but preserves host link state and device configuration.
2.  Make receive delivery distinguish malformed guest descriptors from normal
    packet loss. A well-formed chain too small for the ten-byte header plus
    frame is completed with zero bytes and the frame is dropped; malformed
    addresses, direction changes, loops, or queues still return `DeviceError`.
    On transmit, require a complete ten-byte header and zero flags, GSO type,
    header length, GSO size, checksum start, and checksum offset before passing
    the frame to `NetworkBackend`.
3.  In `src/machine.rs` and `src/browser_runtime.rs`, add
    `virtio_network_set_carrier(slot, up)`, apply the controller's current link
    state when the device is created, and deliver later carrier events. In
    `src/browser.rs` and `src/browser_abi.rs`, bound pending network events and
    return a distinct nonfatal result when an ingress frame is rejected.
4.  Define the origin protocol in `js/network/README.md`: the endpoint is an
    HTTP Upgrade to WebSocket; every binary message in either direction is
    exactly one Ethernet frame without FCS or an application header; text
    messages are protocol errors; message order is frame order; standard
    WebSocket close and control frames provide lifecycle and keepalive; and the
    documented size/backpressure limits permit packet loss. No subprotocol is
    negotiated in version one.
5.  Export `WebSocketNetwork` from `js/network/index.ts`. Integration constructs
    it with an endpoint URL, passes its bound `transmit(packet)` as
    `networkWrite` while instantiating `Riscbox`, then calls
    `network.attach(runtime)` before `connect()` and `Riscbox.start(..., true)`.
    `attach()` may be called once and `connect()` requires an attached runtime.
    Keep `js/riscbox.js` dependency-free and retain its lower-level callbacks
    for custom frontends.
6.  Extend `js/tsconfig.json`, `Makefile`, and
    `images/bin/build-distribution` so `make js`, `make check`, and
    distributions include `build/js/network/*.js` and declarations beside the
    existing 9p modules. Update `README.md` and `images/DEPLOYMENT.md` with the
    typed adapter, protocol, local test command, and the production origin
    service responsibilities that remain unimplemented.

### Milestones and acceptance

1.  Harden the Rust device. Add focused tests for feature bits and config
    layout, carrier transitions and interrupt acknowledgement, duplicate
    carrier events, random unicast/local MACs across two VMs, FIFO limits and
    ordering, startup before RX buffers exist, undersized buffers, malformed
    chains, unsupported TX headers, ring exhaustion, and reset lifetime.
2.  Complete platform integration. Test configuration rejection, slot/IRQ
    assignment, FDT `virtio,mmio` discovery with networking enabled, carrier
    state before and after machine creation, raw ABI ingress rejection, and
    sustained bidirectional frame flow without unbounded queues.
3.  Add the TypeScript adapter and protocol documentation. Unit-test open,
    binary receive, transmit copying, wrong message types, frame limits,
    buffering high-water behavior, close, errors, reconnect backoff, and
    independent adapters for two VMs with injected sockets.
4.  Add a local Node origin stub which serves the WASM/config/firmware test
    assets and implements only the documented WebSocket frame echo/fixture
    behavior. Drive a temporary-profile headless Chrome instance against it to
    boot a focused non-PIE network firmware probe and verify link transitions,
    MAC uniqueness, ARP-style broadcast ingress, and repeated bidirectional
    Ethernet frames through the real WASM and browser adapter. Close Chrome and
    the stub after the test. Include this path in `make test` and keep the stub
    under tests rather than the distribution.
5.  Run `make check`, rebuild WASM from a clean tree, and boot prepared Alpine
    locally through the Node stub far enough to verify VirtIO discovery,
    interface carrier, DHCP traffic exchange with a deterministic stub, and
    clean shutdown. Update current documentation and move completed results to
    `CHANGELOG.md`; remove this active design when the milestone is complete.

Candidate work
--------------

### Framebuffer demonstration

Add guest framebuffer and input demonstration programs, configure those devices
in an image, and connect dirty-region callbacks to a canvas in the Risclet page.
Validate the complete guest-to-page path together. The current Risclet demo is
intentionally terminal-only.

### Boot and image loading

Evaluate a standard bootloader path, compressed kernel loading, and other image
features only against a concrete Alpine deployment problem. Linux already
handles compressed initrds after Riscbox loads them opaquely. Any new loader
must justify its code size and failure surface relative to image preparation.

### Performance

Measure WASM throughput, module size, allocation, and memory use before choosing
work. Candidate measurements include interpreter hot paths, HTTP block request
latency, 9p request/reply copy volume, concurrent request latency, resident and
logical 9p tree sizes, and peak lazy-load memory.

Design and milestone format
---------------------------

Add an active design here before implementing work that spans subsystems. Keep
it short and current, using these sections as needed:

*   Problem and evidence: the guest failure, deployment need, or measurement.
*   Scope and exclusions: the bounded behavior and explicit non-goals.
*   Data and ownership: types, state, lifetimes, storage, and concurrency.
*   Flow and interfaces: files, function signatures, ABI or configuration
    changes, and error handling.
*   Milestones and acceptance: ordered increments with focused tests, WASM
    checks, guest validation, and measurements.

Remove completed milestones rather than accumulating checked-off plans. Move
lasting decisions and results into the appropriate current or historical
document during the same commit.

Deferred findings
-----------------

Record an out-of-scope issue here when development uncovers it. Include the
observed behavior, affected guest or interface, evidence or reproduction, why
it is deferred, and the condition that should trigger reconsideration. Remove
the entry when it becomes an active design or is resolved.

No deferred findings are currently recorded.
