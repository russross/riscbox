const assert = require("node:assert/strict");
const test = require("node:test");

const { Riscbox } = require("./riscbox.js");

function fakeModule() {
    const memory = new WebAssembly.Memory({ initial: 1 });
    const calls = [];
    let next = 1024;
    let begun = false;
    const exports = {
        memory,
        riscbox_alloc(length) {
            const ptr = next;
            next += length;
            return ptr;
        },
        riscbox_free(ptr, length) {
            calls.push(["free", ptr, length]);
        },
        riscbox_next_action() { return 0; },
        riscbox_configure_quantum() { return 0; },
        riscbox_wake_delay_ms(_low, _high, delay) { return delay; },
        riscbox_quantum_begin() {
            if (begun) return -1;
            begun = true;
            return 0;
        },
        riscbox_quantum_run() { return 3; },
        riscbox_quantum_finish() { return 0; },
        riscbox_quantum_abort() {},
        riscbox_timing_stat() { return 0; },
        riscbox_action_value() { return 0; },
        riscbox_action_data_address() { return 0; },
        riscbox_action_data_length() { return 0; },
    };
    for (const name of [
        "start", "start_resolved", "console_input", "console_resize", "key_event", "pointer_event",
        "wheel_event", "network_input", "network_carrier",
    ]) {
        exports[`riscbox_${name}`] = (...args) => {
            calls.push([name, ...args]);
            return 0;
        };
    }
    return { exports, calls };
}

test("host block requests copy replies and retire promises on reset", async () => {
    const fake = fakeModule();
    const completed = [];
    let resolveRead;
    const calls = [];
    const provider = {
        read(sector, length) {
            calls.push(["read", sector, length]);
            return new Promise((resolve) => { resolveRead = resolve; });
        },
        write(sector, bytes) { calls.push(["write", sector, bytes]); },
        reset() { calls.push(["reset"]); },
        close() { calls.push(["close"]); },
    };
    const runtime = new Riscbox(fake.exports, { blockProviders: new Map([[7, provider]]) });
    assert.throws(() => runtime.startResolved({ version: 1, machine: "riscv64", memory_size: 32,
        drive0: { provider: 7, capacity_sectors: Number.MAX_SAFE_INTEGER + 1 } }),
    /safe integer/);
    runtime.startResolved({ version: 1, machine: "riscv64", memory_size: 32,
        drive0: { provider: 7, capacity_sectors: 16 } });
    let action = 12;
    fake.exports.riscbox_next_action = () => { const current = action; action = 0; return current; };
    fake.exports.riscbox_action_value = () => 0;
    fake.exports.riscbox_action_endpoint = () => 7;
    fake.exports.riscbox_action_generation = () => action === 0 ? 1 : 2;
    fake.exports.riscbox_action_request_id = () => 4;
    fake.exports.riscbox_action_reply_capacity = () => 512;
    fake.exports.riscbox_action_sector_low = () => 5;
    fake.exports.riscbox_action_sector_high = () => 1;
    fake.exports.riscbox_block_complete = (...args) => {
        completed.push([args.slice(0, 4), runtime.bytes(args[4], args[5])]);
        return 0;
    };
    runtime.drainActions();
    await new Promise((resolve) => setImmediate(resolve));
    assert.deepEqual(calls[0], ["read", 0x1_0000_0005n, 512]);
    action = 13;
    fake.exports.riscbox_action_generation = () => 2;
    runtime.drainActions();
    resolveRead(new Uint8Array(512));
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(completed.length, 0);
    action = 12;
    runtime.drainActions();
    await new Promise((resolve) => setImmediate(resolve));
    resolveRead(new Uint8Array(512).fill(0x5a));
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(completed.length, 1);
    assert.deepEqual(completed[0][0], [7, 2, 4, 0]);
    assert.deepEqual(completed[0][1], new Uint8Array(512).fill(0x5a));
    action = 14;
    runtime.drainActions();
    assert.deepEqual(calls.filter((call) => call[0] === "reset" || call[0] === "close"), [["reset"], ["close"]]);
});

