(function (root) {
    "use strict";

    const encoder = new TextEncoder();
    const decoder = new TextDecoder();
    // Guest timer deadlines use 10 MHz ticks; host wakeup delays use milliseconds.
    const GUEST_TICKS_PER_MILLISECOND = 10_000;
    const QUANTUM_BUDGET_REACHED = 0;
    const QUANTUM_WFI_SLEEP = 1;
    const QUANTUM_HOST_SERVICE_REQUIRED = 2;
    const QUANTUM_VM_INACTIVE = 3;
    const QUANTUM_START_VM_INACTIVE = -1;

    class Riscbox {
        constructor(exports, options = {}) {
            if (!(exports.memory instanceof WebAssembly.Memory))
                throw new TypeError("Riscbox WASM must export memory");
            if (typeof exports.riscbox_configure_quantum !== "function" ||
                typeof exports.riscbox_wake_delay_ms !== "function" ||
                typeof exports.riscbox_quantum_begin !== "function" ||
                typeof exports.riscbox_quantum_run !== "function" ||
                typeof exports.riscbox_quantum_finish !== "function" ||
                typeof exports.riscbox_quantum_abort !== "function" ||
                typeof exports.riscbox_timing_stat !== "function")
                throw new TypeError("Riscbox WASM has an incompatible run interface");
            if (Object.hasOwn(options, "timesliceMs"))
                throw new TypeError("timesliceMs has been renamed to targetQuantumMs");
            const targetQuantumMs = options.targetQuantumMs ?? 50;
            if (!Number.isFinite(targetQuantumMs) || targetQuantumMs <= 0 || targetQuantumMs > 100)
                throw new RangeError("targetQuantumMs must be greater than zero and at most 100");
            if (Object.hasOwn(options, "guestClockSkew"))
                throw new TypeError("guestClockSkew is now adaptive and cannot be configured");
            this.exports = exports;
            this.options = options;
            if (exports.riscbox_configure_quantum(targetQuantumMs, options.debugTiming ? 1 : 0) !== 0)
                throw new Error("Riscbox WASM timing configuration failed");
            this.timing = options.debugTiming ? {
                nextReport: performance.now() + 1_000,
                quanta: 0, cpuRuns: 0, cycles: 0, activeMs: 0,
                timerReprogrammingExits: 0, wfiQuanta: 0, wfiMs: 0,
                catchUpWaits: 0, catchUpMs: 0,
                timerIntervals: [],
                intervalNoCatchUpSkew: 0,
                sessionCatchUpSkews: [],
            } : null;
            this.p9Sessions = new Map();
            this.p9Requests = new Map();
            this.hints = new Set();
            this.settledHints = 0;
            this.quantumRunning = false;
            this.wakeupTimer = null;
            this.wakeupToken = 0;
            this.wakeupChannel = null;
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

        scheduleWakeup(delay) {
            // A guest-clock lead delays the next browser task until host time catches up.
            if (this.quantumRunning)
                return;
            const now = Date.now();
            const adjusted = this.exports.riscbox_wake_delay_ms(
                now >>> 0, Math.floor(now / 0x1_0000_0000) >>> 0, delay,
            );
            if (this.timing && adjusted > delay) {
                this.timing.catchUpWaits++;
                this.timing.catchUpMs += adjusted - delay;
            }
            this.cancelWakeup();
            const token = this.wakeupToken;
            if (adjusted === 0) {
                // A message is a new browser task without nested timer clamping.
                if (this.wakeupChannel === null) {
                    const channel = new MessageChannel();
                    channel.port1.onmessage = ({ data }) => {
                        if (data !== this.wakeupToken)
                            return;
                        void this.runQuantum().catch((error) => this.options.onError?.(error));
                    };
                    channel.port1.unref?.();
                    channel.port2.unref?.();
                    this.wakeupChannel = channel;
                }
                this.wakeupChannel.port2.postMessage(token);
            } else {
                this.wakeupTimer = setTimeout(() => {
                    this.wakeupTimer = null;
                    if (token === this.wakeupToken)
                        void this.runQuantum().catch((error) => this.options.onError?.(error));
                }, adjusted);
            }
        }

        cancelWakeup() {
            this.wakeupToken++;
            if (this.wakeupTimer !== null) {
                clearTimeout(this.wakeupTimer);
                this.wakeupTimer = null;
            }
        }

        reportTiming(now) {
            // Only diagnostic mode accumulates and formats interval statistics.
            const timing = this.timing;
            if (!timing || now < timing.nextReport)
                return;
            const cpuRunsPerQuantum = timing.quanta ? timing.cpuRuns / timing.quanta : 0;
            const activeMCyclesPerSecond = timing.activeMs
                ? timing.cycles / timing.activeMs / 1_000 : 0;
            const intervals = timing.timerIntervals;
            const medianTicks = intervals.length
                ? intervals.slice().sort((a, b) => a - b)[Math.floor(intervals.length / 2)] : 0;
            const skews = timing.sessionCatchUpSkews.slice().sort((a, b) => a - b);
            const skewPercentile = (fraction) => skews.length
                ? skews[Math.ceil(fraction * skews.length) - 1] * 100 : 0;
            console.log("Riscbox timing", {
                estimatedEmulatedMCyclesPerSecond: this.exports.riscbox_timing_stat(0) / 1_000_000,
                activeEmulatedMCyclesPerSecond: activeMCyclesPerSecond,
                cpuRunsPerQuantum, timerReprogrammingExits: timing.timerReprogrammingExits,
                medianTimerIntervalMs: medianTicks / GUEST_TICKS_PER_MILLISECOND,
                wfiQuanta: timing.wfiQuanta, wfiMs: timing.wfiMs,
                catchUpWaits: timing.catchUpWaits, catchUpMs: timing.catchUpMs,
                adaptiveGuestClockSkewPercent: this.exports.riscbox_timing_stat(6) * 100,
                intervalNoCatchUpSkewPercent: timing.intervalNoCatchUpSkew * 100,
                sessionPotentialCatchUpSkewP50Percent: skewPercentile(0.50),
                sessionPotentialCatchUpSkewP90Percent: skewPercentile(0.90),
                sessionPotentialCatchUpSkewP99Percent: skewPercentile(0.99),
            });
            timing.nextReport = now + 5_000;
            timing.quanta = timing.cpuRuns = timing.cycles = timing.activeMs = 0;
            timing.timerReprogrammingExits = timing.wfiQuanta = timing.wfiMs = 0;
            timing.catchUpWaits = timing.catchUpMs = 0;
            timing.timerIntervals = [];
            timing.intervalNoCatchUpSkew = 0;
        }

        async runQuantum() {
            // One quantum may pause for host service and resume through microtasks.
            if (this.quantumRunning)
                return;
            const now = Date.now();
            const begin = this.exports.riscbox_quantum_begin(
                now >>> 0, Math.floor(now / 0x1_0000_0000) >>> 0,
            );
            if (begin === QUANTUM_START_VM_INACTIVE)
                return;
            if (begin < QUANTUM_START_VM_INACTIVE)
                throw new Error(`Riscbox WASM could not begin a quantum: ${begin}`);
            if (begin > 0) {
                this.scheduleWakeup(begin);
                return;
            }
            this.cancelWakeup();
            if (this.timing && this.wfiStartedAt !== undefined) {
                this.timing.wfiMs += performance.now() - this.wfiStartedAt;
                this.wfiStartedAt = undefined;
            }
            this.quantumRunning = true;
            const startedAt = performance.now();
            let nextDelay = 0;
            let wfiSleep = false;
            let vmInactive = false;
            let completed = false;
            try {
                for (;;) {
                    const reason = this.exports.riscbox_quantum_run();
                    if (reason < 0 || reason > QUANTUM_VM_INACTIVE)
                        throw new Error(`Riscbox WASM returned invalid quantum outcome ${reason}`);
                    this.drainActions();
                    if (reason === QUANTUM_BUDGET_REACHED ||
                        reason === QUANTUM_WFI_SLEEP || reason === QUANTUM_VM_INACTIVE) {
                        wfiSleep = reason === QUANTUM_WFI_SLEEP;
                        vmInactive = reason === QUANTUM_VM_INACTIVE;
                        break;
                    }
                    if (reason === QUANTUM_HOST_SERVICE_REQUIRED) {
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
                const end = Date.now();
                nextDelay = this.exports.riscbox_quantum_finish(
                    performance.now() - startedAt,
                    end >>> 0, Math.floor(end / 0x1_0000_0000) >>> 0,
                );
                if (nextDelay < 0)
                    throw new Error("Riscbox WASM could not finish the quantum");
                completed = true;
                if (this.timing) {
                    const timing = this.timing;
                    timing.quanta++;
                    timing.cpuRuns += this.exports.riscbox_timing_stat(1);
                    timing.timerReprogrammingExits += this.exports.riscbox_timing_stat(2);
                    timing.cycles += this.exports.riscbox_timing_stat(4);
                    const interval = this.exports.riscbox_timing_stat(3);
                    if (interval > 0 && timing.timerIntervals.length < 10_000)
                        timing.timerIntervals.push(interval);
                    if (!wfiSleep && !vmInactive) {
                        const requiredSkew = this.exports.riscbox_timing_stat(5);
                        if (requiredSkew > 0) {
                            timing.intervalNoCatchUpSkew = Math.max(
                                timing.intervalNoCatchUpSkew, requiredSkew,
                            );
                            timing.sessionCatchUpSkews.push(requiredSkew);
                        }
                    }
                    timing.activeMs += performance.now() - startedAt;
                    if (wfiSleep) timing.wfiQuanta++;
                    this.reportTiming(performance.now());
                }
            } finally {
                if (!completed)
                    this.exports.riscbox_quantum_abort();
                this.quantumRunning = false;
            }
            if (wfiSleep && this.timing)
                this.wfiStartedAt = performance.now();
            if (!vmInactive)
                this.scheduleWakeup(nextDelay);
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
                            this.scheduleWakeup(0);
                    }).catch((error) => this.options.onError?.(error));
                } else if (kind === 2) {
                    this.started = true;
                    this.options.onVmStarted?.();
                    this.scheduleWakeup(0);
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
            // A late completion clears its hint and replaces a WFI wakeup timer.
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
            this.scheduleWakeup(0);
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
