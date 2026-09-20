import assert from "node:assert/strict";
import test from "node:test";

import {
    MAX_FILE_SIZE,
    Memory9PServer,
    SeedBuilder,
    createHttpsSeedPlugin,
    createTarSeedPlugin,
} from "../build/js/p9/index.js";

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

function ok(result) {
    assert.equal(result.kind, "ok");
    return result.value;
}

function errorCode(result) {
    assert.equal(result.kind, "error");
    return result.error.code;
}

function deferred() {
    let resolve;
    let reject;
    const promise = new Promise((resolvePromise, rejectPromise) => {
        resolve = resolvePromise;
        reject = rejectPromise;
    });
    return { promise, resolve, reject };
}

test("host API uses copied bytes, nested paths, and notifications", () => {
    const original = encoder.encode("one");
    const server = new Memory9PServer({ src: { "main.c": original } });
    const changes = [];
    server.subscribe((change) => changes.push(change));
    original[0] = 0;
    assert.equal(decoder.decode(ok(server.readFile("src/main.c"))), "one");

    const read = ok(server.readFile("src/main.c"));
    read[0] = 0;
    assert.equal(decoder.decode(ok(server.readFile("src/main.c"))), "one");

    ok(server.writeFile("src/main.c", "two", "editor"));
    ok(server.writeFile("build/result.txt", "ok"));
    ok(server.rename("build/result.txt", "result.txt"));
    ok(server.remove("result.txt"));
    server.loadFiles({ "__proto__/safe.txt": "safe" });
    assert.equal(decoder.decode(ok(server.readFile("__proto__/safe.txt"))), "safe");
    assert.deepEqual(ok(server.listFiles()), ["__proto__/safe.txt"]);
    assert.deepEqual(changes.map((change) => change.kind), ["write", "create", "rename", "remove", "reset"]);
    assert.equal(changes[0].source, "editor");
});

test("host API enforces configured file and tree quotas atomically", () => {
    assert.equal(MAX_FILE_SIZE, 256 * 1024 * 1024);
    const server = new Memory9PServer({}, { maxFileBytes: 4, maxTreeBytes: 5 });
    ok(server.writeFile("maximum.bin", new Uint8Array(4)));
    assert.equal(errorCode(server.writeFile("too-large.bin", new Uint8Array(5))), "too-large");
    assert.equal(errorCode(server.writeFile("other.bin", new Uint8Array(2))), "no-space");
    assert.deepEqual(ok(server.listFiles()), ["maximum.bin"]);
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
    assert.deepEqual(ok(server.listFiles()), ["next"]);
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
    assert.equal(decoder.decode(ok(server.readFile("hello.txt"))), "hello world");

    await exchange(
        session,
        new Message(26).u32(2).u32(8).u32(0).u32(0).u32(0).u64(5)
            .u64(0).u64(0).u64(0).u64(0),
    );
    assert.equal(decoder.decode(ok(server.readFile("hello.txt"))), "hello");

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
    assert.deepEqual(ok(server.listFiles()), []);
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
    assert.equal(errorCode(server.writeFile("new", "x")), "no-space");
    await exchange(session, new Message(120).u32(2));
    await exchange(session, new Message(120).u32(3));
    ok(server.writeFile("new", "x"));
    assert.equal(decoder.decode(ok(server.readFile("new"))), "x");
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
    ok(server.writeFile("c", "c"));
    ok(server.rename("b", "d"));
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
    assert.equal(decoder.decode(ok(server.readFile("file"))), "abcde");
    const overflow = await exchange(first, new Message(118).u32(2).u64(0).u32(1).data(encoder.encode("f")), 7);
    assert.equal(overflow.u32(), 27);
    await exchange(
        first,
        new Message(26).u32(2).u32(8).u32(0).u32(0).u32(0).u64(2)
            .u64(0).u64(0).u64(0).u64(0),
    );
    ok(server.writeFile("other", "xyz"));
    assert.equal(decoder.decode(ok(server.readFile("file"))), "ab");
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
    ok(server.rename("source", "target"));
    assert.equal(decoder.decode(ok(server.readFile("target"))), "new");
    const renamed = await exchange(session, new Message(110).u32(1).u32(4).u16(1).string("target"));
    renamed.u16();
    assert.equal(renamed.qid().path, sourceQid.path);
    const old = await exchange(session, new Message(116).u32(3).u64(0).u32(3));
    assert.equal(decoder.decode(old.remaining().subarray(4)), "old");
    assert.notEqual(sourceQid.path, targetQid.path);
    session.close();
});

test("seed builder validates paths and shares one lazy inode across hard links", async () => {
    const entries = new SeedBuilder()
        .addDirectory("src")
        .addFile("src/main.c", 4, "body", { inodeKey: "source" })
        .addHardLink("copy.c", "source")
        .addSymlink("latest", "src/main.c")
        .finish();
    assert.throws(() => new SeedBuilder().addHardLink("lost", "missing").finish(), /dangling/);
    assert.throws(
        () => new SeedBuilder().addFile("same", 0, "a").addDirectory("same").finish(),
        /duplicate/,
    );
    const pending = deferred();
    let loads = 0;
    const plugin = Object.freeze({
        entries,
        loader: { load: () => { loads += 1; return pending.promise; } },
    });
    const server = new Memory9PServer({}, {}, plugin);
    assert.throws(() => new Memory9PServer({}, { maxTreeBytes: 3 }, plugin));
    assert.deepEqual(server.readFile("src/main.c"), {
        kind: "not-loaded",
        paths: ["copy.c", "src/main.c"],
    });
    const first = server.readFileAsync("src/main.c");
    const second = server.readFileAsync("copy.c");
    assert.equal(loads, 1);
    pending.resolve(encoder.encode("code"));
    assert.equal(decoder.decode(ok(await first)), "code");
    assert.equal(decoder.decode(ok(await second)), "code");
    assert.equal(decoder.decode(ok(server.readFile("copy.c"))), "code");
});