test("host write errors complete with guest I/O status", async () => {
    const fake = fakeModule();
    const completions = [];
    const errors = [];
    const payload = new Uint8Array(fake.exports.memory.buffer, 32, 512);
    payload.fill(0x6c);
    const runtime = new Riscbox(fake.exports, {
        blockProviders: new Map([[1, {
            read() { return new Uint8Array(512); },
            write(sector, bytes) {
                assert.equal(sector, 9n);
                assert.deepEqual(bytes, new Uint8Array(512).fill(0x6c));
                throw new Error("storage failed");
            },
            reset() {}, close() {},
        }]]),
        onError: (error) => errors.push(error.message),
    });
    runtime.startResolved({ version: 1, machine: "riscv64", memory_size: 32,
        drive0: { provider: 1, capacity_sectors: 16 } });
    let action = 12;
    fake.exports.riscbox_next_action = () => { const current = action; action = 0; return current; };
    fake.exports.riscbox_action_value = () => 1;
    fake.exports.riscbox_action_endpoint = () => 1;
    fake.exports.riscbox_action_generation = () => 1;
    fake.exports.riscbox_action_request_id = () => 2;
    fake.exports.riscbox_action_reply_capacity = () => 512;
    fake.exports.riscbox_action_sector_low = () => 9;
    fake.exports.riscbox_action_sector_high = () => 0;
    fake.exports.riscbox_action_data_address = () => 32;
    fake.exports.riscbox_action_data_length = () => 512;
    fake.exports.riscbox_block_complete = (...args) => { completions.push(args); return 0; };
    runtime.drainActions();
    await new Promise((resolve) => setImmediate(resolve));
    assert.deepEqual(completions, [[1, 1, 2, 1, 0, 0]]);
    assert.deepEqual(errors, ["storage failed"]);
});

test("configuration URL resolves defaults and assets before the WASM call", async () => {
    const fake = fakeModule();
    const fetched = [];
    const runtime = new Riscbox(fake.exports, {
        fetch: async (url, options) => {
            fetched.push([url, options.cache]);
            return { status: 200, arrayBuffer: async () => Buffer.from(
                '{version:1,machine:"riscv64",memory_size:128,bios:"fw.bin",' +
                'drive0:{file:"disk/blk.txt"},console:"uart",}',
            ) };
        },
    });
    await runtime.startFromUrl("https://host/vm/riscbox.cfg", 256, "quiet");
    assert.deepEqual(fetched, [["https://host/vm/riscbox.cfg", "no-store"]]);
    const call = fake.calls.find((entry) => entry[0] === "start_resolved");
    const config = JSON.parse(new TextDecoder().decode(runtime.bytes(call[1], call[2])));
    assert.equal(call[3], 256);
    assert.equal(config.bios, "https://host/vm/fw.bin");
    assert.equal(config.drive0.file, "https://host/vm/disk/blk.txt");
    assert.equal(config.cmdline, " quiet");
    assert.equal(config.uart_output, false);
    assert.equal(config.rtc_local_time, false);
    assert.throws(() => runtime.startResolved({ version: 1 }), /machine must be string/);
});

test("resolved configuration can replace an HTTP manifest with a host provider", async () => {
    const fake = fakeModule();
    const config = await Riscbox.loadResolvedConfig(
        "https://host/profile/riscbox.cfg", "",
        async (_url, options) => {
            assert.deepEqual(options, { cache: "no-store" });
            return { status: 200, arrayBuffer: async () => Buffer.from(
                '{version:1,machine:"riscv64",memory_size:256,bios:"fw.bin",' +
                'drive0:{file:"drive-abcd1234/blk.txt"}}',
            ) };
        },
    );
    assert.equal(config.drive0.file, "https://host/profile/drive-abcd1234/blk.txt");
    config.drive0 = { provider: 1, capacity_sectors: "1048576" };
    const provider = { read() {}, write() {}, reset() {}, close() {} };
    const runtime = new Riscbox(fake.exports, { blockProviders: new Map([[1, provider]]) });
    runtime.startResolved(config, 512);
    const call = fake.calls.find((entry) => entry[0] === "start_resolved");
    const resolved = JSON.parse(new TextDecoder().decode(runtime.bytes(call[1], call[2])));
    assert.deepEqual(resolved.drive0, { provider: 1, capacity_sectors: "1048576" });
    assert.equal(resolved.bios, "https://host/profile/fw.bin");
    assert.equal(call[3], 512);
});

