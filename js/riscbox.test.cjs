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
        fetch: async (requestUrl) => {
            assert.equal(requestUrl, "https://host/vm.cfg");
            return { status: 200, arrayBuffer: async () => Uint8Array.of(1, 2).buffer };
        },
        onVmStarted: () => started++,
    });
    runtime.drainActions();
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(fake.calls.find((call) => call[0] === "http_complete")[1], 17);
    assert.equal(started, 1);
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

test("9p host import invokes the configured server and copies its reply", () => {
    let request;
    let capacity;
    const host = Riscbox.hostImports({
        p9Server: {
            request(bytes, replyCapacity) {
                request = bytes;
                capacity = replyCapacity;
                return Uint8Array.of(7, 0, 0, 0, 101, 3, 0);
            },
        },
    });
    const fake = fakeModule();
    const runtime = host.attach(fake.exports);
    new Uint8Array(fake.exports.memory.buffer, 32, 7)
        .set(Uint8Array.of(7, 0, 0, 0, 100, 3, 0));
    assert.equal(host.imports.p9_request(32, 7, 64, 128), 7);
    assert.deepEqual(request, Uint8Array.of(7, 0, 0, 0, 100, 3, 0));
    assert.equal(capacity, 128);
    assert.deepEqual(runtime.bytes(64, 7), Uint8Array.of(7, 0, 0, 0, 101, 3, 0));
});

test("9p host import rejects absent and oversized server replies", () => {
    const missing = Riscbox.hostImports();
    missing.attach(fakeModule().exports);
    assert.equal(missing.imports.p9_request(0, 0, 0, 8), -5);

    const oversized = Riscbox.hostImports({
        p9Server: { request: () => new Uint8Array(9) },
    });
    oversized.attach(fakeModule().exports);
    assert.equal(oversized.imports.p9_request(0, 0, 0, 8), -71);
});

test("host scheduling delegates exactly once", () => {
    const delays = [];
    const host = Riscbox.hostImports({ schedule: (delay) => delays.push(delay) });
    host.attach(fakeModule().exports);
    host.imports.schedule(7);
    assert.deepEqual(delays, [7]);
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