test("seed failures are retained, validate length, and require explicit retry", async () => {
    const entries = new SeedBuilder().addFile("file", 2, "key").finish();
    let loads = 0;
    const server = new Memory9PServer({}, {}, {
        entries,
        loader: {
            async load() {
                loads += 1;
                if (loads === 1) throw new Error("offline");
                if (loads === 2) return encoder.encode("bad");
                return encoder.encode("ok");
            },
        },
    });
    assert.equal(errorCode(await server.load(["file"])), "io");
    assert.equal(errorCode(await server.load(["file"])), "io");
    assert.equal(loads, 1);
    assert.equal(errorCode(await server.load(["file"], true)), "io");
    assert.equal(loads, 2);
    assert.equal(decoder.decode(ok(await server.readFileAsync("file", true))), "ok");
    assert.equal(loads, 3);
});

test("whole-file replacement wins load races and deletion never resurrects a seed", async () => {
    const entries = new SeedBuilder().addFile("race", 3, "race").addFile("deleted", 3, "deleted").finish();
    const pending = new Map([["race", deferred()], ["deleted", deferred()]]);
    let loads = 0;
    const server = new Memory9PServer({}, {}, {
        entries,
        loader: { load: (key) => { loads += 1; return pending.get(key).promise; } },
    });
    const raced = server.readFileAsync("race");
    ok(server.writeFile("race", "new"));
    pending.get("race").resolve(encoder.encode("old"));
    assert.equal(decoder.decode(ok(await raced)), "new");

    const deleted = server.load(["deleted"]);
    ok(server.remove("deleted"));
    pending.get("deleted").resolve(encoder.encode("old"));
    assert.equal((await deleted).kind, "ok");
    assert.deepEqual(ok(server.listFiles()), ["race"]);
    assert.equal(loads, 2);
});

test("9P clients share one seed load and receive ordinary EIO on loader failure", async () => {
    const entries = new SeedBuilder().addFile("file", 2, "key").finish();
    const pending = deferred();
    let loads = 0;
    const server = new Memory9PServer({}, {}, {
        entries,
        loader: { load: () => { loads += 1; return pending.promise; } },
    });
    const first = server.connect();
    const second = server.connect();
    for (const session of [first, second]) {
        await versionAndAttach(session);
        await exchange(session, new Message(110).u32(1).u32(2).u16(1).string("file"));
    }
    const firstRead = exchange(first, new Message(116).u32(2).u64(0).u32(2));
    const secondRead = exchange(second, new Message(116).u32(2).u64(0).u32(2));
    await Promise.resolve();
    assert.equal(loads, 1);
    pending.resolve(encoder.encode("ok"));
    for (const read of await Promise.all([firstRead, secondRead])) {
        assert.equal(read.u32(), 2);
        assert.equal(decoder.decode(read.remaining()), "ok");
    }
    first.close(); second.close();

    const failed = new Memory9PServer({}, {}, {
        entries,
        loader: { load: async () => { throw new Error("offline"); } },
    }).connect();
    await versionAndAttach(failed);
    await exchange(failed, new Message(110).u32(1).u32(2).u16(1).string("file"));
    const reply = await exchange(failed, new Message(116).u32(2).u64(0).u32(2), 7);
    assert.equal(reply.u32(), 5);
    failed.close();
});

test("HTTPS and tar seed plugins expose immutable lazy deployments", async () => {
    const httpsPlugin = createHttpsSeedPlugin(
        { files: [{ path: "hello.txt", size: 2, source: "data:application/octet-stream,hi" }] },
        new URL("https://example.invalid/base/"),
    );
    const https = new Memory9PServer({}, {}, httpsPlugin);
    assert.equal(decoder.decode(ok(await https.readFileAsync("hello.txt"))), "hi");

    const archive = new Uint8Array(2048);
    archive.set(encoder.encode("file.txt"), 0);
    archive.set(encoder.encode("00000000003\0"), 124);
    archive[156] = 48;
    archive.set(encoder.encode("tar"), 512);
    const tarPlugin = createTarSeedPlugin(archive);
    assert.ok(Object.isFrozen(tarPlugin.entries));
    const tar = new Memory9PServer({}, {}, tarPlugin);
    assert.equal(decoder.decode(ok(await tar.readFileAsync("file.txt"))), "tar");

    const entries = new SeedBuilder().addFile("pinned", 3, "key").finish();
    const mutablePlugin = { entries, loader: { load: async () => encoder.encode("old") } };
    const pinned = new Memory9PServer({}, {}, mutablePlugin);
    mutablePlugin.loader = { load: async () => encoder.encode("new") };
    assert.equal(decoder.decode(ok(await pinned.readFileAsync("pinned"))), "old");
});