test("adapter copies host input into WASM memory and releases it", () => {
    const fake = fakeModule();
    const runtime = new Riscbox(fake.exports);
    runtime.consoleInput(Uint8Array.of(1, 2, 3));
    const call = fake.calls[0];
    assert.deepEqual(call.slice(0, 2), ["console_input", 1024]);
    assert.deepEqual(runtime.bytes(1024, 3), Uint8Array.of(1, 2, 3));
    assert.deepEqual(fake.calls[1], ["free", 1024, 3]);
});

test("lifecycle controls forward to WASM and report guest and host causes", async () => {
    const fake = fakeModule();
    const actions = [10, 0, 11, 0];
    let current = 0;
    fake.exports.riscbox_next_action = () => { current = actions.shift() ?? 0; return current; };
    fake.exports.riscbox_action_value = () => current === 10 ? 0 : 3;
    for (const name of ["halt", "reset", "destroy"]) {
        fake.exports[`riscbox_${name}`] = () => {
            fake.calls.push([name]);
            return 0;
        };
    }
    const events = [];
    const runtime = new Riscbox(fake.exports, {
        onVmHalted: (cause) => events.push(["halted", cause]),
        onVmReset: (cause) => events.push(["reset", cause]),
        consoleReset: () => events.push(["terminal reset"]),
        framebufferClear: () => events.push(["screen clear"]),
    });
    await runtime.halt();
    await runtime.reset();
    await runtime.destroy();
    assert.deepEqual(fake.calls.filter(([name]) => ["halt", "reset", "destroy"].includes(name)),
        [["halt"], ["reset"], ["destroy"]]);
    assert.deepEqual(events, [
        ["halted", "guest-poweroff"], ["terminal reset"], ["screen clear"],
        ["reset", "host-reset"],
    ]);
});

test("HTTP actions complete requests and continue draining startup", async () => {
    const fake = fakeModule();
    const url = Buffer.from("https://host/vm.cfg");
    new Uint8Array(fake.exports.memory.buffer, 64, url.length).set(url);
    const actions = [1, 0, 2, 0];
    fake.exports.riscbox_next_action = () => actions.shift();
    fake.exports.riscbox_action_value = () => 17;
    fake.exports.riscbox_action_data_address = () => 64;
    fake.exports.riscbox_action_data_length = () => url.length;
    fake.exports.riscbox_http_complete = (...args) => {
        fake.calls.push(["http_complete", ...args]);
        return 0;
    };
    let started = 0;
    const runtime = new Riscbox(fake.exports, {
        fetch: async (requestUrl, options) => {
            assert.equal(requestUrl, "https://host/vm.cfg");
            assert.deepEqual(options, { cache: "no-store" });
            return { status: 200, arrayBuffer: async () => Uint8Array.of(1, 2).buffer };
        },
        onVmStarted: () => started++,
    });
    runtime.configUrl = "https://host/vm.cfg";
    runtime.drainActions();
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(fake.calls.find((call) => call[0] === "http_complete")[1], 17);
    assert.equal(started, 1);
});

for (const [assetUrl, expectedCache] of [
    ["https://host/drive-abcd1234/blk.txt", "force-cache"],
    ["https://host/drive-abcd1234/blk000000001.bin", "force-cache"],
    ["https://host/linux-a837bc72.gz", "force-cache"],
    ["https://host/fw_dynamic.bin-81ceef21.gz", "force-cache"],
    ["https://host/initrd.img", "default"],
]) test(`HTTP asset ${assetUrl} uses ${expectedCache}`, async () => {
    const fake = fakeModule();
    const url = Buffer.from(assetUrl);
    new Uint8Array(fake.exports.memory.buffer, 64, url.length).set(url);
    const actions = [1, 0];
    fake.exports.riscbox_next_action = () => actions.shift();
    fake.exports.riscbox_action_value = () => 18;
    fake.exports.riscbox_action_data_address = () => 64;
    fake.exports.riscbox_action_data_length = () => url.length;
    fake.exports.riscbox_http_complete = () => 0;
    const runtime = new Riscbox(fake.exports, {
        fetch: async (_requestUrl, options) => {
            assert.deepEqual(options, { cache: expectedCache });
            return { status: 200, arrayBuffer: async () => new ArrayBuffer(0) };
        },
    });
    runtime.configUrl = "https://host/riscbox.cfg";
    runtime.drainActions();
    await new Promise((resolve) => setImmediate(resolve));
});

