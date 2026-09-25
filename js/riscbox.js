(function (root) {
    "use strict";

    const encoder = new TextEncoder();
    const decoder = new TextDecoder();
    const TURN_CYCLES = 3_000_000;
    // C checks its budget at code block boundaries, so reserve one page of slack.
    const CPU_OVERRUN_GUARD = 4_096;
    const RUN_WAITING = 1;
    const RUN_HOST_ATTENTION = 2;
    const RUN_IDLE = 3;

    class Riscbox {
        constructor(exports, options = {}) {
            if (!(exports.memory instanceof WebAssembly.Memory))
                throw new TypeError("Riscbox WASM must export memory");
            if (typeof exports.riscbox_run !== "function" ||
                typeof exports.riscbox_run_cycles !== "function" ||
                typeof exports.riscbox_run_delay_ms !== "function")
                throw new TypeError("Riscbox WASM has an incompatible run interface");
            this.exports = exports;
            this.options = options;
            this.p9Sessions = new Map();
            this.p9Requests = new Map();
            this.hints = new Set();
            this.settledHints = 0;
            this.driving = false;
            this.timer = null;
            this.started = false;
        }

        static hostImports(options = {}) {
            let runtime = null;
            const bytes = (ptr, len) => runtime.bytes(ptr, len);
            const imports = {
                vm_started() {
                    options.onVmStarted?.();
                },
                random_fill(ptr, len) {
                    try {
                        const crypto = globalThis.crypto;
                        if (!crypto || typeof crypto.getRandomValues !== "function")
                            return -1;
                        const destination = new Uint8Array(
                            runtime.exports.memory.buffer, ptr, len,
                        );
                        crypto.getRandomValues(destination);
                        return 0;
                    } catch (error) {
                        options.onError?.(error);
                        return -1;
                    }
                },
                console_write(ptr, len) {
                    const data = bytes(ptr, len);
                    options.consoleWrite?.(decoder.decode(data));
                },
                framebuffer_refresh(ptr, x, y, width, height, stride) {
                    options.framebufferRefresh?.(
                        bytes(ptr, stride * height),
                        { x, y, width, height, stride },
                    );
                },
                network_write(ptr, len) {
                    options.networkWrite?.(bytes(ptr, len));
                },
            };
            return {
                imports,
                attach(exports) {
                    runtime = new Riscbox(exports, options);
                    return runtime;
                },
            };
        }

        static async instantiate(source, options = {}) {
            const host = Riscbox.hostImports(options);
            const result = await WebAssembly.instantiate(source, {
                riscbox_host: host.imports,
            });
            return host.attach(result.instance?.exports ?? result.exports);
        }

        bytes(ptr, len) {
            if (!Number.isInteger(ptr) || !Number.isInteger(len) || ptr < 0 || len < 0)
                throw new RangeError("invalid WASM byte range");
            const end = ptr + len;
            const memory = new Uint8Array(this.exports.memory.buffer);
            if (!Number.isSafeInteger(end) || end > memory.length)
                throw new RangeError("WASM byte range is outside memory");
            return memory.slice(ptr, end);
        }

        withBytes(value, callback) {
            const data = typeof value === "string" ? encoder.encode(value) : value;
            if (!(data instanceof Uint8Array))
                throw new TypeError("expected a string or Uint8Array");
            if (data.length === 0)
                return callback(0, 0);
            const ptr = this.exports.riscbox_alloc(data.length);
            if (!Number.isInteger(ptr) || ptr <= 0)
                throw new Error("Riscbox WASM allocation failed");
            try {
                new Uint8Array(this.exports.memory.buffer, ptr, data.length).set(data);
                return callback(ptr, data.length);
            } finally {
                this.exports.riscbox_free(ptr, data.length);
            }
        }

        start(configUrl, ramMiB, commandLine = "", width = 0,
              height = 0, hasNetwork = false) {
            this.configUrl = configUrl;
            const result = this.withBytes(configUrl, (urlPtr, urlLen) =>
                this.withBytes(commandLine, (commandPtr, commandLen) =>
                    this.exports.riscbox_start(
                        urlPtr, urlLen, ramMiB, commandPtr, commandLen,
                        width, height, hasNetwork ? 1 : 0,
                    )));
            this.drainActions();
            return result;
        }

        run() {
            return this.drive();
        }

        schedule(delay) {
            // A newly completed device request replaces any outstanding WFI timer.
            if (this.driving)
                return;
            if (this.timer !== null)
                clearTimeout(this.timer);
            this.timer = setTimeout(() => {
                this.timer = null;
                void this.drive().catch((error) => this.options.onError?.(error));
            }, delay);
        }

        async drive() {
            // The timer only starts a turn; this loop owns every continuation in it.
            if (this.driving)
                return;
            if (this.timer !== null) {
                clearTimeout(this.timer);
                this.timer = null;
            }
            this.driving = true;
            let remaining = TURN_CYCLES;
            let nextDelay = 0;
            try {
                while (remaining > CPU_OVERRUN_GUARD) {
                    // Refresh guest-visible time on each entry and subtract actual C cycles.
                    const now = Date.now();
                    const reason = this.exports.riscbox_run(
                        now >>> 0, Math.floor(now / 0x1_0000_0000) >>> 0,
                        remaining - CPU_OVERRUN_GUARD,
                    );
                    if (reason < 0)
                        throw new Error("Riscbox WASM run failed");
                    if (reason > RUN_IDLE)
                        throw new Error(`Riscbox WASM returned invalid run reason ${reason}`);
                    const cycles = this.exports.riscbox_run_cycles();
                    if (cycles > remaining)
                        throw new Error(`Riscbox WASM used ${cycles} cycles from a ${remaining}-cycle budget`);
                    if (reason === 0 && cycles === 0)
                        throw new Error("Riscbox WASM made no progress in a runnable turn");
                    remaining -= cycles;
                    this.drainActions();
                    if (reason === RUN_IDLE)
                        return;
                    if (reason === RUN_WAITING) {
                        nextDelay = this.exports.riscbox_run_delay_ms();
                        break;
                    }
                    if (reason === RUN_HOST_ATTENTION) {
                        // Each settled hinted reply restarts the dry-yield allowance.
                        let dryYields = 0;
                        while (this.hints.size > 0 && dryYields < 20) {
                            const settled = this.settledHints;
                            await Promise.resolve();
                            dryYields = this.settledHints === settled ? dryYields + 1 : 0;
                        }
                        if (dryYields === 20 && this.hints.size > 0)
                            console.error("Riscbox 9p response hint did not settle", [...this.hints]);
                    }
                }
            } finally {
                this.driving = false;
            }
            this.schedule(nextDelay);
        }

        drainActions() {
            if (!this.exports.riscbox_next_action)
                return;
            for (;;) {
                const kind = this.exports.riscbox_next_action();
                if (kind === 0)
                    return;
                const value = this.exports.riscbox_action_value();
                const ptr = this.exports.riscbox_action_data_address();
                const len = this.exports.riscbox_action_data_length();
                if (kind === 1) {
                    const url = decoder.decode(this.bytes(ptr, len));
                    const fetchRequest = this.options.fetch ?? globalThis.fetch;
                    if (typeof fetchRequest !== "function")
                        throw new Error("Riscbox HTTP fetch is not available");
                    const cache = url === this.configUrl ? "no-store" : "default";
                    Promise.resolve(fetchRequest(url, { cache })).then(async (response) => {
                        const data = new Uint8Array(await response.arrayBuffer());
                        const status = response.status ?? 200;
                        this.withBytes(data, (dataPtr, dataLen) => {
                            const result = this.exports.riscbox_http_complete(
                                value, status, dataPtr, dataLen,
                            );
                            if (result !== 0)
                                throw new Error(`Riscbox rejected HTTP response ${value}`);
                        });
                        this.drainActions();
                        if (this.started)
                            this.schedule(0);
                    }).catch((error) => this.options.onError?.(error));
                } else if (kind === 2) {
                    this.started = true;
                    this.options.onVmStarted?.();
                    this.schedule(0);
                } else if (kind === 3) {
                    this.options.consoleWrite?.(decoder.decode(this.bytes(ptr, len)));
                } else if (kind === 4) {
                    this.options.networkWrite?.(this.bytes(ptr, len));
                } else if (kind === 6) {
                    const x = this.exports.riscbox_action_x();
                    const y = this.exports.riscbox_action_y();
                    const width = this.exports.riscbox_action_width();
                    const height = this.exports.riscbox_action_height();
                    const stride = this.exports.riscbox_action_stride();
                    const end = ptr + len;
                    const memory = new Uint8Array(this.exports.memory.buffer);
                    if (!Number.isSafeInteger(end) || ptr < 0 || len < 0 || end > memory.length)
                        throw new RangeError("framebuffer range is outside WASM memory");
                    this.options.framebufferRefresh?.(
                        memory.subarray(ptr, end),
                        { x, y, width, height, stride },
                    );
                } else if (kind === 7) {
                    const endpoint = this.exports.riscbox_action_endpoint();
                    const generation = this.exports.riscbox_action_generation();
                    const serverKey = decoder.decode(this.bytes(ptr, len));
                    const server = this.options.p9Servers?.get(serverKey);
                    if (!server || typeof server.connect !== "function")
                        throw new Error(`9p server is not registered: ${serverKey}`);
                    const session = server.connect();
                    if (!session || typeof session.request !== "function" ||
                        typeof session.close !== "function")
                        throw new TypeError(`9p server returned an invalid session: ${serverKey}`);
                    const previous = this.p9Sessions.get(endpoint);
                    if (previous)
                        this.retireP9(endpoint, previous.generation);
                    previous?.session.close();
                    this.p9Sessions.set(endpoint, { generation, session });
                } else if (kind === 8) {
                    const endpoint = this.exports.riscbox_action_endpoint();
                    const generation = this.exports.riscbox_action_generation();
                    const requestId = this.exports.riscbox_action_request_id();
                    const replyCapacity = this.exports.riscbox_action_reply_capacity();
                    const current = this.p9Sessions.get(endpoint);
                    if (!current || current.generation !== generation)
                        throw new Error(`9p request targets an inactive endpoint ${endpoint}`);
                    const request = this.bytes(ptr, len);
                    const key = `${endpoint}:${generation}:${requestId}`;
                    // Keys include the endpoint generation so a reset cannot reuse a hint.
                    this.p9Requests.set(key, true);
                    const expectResponse = () => {
                        if (this.p9Requests.has(key))
                            this.hints.add(key);
                    };
                    let outcomePromise;
                    try {
                        outcomePromise = current.session.request(
                            request, replyCapacity, expectResponse,
                        );
                        if (!outcomePromise || typeof outcomePromise.then !== "function")
                            throw new TypeError("9p session request must return a promise");
                    } catch (error) {
                        outcomePromise = Promise.reject(error);
                    }
                    outcomePromise.then((outcome) => {
                        // Only promise settlement supplies a reply or suppression result.
                        if (outcome?.kind === "suppressed")
                            return { outcome: 1, bytes: new Uint8Array() };
                        if (outcome?.kind === "reply" &&
                            outcome.bytes instanceof Uint8Array &&
                            outcome.bytes.length <= replyCapacity)
                            return { outcome: 0, bytes: outcome.bytes };
                        else
                            throw new TypeError("9p session returned an invalid outcome");
                    }).catch((error) => {
                        const active = this.p9Sessions.get(endpoint);
                        if (active?.generation === generation) {
                            this.p9Sessions.delete(endpoint);
                            active.session.close();
                        }
                        if (this.p9Requests.has(key))
                            this.completeP9(endpoint, generation, requestId, 2);
                        this.retireP9(endpoint, generation);
                        this.options.onError?.(error);
                        return null;
                    }).then((completion) => {
                        if (completion && this.p9Requests.has(key))
                            this.completeP9(
                                endpoint, generation, requestId,
                                completion.outcome, completion.bytes,
                            );
                    }).catch((error) => {
                        this.options.onError?.(error);
                    });
                } else if (kind === 9) {
                    const endpoint = this.exports.riscbox_action_endpoint();
                    const generation = this.exports.riscbox_action_generation();
                    const current = this.p9Sessions.get(endpoint);
                    if (current?.generation === generation) {
                        this.retireP9(endpoint, generation);
                        this.p9Sessions.delete(endpoint);
                        current.session.close();
                    }
                } else {
                    throw new Error(`unknown Riscbox host action ${kind}`);
                }
            }
        }

        completeP9(endpoint, generation, requestId, outcome, bytes = new Uint8Array()) {
            // A late completion clears its hint and replaces a sleeping turn timer.
            const key = `${endpoint}:${generation}:${requestId}`;
            this.p9Requests.delete(key);
            if (this.hints.delete(key))
                this.settledHints++;
            const result = this.withBytes(bytes, (ptr, len) =>
                this.exports.riscbox_p9_complete(
                    endpoint, generation, requestId, outcome, ptr, len,
                ));
            if (result !== 0)
                throw new Error(`Riscbox rejected 9p completion ${requestId}`);
            this.drainActions();
            this.schedule(0);
        }

        retireP9(endpoint, generation) {
            const prefix = `${endpoint}:${generation}:`;
            for (const key of this.p9Requests.keys()) {
                if (key.startsWith(prefix)) {
                    this.p9Requests.delete(key);
                    this.hints.delete(key);
                }
            }
        }

        consoleInput(data) {
            return this.withBytes(data, (ptr, len) =>
                this.exports.riscbox_console_input(ptr, len));
        }

        consoleResize(width, height) {
            return this.exports.riscbox_console_resize(width, height);
        }

        keyEvent(isDown, keyCode) {
            return this.exports.riscbox_key_event(isDown ? 1 : 0, keyCode);
        }

        pointerEvent(x, y, buttons) {
            return this.exports.riscbox_pointer_event(x, y, buttons);
        }

        wheelEvent(delta) {
            return this.exports.riscbox_wheel_event(delta);
        }

        networkInput(packet) {
            return this.withBytes(packet, (ptr, len) =>
                this.exports.riscbox_network_input(ptr, len));
        }

        networkCarrier(up) {
            return this.exports.riscbox_network_carrier(up ? 1 : 0);
        }

        ccall(name, returnType, argumentTypes, args = []) {
            if (name !== "vm_start")
                throw new Error(`unsupported compatibility call: ${name}`);
            if (returnType !== null || argumentTypes.length !== 6 || args.length !== 6)
                throw new TypeError("vm_start compatibility signature mismatch");
            return this.start(...args);
        }
    }

    root.Riscbox = Riscbox;
    if (typeof module === "object" && module.exports)
        module.exports = { Riscbox };
}(globalThis));
