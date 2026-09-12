import assert from "node:assert/strict";
import test from "node:test";

import { MAX_FILE_SIZE, Memory9PServer, P9Error } from "./p9.js";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

class Message {
    constructor(type, tag = 1) {
        this.bytes = [0, 0, 0, 0, type, tag & 0xff, tag >>> 8];
    }

    u8(value) {
        this.bytes.push(value & 0xff);
        return this;
    }

    u16(value) {
        this.bytes.push(value & 0xff, value >>> 8 & 0xff);
        return this;
    }

    u32(value) {
        this.bytes.push(value & 0xff, value >>> 8 & 0xff, value >>> 16 & 0xff, value >>> 24 & 0xff);
        return this;
    }

    u64(value, high = 0) {
        return this.u32(value).u32(high);
    }

    string(value) {
        const data = encoder.encode(value);
        this.u16(data.length);
        this.bytes.push(...data);
        return this;
    }

    data(value) {
        this.bytes.push(...value);
        return this;
    }

    finish() {
        const result = Uint8Array.from(this.bytes);
        new DataView(result.buffer).setUint32(0, result.length, true);
        return result;
    }
}

class Reply {
    constructor(data) {
        this.data = data;
        this.view = new DataView(data.buffer, data.byteOffset, data.byteLength);
        this.offset = 0;
        assert.equal(this.u32(), data.length);
        this.type = this.u8();
        this.tag = this.u16();
    }

    u8() {
        return this.data[this.offset++];
    }

    u16() {
        const value = this.view.getUint16(this.offset, true);
        this.offset += 2;
        return value;
    }

    u32() {
        const value = this.view.getUint32(this.offset, true);
        this.offset += 4;
        return value;
    }

    u64() {
        const low = this.u32();
        assert.equal(this.u32(), 0);
        return low;
    }

    string() {
        const size = this.u16();
        const value = decoder.decode(this.data.subarray(this.offset, this.offset + size));
        this.offset += size;
        return value;
    }

    qid() {
        return { type: this.u8(), version: this.u32(), path: this.u64() };
    }

    remaining() {
        return this.data.subarray(this.offset);
    }
}

function exchange(server, message, expectedType = message.bytes[4] + 1) {
    const reply = new Reply(server.request(message.finish(), 65536));
    assert.equal(reply.type, expectedType);
    assert.equal(reply.tag, message.bytes[5] | message.bytes[6] << 8);
    return reply;
}

function versionAndAttach(server) {
    const version = exchange(server, new Message(100, 0xffff).u32(65536).string("9P2000.L"));
    assert.equal(version.u32(), 65536);
    assert.equal(version.string(), "9P2000.L");
    const attach = exchange(server, new Message(104).u32(1).u32(0xffffffff).string("student").string("").u32(1000));
    assert.equal(attach.qid().type, 0x80);
}

test("host API uses copied bytes, nested paths, and notifications", () => {
    const original = encoder.encode("one");
    const server = new Memory9PServer({ src: { "main.c": original } });
    const changes = [];
    server.subscribe((change) => changes.push(change));
    original[0] = 0;
    assert.equal(decoder.decode(server.readFile("src/main.c")), "one");

    const read = server.readFile("src/main.c");
    read[0] = 0;
    assert.equal(decoder.decode(server.readFile("src/main.c")), "one");

    server.writeFile("src/main.c", "two", "editor");
    server.writeFile("build/result.txt", "ok");
    server.rename("build/result.txt", "result.txt");
    server.remove("result.txt");
    server.loadFiles({ "__proto__/safe.txt": "safe" });
    assert.equal(decoder.decode(server.readFile("__proto__/safe.txt")), "safe");
    assert.deepEqual(server.listFiles(), ["__proto__/safe.txt"]);
    assert.deepEqual(changes.map((change) => change.kind), ["write", "create", "rename", "remove", "reset"]);
    assert.equal(changes[0].source, "editor");
});

test("host API enforces the 16 MiB per-file limit", () => {
    const server = new Memory9PServer();
    server.writeFile("maximum.bin", new Uint8Array(MAX_FILE_SIZE));
    assert.throws(
        () => server.writeFile("too-large.bin", new Uint8Array(MAX_FILE_SIZE + 1)),
        (error) => error instanceof P9Error && error.errno === 27,
    );
});