test("host imports copy output and validate memory ranges", () => {
    const output = [];
    const host = Riscbox.hostImports({
        consoleWrite: (text) => output.push(text),
        networkWrite: (packet) => output.push(packet),
    });
    const fake = fakeModule();
    const runtime = host.attach(fake.exports);
    new Uint8Array(fake.exports.memory.buffer, 32, 5).set(Buffer.from("hello"));
    host.imports.console_write(32, 5);
    host.imports.network_write(32, 3);
    assert.equal(output[0], "hello");
    assert.deepEqual(output[1], Uint8Array.of(104, 101, 108));
    assert.throws(() => runtime.bytes(65535, 2), RangeError);
});

test("random host import fills WASM memory from Web Crypto", () => {
    const host = Riscbox.hostImports();
    const fake = fakeModule();
    const runtime = host.attach(fake.exports);
    assert.equal(host.imports.random_fill(32, 32), 0);
    assert.notDeepEqual(runtime.bytes(32, 32), new Uint8Array(32));
    assert.equal(host.imports.random_fill(65535, 2), -1);
});

test("cancelled startup ignores late HTTP failures", async () => {
    const fake = fakeModule();
    fake.exports.riscbox_destroy = () => 0;
    const url = Buffer.from("https://host/firmware");
    new Uint8Array(fake.exports.memory.buffer, 64, url.length).set(url);
    const actions = [1, 0];
    fake.exports.riscbox_next_action = () => actions.shift() ?? 0;
    fake.exports.riscbox_action_value = () => 7;
    fake.exports.riscbox_action_data_address = () => 64;
    fake.exports.riscbox_action_data_length = () => url.length;
    let fail;
    const errors = [];
    const runtime = new Riscbox(fake.exports, {
        fetch: () => new Promise((_resolve, reject) => { fail = reject; }),
        onError: error => errors.push(error),
    });
    runtime.drainActions();
    await runtime.destroy();
    fail(new Error("obsolete request failed"));
    await new Promise(resolve => setImmediate(resolve));
    assert.deepEqual(errors, []);
});

test("adapter replaces a WFI wakeup with an immediate quantum", async () => {
    const fake = fakeModule();
    let runs = 0;
    fake.exports.riscbox_quantum_run = () => { runs++; return 3; };
    fake.exports.riscbox_quantum_finish = () => 100;
    const runtime = new Riscbox(fake.exports);
    runtime.scheduleWakeup(100);
    runtime.scheduleWakeup(0);
    await new Promise((resolve) => setTimeout(resolve, 10));
    assert.equal(runs, 1);
    clearTimeout(runtime.wakeupTimer);
});

test("immediate wakeups use a task and discard replaced requests", async () => {
    const fake = fakeModule();
    let runs = 0;
    fake.exports.riscbox_quantum_run = () => { runs++; return 3; };
    const runtime = new Riscbox(fake.exports);
    const originalTimeout = global.setTimeout;
    let zeroDelayTimers = 0;
    global.setTimeout = (callback, delay, ...args) => {
        if (delay === 0) zeroDelayTimers++;
        return originalTimeout(callback, delay, ...args);
    };
    try {
        runtime.scheduleWakeup(0);
        runtime.scheduleWakeup(100);
        runtime.scheduleWakeup(0);
        await new Promise((resolve) => originalTimeout(resolve, 10));
        assert.equal(runs, 1);
        assert.equal(zeroDelayTimers, 0);
    } finally {
        global.setTimeout = originalTimeout;
        runtime.cancelWakeup();
    }
});

