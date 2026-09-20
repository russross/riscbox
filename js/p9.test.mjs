import assert from "node:assert/strict";
import test from "node:test";

import { MAX_FILE_SIZE, Memory9PServer, P9Error } from "../build/js/p9/index.js";

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

async function exchange(session, message, expectedType = message.bytes[4] + 1) {
    const outcome = await session.request(message.finish(), 65536);
    assert.equal(outcome.kind, "reply");
    const reply = new Reply(outcome.bytes);
    assert.equal(reply.type, expectedType);
    assert.equal(reply.tag, message.bytes[5] | message.bytes[6] << 8);
    return reply;
}

async function versionAndAttach(session) {
    const version = await exchange(session, new Message(100, 0xffff).u32(65536).string("9P2000.L"));
    assert.equal(version.u32(), 65536);
    assert.equal(version.string(), "9P2000.L");
    const attach = await exchange(session, new Message(104).u32(1).u32(0xffffffff).string("student").string("").u32(1000));
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

test("host API enforces configured file and tree quotas atomically", () => {
    assert.equal(MAX_FILE_SIZE, 256 * 1024 * 1024);
    const server = new Memory9PServer({}, { maxFileBytes: 4, maxTreeBytes: 5 });
    server.writeFile("maximum.bin", new Uint8Array(4));
    assert.throws(
        () => server.writeFile("too-large.bin", new Uint8Array(5)),
        (error) => error instanceof P9Error && error.errno === 27,
    );
    assert.throws(
        () => server.writeFile("other.bin", new Uint8Array(2)),
        (error) => error instanceof P9Error && error.errno === 28,
    );
    assert.deepEqual(server.listFiles(), ["maximum.bin"]);
    assert.throws(() => new Memory9PServer({ a: "1234", b: "12" }, { maxTreeBytes: 5 }));
});

test("tree replacement validates before retiring shared-session fids", async () => {
    const server = new Memory9PServer({ old: "old" }, { maxTreeBytes: 4 });
    const session = server.connect();
    await versionAndAttach(session);
    await exchange(session, new Message(110).u32(1).u32(2).u16(1).string("old"));
    assert.throws(() => server.loadTree({ invalid: "12345" }));
    const retained = await exchange(session, new Message(116).u32(2).u64(0).u32(3));
    assert.equal(retained.u32(), 3);
    assert.equal(decoder.decode(retained.remaining()), "old");
    server.loadTree({ next: "next" });
    const stale = await exchange(session, new Message(116).u32(2).u64(0).u32(3), 7);
    assert.equal(stale.u32(), 9);
    assert.deepEqual(server.listFiles(), ["next"]);
    session.close();
});

test("9P reads and writes regular files", async () => {
    const server = new Memory9PServer({ "hello.txt": "hello" });
    const session = server.connect();
    await versionAndAttach(session);

    const walk = await exchange(session, new Message(110).u32(1).u32(2).u16(1).string("hello.txt"));
    assert.equal(walk.u16(), 1);
    assert.equal(walk.qid().type, 0);
    (await exchange(session, new Message(12).u32(2).u32(2))).qid();

    const read = await exchange(session, new Message(116).u32(2).u64(1).u32(3));
    assert.equal(read.u32(), 3);
    assert.equal(decoder.decode(read.remaining()), "ell");

    const write = await exchange(session, new Message(118).u32(2).u64(5).u32(6).data(encoder.encode(" world")));
    assert.equal(write.u32(), 6);
    assert.equal(decoder.decode(server.readFile("hello.txt")), "hello world");

    await exchange(
        session,
        new Message(26).u32(2).u32(8).u32(0).u32(0).u32(0).u64(5)
            .u64(0).u64(0).u64(0).u64(0),
    );
    assert.equal(decoder.decode(server.readFile("hello.txt")), "hello");

    const getattr = await exchange(session, new Message(24).u32(2).u64(0x3fff));
    assert.equal(getattr.u64(), 0x3fff);
    getattr.qid();
    getattr.u32(); getattr.u32(); getattr.u32(); getattr.u64(); getattr.u64();
    assert.equal(getattr.u64(), 5);
    await exchange(session, new Message(120).u32(2));
});

test("9P creates, lists, renames, links, and removes tree entries", async () => {
    const server = new Memory9PServer();
    const session = server.connect();
    const changes = [];
    server.subscribe((change) => changes.push(change));
    await versionAndAttach(session);

    (await exchange(session, new Message(72).u32(1).string("build").u32(0o755).u32(1000))).qid();
    await exchange(session, new Message(110).u32(1).u32(2).u16(1).string("build"));
    (await exchange(session, new Message(14).u32(2).string("output").u32(2).u32(0o644).u32(1000))).qid();
    await exchange(session, new Message(118).u32(2).u64(0).u32(2).data(encoder.encode("ok")));

    const directory = await exchange(session, new Message(40).u32(1).u64(0).u32(4096));
    const directorySize = directory.u32();
    assert.ok(directorySize > 0);
    assert.equal(directory.qid().type, 0x80);
    directory.u64();
    assert.equal(directory.u8(), 4);
    assert.equal(directory.string(), "build");

    await exchange(session, new Message(74).u32(1).string("build").u32(1).string("out"));
    (await exchange(session, new Message(16).u32(1).string("latest").string("out/output").u32(1000))).qid();
    await exchange(session, new Message(110).u32(1).u32(3).u16(1).string("latest"));
    assert.equal((await exchange(session, new Message(22).u32(3))).string(), "out/output");

    await exchange(session, new Message(76).u32(1).string("latest").u32(0));
    const notEmpty = await exchange(session, new Message(76).u32(1).string("out").u32(0x200), 7);
    assert.equal(notEmpty.u32(), 39);
    await exchange(session, new Message(110).u32(1).u32(4).u16(1).string("out"));
    await exchange(session, new Message(76).u32(4).string("output").u32(0));
    await exchange(session, new Message(76).u32(1).string("out").u32(0x200));
    assert.deepEqual(server.listFiles(), []);
    assert.ok(changes.every((change) => change.source === "guest"));
});

test("malformed frames and unsupported large offsets return Rlerror", async () => {
    const server = new Memory9PServer({ file: "x" });
    const session = server.connect();
    const malformed = new Message(100, 0xffff).u32(65536).string("9P2000.L").finish();
    new DataView(malformed.buffer).setUint32(0, malformed.length + 1, true);
    const malformedOutcome = await session.request(malformed, 65536);
    assert.equal(new Reply(malformedOutcome.bytes).u32(), 71);

    await versionAndAttach(session);
    await exchange(session, new Message(110).u32(1).u32(2).u16(1).string("file"));
    const largeOutcome = await session.request(new Message(116).u32(2).u64(0, 1).u32(1).finish(), 65536);
    const reply = new Reply(largeOutcome.bytes);
    assert.equal(reply.type, 7);
    assert.equal(reply.u32(), 27);
});

test("connections have independent fid and version state over one tree", async () => {
    const server = new Memory9PServer({ "shared.txt": "one" });
    const first = server.connect();
    const second = server.connect();
    const request = async (session, message) => {
        const outcome = await session.request(message.finish(), 65536);
        assert.equal(outcome.kind, "reply");
        return new Reply(outcome.bytes);
    };

    await request(first, new Message(100, 0xffff).u32(4096).string("9P2000.L"));
    await request(second, new Message(100, 0xffff).u32(8192).string("9P2000.L"));
    await request(first, new Message(104).u32(1).u32(0xffffffff).string("one").string("").u32(1));
    await request(second, new Message(104).u32(1).u32(0xffffffff).string("two").string("").u32(2));
    await request(first, new Message(110).u32(1).u32(2).u16(1).string("shared.txt"));
    await request(second, new Message(110).u32(1).u32(2).u16(1).string("shared.txt"));
    await request(first, new Message(118).u32(2).u64(0).u32(3).data(encoder.encode("two")));
    const read = await request(second, new Message(116).u32(2).u64(0).u32(3));
    assert.equal(read.u32(), 3);
    assert.equal(decoder.decode(read.remaining()), "two");

    first.close();
    assert.rejects(() => first.request(new Message(120).u32(2).finish(), 65536));
    assert.equal((await request(second, new Message(24).u32(2).u64(0x3fff))).type, 25);
    second.close();
});

test("flush suppresses the original response before Rflush and permits tag reuse", async () => {
    const session = new Memory9PServer({ file: "x" }).connect();
    const send = (message) => session.request(message.finish(), 65536);
    await send(new Message(100, 0xffff).u32(65536).string("9P2000.L"));
    await send(new Message(104, 1).u32(1).u32(0xffffffff).string("user").string("").u32(1));
    await send(new Message(110, 2).u32(1).u32(2).u16(1).string("file"));

    assert.equal((await send(new Message(116, 4).u32(2).u64(0).u32(1))).kind, "reply");
    assert.equal(new Reply((await send(new Message(108, 8).u16(4))).bytes).type, 109);

    const order = [];
    const original = send(new Message(116, 5).u32(2).u64(0).u32(1))
        .then((outcome) => order.push(["original", outcome.kind]));
    const flushed = send(new Message(108, 6).u16(5)).then((outcome) => {
        order.push(["flush", outcome.kind]);
        assert.equal(new Reply(outcome.bytes).type, 109);
    });
    const reused = send(new Message(116, 5).u32(2).u64(0).u32(1));
    await Promise.all([original, flushed, reused]);
    assert.deepEqual(order, [["original", "suppressed"], ["flush", "reply"]]);
    assert.equal(new Reply((await reused).bytes).type, 117);

    const repeated = await send(new Message(108, 7).u16(5));
    assert.equal(new Reply(repeated.bytes).type, 109);
    session.close();
});

test("Tversion suppresses older session work and clears fids", async () => {
    const session = new Memory9PServer({ file: "x" }).connect();
    const send = (message) => session.request(message.finish(), 65536);
    await send(new Message(100, 0xffff).u32(65536).string("9P2000.L"));
    await send(new Message(104, 1).u32(1).u32(0xffffffff).string("user").string("").u32(1));
    await send(new Message(110, 2).u32(1).u32(2).u16(1).string("file"));
    const old = send(new Message(116, 3).u32(2).u64(0).u32(1));
    const version = send(new Message(100, 0xffff).u32(4096).string("9P2000.L"));
    assert.equal((await old).kind, "suppressed");
    assert.equal(new Reply((await version).bytes).type, 101);
    const staleFid = new Reply((await send(new Message(116, 4).u32(2).u64(0).u32(1))).bytes);
    assert.equal(staleFid.type, 7);
    assert.equal(staleFid.u32(), 9);
    session.close();
});

test("hard links preserve QIDs and open unlinked inodes until the last clunk", async () => {
    const server = new Memory9PServer({ original: "abc" }, { maxInodes: 2 });
    const session = server.connect();
    await versionAndAttach(session);
    const original = await exchange(session, new Message(110).u32(1).u32(2).u16(1).string("original"));
    assert.equal(original.u16(), 1);
    const originalQid = original.qid();
    await exchange(session, new Message(70).u32(1).u32(2).string("alias"));
    const alias = await exchange(session, new Message(110).u32(1).u32(3).u16(1).string("alias"));
    assert.equal(alias.u16(), 1);
    assert.equal(alias.qid().path, originalQid.path);

    const getattr = await exchange(session, new Message(24).u32(2).u64(0x3fff));
    getattr.u64(); getattr.qid(); getattr.u32(); getattr.u32(); getattr.u32();
    assert.equal(getattr.u64(), 2);
    await exchange(session, new Message(76).u32(1).string("original").u32(0));
    await exchange(session, new Message(76).u32(1).string("alias").u32(0));
    await exchange(session, new Message(118).u32(2).u64(0).u32(1).data(encoder.encode("z")));
    assert.throws(
        () => server.writeFile("new", "x"),
        (error) => error instanceof P9Error && error.errno === 28,
    );
    await exchange(session, new Message(120).u32(2));
    await exchange(session, new Message(120).u32(3));
    server.writeFile("new", "x");
    assert.equal(decoder.decode(server.readFile("new")), "x");
    session.close();
});

test("directory cookies remain stable across insertion and same-directory rename", async () => {
    const server = new Memory9PServer({ a: "a", b: "b" });
    const session = server.connect();
    await versionAndAttach(session);
    const first = await exchange(session, new Message(40).u32(1).u64(0).u32(25));
    assert.equal(first.u32(), 25);
    first.qid();
    const cookie = first.u64();
    first.u8();
    assert.equal(first.string(), "a");
    server.writeFile("c", "c");
    server.rename("b", "d");
    const rest = await exchange(session, new Message(40).u32(1).u64(cookie).u32(4096));
    rest.u32();
    const names = [];
    while (rest.remaining().length !== 0) {
        rest.qid(); rest.u64(); rest.u8(); names.push(rest.string());
    }
    assert.deepEqual(names, ["d", "c"]);
    session.close();
});

test("append and truncation enforce quotas across shared sessions", async () => {
    const server = new Memory9PServer({ file: "abc" }, { maxFileBytes: 5, maxTreeBytes: 5 });
    const first = server.connect();
    const second = server.connect();
    for (const session of [first, second]) {
        await versionAndAttach(session);
        await exchange(session, new Message(110).u32(1).u32(2).u16(1).string("file"));
        await exchange(session, new Message(12).u32(2).u32(0x400));
    }
    const writes = await Promise.all([
        exchange(first, new Message(118).u32(2).u64(0).u32(1).data(encoder.encode("d"))),
        exchange(second, new Message(118).u32(2).u64(0).u32(1).data(encoder.encode("e"))),
    ]);
    assert.deepEqual(writes.map((reply) => reply.u32()), [1, 1]);
    assert.equal(decoder.decode(server.readFile("file")), "abcde");
    const overflow = await exchange(first, new Message(118).u32(2).u64(0).u32(1).data(encoder.encode("f")), 7);
    assert.equal(overflow.u32(), 27);
    await exchange(
        first,
        new Message(26).u32(2).u32(8).u32(0).u32(0).u32(0).u64(2)
            .u64(0).u64(0).u64(0).u64(0),
    );
    server.writeFile("other", "xyz");
    assert.equal(decoder.decode(server.readFile("file")), "ab");
    first.close(); second.close();
});

test("byte-range locks conflict across sessions and release on close", async () => {
    const server = new Memory9PServer({ file: "data" });
    const first = server.connect();
    const second = server.connect();
    for (const session of [first, second]) {
        await versionAndAttach(session);
        await exchange(session, new Message(110).u32(1).u32(2).u16(1).string("file"));
    }
    const lock = (session, client) => exchange(
        session,
        new Message(52).u32(2).u8(1).u32(0).u64(0).u64(0).u32(7).string(client),
    );
    assert.equal((await lock(first, "first")).u8(), 0);
    assert.equal((await lock(second, "second")).u8(), 1);
    const conflict = await exchange(
        second,
        new Message(54).u32(2).u8(1).u64(0).u64(0).u32(7).string("second"),
    );
    assert.equal(conflict.u8(), 1);
    assert.equal(conflict.u64(), 0);
    assert.equal(conflict.u64(), 0);
    assert.equal(conflict.u32(), 7);
    assert.equal(conflict.string(), "first");
    first.close();
    assert.equal((await lock(second, "second")).u8(), 0);
    second.close();
});

test("rename replacement is atomic while existing fids retain their inode", async () => {
    const server = new Memory9PServer({ source: "new", target: "old" });
    const session = server.connect();
    await versionAndAttach(session);
    const source = await exchange(session, new Message(110).u32(1).u32(2).u16(1).string("source"));
    source.u16();
    const sourceQid = source.qid();
    const target = await exchange(session, new Message(110).u32(1).u32(3).u16(1).string("target"));
    target.u16();
    const targetQid = target.qid();
    server.rename("source", "target");
    assert.equal(decoder.decode(server.readFile("target")), "new");
    const renamed = await exchange(session, new Message(110).u32(1).u32(4).u16(1).string("target"));
    renamed.u16();
    assert.equal(renamed.qid().path, sourceQid.path);
    const old = await exchange(session, new Message(116).u32(3).u64(0).u32(3));
    assert.equal(decoder.decode(old.remaining().subarray(4)), "old");
    assert.notEqual(sourceQid.path, targetQid.path);
    session.close();
});
