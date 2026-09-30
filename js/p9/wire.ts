// Packets are copied at every WASM boundary. All filesystem identities stay exact.
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });
export class Writer {
    private readonly parts: Uint8Array[] = [];
    u32(value: number): this {
        if (!Number.isInteger(value) || value < 0 || value > 0xffff_ffff) throw new RangeError("invalid u32");
        const bytes = new Uint8Array(4);
        new DataView(bytes.buffer).setUint32(0, value, true);
        this.parts.push(bytes); return this;
    }
    u64(value: bigint | number): this {
        if (typeof value === "number" && !Number.isSafeInteger(value)) throw new RangeError("invalid integer");
        const integer = BigInt(value);
        if (integer < 0n || integer > 0xffff_ffff_ffff_ffffn) throw new RangeError("invalid u64");
        const bytes = new Uint8Array(8);
        new DataView(bytes.buffer).setBigUint64(0, integer, true);
        this.parts.push(bytes); return this;
    }
    raw(bytes: Uint8Array): this { this.parts.push(bytes); return this; }
    blob(bytes: Uint8Array): this { return this.u32(bytes.length).raw(bytes); }
    str(value: string): this { return this.blob(encoder.encode(value)); }
    finish(): Uint8Array {
        const bytes = new Uint8Array(this.parts.reduce((size, part) => size + part.length, 0));
        let offset = 0;
        for (const part of this.parts) { bytes.set(part, offset); offset += part.length; }
        return bytes;
    }
}
export class Reader {
    private offset = 0;
    private readonly view: DataView;
    constructor(private readonly bytes: Uint8Array) {
        this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    }
    u32(): number { const value = this.view.getUint32(this.offset, true); this.offset += 4; return value; }
    u64(): bigint { const value = this.view.getBigUint64(this.offset, true); this.offset += 8; return value; }
    str(): string {
        const length = this.u32();
        if (length > this.bytes.length - this.offset) throw new RangeError("truncated filesystem string");
        const value = decoder.decode(this.bytes.subarray(this.offset, this.offset + length));
        this.offset += length; return value;
    }
    end(): void { if (this.offset !== this.bytes.length) throw new RangeError("trailing filesystem bytes"); }
}