test("quantum begin receives complete host epoch milliseconds across the ABI", async () => {
    const fake = fakeModule();
    const calls = [];
    fake.exports.riscbox_quantum_begin = (low, high) => {
        calls.push([low, high]);
        return 0;
    };
    const originalNow = Date.now;
    Date.now = () => 1_730_000_000_123;
    try {
        const runtime = new Riscbox(fake.exports);
        runtime.scheduleWakeup = () => {};
        await runtime.runQuantum();
    } finally {
        Date.now = originalNow;
    }
    assert.deepEqual(calls, [[1_730_000_000_123 >>> 0,
        Math.floor(1_730_000_000_123 / 0x1_0000_0000) >>> 0]]);
});

test("configured quantum duration and diagnostics are passed to WASM", () => {
    const fake = fakeModule();
    const calls = [];
    fake.exports.riscbox_configure_quantum = (...args) => {
        calls.push(args);
        return 0;
    };
    new Riscbox(fake.exports, { targetQuantumMs: 5, debugTiming: true });
    new Riscbox(fake.exports);
    assert.deepEqual(calls, [[5, 1], [20, 0]]);
    assert.throws(() => new Riscbox(fake.exports, { targetQuantumMs: 0 }), /targetQuantumMs/);
    assert.throws(() => new Riscbox(fake.exports, { guestClockSkew: 0.2 }), /adaptive/);
    assert.throws(() => new Riscbox(fake.exports, { timesliceMs: 5 }), /renamed to targetQuantumMs/);
});

test("timing diagnostics report adaptive skew and rate variance", async () => {
    const fake = fakeModule();
    fake.exports.riscbox_quantum_run = () => 0;
    fake.exports.riscbox_timing_stat = (kind) => kind === 5 ? 0.25 : kind === 6 ? 0.20
        : kind === 7 ? 200 : 0;
    const runtime = new Riscbox(fake.exports, { debugTiming: true });
    runtime.scheduleWakeup = () => {};
    await runtime.runQuantum();
    await runtime.runQuantum();
    assert.deepEqual(runtime.timing.sessionCatchUpSkews, [0.25]);
    runtime.timing.sessionCatchUpSkews.push(0.05, 0.10, 0.20, 0.30);
    runtime.timing.rateSamples = 2;
    runtime.timing.rateMean = 15;
    runtime.timing.rateM2 = 50;
    runtime.timing.nextReport = 0;
    const originalLog = console.log;
    let report;
    console.log = (_label, value) => { report = value; };
    try {
        runtime.reportTiming(performance.now());
    } finally {
        console.log = originalLog;
    }
    assert.equal(report.adaptiveGuestClockSkewPercent, 20);
    assert.equal(report.carriedGuestMs, 0.02);
    assert.equal(report.intervalEmulatedMCyclesPerSecondMean, 15);
    assert.equal(report.intervalEmulatedMCyclesPerSecondStdDev, 5);
    assert.equal(report.intervalNoCatchUpSkewPercent, 25);
    assert.equal(report.sessionPotentialCatchUpSkewP50Percent, 20);
    assert.equal(report.sessionPotentialCatchUpSkewP90Percent, 30);
    assert.equal(report.sessionPotentialCatchUpSkewP99Percent, 30);
});

test("WASM chooses the scheduled wake delay", () => {
    const fake = fakeModule();
    fake.exports.riscbox_wake_delay_ms = (_low, _high, delay) => Math.max(delay, 5);
    const runtime = new Riscbox(fake.exports);
    const originalTimeout = global.setTimeout;
    let delay;
    global.setTimeout = (_callback, milliseconds) => { delay = milliseconds; return 1; };
    try {
        runtime.scheduleWakeup(0);
        assert.equal(delay, 5);
        runtime.scheduleWakeup(10);
        assert.equal(delay, 10);
    } finally {
        global.setTimeout = originalTimeout;
    }
});

test("an already active quantum reports an error", async () => {
    const fake = fakeModule();
    fake.exports.riscbox_quantum_begin = () => -2;
    await assert.rejects(new Riscbox(fake.exports).runQuantum(), /could not begin a quantum/);
});