test("9P reads and writes regular files", () => {
    const server = new Memory9PServer({ "hello.txt": "hello" });
    versionAndAttach(server);

    const walk = exchange(server, new Message(110).u32(1).u32(2).u16(1).string("hello.txt"));
    assert.equal(walk.u16(), 1);
    assert.equal(walk.qid().type, 0);
    exchange(server, new Message(12).u32(2).u32(2)).qid();

    const read = exchange(server, new Message(116).u32(2).u64(1).u32(3));
    assert.equal(read.u32(), 3);
    assert.equal(decoder.decode(read.remaining()), "ell");

    const write = exchange(server, new Message(118).u32(2).u64(5).u32(6).data(encoder.encode(" world")));
    assert.equal(write.u32(), 6);
    assert.equal(decoder.decode(server.readFile("hello.txt")), "hello world");

    exchange(
        server,
        new Message(26).u32(2).u32(8).u32(0).u32(0).u32(0).u64(5)
            .u64(0).u64(0).u64(0).u64(0),
    );
    assert.equal(decoder.decode(server.readFile("hello.txt")), "hello");

    const getattr = exchange(server, new Message(24).u32(2).u64(0x3fff));
    assert.equal(getattr.u64(), 0x3fff);
    getattr.qid();
    getattr.u32(); getattr.u32(); getattr.u32(); getattr.u64(); getattr.u64();
    assert.equal(getattr.u64(), 5);
    exchange(server, new Message(120).u32(2));
});

test("9P creates, lists, renames, links, and removes tree entries", () => {
    const server = new Memory9PServer();
    const changes = [];
    server.subscribe((change) => changes.push(change));
    versionAndAttach(server);

    exchange(server, new Message(72).u32(1).string("build").u32(0o755).u32(1000)).qid();
    exchange(server, new Message(110).u32(1).u32(2).u16(1).string("build"));
    exchange(server, new Message(14).u32(2).string("output").u32(2).u32(0o644).u32(1000)).qid();
    exchange(server, new Message(118).u32(2).u64(0).u32(2).data(encoder.encode("ok")));

    const directory = exchange(server, new Message(40).u32(1).u64(0).u32(4096));
    const directorySize = directory.u32();
    assert.ok(directorySize > 0);
    assert.equal(directory.qid().type, 0x80);
    directory.u64();
    assert.equal(directory.u8(), 4);
    assert.equal(directory.string(), "build");

    exchange(server, new Message(74).u32(1).string("build").u32(1).string("out"));
    exchange(server, new Message(16).u32(1).string("latest").string("out/output").u32(1000)).qid();
    exchange(server, new Message(110).u32(1).u32(3).u16(1).string("latest"));
    assert.equal(exchange(server, new Message(22).u32(3)).string(), "out/output");

    exchange(server, new Message(76).u32(1).string("latest").u32(0));
    const notEmpty = exchange(server, new Message(76).u32(1).string("out").u32(0x200), 7);
    assert.equal(notEmpty.u32(), 39);
    exchange(server, new Message(110).u32(1).u32(4).u16(1).string("out"));
    exchange(server, new Message(76).u32(4).string("output").u32(0));
    exchange(server, new Message(76).u32(1).string("out").u32(0x200));
    assert.deepEqual(server.listFiles(), []);
    assert.ok(changes.every((change) => change.source === "guest"));
});

test("malformed frames and unsupported large offsets return Rlerror", () => {
    const server = new Memory9PServer({ file: "x" });
    const malformed = new Message(100, 0xffff).u32(65536).string("9P2000.L").finish();
    new DataView(malformed.buffer).setUint32(0, malformed.length + 1, true);
    assert.equal(new Reply(server.request(malformed, 65536)).u32(), 71);

    versionAndAttach(server);
    exchange(server, new Message(110).u32(1).u32(2).u16(1).string("file"));
    const reply = new Reply(server.request(new Message(116).u32(2).u64(0, 1).u32(1).finish(), 65536));
    assert.equal(reply.type, 7);
    assert.equal(reply.u32(), 27);
});
