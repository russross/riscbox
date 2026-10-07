(function (root) {
    "use strict";

    // STORAGE_IMPLEMENTATION

    const runtimeEncoder = new TextEncoder();
    const textDecoder = new TextDecoder();
    // Capture the adapter's location while its script is executing, before application calls.
    const adapterUrl = typeof document === "object" ? document.currentScript?.src : undefined;
    const defaultWasmUrl = adapterUrl ? new URL("riscbox.wasm", adapterUrl).href : undefined;
    // Guest timer deadlines use 10 MHz ticks; host wakeup delays use milliseconds.
    const GUEST_TICKS_PER_MILLISECOND = 10_000;
    const QUANTUM_BUDGET_REACHED = 0;
    const QUANTUM_WFI_SLEEP = 1;
    const QUANTUM_HOST_SERVICE_REQUIRED = 2;
    const QUANTUM_VM_INACTIVE = 3;
    const QUANTUM_START_VM_INACTIVE = -1;
    const LIFECYCLE_CAUSES = [
        "guest-poweroff", "guest-reboot", "host-halt", "host-reset",
        "host-boot", "guest-failure",
    ];

    function parseConfig(source) {
        let offset = 0;
        const fail = (message) => { throw new SyntaxError(`configuration: ${message} at offset ${offset}`); };
        const space = () => {
            for (;;) {
                while (/\s/.test(source[offset] ?? "") && offset < source.length) offset++;
                if (source.startsWith("//", offset)) {
                    offset = source.indexOf("\n", offset + 2);
                    if (offset < 0) offset = source.length;
                } else if (source.startsWith("/*", offset)) {
                    const end = source.indexOf("*/", offset + 2);
                    if (end < 0) fail("unterminated comment");
                    offset = end + 2;
                } else return;
            }
        };
        const quoted = () => {
            offset++;
            let result = "";
            for (;;) {
                const ch = source[offset++];
                if (ch === undefined || ch === "\n" || ch === "\r") fail("unterminated string");
                if (ch === '"') return result;
                if (ch !== "\\") { result += ch; continue; }
                const escape = source[offset++];
                if (escape === "x") {
                    const hex = source.slice(offset, offset + 2);
                    if (!/^[0-9a-fA-F]{2}$/.test(hex)) fail("invalid hex escape");
                    result += String.fromCharCode(parseInt(hex, 16));
                    offset += 2;
                } else if (escape === "n") result += "\n";
                else if (escape === "r") result += "\r";
                else if (escape === "t") result += "\t";
                else if (escape === "\\" || escape === '"' || escape === "'") result += escape;
                else fail("unknown escape code");
            }
        };
        const identifier = () => {
            const match = /^[A-Za-z_$][A-Za-z0-9_$]*/.exec(source.slice(offset));
            if (!match) fail("invalid identifier");
            offset += match[0].length;
            return match[0];
        };
        const value = () => {
            space();
            if (source[offset] === '"') return quoted();
            if (source[offset] === "{") {
                offset++;
                const object = Object.create(null);
                space();
                while (source[offset] !== "}") {
                    const key = source[offset] === '"' ? quoted() : identifier();
                    space();
                    if (source[offset++] !== ":") fail("expected ':'");
                    object[key] = value();
                    space();
                    if (source[offset] === "}") break;
                    if (source[offset++] !== ",") fail("expected ','");
                    space();
                }
                offset++;
                return object;
            }
            if (source[offset] === "[") {
                offset++;
                const array = [];
                space();
                while (source[offset] !== "]") {
                    array.push(value());
                    space();
                    if (source[offset] === "]") break;
                    if (source[offset++] !== ",") fail("expected ','");
                    space();
                }
                offset++;
                return array;
            }
            const number = /^(?:0[xX][0-9a-fA-F]+|0[0-7]*|[1-9][0-9]*)/.exec(source.slice(offset));
            if (number) {
                offset += number[0].length;
                return number[0].startsWith("0") && !/^0[xX]/.test(number[0])
                    ? parseInt(number[0], 8) : Number(number[0]);
            }
            const word = identifier();
            if (word === "true") return true;
            if (word === "false") return false;
            if (word === "null") return null;
            fail("unknown identifier");
        };
        const result = value();
        space();
        if (offset !== source.length) fail("unexpected characters after value");
        return result;
    }

    function resolveConfig(source, baseUrl, commandLine = "") {
        if (!source || typeof source !== "object" || Array.isArray(source))
            throw new TypeError("configuration must be an object");
        const required = (name, type) => {
            if (typeof source[name] !== type) throw new TypeError(`${name} must be ${type}`);
            return source[name];
        };
        const optional = (name, type, fallback) => {
            if (source[name] === undefined) return fallback;
            return required(name, type);
        };
        const path = (name) => {
            const value = optional(name, "string", null);
            return value === null || !baseUrl ? value : new URL(value, baseUrl).href;
        };
        if (required("version", "number") !== 1) throw new RangeError("unsupported configuration version");
        const resolved = {
            version: 1,
            machine: required("machine", "string"),
            memory_size: required("memory_size", "number"),
            console: optional("console", "string", "virtio"),
            uart_output: optional("uart_output", "boolean", false),
            rtc_local_time: optional("rtc_local_time", "boolean", false),
            cmdline: optional("cmdline", "string", ""),
        };
        if (resolved.machine !== "riscv64") throw new RangeError("machine must be riscv64");
        integer("memory_size", resolved.memory_size, 1, 0x7fff_ffff);
        if (resolved.console !== "virtio" && resolved.console !== "uart")
            throw new RangeError("console must be virtio or uart");
        if (commandLine) resolved.cmdline = commandLine.startsWith("!")
            ? commandLine.slice(1) : `${resolved.cmdline} ${commandLine}`;
        for (const name of ["bios", "kernel", "initrd"]) {
            const url = path(name);
            if (url !== null) resolved[name] = url;
        }
        for (const name of ["bios_address", "kernel_address", "initrd_address", "fdt_address"]) {
            if (source[name] !== undefined) resolved[name] = source[name];
        }
        for (const [prefix, limit] of [["drive", 4], ["fs", 4], ["eth", 1]]) {
            if (source[`${prefix}${limit}`] !== undefined)
                throw new RangeError(`too many ${prefix} entries`);
            for (let index = 0; index < limit; index++) {
                const name = `${prefix}${index}`;
                if (source[name] === undefined) {
                    for (let following = index + 1; following < limit; following++) {
                        if (source[`${prefix}${following}`] !== undefined)
                            throw new RangeError(`${prefix} entries must be consecutive`);
                    }
                    break;
                }
                const entry = source[name];
                if (!entry || typeof entry !== "object" || Array.isArray(entry))
                    throw new TypeError(`${name} must be an object`);
                if (prefix === "fs" && (typeof entry.server !== "string" || !entry.server ||
                    typeof entry.tag !== "string" || !entry.tag))
                    throw new TypeError(`${name} needs nonempty server and tag strings`);
                if (prefix === "eth" && entry.driver !== "user")
                    throw new TypeError(`${name} driver must be user`);
                resolved[name] = prefix === "drive"
                    ? (entry.capacity_sectors === undefined && entry.bytes === undefined && entry.provider === undefined
                        ? { ...entry, file: resolveConfigPath(entry.file, baseUrl) }
                        : resolveArrayDrive(entry))
                    : entry;
            }
        }
        for (const name of ["display0", "input_device"]) {
            if (source[name] !== undefined) resolved[name] = source[name];
        }
        if (resolved.input_device !== undefined && resolved.input_device !== "virtio")
            throw new RangeError("input_device must be virtio");
        if (resolved.display0 !== undefined) {
            if (resolved.display0?.device !== "simplefb") throw new TypeError("display0 device must be simplefb");
            integer("display width", resolved.display0.width, 1, 0x7fff_ffff);
            integer("display height", resolved.display0.height, 1, 0x7fff_ffff);
        }
        return resolved;
    }

    function resolveConfigPath(value, baseUrl) {
        if (typeof value !== "string") throw new TypeError("drive file must be a string");
        return baseUrl ? new URL(value, baseUrl).href : value;
    }

    function resolveArrayDrive(entry) {
        if (entry.provider !== undefined) throw new TypeError("unsupported array drive property provider");
        const bytes = entry.bytes;
        if (bytes !== undefined && (!(bytes instanceof Uint8Array) || bytes.length === 0 || bytes.length % 512 !== 0))
            throw new TypeError("array drives need a whole-sector Uint8Array");
        const capacity = entry.capacity_sectors ?? (bytes ? BigInt(bytes.length / 512) : undefined);
        if (typeof capacity === "number" && !Number.isSafeInteger(capacity) ||
            !["number", "bigint", "string"].includes(typeof capacity) || entry.file !== undefined)
            throw new TypeError("array drive needs sector capacity or initial bytes");
        if (bytes && BigInt(capacity) !== BigInt(bytes.length / 512)) throw new RangeError("array capacity does not match bytes");
        return { capacity_sectors: String(capacity), ...(bytes === undefined ? {} : { bytes }) };
    }

    class RiscboxRuntime {
        constructor(exports, options = {}) {
            if (!(exports.memory instanceof WebAssembly.Memory))
                throw new TypeError("Riscbox WASM must export memory");
            if (typeof exports.riscbox_configure_quantum !== "function" ||
                typeof exports.riscbox_wake_delay_ms !== "function" ||
                typeof exports.riscbox_quantum_begin !== "function" ||
                typeof exports.riscbox_quantum_run !== "function" ||
                typeof exports.riscbox_quantum_finish !== "function" ||
                typeof exports.riscbox_quantum_abort !== "function" ||
                typeof exports.riscbox_timing_stat !== "function" ||
                typeof exports.riscbox_speed_stat !== "function")
                throw new TypeError("Riscbox WASM has an incompatible run interface");
            const targetQuantumMs = options.targetQuantumMs ?? 20;
            if (!Number.isFinite(targetQuantumMs) || targetQuantumMs <= 0 || targetQuantumMs > 100)
                throw new RangeError("targetQuantumMs must be greater than zero and at most 100");
            this.exports = exports;
            this.options = options;
            if (exports.riscbox_configure_quantum(targetQuantumMs, options.debugTiming ? 1 : 0) !== 0)
                throw new Error("Riscbox WASM timing configuration failed");
            this.timing = options.debugTiming ? {
                nextReport: performance.now() + 1_000,
                quanta: 0, cpuRuns: 0, cycles: 0, activeMs: 0,
                timerReprogrammingExits: 0, wfiQuanta: 0, wfiMs: 0,
                carriedGuestMs: 0,
                rateSamples: 0, rateMean: 0, rateM2: 0,
                timerIntervals: [],
                intervalNoCatchUpSkew: 0,
                sessionCatchUpSkews: [],
            } : null;
            this.filesystems = new Map();
            this.servicingFilesystems = false;
            this.quantumRunning = false;
            this.wakeupTimer = null;
            this.wakeupToken = 0;
            this.wakeupChannel = null;
            this.started = false;
            this.state = "empty";
            this.bootStartedAt = null;
            this.haltedAt = null;
            this.pendingHaltNotification = null;
            this.consoleDecoder = new TextDecoder();
            this.pendingControls = [];
            this.disks = new Map();
            this.diskCount = 0;
            this.preparation = null;
            this.httpGeneration = 0;
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
                    runtime.writeConsole(bytes(ptr, len));
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
                    runtime = new RiscboxRuntime(exports, options);
                    return runtime;
                },
            };
        }

        static async instantiate(source, options = {}) {
            const host = RiscboxRuntime.hostImports(options);
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
            const data = typeof value === "string" ? runtimeEncoder.encode(value) : value;
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

        prepareResolved(config, ramMiB = 0, width = 0, height = 0, hasNetwork = false) {
            if (this.preparation) throw new Error("VM preparation is already active");
            const resolved = resolveConfig(config, null);
            this.config = resolved;
            this.hasNetwork = hasNetwork;
            const initial = [];
            let diskCount = 0;
            for (let index = 0; index < 4; index++) {
                const drive = resolved[`drive${index}`];
                if (!drive) break;
                diskCount++;
                if (drive.bytes !== undefined) {
                    initial.push([index, drive.bytes.slice()]);
                    delete drive.bytes;
                }
            }
            return new Promise((resolve, reject) => {
                this.preparation = { resolve, reject, initial, diskCount };
                try {
                    const result = this.withBytes(JSON.stringify(resolved), (ptr, length) =>
                        this.exports.riscbox_prepare_resolved(ptr, length, ramMiB, width, height, hasNetwork ? 1 : 0));
                    if (result !== 0) throw new Error("Riscbox rejected resolved configuration");
                    this.drainActions();
                } catch (error) { this.failPreparation(error); }
            });
        }

        filesystem(name) { return FilesystemHandle.open(this, name); }

        block(index) {
            if (!Number.isInteger(index) || index < 0 || index >= this.diskCount) throw new RangeError("unknown disk index");
            if (!this.disks.has(index)) this.disks.set(index, new DiskHandle(this, index));
            return this.disks.get(index);
        }

        failPreparation(error) {
            const preparation = this.preparation;
            this.preparation = null;
            preparation?.reject(error);
        }

        control(name, allowedStates) {
            return new Promise((resolve, reject) => {
                const run = () => {
                    try {
                        if (allowedStates && !allowedStates.includes(this.state))
                            throw new Error(`${name} requires ${allowedStates.join(" or ")} VM; current state is ${this.state}`);
                        const command = this.exports[`riscbox_${name}`];
                        if (typeof command !== "function" || command() !== 0)
                            throw new Error(`Riscbox could not ${name} the VM`);
                        if (name === "reset" || name === "request_shutdown" ||
                            name === "request_reboot") this.scheduleWakeup(0);
                        if (name === "cold_reset") {
                            this.httpGeneration++;
                            this.consoleDecoder = new TextDecoder();
                            this.options.consoleReset?.();
                            this.options.framebufferClear?.();
                            this.serviceStorage();
                        }
                        if (name === "destroy") {
                            this.failPreparation(new Error("VM preparation was cancelled"));
                            for (const filesystem of this.filesystems.values()) filesystem.invalidate();
                            for (const disk of this.disks.values()) disk.invalidate();
                            this.filesystems.clear(); this.disks.clear(); this.diskCount = 0;
                            this.httpGeneration++;
                            this.started = false;
                            this.state = "empty";
                            this.consoleDecoder = new TextDecoder();
                            this.config = null;
                            this.cancelWakeup();
                            this.options.onVmDestroyed?.();
                        }
                        this.drainActions();
                        resolve();
                    } catch (error) {
                        reject(error);
                    }
                };
                if (this.quantumRunning) this.pendingControls.push(run);
                else run();
            });
        }

        halt() { return this.control("halt"); }
        reset() { return this.control("reset"); }
        coldReset() { return this.control("cold_reset"); }
        boot() { return this.control("reset"); }
        destroy() { return this.control("destroy"); }
        requestShutdown() { return this.control("request_shutdown"); }
        requestReboot() { return this.control("request_reboot"); }

        scheduleWakeup(delay) {
            // The machine preserves the requested wake delay between quanta.
            if (this.quantumRunning)
                return;
            const now = Date.now();
            const adjusted = this.exports.riscbox_wake_delay_ms(
                now >>> 0, Math.floor(now / 0x1_0000_0000) >>> 0, delay,
            );
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
            const rateStdDev = timing.rateSamples > 0
                ? Math.sqrt(timing.rateM2 / timing.rateSamples) : 0;
            const intervals = timing.timerIntervals;
            const medianTicks = intervals.length
                ? intervals.slice().sort((a, b) => a - b)[Math.floor(intervals.length / 2)] : 0;
            const skews = timing.sessionCatchUpSkews.slice().sort((a, b) => a - b);
            const skewPercentile = (fraction) => skews.length
                ? skews[Math.ceil(fraction * skews.length) - 1] * 100 : 0;
            console.log("Riscbox timing", {
                estimatedEmulatedMCyclesPerSecond: this.exports.riscbox_timing_stat(0) / 1_000_000,
                intervalEmulatedMCyclesPerSecondMean: timing.rateMean,
                intervalEmulatedMCyclesPerSecondStdDev: rateStdDev,
                activeEmulatedMCyclesPerSecond: activeMCyclesPerSecond,
                cpuRunsPerQuantum, timerReprogrammingExits: timing.timerReprogrammingExits,
                medianTimerIntervalMs: medianTicks / GUEST_TICKS_PER_MILLISECOND,
                wfiQuanta: timing.wfiQuanta, wfiMs: timing.wfiMs,
                carriedGuestMs: timing.carriedGuestMs,
                adaptiveGuestClockSkewPercent: this.exports.riscbox_timing_stat(6) * 100,
                intervalNoCatchUpSkewPercent: timing.intervalNoCatchUpSkew * 100,
                sessionPotentialCatchUpSkewP50Percent: skewPercentile(0.50),
                sessionPotentialCatchUpSkewP90Percent: skewPercentile(0.90),
                sessionPotentialCatchUpSkewP99Percent: skewPercentile(0.99),
            });
            timing.nextReport = now + 5_000;
            timing.quanta = timing.cpuRuns = timing.cycles = timing.activeMs = 0;
            timing.timerReprogrammingExits = timing.wfiQuanta = timing.wfiMs = 0;
            timing.carriedGuestMs = 0;
            timing.rateSamples = timing.rateMean = timing.rateM2 = 0;
            timing.timerIntervals = [];
            timing.intervalNoCatchUpSkew = 0;
        }

        async runQuantum() {
            // Host service drains copied queues before the same quantum resumes.
            if (this.quantumRunning)
                return;
            const now = Date.now();
            const begin = this.exports.riscbox_quantum_begin(
                now >>> 0, Math.floor(now / 0x1_0000_0000) >>> 0,
            );
            if (begin === QUANTUM_START_VM_INACTIVE)
                return;
            if (begin !== 0)
                throw new Error(`Riscbox WASM could not begin a quantum: ${begin}`);
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

                }
                const end = Date.now();
                const elapsedMs = performance.now() - startedAt;
                nextDelay = this.exports.riscbox_quantum_finish(
                    elapsedMs,
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
                    const cycles = this.exports.riscbox_timing_stat(4);
                    timing.cycles += cycles;
                    timing.carriedGuestMs = this.exports.riscbox_timing_stat(7)
                        / GUEST_TICKS_PER_MILLISECOND;
                    if (!wfiSleep && !vmInactive && elapsedMs > 0 && cycles > 0) {
                        const rate = cycles / elapsedMs / 1_000;
                        timing.rateSamples++;
                        const delta = rate - timing.rateMean;
                        timing.rateMean += delta / timing.rateSamples;
                        timing.rateM2 += delta * (rate - timing.rateMean);
                    }
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
                    if (!vmInactive && this.started)
                        this.reportTiming(performance.now());
                }
            } finally {
                if (!completed)
                    this.exports.riscbox_quantum_abort();
                // Publish guest halt only after the final quantum sample has
                // reached Rust, so callbacks can read the frozen statistics.
                const haltCause = this.pendingHaltNotification;
                this.pendingHaltNotification = null;
                try {
                    if (haltCause !== null) {
                        this.haltedAt = performance.now();
                        this.options.onVmHalted?.(haltCause);
                    }
                } finally {
                    this.quantumRunning = false;
                    for (const control of this.pendingControls.splice(0)) control();
                }
            }
            if (wfiSleep && this.timing)
                this.wfiStartedAt = performance.now();
            if (!vmInactive && this.started)
                this.scheduleWakeup(nextDelay);
        }

        drainActions() {
            if (!this.exports.riscbox_next_action)
                return;
            let serviced = false;
            for (;;) {
                const kind = this.exports.riscbox_next_action();
                if (kind === 0) {
                    if (serviced) return;
                    if (!this.serviceStorage()) return;
                    serviced = true;
                    continue;
                }
                serviced = false;
                const value = this.exports.riscbox_action_value();
                const ptr = this.exports.riscbox_action_data_address();
                const len = this.exports.riscbox_action_data_length();
                if (kind === 15) {
                    const preparation = this.preparation;
                    if (!preparation) throw new Error("unexpected prepared platform");
                    try {
                        this.diskCount = preparation.diskCount;
                        for (const [index, bytes] of preparation.initial) this.block(index).write(0n, bytes);
                        this.preparation = null;
                        this.state = "halted";
                        preparation.resolve();
                    } catch (error) { this.failPreparation(error); }
                } else if (kind === 1) {
                    const generation = this.httpGeneration;
                    const url = textDecoder.decode(this.bytes(ptr, len));
                    const fetchRequest = this.options.fetch ?? globalThis.fetch;
                    if (typeof fetchRequest !== "function")
                        throw new Error("Riscbox HTTP fetch is not available");
                    const hashedPath = /(?:^|\/)[^/?#]*-[0-9a-f]{8,64}(?:\.|\/|[?#]|$)/i.test(url);
                    const cache = hashedPath ? "force-cache" : "default";
                    const disk = this.exports.riscbox_action_disk();
                    let completed = false;
                    const load = async () => {
                        if (disk !== 0 && this.options.fetchBlock) {
                            const data = await this.options.fetchBlock({ disk: disk - 1, url, cache });
                            if (!(data instanceof Uint8Array)) throw new TypeError("fetchBlock must return Uint8Array");
                            return { data, status: 200 };
                        }
                        const response = await fetchRequest(url, { cache, signal: this.preparationSignal });
                        return { data: new Uint8Array(await response.arrayBuffer()), status: response.status ?? 200 };
                    };
                    load().then(({ data, status }) => {
                        if (generation !== this.httpGeneration) return;
                        this.withBytes(data, (dataPtr, dataLen) => {
                            const result = this.exports.riscbox_http_complete(
                                value, status, dataPtr, dataLen,
                            );
                            completed = true;
                            if (result !== 0)
                                throw new Error(`Riscbox rejected HTTP response ${value}`);
                        });
                        this.drainActions();
                        if (this.started)
                            this.scheduleWakeup(0);
                    }).catch((error) => {
                        if (generation !== this.httpGeneration) return;
                        if (disk !== 0 && !completed) {
                            const result = this.exports.riscbox_http_complete(value, 500, 0, 0);
                            this.drainActions();
                            if (this.started) this.scheduleWakeup(0);
                            if (result === 0) return;
                        }
                        this.failPreparation(error);
                        this.options.onError?.(error);
                    });
                } else if (kind === 2) {
                    this.bootStartedAt = performance.now();
                    this.haltedAt = null;
                    this.started = true;
                    this.state = "running";
                    this.options.onVmStarted?.();
                    this.scheduleWakeup(0);
                } else if (kind === 3) {
                    this.writeConsole(this.bytes(ptr, len));
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
                } else if (kind === 10) {
                    this.haltedAt = performance.now();
                    this.httpGeneration++;
                    const cause = LIFECYCLE_CAUSES[value];
                    if (cause === undefined)
                        throw new Error(`invalid VM halt cause ${value}`);
                    this.started = false;
                    this.state = "halted";
                    this.cancelWakeup();
                    const tail = this.consoleDecoder.decode();
                    if (tail) this.options.consoleWrite?.(tail);
                    if (this.quantumRunning) this.pendingHaltNotification = cause;
                    else this.options.onVmHalted?.(cause);
                } else if (kind === 11) {
                    this.bootStartedAt = performance.now();
                    this.haltedAt = null;
                    this.httpGeneration++;
                    const cause = LIFECYCLE_CAUSES[value];
                    if (cause === undefined)
                        throw new Error(`invalid VM reset cause ${value}`);
                    this.started = true;
                    this.state = "running";
                    this.consoleDecoder = new TextDecoder();
                    this.options.consoleReset?.();
                    this.options.framebufferClear?.();
                    this.options.onVmReset?.(cause);
                    if (cause === "host-boot") this.options.onVmStarted?.();
                } else {
                    throw new Error(`unknown Riscbox host action ${kind}`);
                }
            }
        }

        reportFilesystemError(error) {
            if (this.options.onError) this.options.onError(error);
            else console.error(error);
        }

        // Console chunks share one decoder per VM; other strings decode independently.
        writeConsole(bytes) {
            const text = this.consoleDecoder.decode(bytes, { stream: true });
            if (text) this.options.consoleWrite?.(text);
        }

        // Events and disk completions are serviced after WASM releases its borrows.
        serviceStorage() {
            if (this.servicingFilesystems) return false;
            this.servicingFilesystems = true;
            let polled = false;
            try {
                for (const filesystem of this.filesystems.values()) filesystem.poll();
                for (const disk of this.disks.values()) polled = disk.poll() || polled;
            } finally { this.servicingFilesystems = false; }
            return polled;
        }

        filesystemChanged() {
            this.serviceStorage();
            if (this.started && !this.quantumRunning) this.scheduleWakeup(0);
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

    }

    // The embedding boundary owns its runtime privately; scheduling stays automatic.
    class Riscbox {
        #runtime;
        static FilesystemError = FilesystemError;
        static BlockError = BlockError;
        constructor(runtime) {
            if (!(runtime instanceof RiscboxRuntime)) throw new TypeError("use Riscbox.prepare()");
            this.#runtime = runtime;
        }
        static async prepare(options) {
            if (!options || typeof options !== "object" || Array.isArray(options))
                throw new TypeError("preparation options must be an object");
            // Runtime options are copied separately from the inputs used only during construction.
            const names = ["fetch", "fetchBlock", "targetQuantumMs", "debugTiming", "consoleWrite",
                "consoleReset", "onVmStarted", "onVmHalted", "onVmReset", "onVmDestroyed",
                "onError", "networkWrite", "framebufferClear", "framebufferRefresh"];
            const setupNames = ["config", "wasmUrl", "blocks", "ramMiB", "width", "height",
                "hasNetwork", "commandLine", "signal"];
            const runtimeOptions = {};
            for (const name of Object.keys(options)) {
                if (setupNames.includes(name)) continue;
                if (!names.includes(name)) throw new TypeError(`unknown Riscbox option ${name}`);
                if (name !== "targetQuantumMs" && name !== "debugTiming" && typeof options[name] !== "function")
                    throw new TypeError(`${name} must be a function`);
                runtimeOptions[name] = options[name];
            }
            if (options.debugTiming !== undefined && typeof options.debugTiming !== "boolean")
                throw new TypeError("debugTiming must be boolean");
            // A destruction notification belongs to the client returned after successful preparation.
            const onVmDestroyed = runtimeOptions.onVmDestroyed;
            delete runtimeOptions.onVmDestroyed;
            // Named machine overrides retain configured values when their numeric value is zero.
            const { ramMiB = 0, width = 0, height = 0, hasNetwork = false,
                commandLine = "", signal } = options;
            integer("ramMiB", ramMiB, 0, 0xffff_ffff);
            integer("width", width, 0, 0xffff_ffff);
            integer("height", height, 0, 0xffff_ffff);
            if (typeof hasNetwork !== "boolean") throw new TypeError("hasNetwork must be boolean");
            if (typeof commandLine !== "string") throw new TypeError("commandLine must be string");
            if (signal !== undefined && !(signal instanceof AbortSignal))
                throw new TypeError("signal must be an AbortSignal");
            const fetchRequest = options.fetch ?? globalThis.fetch;
            if (typeof fetchRequest !== "function") throw new Error("Riscbox HTTP fetch is not available");
            const wasmUrl = options.wasmUrl ?? defaultWasmUrl;
            if (!(wasmUrl instanceof URL) && (typeof wasmUrl !== "string" || !wasmUrl))
                throw new TypeError("wasmUrl is required when the adapter has no script URL");

            // Copy application-owned inputs before fetching; each configured slot has one backing source.
            const blocks = copyBlockOverrides(options.blocks);
            const source = configSource(options.config);
            const controller = new AbortController();
            let runtime;
            const cancel = () => {
                controller.abort(signal.reason);
                if (runtime) void runtime.destroy().catch(error => {
                    if (runtimeOptions.onError) runtimeOptions.onError(error);
                    else console.error(error);
                });
            };
            signal?.throwIfAborted();
            signal?.addEventListener("abort", cancel, { once: true });
            try {
                // URL sources fetch text before sharing resolution and block replacement with inline sources.
                const loadConfig = async () => {
                    let value = source.value;
                    if (source.url !== undefined) {
                        const response = await fetchRequest(source.url, { cache: "no-cache", signal: controller.signal });
                        if (!response.ok) throw new Error(`configuration HTTP ${response.status}`);
                        value = parseConfig(await response.text());
                    } else if (source.text !== undefined) value = parseConfig(source.text);
                    const merged = { ...value };
                    for (const [name, drive] of Object.entries(blocks)) {
                        if (merged[name] === undefined) throw new RangeError(`block override ${name} has no configured drive`);
                        merged[name] = drive;
                    }
                    return resolveConfig(merged, source.baseUrl, commandLine);
                };
                // Streaming compilation uses the same validated fetch policy as configuration loading.
                const loadRuntime = async () => {
                    const response = await fetchRequest(wasmUrl, { cache: "no-cache", signal: controller.signal });
                    if (!response.ok) throw new Error(`WASM HTTP ${response.status}`);
                    const host = RiscboxRuntime.hostImports(runtimeOptions);
                    const result = await WebAssembly.instantiateStreaming(response, { riscbox_host: host.imports });
                    runtime = host.attach(result.instance.exports);
                    runtime.state = "preparing";
                    runtime.preparationSignal = controller.signal;
                    return runtime;
                };

                // Both downloads settle before failure cleanup, so a late instance cannot escape ownership.
                const stopOnFailure = promise => promise.catch(error => {
                    controller.abort(error);
                    throw error;
                });
                const results = await Promise.allSettled([stopOnFailure(loadConfig()), stopOnFailure(loadRuntime())]);
                const failure = results.find(result => result.status === "rejected");
                if (failure) throw controller.signal.reason;
                controller.signal.throwIfAborted();
                await runtime.prepareResolved(results[0].value, ramMiB, width, height, hasNetwork);
                controller.signal.throwIfAborted();
                runtime.preparationSignal = undefined;
                const client = new Riscbox(runtime);
                runtime.options.onVmDestroyed = () => {
                    runtime.state = "destroyed";
                    onVmDestroyed?.();
                };
                return client;
            } catch (error) {
                controller.abort();
                if (runtime && runtime.state !== "empty") await runtime.destroy();
                throw error;
            } finally {
                signal?.removeEventListener("abort", cancel);
            }
        }
        get state() { return this.#runtime.state; }
        get started() { return this.#runtime.started; }

        // Completed quanta supply rates and cycling time; the monotonic host
        // clock supplies uptime, frozen at the most recent halt notification.
        speed() {
            this.#requireState("speed", ["halted", "running"]);
            const runtime = this.#runtime;
            const now = runtime.haltedAt ?? performance.now();
            return {
                mcycles1s: runtime.exports.riscbox_speed_stat(0),
                mcycles5s: runtime.exports.riscbox_speed_stat(1),
                mcycles15s: runtime.exports.riscbox_speed_stat(2),
                uptimeSeconds: runtime.bootStartedAt === null ? 0 : (now - runtime.bootStartedAt) / 1_000,
                cyclingSeconds: runtime.exports.riscbox_speed_stat(3),
            };
        }

        // Controls check state again when queued execution reaches the WASM boundary.
        boot() { return this.#runtime.control("reset", ["halted"]); }
        reset() { return this.#runtime.control("reset", ["running"]); }
        halt() { return this.#runtime.control("halt", ["running"]); }
        coldReset() { return this.#runtime.control("cold_reset", ["halted"]); }
        destroy() { return this.#runtime.control("destroy", ["halted"]); }
        requestShutdown() { return this.#runtime.control("request_shutdown", ["running"]); }
        requestReboot() { return this.#runtime.control("request_reboot", ["running"]); }
        filesystem(name) {
            this.#requireState("filesystem", ["halted", "running"]);
            if (typeof name !== "string" || !name) throw new TypeError("filesystem name must be nonempty string");
            return this.#runtime.filesystem(name).client;
        }
        block(index) {
            this.#requireState("block", ["halted", "running"]);
            return this.#runtime.block(index).client;
        }
        #requireState(call, states) {
            if (!states.includes(this.state))
                throw new Error(`${call} requires ${states.join(" or ")} VM; current state is ${this.state}`);
        }
        #requireInput(call) {
            this.#requireState(call, ["running"]);
            if (this.#runtime.config.input_device !== "virtio") throw new Error(`${call} requires VirtIO input`);
        }

        // Input validates values before integer conversion; the console reports backpressure.
        consoleInput(bytes) {
            this.#requireState("consoleInput", ["running"]);
            if (!(bytes instanceof Uint8Array)) throw new TypeError("consoleInput requires Uint8Array");
            return this.#runtime.consoleInput(bytes);
        }
        consoleResize(columns, rows) {
            this.#requireState("consoleResize", ["running"]);
            if (this.#runtime.config.console !== "virtio") throw new Error("consoleResize requires VirtIO console");
            integer("columns", columns, 1, 65535); integer("rows", rows, 1, 65535);
            return this.#runtime.consoleResize(columns, rows);
        }
        keyEvent(down, code) {
            this.#requireInput("keyEvent");
            if (typeof down !== "boolean") throw new TypeError("down must be boolean");
            integer("code", code, 0, 65535);
            return this.#runtime.keyEvent(down, code);
        }
        pointerEvent(x, y, buttons) {
            this.#requireInput("pointerEvent");
            integer("x", x, 0, 0xffff_ffff); integer("y", y, 0, 0xffff_ffff);
            integer("buttons", buttons, 0, 7);
            return this.#runtime.pointerEvent(x, y, buttons);
        }
        wheelEvent(delta) {
            this.#requireInput("wheelEvent");
            integer("delta", delta, -0x8000_0000, 0x7fff_ffff);
            return this.#runtime.wheelEvent(delta);
        }

        // Transport carrier can precede boot; frames are dropped outside execution.
        networkInput(packet) {
            this.#requireState("networkInput", ["halted", "running"]);
            if (!(packet instanceof Uint8Array)) throw new TypeError("networkInput requires Uint8Array");
            if (this.state !== "running" || packet.length === 0 || packet.length > 65535) return 1;
            if (!this.#runtime.hasNetwork || !this.#runtime.config.eth0)
                throw new Error("networkInput requires an enabled network device");
            return this.#runtime.networkInput(packet);
        }
        networkCarrier(up) {
            this.#requireState("networkCarrier", ["halted", "running"]);
            if (typeof up !== "boolean") throw new TypeError("up must be boolean");
            return this.#runtime.networkCarrier(up);
        }
    }
    // Overrides replace backing sources without changing the configured device topology.
    function copyBlockOverrides(blocks = {}) {
        if (!blocks || typeof blocks !== "object" || Array.isArray(blocks))
            throw new TypeError("blocks must be an object keyed by drive0 through drive3");
        const copied = {};
        for (const [name, drive] of Object.entries(blocks)) {
            if (!/^drive[0-3]$/.test(name)) throw new TypeError(`unknown block slot ${name}`);
            if (!drive || typeof drive !== "object" || Array.isArray(drive))
                throw new TypeError(`${name} must be a drive source`);
            copied[name] = drive.bytes !== undefined || drive.capacity_sectors !== undefined || drive.provider !== undefined
                ? resolveArrayDrive(drive) : { ...drive };
            if (copied[name].bytes !== undefined) copied[name].bytes = copied[name].bytes.slice();
        }
        return copied;
    }

    // Explicit source forms keep configuration text distinct from URLs and establish asset resolution.
    function configSource(source) {
        if (!source || typeof source !== "object" || Array.isArray(source))
            throw new TypeError("config needs url, text, or value");
        const kinds = ["url", "text", "value"].filter(name => source[name] !== undefined);
        if (kinds.length !== 1) throw new TypeError("config needs exactly one of url, text, or value");
        const kind = kinds[0];
        for (const name of Object.keys(source)) {
            if (name !== kind && (name !== "baseUrl" || kind === "url"))
                throw new TypeError(`unknown config source property ${name}`);
        }
        // URL sources define their own asset base; inline sources can select an explicit document base.
        const documentUrl = typeof document === "object" ? document.baseURI : globalThis.location?.href;
        if (kind === "url") {
            if (!(source.url instanceof URL) && (typeof source.url !== "string" || !source.url))
                throw new TypeError("config url must be a nonempty URL");
            const url = new URL(source.url, documentUrl).href;
            return { url, baseUrl: url };
        }
        // Inline object disk bytes are snapshots, independent of later application buffer writes.
        if (kind === "text" && typeof source.text !== "string") throw new TypeError("config text must be a string");
        if (kind === "value" && (!source.value || typeof source.value !== "object" || Array.isArray(source.value)))
            throw new TypeError("config value must be an object");
        if (source.baseUrl !== undefined && !(source.baseUrl instanceof URL) &&
            (typeof source.baseUrl !== "string" || !source.baseUrl)) throw new TypeError("baseUrl must be a nonempty URL");
        const baseUrl = source.baseUrl === undefined ? documentUrl : new URL(source.baseUrl, documentUrl).href;
        const value = kind === "value" ? { ...source.value } : undefined;
        if (value) {
            for (let index = 0; index < 4; index++) {
                const name = `drive${index}`;
                if (value[name]?.bytes instanceof Uint8Array) value[name] = { ...value[name], bytes: value[name].bytes.slice() };
            }
        }
        return { text: source.text, value, baseUrl };
    }
    function integer(name, value, minimum, maximum) {
        if (!Number.isInteger(value) || value < minimum || value > maximum)
            throw new RangeError(`${name} must be an integer from ${minimum} to ${maximum}`);
    }

    root.Riscbox = Riscbox;
    if (typeof module === "object" && module.exports)
        module.exports = { Riscbox, FilesystemError, BlockError };
    // DEVELOPMENT_EXPORTS
}(globalThis));
