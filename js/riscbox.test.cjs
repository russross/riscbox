const assert = require("node:assert/strict");
const test = require("node:test");

const { Riscbox } = require("./riscbox.js");

function fakeModule() {
    const memory = new WebAssembly.Memory({ initial: 1 });
    const calls = [];
    let next = 1024;
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
        riscbox_action_value() { return 0; },
        riscbox_action_data_address() { return 0; },
        riscbox_action_data_length() { return 0; },
    };
    for (const name of [
        "start", "console_input", "console_resize", "key_event", "pointer_event",
        "wheel_event", "network_input", "network_carrier",
    ]) {
        exports[`riscbox_${name}`] = (...args) => {
            calls.push([name, ...args]);
            return 0;
        };
    }
    return { exports, calls };
}

test("adapter copies host input into WASM memory and releases it", () => {
    const fake = fakeModule();
    const runtime = new Riscbox(fake.exports);
    runtime.consoleInput(Uint8Array.of(1, 2, 3));
    const call = fake.calls[0];
    assert.deepEqual(call.slice(0, 2), ["console_input", 1024]);
    assert.deepEqual(runtime.bytes(1024, 3), Uint8Array.of(1, 2, 3));
    assert.deepEqual(fake.calls[1], ["free", 1024, 3]);
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

test("non-configuration HTTP actions retain normal content caching", async () => {
    const fake = fakeModule();
    const url = Buffer.from("https://host/drive-abcd1234/blk.txt");
    new Uint8Array(fake.exports.memory.buffer, 64, url.length).set(url);
    const actions = [1, 0];
    fake.exports.riscbox_next_action = () => actions.shift();
    fake.exports.riscbox_action_value = () => 18;
    fake.exports.riscbox_action_data_address = () => 64;
    fake.exports.riscbox_action_data_length = () => url.length;
    fake.exports.riscbox_http_complete = () => 0;
    const runtime = new Riscbox(fake.exports, {
        fetch: async (_requestUrl, options) => {
            assert.deepEqual(options, { cache: "default" });
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

test("9p actions create independent sessions and complete out of order", async () => {
    const fake = fakeModule();
    const actions = [
        { kind: 7, endpoint: 1, generation: 1, bytes: Buffer.from("shared") },
        { kind: 7, endpoint: 2, generation: 1, bytes: Buffer.from("shared") },
        { kind: 8, endpoint: 1, generation: 1, requestId: 11,
          capacity: 8, bytes: Uint8Array.of(7, 0, 0, 0, 100, 1, 0) },
        { kind: 8, endpoint: 2, generation: 1, requestId: 12,
          capacity: 8, bytes: Uint8Array.of(7, 0, 0, 0, 100, 2, 0) },
    ];
    let current;
    fake.exports.riscbox_next_action = () => {
        current = actions.shift();
        if (!current) return 0;
        new Uint8Array(fake.exports.memory.buffer, 64, current.bytes.length).set(current.bytes);
        return current.kind;
    };
    fake.exports.riscbox_action_data_address = () => 64;
    fake.exports.riscbox_action_data_length = () => current.bytes.length;
    fake.exports.riscbox_action_endpoint = () => current.endpoint;
    fake.exports.riscbox_action_generation = () => current.generation;
    fake.exports.riscbox_action_request_id = () => current.requestId ?? 0;
    fake.exports.riscbox_action_reply_capacity = () => current.capacity ?? 0;
    fake.exports.riscbox_p9_complete = (...args) => {
        fake.calls.push(["p9_complete", ...args]);
        return 0;
    };
    const pending = [];
    let connections = 0;
    const server = {
        connect() {
            connections++;
            return {
                request(bytes, capacity) {
                    return new Promise((resolve) => pending.push({ bytes, capacity, resolve }));
                },
                close() {},
            };
        },
    };
    const runtime = new Riscbox(fake.exports, {
        p9Servers: new Map([["shared", server]]),
    });
    runtime.drainActions();
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(connections, 2);
    assert.deepEqual(pending.map((entry) => entry.bytes[5]), [1, 2]);
    assert.deepEqual(pending.map((entry) => entry.capacity), [8, 8]);
    pending[1].resolve({ kind: "suppressed" });
    pending[0].resolve({
        kind: "reply",
        bytes: Uint8Array.of(7, 0, 0, 0, 101, 1, 0),
    });
    await new Promise((resolve) => setImmediate(resolve));
    const completions = fake.calls.filter((call) => call[0] === "p9_complete");
    assert.deepEqual(completions.map((call) => [call[1], call[3], call[4]]), [
        [2, 12, 1],
        [1, 11, 0],
    ]);
});

test("9p close retires only the matching endpoint generation", () => {
    const fake = fakeModule();
    const closed = [];
    const actions = [
        { kind: 7, endpoint: 1, generation: 1, bytes: Buffer.from("shared") },
        { kind: 9, endpoint: 1, generation: 1, bytes: new Uint8Array() },
        { kind: 7, endpoint: 1, generation: 2, bytes: Buffer.from("shared") },
        { kind: 9, endpoint: 1, generation: 1, bytes: new Uint8Array() },
    ];
    let current;
    fake.exports.riscbox_next_action = () => {
        current = actions.shift();
        if (!current) return 0;
        new Uint8Array(fake.exports.memory.buffer, 64, current.bytes.length).set(current.bytes);
        return current.kind;
    };
    fake.exports.riscbox_action_data_address = () => 64;
    fake.exports.riscbox_action_data_length = () => current.bytes.length;
    fake.exports.riscbox_action_endpoint = () => current.endpoint;
    fake.exports.riscbox_action_generation = () => current.generation;
    const server = {
        connect() {
            const id = closed.length;
            return { request() {}, close() { closed.push(id); } };
        },
    };
    new Riscbox(fake.exports, {
        p9Servers: new Map([["shared", server]]),
    }).drainActions();
    assert.deepEqual(closed, [0]);
});

test("9p endpoint failures close the session and stop the request", async () => {
    const fake = fakeModule();
    const actions = [
        { kind: 7, endpoint: 3, generation: 1, bytes: Buffer.from("broken") },
        { kind: 8, endpoint: 3, generation: 1, requestId: 9,
          capacity: 8, bytes: Uint8Array.of(7, 0, 0, 0, 100, 1, 0) },
    ];
    let current;
    fake.exports.riscbox_next_action = () => {
        current = actions.shift();
        if (!current) return 0;
        new Uint8Array(fake.exports.memory.buffer, 64, current.bytes.length).set(current.bytes);
        return current.kind;
    };
    fake.exports.riscbox_action_value = () => 0;
    fake.exports.riscbox_action_data_address = () => 64;
    fake.exports.riscbox_action_data_length = () => current.bytes.length;
    fake.exports.riscbox_action_endpoint = () => current.endpoint;
    fake.exports.riscbox_action_generation = () => current.generation;
    fake.exports.riscbox_action_request_id = () => current.requestId ?? 0;
    fake.exports.riscbox_action_reply_capacity = () => current.capacity ?? 0;
    fake.exports.riscbox_p9_complete = (...args) => {
        fake.calls.push(["p9_complete", ...args]);
        return 0;
    };
    let closes = 0;
    const errors = [];
    const runtime = new Riscbox(fake.exports, {
        p9Servers: new Map([["broken", {
            connect: () => ({
                request: async () => { throw new Error("server failed"); },
                close: () => closes++,
            }),
        }]]),
        onError: (error) => errors.push(error.message),
    });
    runtime.drainActions();
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(closes, 1);
    assert.deepEqual(errors, ["server failed"]);
    const completion = fake.calls.find((call) => call[0] === "p9_complete");
    assert.deepEqual(completion.slice(1, 5), [3, 1, 9, 2]);
});

test("9p registration is validated before later startup actions", () => {
    const fake = fakeModule();
    const key = Buffer.from("missing");
    new Uint8Array(fake.exports.memory.buffer, 64, key.length).set(key);
    const actions = [7, 2];
    fake.exports.riscbox_next_action = () => actions.shift() ?? 0;
    fake.exports.riscbox_action_data_address = () => 64;
    fake.exports.riscbox_action_data_length = () => key.length;
    fake.exports.riscbox_action_endpoint = () => 1;
    fake.exports.riscbox_action_generation = () => 1;
    assert.throws(
        () => new Riscbox(fake.exports, { p9Servers: new Map() }).drainActions(),
        /9p server is not registered: missing/,
    );
    assert.deepEqual(actions, [2]);
});

test("host scheduling delegates exactly once", () => {
    const delays = [];
    const host = Riscbox.hostImports({ schedule: (delay) => delays.push(delay) });
    host.attach(fakeModule().exports);
    host.imports.schedule(7);
    assert.deepEqual(delays, [7]);
});

test("run passes complete epoch milliseconds across the 32-bit WASM ABI", () => {
    const fake = fakeModule();
    const calls = [];
    fake.exports.riscbox_run = (low, high) => {
        calls.push([low, high]);
        return 0;
    };
    const originalNow = Date.now;
    Date.now = () => 1_730_000_000_123;
    try {
        new Riscbox(fake.exports).run();
    } finally {
        Date.now = originalNow;
    }
    assert.deepEqual(calls, [[0xcc09_147b, 0x192]]);
});

test("vm_start compatibility call marshals strings and scalar options", () => {
    const fake = fakeModule();
    const runtime = new Riscbox(fake.exports);
    runtime.ccall(
        "vm_start", null,
        ["string", "number", "string", "string", "number", "number", "number"],
        ["https://host/vm.cfg", 256, "quiet", "secret", 640, 480, 1],
    );
    const call = fake.calls.find((entry) => entry[0] === "start");
    assert.deepEqual(call.slice(3, 4), [256]);
    assert.deepEqual(call.slice(-3), [640, 480, 1]);
    assert.equal(new TextDecoder().decode(runtime.bytes(call[1], call[2])), "https://host/vm.cfg");
    assert.equal(new TextDecoder().decode(runtime.bytes(call[4], call[5])), "quiet");
    assert.equal(new TextDecoder().decode(runtime.bytes(call[6], call[7])), "secret");
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