test("host service resumes without yielding to promise continuations", async () => {
    const fake = fakeModule();
    let runs = 0;
    let continued = false;
    queueMicrotask(() => { continued = true; });
    fake.exports.riscbox_quantum_run = () => {
        assert.equal(continued, false);
        return ++runs === 1 ? 2 : 3;
    };
    await new Riscbox(fake.exports).runQuantum();
    assert.equal(runs, 2);
    assert.equal(continued, true);
});

test("host service starts HTTP work before the unused quantum budget resumes", async () => {
    const fake = fakeModule();
    const url = Buffer.from("https://host/block.bin");
    new Uint8Array(fake.exports.memory.buffer, 64, url.length).set(url);
    const actions = [1, 0];
    fake.exports.riscbox_next_action = () => actions.shift() ?? 0;
    fake.exports.riscbox_action_value = () => 9;
    fake.exports.riscbox_action_data_address = () => 64;
    fake.exports.riscbox_action_data_length = () => url.length;
    let fetches = 0;
    let runs = 0;
    fake.exports.riscbox_quantum_run = () => {
        runs++;
        if (runs === 2) assert.equal(fetches, 1);
        return runs === 1 ? 2 : 3;
    };
    const runtime = new Riscbox(fake.exports, {
        fetch() {
            fetches++;
            return new Promise(() => {});
        },
    });
    await runtime.runQuantum();
    assert.equal(runs, 2);
});

test("vm_start compatibility call marshals current strings and scalar options", () => {
    const fake = fakeModule();
    const runtime = new Riscbox(fake.exports);
    runtime.ccall(
        "vm_start", null,
        ["string", "number", "string", "number", "number", "number"],
        ["https://host/vm.cfg", 256, "quiet", 640, 480, 1],
    );
    const call = fake.calls.find((entry) => entry[0] === "start");
    assert.deepEqual(call.slice(3, 4), [256]);
    assert.deepEqual(call.slice(-3), [640, 480, 1]);
    assert.equal(new TextDecoder().decode(runtime.bytes(call[1], call[2])), "https://host/vm.cfg");
    assert.equal(new TextDecoder().decode(runtime.bytes(call[4], call[5])), "quiet");
});

test("input events use stable scalar exports", () => {
    const fake = fakeModule();
    const runtime = new Riscbox(fake.exports);
    runtime.consoleResize(80, 25);
    runtime.keyEvent(true, 30);
    runtime.pointerEvent(10, 20, 1);
    runtime.wheelEvent(-1);
    runtime.networkCarrier(true);
    assert.deepEqual(fake.calls, [
        ["console_resize", 80, 25],
        ["key_event", 1, 30],
        ["pointer_event", 10, 20, 1],
        ["wheel_event", -1],
        ["network_carrier", 1],
    ]);
});

test("framebuffer actions expose a zero-copy pixel view and geometry", () => {
    const fake = fakeModule();
    new Uint8Array(fake.exports.memory.buffer, 2048, 8)
        .set(Uint8Array.of(1, 2, 3, 4, 5, 6, 7, 8));
    const actions = [6, 0];
    fake.exports.riscbox_next_action = () => actions.shift();
    fake.exports.riscbox_action_value = () => 0;
    fake.exports.riscbox_action_data_address = () => 2048;
    fake.exports.riscbox_action_data_length = () => 8;
    fake.exports.riscbox_action_x = () => 0;
    fake.exports.riscbox_action_y = () => 3;
    fake.exports.riscbox_action_width = () => 2;
    fake.exports.riscbox_action_height = () => 1;
    fake.exports.riscbox_action_stride = () => 8;
    let pixels;
    let geometry;
    const runtime = new Riscbox(fake.exports, {
        framebufferRefresh(data, update) {
            pixels = data;
            geometry = update;
        },
    });
    runtime.drainActions();
    assert.deepEqual(pixels, Uint8Array.of(1, 2, 3, 4, 5, 6, 7, 8));
    assert.deepEqual(geometry, { x: 0, y: 3, width: 2, height: 1, stride: 8 });
    new Uint8Array(fake.exports.memory.buffer)[2048] = 9;
    assert.equal(pixels[0], 9);
});
