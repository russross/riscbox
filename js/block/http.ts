import type { BlockProvider } from "./index.js";

const SECTOR_SIZE = 512;
const CLUSTER_SIZE = 4096;
const DEFAULT_CACHE_BYTES = 16 * 1024 * 1024;
const MAX_U64 = (1n << 64n) - 1n;

export interface HttpBlockOptions {
    cacheBytes?: number;
    fetch?: typeof fetch;
}

export interface BlockManifest {
    blockSize: number;
    blockCount: number;
    prefetch: number[];
}

/** Parse the integer-only subset used by split-image descriptors. */
export function parseBlockManifest(source: string): BlockManifest {
    // The splitter emits unquoted keys and a trailing comma. This lexer also
    // accepts the comments and quoted keys used by deployed descriptors.
    const tokens = source.match(/\/\*[\s\S]*?\*\/|\/\/[^\n]*|"(?:\\.|[^"\\])*"|[A-Za-z_$][\w$]*|0[xX][\da-fA-F]+|\d+|[{}\[\]:,]|\S/g) ?? [];
    const values = tokens.filter((token) => !token.startsWith("//") && !token.startsWith("/*"));
    let offset = 0;
    const take = () => values[offset++];
    const expect = (expected: string) => {
        if (take() !== expected) throw new SyntaxError(`expected '${expected}' in block manifest`);
    };
    const integer = (): number => {
        const token = take();
        if (!token || !/^(?:0[xX][\da-fA-F]+|\d+)$/.test(token))
            throw new SyntaxError("expected integer in block manifest");
        const value = Number(token);
        if (!Number.isSafeInteger(value)) throw new RangeError("block manifest integer is too large");
        return value;
    };

    // Only fields that affect the disk layout or startup fetches are accepted.
    expect("{");
    let blockKib: number | undefined;
    let blockCount: number | undefined;
    let prefetch: number[] = [];
    while (values[offset] !== "}") {
        const key = take()?.replace(/^"|"$/g, "");
        expect(":");
        if (key === "block_size") blockKib = integer();
        else if (key === "n_block") blockCount = integer();
        else if (key === "prefetch") {
            expect("[");
            while (values[offset] !== "]") {
                prefetch.push(integer());
                if (values[offset] === "]") break;
                expect(",");
            }
            expect("]");
        } else throw new SyntaxError(`unknown block manifest field ${key}`);
        if (values[offset] === "}") break;
        expect(",");
    }
    expect("}");
    if (offset !== values.length) throw new SyntaxError("trailing block manifest content");

    // Rust's legacy descriptor uses KiB and signed 32-bit positive counts.
    const blockSize = (blockKib ?? 0) * 1024;
    if ((blockKib ?? 0) > 0x7fffffff || !Number.isSafeInteger(blockSize) ||
        blockSize < SECTOR_SIZE ||
        !Number.isInteger(Math.log2(blockSize)) || blockCount === undefined ||
        blockCount < 1 || blockCount > 0x7fffffff)
        throw new RangeError("invalid block manifest dimensions");
    if (prefetch.some((index) => index < 0 || index >= blockCount))
        throw new RangeError("invalid prefetch block");
    return { blockSize, blockCount, prefetch };
}

/** Create a session-local, copy-on-write provider for a split HTTP disk. */
export async function openHttpBlockProvider(
    manifestUrl: string, options: HttpBlockOptions = {},
): Promise<HttpBlockProvider> {
    const request = options.fetch ?? globalThis.fetch;
    if (typeof request !== "function") throw new Error("HTTP fetch is not available");
    // Content-named paths may use the browser's cache across VM sessions.
    const response = await request(manifestUrl, { cache: cacheMode(manifestUrl) });
    if (!response.ok) throw new Error(`block manifest HTTP ${response.status}`);
    const manifest = parseBlockManifest(await response.text());
    return new HttpBlockProvider(manifestUrl, manifest, request,
        options.cacheBytes ?? DEFAULT_CACHE_BYTES);
}

