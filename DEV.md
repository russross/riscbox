Riscbox development
===================

This file holds deferred investigations and future plans. Implemented behavior belongs in the distribution guides and `AGENTS.md`; completed work belongs in `CHANGELOG.md`.


Deferred: pygame in a constrained guest
--------------------------------------

Status: deferred as of 2026-10-11. Research and architecture planning only; no guest package build, device implementation, compatibility test, or performance measurement has been completed. Resume with the experiment below before committing to a display architecture.

The goal is ordinary pygame applications inside the RV64 Linux guest: drawing, images, fonts, keyboard, and mouse input displayed in the browser. CPU performance is severely constrained. Avoid a display server, desktop session, and guest software OpenGL. Sound output is unnecessary; compatibility with programs that initialize their mixer is useful.

The preferred investigation is pygame with SDL2's DirectFB backend and a minimal, single-application DirectFB2 build. First exercise the existing simple framebuffer and VirtIO input devices. Consider a standard 2D VirtIO GPU only after measuring that baseline. DirectFB2 is a candidate guest dependency, not an adopted or validated dependency.


Research findings
-----------------

### SDL2 and pygame display paths

Stock SDL2's KMSDRM backend exposes GBM/EGL rendering and does not implement the direct window-framebuffer callbacks used by SDL software surfaces. SDL's window-surface implementation can instead create a renderer and texture. Selecting `SDL_RENDER_DRIVER=software` does not add the missing direct framebuffer implementation. A working Linux DRM device therefore does not establish an efficient software-only pygame presentation path.

