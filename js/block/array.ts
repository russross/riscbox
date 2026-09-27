import type { BlockProvider } from "./index.js";

const SECTOR_SIZE = 512;
const MAX_U64 = (1n << 64n) - 1n;

/** A writable block device over the caller's exact Uint8Array view. */
export class ArrayBlockProvider implements BlockProvider {
    readonly capacitySectors: bigint;
    readonly bytes: Uint8Array;
    private closed = false;

    constructor(bytes: Uint8Array) {
        // Keep the supplied view, including its byte offset in a larger buffer.
        // The guest sees only complete 512-byte sectors in that view.
        if (!(bytes instanceof Uint8Array) || bytes.length === 0 ||
            bytes.length % SECTOR_SIZE !== 0)
            throw new RangeError("array length must be a positive whole number of sectors");
        this.bytes = bytes;
        this.capacitySectors = BigInt(bytes.length / SECTOR_SIZE);
        if (this.capacitySectors > MAX_U64)
            throw new RangeError("array exceeds block capacity limit");
    }

    private offset(sector: bigint, length: number): number {
        // Validation precedes Number conversion, so the offset is bounded by
        // the actual typed array length and remains exactly representable.
        if (this.closed) throw new Error("block provider is closed");
        if (typeof sector !== "bigint" || sector < 0n || !Number.isSafeInteger(length) ||
            length < SECTOR_SIZE || length % SECTOR_SIZE !== 0 ||
            sector + BigInt(length / SECTOR_SIZE) > this.capacitySectors)
            throw new RangeError("block request is outside the array");
        return Number(sector) * SECTOR_SIZE;
    }

    read(sector: bigint, length: number): Uint8Array {
        // A copy keeps guest reply bytes stable if the host edits the array.
        const offset = this.offset(sector, length);
        return this.bytes.slice(offset, offset + length);
    }

    write(sector: bigint, bytes: Uint8Array): void {
        // The host can inspect the same view after the guest shuts down.
        if (!(bytes instanceof Uint8Array)) throw new TypeError("write requires Uint8Array");
        this.bytes.set(bytes, this.offset(sector, bytes.length));
    }

    reset(): void { /* The caller's bytes remain attached across VM resets. */ }
    close(): void { this.closed = true; }
}