function cacheMode(url: string): RequestCache {
    return /(?:^|\/)[^/?#]*-[0-9a-f]{8,64}(?:\.|\/|[?#]|$)/i.test(url)
        ? "force-cache" : "default";
}

export class HttpBlockProvider implements BlockProvider {
    readonly capacitySectors: bigint;
    private readonly blockSize: number;
    private readonly blockCount: number;
    private readonly blockBase: string;
    private readonly request: typeof fetch;
    private cacheLimit: number;
    // Map insertion order is the clean-block LRU order. Overlay clusters are
    // permanent for the current VM session and never enter this cache.
    private readonly cache = new Map<number, Uint8Array>();
    private readonly pending = new Map<number, Promise<Uint8Array>>();
    private readonly overlays = new Map<bigint, Uint8Array>();
    private closed = false;
    private generation = 0;

    constructor(manifestUrl: string, manifest: BlockManifest, request: typeof fetch,
        cacheBytes = DEFAULT_CACHE_BYTES) {
        if (!Number.isSafeInteger(cacheBytes) || cacheBytes < 1)
            throw new RangeError("cacheBytes must be a positive safe integer");
        this.blockSize = manifest.blockSize;
        this.blockCount = manifest.blockCount;
        this.blockBase = manifestUrl.slice(0, manifestUrl.lastIndexOf("/") + 1);
        this.request = request;
        this.cacheLimit = Math.max(cacheBytes, this.blockSize);
        this.capacitySectors = BigInt(this.blockCount) * BigInt(this.blockSize / SECTOR_SIZE);
        if (this.capacitySectors > MAX_U64) throw new RangeError("block image exceeds capacity limit");
        // A failed hint is retried when a guest request actually needs the block.
        for (const index of manifest.prefetch) void this.load(index).catch(() => {});
    }

    private validate(sector: bigint, length: number): bigint {
        if (this.closed) throw new Error("block provider is closed");
        if (typeof sector !== "bigint" || sector < 0n || !Number.isSafeInteger(length) ||
            length < SECTOR_SIZE || length % SECTOR_SIZE !== 0 ||
            sector + BigInt(length / SECTOR_SIZE) > this.capacitySectors)
            throw new RangeError("block request is outside the image");
        return sector * BigInt(SECTOR_SIZE);
    }

    private async load(index: number): Promise<Uint8Array> {
        // A pending fetch is shared by all requests for the same clean block.
        const cached = this.cache.get(index);
        if (cached) {
            this.cache.delete(index);
            this.cache.set(index, cached);
            return cached;
        }
        const pending = this.pending.get(index);
        if (pending) return pending;
        const url = `${this.blockBase}blk${String(index).padStart(9, "0")}.bin`;
        const operation = (async () => {
            const response = await this.request(url, { cache: cacheMode(url) });
            if (!response.ok) throw new Error(`block ${index} HTTP ${response.status}`);
            const data = new Uint8Array(await response.arrayBuffer());
            if (data.length !== this.blockSize)
                throw new Error(`block ${index} has ${data.length} bytes; expected ${this.blockSize}`);
            if (!this.closed) {
                this.cache.set(index, data);
                this.trimCache();
            }
            return data;
        })();
        this.pending.set(index, operation);
        try { return await operation; }
        finally { this.pending.delete(index); }
    }

    private trimCache(): void {
        while (this.cache.size * this.blockSize > this.cacheLimit) {
            const oldest = this.cache.keys().next().value;
            if (oldest === undefined) break;
            this.cache.delete(oldest);
        }
    }

    private async source(byte: bigint, length: number): Promise<Uint8Array> {
        // Copy a byte span from one or more clean blocks without retaining a
        // view into a cache entry that may be evicted by the next fetch.
        const output = new Uint8Array(length);
        for (let position = 0; position < length;) {
            const address = byte + BigInt(position);
            const block = Number(address / BigInt(this.blockSize));
            const offset = Number(address % BigInt(this.blockSize));
            const count = Math.min(length - position, this.blockSize - offset);
            output.set((await this.load(block)).subarray(offset, offset + count), position);
            position += count;
        }
        return output;
    }

    async read(sector: bigint, length: number): Promise<Uint8Array> {
        // An unusually wide request may keep every touched clean block long
        // enough to finish, matching the original Rust store's cache policy.
        const start = this.validate(sector, length);
        const generation = this.generation;
        this.cacheLimit = Math.max(this.cacheLimit,
            (Number((start + BigInt(length - 1)) / BigInt(this.blockSize) -
                start / BigInt(this.blockSize)) + 1) * this.blockSize);
        const output = new Uint8Array(length);
        for (let position = 0; position < length;) {
            const address = start + BigInt(position);
            const cluster = address / BigInt(CLUSTER_SIZE);
            const offset = Number(address % BigInt(CLUSTER_SIZE));
            const count = Math.min(length - position, CLUSTER_SIZE - offset);
            const overlay = this.overlays.get(cluster);
            output.set(overlay ? overlay.subarray(offset, offset + count)
                : await this.source(address, count), position);
            if (this.generation !== generation) throw new Error("block request retired by reset");
            position += count;
        }
        return output;
    }

    async write(sector: bigint, bytes: Uint8Array): Promise<void> {
        // Stage modified clusters locally so a reset during a source fetch
        // cannot leave a partially accepted guest write in the overlay.
        const start = this.validate(sector, bytes.length);
        const generation = this.generation;
        const changed = new Map<bigint, Uint8Array>();
        this.cacheLimit = Math.max(this.cacheLimit,
            (Number((start + BigInt(bytes.length - 1)) / BigInt(this.blockSize) -
                start / BigInt(this.blockSize)) + 1) * this.blockSize);
        for (let position = 0; position < bytes.length;) {
            const address = start + BigInt(position);
            const cluster = address / BigInt(CLUSTER_SIZE);
            const offset = Number(address % BigInt(CLUSTER_SIZE));
            const count = Math.min(bytes.length - position, CLUSTER_SIZE - offset);
            let overlay = changed.get(cluster);
            if (!overlay) {
                const original = this.overlays.get(cluster);
                if (original) overlay = original.slice();
                else {
                    const clusterStart = cluster * BigInt(CLUSTER_SIZE);
                    const available = Number(this.capacitySectors * BigInt(SECTOR_SIZE) - clusterStart);
                    overlay = new Uint8Array(CLUSTER_SIZE);
                    overlay.set(await this.source(clusterStart, Math.min(CLUSTER_SIZE, available)));
                }
                if (this.generation !== generation) throw new Error("block request retired by reset");
                changed.set(cluster, overlay);
            }
            overlay.set(bytes.subarray(position, position + count), offset);
            position += count;
        }
        for (const [cluster, overlay] of changed) this.overlays.set(cluster, overlay);
    }

    reset(): void { this.generation++; }
    close(): void {
        this.generation++;
        this.closed = true;
        this.cache.clear();
        this.overlays.clear();
    }
}
