// Independent binary packets exercise the deployed synchronous filesystem ABI.
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
    const config = encoder.encode(JSON.stringify({ version: 1, machine: "riscv64", memory_size: 32,
        console: "uart", uart_output: true, rtc_local_time: false, cmdline: "", bios: "firmware.bin",
        fs0: { server: "share", tag: "shared" } }));
    const prepare = () => {
        check(send(config, (address, length) => api.riscbox_prepare_resolved(address, length, 0, 0, 0, 0)) === 0, "prepare");
        check(api.riscbox_next_action() === 1, "firmware action");
        const id = api.riscbox_action_value();
        check(send(firmware, (address, length) => api.riscbox_http_complete(id, 200, address, length)) === 0, "firmware completion");
        check(api.riscbox_next_action() === 15, "powered-off preparation");
        check(api.riscbox_next_action() === 0, "no startup work");
        return send(encoder.encode("share"), api.riscbox_fs_get);
    };
    const handle = prepare();
    check(handle !== 0, "automatic share");
    check(command(handle, 13, u32(1)) === 0, "subscribe");
    check(api.riscbox_fs_next_change(handle) === 1 && new DataView(data().buffer).getUint32(0, true) === 9, "rescan");
    check(command(handle, 3, text("dir")) === 0, "mkdir");
    check(command(handle, 2, join(text("dir/file"), text("content"))) === 0, "whole write");
    check(command(handle, 1, text("dir/file")) === 0 && decoder.decode(data()) === "content", "synchronous read");
    check(api.riscbox_fs_next_change(handle) === 1, "create event");
    check(api.riscbox_fs_next_change(handle) === 1, "write event");
    check(new DataView(data().buffer).getBigUint64(16, true) === 0x0123456789abcdefn, "origin width");
    check(command(handle, 6, text("dir")) === 0 && new DataView(data().buffer).getUint32(0, true) === 1, "directory list");
    check(command(handle, 8, text("dir/file")) === 0 && new DataView(data().buffer).getBigUint64(32, true) === 7n, "stat size");
    const attributes = join(text("dir/file"), u32(0o751), u32(123), u32(456), u64(789), u32(123456789), u64(900), u32(987654321));
    check(command(handle, 14, attributes) === 0, "restore attributes");
    check(command(handle, 8, text("dir/file")) === 0, "restored stat");
    const restored = data();
    const metadata = new DataView(restored.buffer);
    check(metadata.getUint32(12, true) === 0o751 && metadata.getUint32(16, true) === 123
        && metadata.getUint32(20, true) === 456, "permissions and ownership restored");
    check(metadata.getBigUint64(40, true) === 789n && metadata.getUint32(48, true) === 123456789
        && metadata.getBigUint64(52, true) === 900n && metadata.getUint32(60, true) === 987654321, "timestamp precision restored");
    for (let length = 0; length < attributes.length; length++) {
        check(command(handle, 14, attributes.subarray(0, length)) === -22, "truncated attribute packet");
    }
    const invalid = attributes.slice();
    new DataView(invalid.buffer).setUint32(invalid.length - 4, 1_000_000_000, true);
    check(command(handle, 14, invalid) === -22, "invalid nanoseconds");
    check(command(handle, 8, text("dir/file")) === 0 && data().every((byte, index) => byte === restored[index]), "failed attributes leave stat unchanged");
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
    api.memory.grow(1);
    check(command(handle, 1, text("dir/file")) === 0 && decoder.decode(data()) === "content", "memory growth");
    check(command(handle, 15) === 0, "recursive clear");
    check(command(handle, 1, text("dir/file")) === -2, "clear removes content");
    check(command(handle, 12) === 0, "halted namespace reset");
    check(api.riscbox_destroy() === 0, "destroy");
    check(command(handle, 7) === -9, "destroy invalidates handles");
    const replacement = prepare();
    check(replacement !== handle, "new lifetime uses fresh handles");
    check(command(replacement, 7) === 0 && new DataView(data().buffer).getUint32(0, true) === 0, "fresh namespace");
    check(api.riscbox_destroy() === 0, "replacement destroy");
    return 1;
}
