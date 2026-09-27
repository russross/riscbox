import assert from "node:assert/strict";
import { test } from "node:test";
import { openHttpBlockProvider, parseBlockManifest } from "../build/js/block/http.js";
import { ArrayBlockProvider } from "../build/js/block/array.js";

test("array provider writes through its exact view and survives reset", () => {
    const backing = new Uint8Array(2048);
    const view = backing.subarray(512, 1536);
    const provider = new ArrayBlockProvider(view);
    assert.equal(provider.capacitySectors, 2n);
    const payload = new Uint8Array(512).fill(7);
    provider.write(1n, payload);
    provider.reset();
    assert.equal(backing[1024], 7);
    const result = provider.read(1n, 512);
    result[0] = 8;
    assert.equal(backing[1024], 7);
    assert.throws(() => provider.write(2n, payload), RangeError);
    provider.close();
    assert.equal(backing[1024], 7);
    assert.throws(() => provider.read(0n, 512), /closed/);
    assert.throws(() => new ArrayBlockProvider(new Uint8Array(513)), RangeError);
});

test("split manifest dimensions and prefetch are validated", () => {
    assert.deepEqual(parseBlockManifest("{ block_size: 1, n_block: 2, prefetch: [1,], }"),
        { blockSize: 1024, blockCount: 2, prefetch: [1] });
    assert.deepEqual(parseBlockManifest("{/* layout */ \"block_size\": 1, // KiB\n n_block: 1}"),
        { blockSize: 1024, blockCount: 1, prefetch: [] });
    assert.throws(() => parseBlockManifest("{ block_size: 3, n_block: 2 }"), RangeError);
    assert.throws(() => parseBlockManifest("{ block_size: 1, n_block: 2, prefetch: [2] }"), RangeError);
});

test("HTTP provider retains CoW writes across reset and bounds clean cache", async () => {
    const blocks = [new Uint8Array(1024).fill(1), new Uint8Array(1024).fill(2),
        new Uint8Array(1024).fill(3), new Uint8Array(1024).fill(4)];
    const requests = [];
    const fetch = async (url, options) => {
        requests.push([url, options.cache]);
        if (url.endsWith("blk.txt")) return { ok: true, text: async () =>
            "{ block_size: 1, n_block: 4, prefetch: [0] }" };
        const index = Number(/blk(\d+)\.bin$/.exec(url)?.[1]);
        return { ok: true, arrayBuffer: async () => blocks[index].buffer };
    };
    const provider = await openHttpBlockProvider(
        "https://example.test/drive-abcdef12/blk.txt", { fetch, cacheBytes: 1024 });
    assert.equal(provider.capacitySectors, 8n);
    assert.deepEqual(await provider.read(1n, 1536),
        Uint8Array.from([...new Uint8Array(512).fill(1),
            ...new Uint8Array(1024).fill(2)]));
    await provider.write(1n, new Uint8Array(512).fill(9));
    provider.reset();
    assert.deepEqual(await provider.read(0n, 1024),
        Uint8Array.from([...new Uint8Array(512).fill(1),
            ...new Uint8Array(512).fill(9)]));
    assert.equal(requests[0][1], "force-cache");
    assert.equal(requests[1][1], "force-cache");
    await assert.rejects(provider.read(-1n, 512), RangeError);
    provider.close();
    await assert.rejects(provider.read(0n, 512), /closed/);
});

test("HTTP failures and wrong block lengths reject guest requests", async () => {
    const fetch = async (url) => url.endsWith("blk.txt")
        ? { ok: true, text: async () => "{ block_size: 1, n_block: 1 }" }
        : { ok: true, arrayBuffer: async () => new Uint8Array(8).buffer };
    const provider = await openHttpBlockProvider("https://example.test/blk.txt", { fetch });
    await assert.rejects(provider.read(0n, 512), /expected 1024/);
});

test("HTTP reset retires an in-flight write without changing its overlay", async () => {
    let release;
    const body = new Promise((resolve) => { release = resolve; });
    const fetch = async (url) => url.endsWith("blk.txt")
        ? { ok: true, text: async () => "{ block_size: 1, n_block: 1 }" }
        : { ok: true, arrayBuffer: async () => body };
    const provider = await openHttpBlockProvider("https://example.test/blk.txt", { fetch });
    const write = provider.write(0n, new Uint8Array(512).fill(9));
    provider.reset();
    release(new Uint8Array(1024).fill(1).buffer);
    await assert.rejects(write, /retired by reset/);
    assert.deepEqual(await provider.read(0n, 1024), new Uint8Array(1024).fill(1));
    await provider.write(1n, new Uint8Array(512).fill(7));
    assert.deepEqual(await provider.read(0n, 1024),
        Uint8Array.from([...new Uint8Array(512).fill(1),
            ...new Uint8Array(512).fill(7)]));
});
