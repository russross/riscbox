import assert from "node:assert/strict";
import test from "node:test";
import { SeedBuilder, createHttpsSeedPlugin, createTarSeedPlugin } from "../build/js/p9/index.js";

test("seed manifests validate paths, parents and shared inode keys without fetching bodies", () => {
    assert.throws(() => new SeedBuilder().addFile("../bad", 1, "key"), /invalid seed path/);
    assert.throws(() => new SeedBuilder().addFile("file", 1, "key").addDirectory("file/child").finish(), /non-directory/);
    assert.throws(() => new SeedBuilder().addHardLink("alias", "absent").finish(), /dangling/);
    const plugin = createHttpsSeedPlugin({ files: [{ path: "dir/file", size: 3, source: "objects/123" }] }, new URL("https://example.org/tree/"));
    assert.equal(plugin.entries[0].key, "https://example.org/tree/objects/123");
    assert.equal(plugin.entries[0].size, 3);
    const literal = createHttpsSeedPlugin({ files: [{ path: "dir/a#?%.txt", size: 0 }] }, new URL("https://example.org/tree/"));
    assert.equal(literal.entries[0].key, "https://example.org/tree/dir/a%23%3F%25.txt");
});

test("HTTP source forwards cancellation and reports failures on demand", async () => {
    const originalFetch = globalThis.fetch;
    const controller = new AbortController();
    let calls = 0;
    globalThis.fetch = async (url, options) => {
        calls++;
        assert.equal(url, "https://example.org/file");
        assert.equal(options.signal, controller.signal);
        return { ok: false, status: 403 };
    };
    try {
        const plugin = createHttpsSeedPlugin({ files: [{ path: "file", size: 3 }] }, new URL("https://example.org/"));
        assert.equal(calls, 0);
        await assert.rejects(plugin.loader.load(plugin.entries[0].key, controller.signal), /HTTP 403/);
        assert.equal(calls, 1);
    } finally { globalThis.fetch = originalFetch; }
});

test("tar source declares shared files and copies bytes only when read", async () => {
    const archive = new Uint8Array(2048);
    const encoder = new TextEncoder();
    archive.set(encoder.encode("file"), 0);
    archive.set(encoder.encode("00000000003"), 124);
    archive[156] = 48;
    archive.set(encoder.encode("old"), 512);
    archive.set(encoder.encode("alias"), 1024);
    archive[1024 + 156] = 49;
    archive.set(encoder.encode("file"), 1024 + 157);
    const plugin = createTarSeedPlugin(archive);
    assert.equal(plugin.entries.length, 2);
    assert.equal(plugin.entries[0].inodeKey, plugin.entries[1].inodeKey);
    const first = await plugin.loader.load(plugin.entries[0].key, new AbortController().signal);
    first[0] = 0;
    assert.equal(archive[512], 111);
    assert.throws(() => createTarSeedPlugin(archive.subarray(0, 513)), /truncated/);
});