Sources: [SDL2 KMSDRM implementation](https://raw.githubusercontent.com/libsdl-org/SDL/SDL2/src/video/kmsdrm/SDL_kmsdrmvideo.c), [SDL2 window-surface implementation](https://raw.githubusercontent.com/libsdl-org/SDL/SDL2/src/video/SDL_video.c).

Ordinary pygame displays use SDL window surfaces. `pygame.SCALED` introduces a renderer/texture path, and `pygame.OPENGL` uses an OpenGL context. Begin with ordinary `display.set_mode()`, drawing and blitting, and `display.update(rects)` or `flip()`. Treat scaling flags, OpenGL, multiple windows, and desktop window behavior as separate compatibility cases. Scale the resulting guest image in the browser where possible.

Sources: [pygame display implementation](https://raw.githubusercontent.com/pygame/pygame/main/src_c/display.c), [pygame display API](https://www.pygame.org/docs/ref/display.html).

### DirectFB2 candidate

SDL2 retains a DirectFB backend. DirectFB2 supports fbdev, DRM/KMS, and Linux input. Its build options default multi-application and kernel Fusion support off, making a single-application library setup possible without a separate display server. Its DRM implementation checks for dumb-buffer support and allocates/maps those buffers without requiring an EGL/GBM renderer.

Sources: [SDL2 DirectFB documentation](https://wiki.libsdl.org/SDL2/README-directfb), [DirectFB2 project](https://directfb2.github.io/), [build options](https://raw.githubusercontent.com/directfb2/DirectFB2/master/meson_options.txt), [DRM initialization](https://raw.githubusercontent.com/directfb2/DirectFB2/master/systems/drmkms/drmkms_system.c), [DRM buffer implementation](https://raw.githubusercontent.com/directfb2/DirectFB2/master/systems/drmkms/drmkms_surface_pool.c).

The SDL documentation includes old examples; do not copy its historical dependency versions or configuration verbatim. Pin and inspect actual SDL2/DirectFB2 versions when resuming. Source inspection supports this candidate architecture, but does not prove compatibility with Alpine riscv64, the current simplefb driver, or pygame. Confirm framebuffer ioctls, input discovery, virtual-terminal requirements, threads, buffering, and the complete guest dependency graph.

### Silent audio

Use `SDL_AUDIODRIVER=dummy` before SDL initialization, with a guest SDL build that includes the dummy backend. This should allow pygame mixer initialization and playback without a guest sound device, ALSA driver, or browser audio component. Verify mixer initialization, playback completion, and application exit in the selected build.

Source: [SDL2 dummy audio implementation](https://raw.githubusercontent.com/libsdl-org/SDL/SDL2/src/audio/dummy/SDL_dummyaudio.c).

Dummy output still allows guest decoding and mixing to consume CPU. Applications we control should skip audio work when disabled; the dummy backend is for compatibility with other programs. A VirtIO sound device that drops samples would retain mixing costs and add driver/transport work. Defer VirtIO sound, kernel sound configuration, AudioWorklet integration, and audio APIs entirely.


Candidate data flows
--------------------

Initial experiment:

    pygame → SDL2 DirectFB → DirectFB2 fbdev → /dev/fb0
        → Riscbox simplefb dirty tracking → browser pixels → Canvas 2D
    browser keyboard/pointer → VirtIO input → evdev → DirectFB2 → SDL2 → pygame

Possible later VirtIO implementation:

    pygame → SDL2 DirectFB → DirectFB2 DRM dumb buffers
        → Linux virtio_gpu → VirtIO MMIO GPU → Rust display resources
        → browser pixels → Canvas 2D

The guest produces pixels in both cases. GPU command translation to WebGL/WebGPU is outside this investigation. A browser Web Component can own presentation and input without adding a guest daemon.

The original Xorg modesetting proposal is superseded by the DirectFB2 investigation because of the performance constraint. A custom SDL software framebuffer or DRM backend remains a fallback if measurements or compatibility failures justify maintaining an SDL patch. Neither Xorg nor custom SDL work is the starting point.


Existing integration points
---------------------------

The following describe the repository at the time of research; retrace them before implementation:

*   `src/config.rs` and `js/riscbox.js` accept `display0: {device: "simplefb", width, height}` and `input_device: "virtio"`. The browser runtime constructs a keyboard/tablet pair; preparation also has width/height overrides whose meaning must remain explicit.
*   `src/machine.rs` owns framebuffer mappings, dirty-page row spans, and concrete `VirtioSlot` variants. `src/virtio.rs` and `src/virtio_devices.rs` own the existing MMIO transport and device interface; `src/fdt.rs` describes optional devices and VirtIO MMIO slots.
*   `src/browser_runtime.rs` emits framebuffer `HostAction`s after CPU work. `src/browser_abi.rs` and `riscbox-wasm/src/lib.rs` expose actions; `js/riscbox.js` dispatches `framebufferRefresh` and `framebufferClear`. Pixel callbacks receive temporary WASM views with full-frame stride and must consume or copy them synchronously.
*   `src/browser_input.rs` queues host input; the public client already supplies `keyEvent`, `pointerEvent`, and `wheelEvent`. `js/riscbox.d.ts` defines their public types. Trace queue pressure and event retirement before extending the presentation component.
*   `kernel/config` enables simplefb, VirtIO input, and evdev, but disables DRM, sound, and virtual terminals. `kernel/Makefile` drives configuration and builds. `tools/build_adapter.mjs` and the root Makefile own adapter assembly and release packaging.


Browser presentation design
---------------------------

Provide an optional native `<riscbox-screen>` component, provisionally implemented in `js/screen.ts`, using Canvas 2D and browser keyboard/pointer events. Keep the client adapter independent of DOM attachment. A proposed component API is `attach(client): void` and `detach(): void`; a checked display subscription on the client would let it attach after preparation. Final API names and callback compatibility require a separate implementation plan.

The component should own a browser pixel buffer, scaling and letterboxing, focus, fullscreen controls, and held-input tracking. Keep guest dimensions distinct from CSS size and device-pixel ratio. Translate physical keyboard codes to Linux input codes and map pointer coordinates into the guest display. Release held keys/buttons on blur or detach; preserve transitions under backpressure and coalesce redundant pointer motion.

Copy callback pixels before returning to the runtime. Never retain a WASM view until an animation-frame callback. Accumulate dirty regions in the browser-owned image and paint the newest state on `requestAnimationFrame`; do not build an unbounded frame queue or tie CPU scheduling to browser paint frequency. Initial simplefb presentation follows existing reset/clear callbacks. Explicitly specify attach, detach, halt, reset, and destroy behavior so old scheduled paints cannot restore stale pixels.

Package the optional module and declarations with the runtime and test public-client integration. The terminal-only release demo need not become the graphical test fixture; choose the image/fixture destination when implementation begins.


Conditional VirtIO GPU design
-----------------------------

Proceed only if the baseline measurements and desired display behavior justify the device. A 2D GPU provides explicit resources, transfers, flushes, and scanout control, but is not automatically faster than simplefb. Additional buffers and transfers may increase copying.

*   Add a flat `src/virtio_gpu.rs` implementing the existing `VirtioDevice` interface, with a concrete slot in `src/machine.rs`. Reuse MMIO transport, PLIC interrupts, and generated FDT enumeration; audit available slot/IRQ capacity.
*   Model resource IDs, pixel formats, rectangles, backing ranges, scanout, and cursor state with explicit types. Bound dimensions, resource count, backing entries, and allocated bytes. Keep guest physical addresses as `u64` and validate descriptor/backing ranges and arithmetic.
*   Implement one scanout, display discovery, 2D resource creation/deletion, backing attachment/detachment, transfers, flushes, scanout selection, and cursor update/move. Implement required response errors and fence ordering, and advertise only supported features. Use the VirtIO specification first and Linux driver behavior to select acceptance cases.
*   Rust owns device resources and presentation state. Transfer commands copy validated guest backing into resources; flushes update the displayed image and dirty regions. Guest command completion follows device semantics and does not wait for an animation frame. Dispatch browser actions only after releasing the exclusive CPU-run borrow. Reset retires resources, cursor state, and pending presentation work.
*   Extend typed configuration with a display variant such as `display0: {device: "virtio-gpu", width, height}`. Generalize display metadata for dimensions, stride, format, dirty regions, and cursor, preserving simplefb behavior. Enable built-in DRM and VirtIO GPU in the pinned kernel and resolve dependencies through its configuration tooling. Determine VT requirements from the selected guest stack; leave sound disabled.

References: [VirtIO 1.2 specification](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html), [QEMU 2D VirtIO GPU overview](https://www.qemu.org/docs/master/system/devices/virtio/virtio-gpu.html).


Resume sequence and acceptance
------------------------------

1.  Confirm package availability and build a pinned minimal Alpine riscv64 guest stack: Python/pygame, SDL2 with DirectFB and dummy audio, DirectFB2 with fbdev/Linux input and single-application operation, and the necessary image/font libraries. Compare available maintained dependencies before adoption. Configure device permissions and startup explicitly; do not assume a stock Alpine SDL package includes DirectFB.
2.  Exercise the existing simplefb path in real WASM/Chrome with small pygame programs covering opaque and alpha blits, fonts, images, sparse updates, full-frame animation, keyboard, pointer, and mixer initialization with dummy audio. Verify exit/restart and VM lifecycle behavior. Resolve mode-setting/ioctl and input-discovery assumptions before broadening scope.
3.  Measure emulated cycles per frame, wall-clock frame rate, memory, and input/presentation latency at modest resolutions such as 320×240 and 640×480. Compare full updates with dirty rectangles and audio disabled with dummy playback. Count guest pixel copies and format conversions. Match surface formats where useful and perform display scaling in the browser. Process count alone is not a performance result.
4.  If warranted, implement and compare the 2D VirtIO GPU/DirectFB2 DRM path using identical workloads. Validate malformed descriptors, invalid rectangles/backing, allocation bounds, resource lifetimes, cursors, modes, ordering, and reset natively and through WASM before guest acceptance. Use incremental Makefile builds and the repository's scoped formatting/lint/test workflow. Chrome validation uses a temporary profile, headed mode when a display exists, and closes test processes afterward.
5.  Turn the selected path into supported configuration and packaged browser APIs. Document the implemented operation flows and invariants in `SEMANTICS.md`, update `AGENTS.md`, `API.md`, `HOWTO.md`, and the changelog as appropriate, and remove completed plans here. Keep manual performance experiments out of routine CI. Record measured limitations rather than promising a frame rate in advance.

The first unresolved decision is whether DirectFB2 on the existing simplefb offers acceptable compatibility and cost. No implementation of VirtIO GPU, custom SDL backends, or browser audio should precede that experiment merely because it appeared in the earlier architecture proposal.
