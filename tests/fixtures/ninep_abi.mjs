// Exercise the deployed scalar exports using independent binary packets.
const encoder = new TextEncoder();
const decoder = new TextDecoder();
function check(condition, message) { if (!condition) throw Error(message); }
function u32(value) { const bytes = new Uint8Array(4); new DataView(bytes.buffer).setUint32(0, value, true); return bytes; }
function u64(value) { const bytes = new Uint8Array(8); new DataView(bytes.buffer).setBigUint64(0, BigInt(value), true); return bytes; }
function join(...parts) {
    const output = new Uint8Array(parts.reduce((length, part) => length + part.length, 0));
    let offset = 0;
    for (const part of parts) { output.set(part, offset); offset += part.length; }
    return output;
}
function blob(bytes) { return join(u32(bytes.length), bytes); }
function text(value) { return blob(encoder.encode(value)); }

export async function abiRegression(api, firmware) {
    const send = (bytes, operation) => {
        const address = api.riscbox_alloc(bytes.length);
        check(address !== 0, "input allocation");
        new Uint8Array(api.memory.buffer, address, bytes.length).set(bytes);
        try { return operation(address, bytes.length); }
        finally { api.riscbox_free(address, bytes.length); }
    };
    const data = () => new Uint8Array(api.memory.buffer, api.riscbox_fs_data_address(), api.riscbox_fs_data_length()).slice();
    const command = (handle, code, body = new Uint8Array()) => send(join(u32(code), u64(200), u64(0x0123456789abcdefn), body),
        (address, length) => api.riscbox_fs_call(handle, address, length));
    const create = () => send(join(u32(1024), u32(4096), u32(64), u32(64), u64(100)), api.riscbox_fs_create);
    const handle = create();
    check(handle !== 0, "create before VM");
    check(command(handle, 13, u32(1)) === 0, "subscribe");
    check(api.riscbox_fs_next_change(handle) === 1 && new DataView(data().buffer).getUint32(0, true) === 9, "rescan");
    check(command(handle, 3, text("dir")) === 0, "mkdir");
    check(command(handle, 2, join(text("dir/file"), text("content"))) === 0, "whole write");
    check(command(handle, 1, text("dir/file")) === 0 && decoder.decode(data()) === "content", "whole read");
    check(api.riscbox_fs_next_change(handle) === 1, "create event");
    check(api.riscbox_fs_next_change(handle) === 1, "write event");
    const event = new DataView(data().buffer);
    check(event.getBigUint64(16, true) === 0x0123456789abcdefn, "origin width");
    check(command(handle, 6, text("dir")) === 0 && new DataView(data().buffer).getUint32(0, true) === 1, "directory list");
    check(command(handle, 8, text("dir/file")) === 0 && new DataView(data().buffer).getBigUint64(32, true) === 7n, "stat size");
    check(command(handle, 9, join(text("symlink"), text("dir/file"))) === 0, "symlink");
    check(command(handle, 10, text("symlink")) === 0 && decoder.decode(data()) === "dir/file", "readlink");
    check(command(handle, 11, join(text("dir/file"), text("hard"))) === 0, "hard link");
    check(command(handle, 5, join(text("hard"), text("moved"))) === 0, "rename");
    check(command(handle, 4, text("moved")) === 0, "remove");
    const malformed = join(u32(2), u64(200), u64(0), text("dir/file"), u32(5), encoder.encode("x"));
    check(send(malformed, (address, length) => api.riscbox_fs_call(handle, address, length)) === -22, "truncated write");
    check(api.riscbox_fs_call(handle, 0xfffffff0, 32) === -22, "unallocated pointer");
    check(send(malformed, (address, length) => api.riscbox_fs_call(handle, address + 1, length - 1)) === -22, "interior pointer");
    check(command(handle, 1, text("dir/file")) === 0 && decoder.decode(data()) === "content", "failed mutation atomicity");

    // Seed installs only namespace metadata. Two host reads share one source
    // and keep the original inode alive through unlink and path replacement.
    const seed = join(u32(1), text("lazy"), u32(2), u32(3), u32(7), u32(16), u64(0xffffffffffffffffn));
    check(command(handle, 14, seed) === 0, "install seed");
    check(api.riscbox_fs_next_load() === 0, "no preload");
    check(command(handle, 8, text("lazy")) === 0 && new DataView(data().buffer).getBigUint64(52, true) === 0xffffffffffffffffn, "timestamp width");
    check(command(handle, 1, text("lazy")) === 1, "pending host read");
    const request = new DataView(data().buffer).getUint32(0, true);
    check(api.riscbox_fs_next_load() === handle, "source action");
    const ticket = data();
    check(ticket.length === 32 && new DataView(ticket.buffer).getUint32(24, true) === 7, "source ID");
    check(command(handle, 1, text("lazy")) === 1, "joined host read");
    const joined = new DataView(data().buffer).getUint32(0, true);
    check(api.riscbox_fs_next_load() === 0, "one dispatch");
    check(command(handle, 4, text("lazy")) === 0, "unlink pinned file");
    check(command(handle, 2, join(text("lazy"), text("replacement"))) === 0, "reuse path");
    // All byte snapshots above are copied before this promise continuation.
    await Promise.resolve();
    check(send(join(ticket, u64(300), u32(0), text("old")), (address, length) => api.riscbox_fs_complete_load(handle, address, length)) === 0, "async completion");
    check(command(handle, 15, u32(request)) === 0 && decoder.decode(data()) === "old", "pinned read result");
    check(command(handle, 15, u32(joined)) === 0 && decoder.decode(data()) === "old", "joined result");
    check(send(join(ticket, u64(300), u32(0), text("old")), (address, length) => api.riscbox_fs_complete_load(handle, address, length)) === 1, "late duplicate ignored");

    check(command(handle, 14, seed) === 0 && command(handle, 1, text("lazy")) === 1, "failure read");
    const failedRequest = new DataView(data().buffer).getUint32(0, true);
    check(api.riscbox_fs_next_load() === handle, "failure source");
    const failedTicket = data();
    check(send(join(failedTicket, u64(300), u32(1), blob(new Uint8Array())), (address, length) => api.riscbox_fs_complete_load(handle, address, length)) === 0, "source failure completion");
    check(command(handle, 15, u32(failedRequest)) === -5, "failed read EIO");
    check(command(handle, 17, text("lazy")) === 0 && command(handle, 1, text("lazy")) === 1, "retry source");
    const retryRequest = new DataView(data().buffer).getUint32(0, true);
    check(api.riscbox_fs_next_load() === handle, "retry action");
    const retryTicket = data();
    const retryCompletion = join(retryTicket, u64(300), u32(0), text("old"));
    check(send(join(retryCompletion, u32(0)), (address, length) => api.riscbox_fs_complete_load(handle, address, length)) === -22, "malformed completion");
    check(command(handle, 15, u32(retryRequest)) === 1, "malformed completion preserves pending read");
    check(send(join(retryTicket, u64(300), u32(0), text("long")), (address, length) => api.riscbox_fs_complete_load(handle, address, length)) === -5, "wrong source length");
    check(command(handle, 15, u32(retryRequest)) === -5, "wrong length settles read");

    check(command(handle, 14, seed) === 0 && command(handle, 1, text("lazy")) === 1, "pending before reset");
    const staleRequest = new DataView(data().buffer).getUint32(0, true);
    check(api.riscbox_fs_next_load() === handle, "reset load action");
    const staleTicket = data();
    check(command(handle, 12) === 0, "namespace reset");
    check(command(handle, 15, u32(staleRequest)) === -116, "stale host read");
    check(send(join(staleTicket, u64(300), u32(0), text("old")), (address, length) => api.riscbox_fs_complete_load(handle, address, length)) === 1, "reset load ignored");

    // The same handle backs two guest endpoints. VM reset and destroy leave
    // host state accessible; close succeeds only after attached VM teardown.
    check(command(handle, 2, join(text("host"), text("kept"))) === 0, "host initial file");
    check(send(encoder.encode("workspace"), (address, length) => api.riscbox_fs_bind(handle, address, length)) === 0, "bind key");
    const config = encoder.encode(JSON.stringify({ version: 1, machine: "riscv64", memory_size: 32,
        console: "uart", uart_output: true, rtc_local_time: false, cmdline: "", bios: "https://host/fw.bin",
        fs0: { server: "workspace", tag: "first" }, fs1: { server: "workspace", tag: "second" } }));
    for (let boot = 0; boot < 2; boot++) {
        check(send(config, (address, length) => api.riscbox_start_resolved(address, length, 32, 0, 0, 0)) === 0, "VM start");
        check(api.riscbox_next_action() === 1, "firmware request");
        const id = api.riscbox_action_value();
        check(send(Uint8Array.of(0x73, 0, 0x50, 0x10), (address, length) => api.riscbox_http_complete(id, 200, address, length)) === 0, "boot image completion");
        check(api.riscbox_next_action() === 2 && api.riscbox_next_action() === 0, "Rust endpoints skip JS sessions");
        check(api.riscbox_fs_close(handle) === -16, "attached handle busy");
        check(api.riscbox_reset() === 0, "VM reset");
        while (api.riscbox_next_action() !== 0) { /* Drain reset notifications. */ }
        check(command(handle, 1, text("host")) === 0 && decoder.decode(data()) === "kept", "VM reset retains data");
        check(api.riscbox_halt() === 0 && api.riscbox_destroy() === 0, "VM destroy");
    }
    check(command(handle, 1, text("host")) === 0 && decoder.decode(data()) === "kept", "destroy retains handle");

    // Actual guest requests reach the same source queue. Completion enters
    // WASM after the promise resolves, and the current quantum then resumes.
    check(command(handle, 14, seed) === 0, "guest seed");
    check(send(config, (address, length) => api.riscbox_start_resolved(address, length, 32, 0, 0, 0)) === 0, "probe start");
    check(api.riscbox_next_action() === 1, "probe firmware request");
    const firmwareRequest = api.riscbox_action_value();
    check(send(firmware, (address, length) => api.riscbox_http_complete(firmwareRequest, 200, address, length)) === 0, "probe firmware");
    check(api.riscbox_next_action() === 2 && api.riscbox_next_action() === 0, "probe ready");
    check(api.riscbox_quantum_begin(100000, 0) === 0, "probe quantum");
    check(api.riscbox_quantum_run() === 2, "guest source service exit");
    check(api.riscbox_fs_next_load() === handle, "guest load handle");
    const guestTicket = data();
    await Promise.resolve();
    check(send(join(guestTicket, u64(300), u32(0), text("old")), (address, length) => api.riscbox_fs_complete_load(handle, address, length)) === 0, "guest source completion");
    let output = "";
    let outcome;
    for (let activation = 0; activation < 8; activation++) {
        outcome = api.riscbox_quantum_run();
        check(outcome >= 0, "probe activation");
        let action;
        while ((action = api.riscbox_next_action()) !== 0) {
            check(action === 3, "probe console action");
            output += decoder.decode(new Uint8Array(api.memory.buffer, api.riscbox_action_data_address(), api.riscbox_action_data_length()).slice());
        }
        if (outcome !== 2) break;
    }
    check(outcome === 1 && output.includes("ABI GUEST PASS") && !output.includes("ABI GUEST FAIL"), "guest reply bytes");
    check(api.riscbox_quantum_finish(1, 100001, 0) >= 0, "probe quantum finish");
    check(api.riscbox_halt() === 0 && api.riscbox_destroy() === 0, "probe teardown");
    check(api.riscbox_fs_close(handle) === 0, "close after teardown");
    check(command(handle, 12) === -9, "closed handle");
    const next = create();
    check(next > handle && api.riscbox_fs_close(next) === 0, "handles not reused");
    return 1;
}
