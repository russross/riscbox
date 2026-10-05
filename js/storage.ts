// Synchronous file storage and halted disk access share one raw boundary.
/** @internal */
export interface StorageExports {
    readonly memory: WebAssembly.Memory;
    riscbox_fs_get(address: number, length: number): number;
    riscbox_fs_call(handle: number, address: number, length: number): number;
    riscbox_fs_next_change(handle: number): number;
    riscbox_fs_status(): number;
    riscbox_fs_data_address(): number;
    riscbox_fs_data_length(): number;
    riscbox_disk_read(disk: number, low: number, high: number, length: number): number;
    riscbox_disk_finish(request: number): number;
    riscbox_disk_write(disk: number, low: number, high: number, address: number, length: number): number;
    riscbox_disk_discard(disk: number): number;
    riscbox_disk_capacity(disk: number, high: number): number;
    riscbox_disk_data_address(): number;
    riscbox_disk_data_length(): number;
}
/** @internal */
export interface StorageRuntime {
    readonly exports: StorageExports;
    readonly filesystems: Map<number, FilesystemHandle>;
    withBytes<Value>(bytes: Uint8Array | string, call: (address: number, length: number) => Value): Value;
    bytes(address: number, length: number): Uint8Array;
    serviceStorage(): void;
    drainActions(): void;
    filesystemChanged(): void;
    reportFilesystemError(error: unknown): void;
}
// Packets are copied at every WASM boundary. All filesystem identities stay exact.
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });
class Writer {
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
    blob(bytes: Uint8Array): this {
        if (!(bytes instanceof Uint8Array)) throw new TypeError("expected Uint8Array");
        return this.u32(bytes.length).raw(bytes);
    }
    str(value: string): this {
        if (typeof value !== "string") throw new TypeError("expected string");
        return this.blob(encoder.encode(value));
    }
    finish(): Uint8Array {
        const bytes = new Uint8Array(this.parts.reduce((size, part) => size + part.length, 0));
        let offset = 0;
        for (const part of this.parts) { bytes.set(part, offset); offset += part.length; }
        return bytes;
    }
}
class Reader {
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

export class FilesystemError extends Error {
    constructor(readonly errno: number) { super(`Filesystem operation failed (errno ${errno})`); this.name = "FilesystemError"; }
}
export type FileKind = "directory" | "file" | "symlink";
export interface DirectoryEntry { readonly name: string; readonly inode: bigint; readonly kind: FileKind; readonly cookie: bigint; }
export interface FileTime { readonly seconds: bigint; readonly nanoseconds: number; }
export interface FileStat {
    readonly inode: bigint; readonly kind: FileKind; readonly mode: number;
    readonly uid: number; readonly gid: number; readonly version: number;
    readonly linkCount: number; readonly size: bigint;
    readonly atime: FileTime; readonly mtime: FileTime; readonly ctime: FileTime;
}
export interface FileAttributes {
    readonly mode: number; readonly uid: number; readonly gid: number;
    readonly atime: FileTime; readonly mtime: FileTime;
}
export interface P9Change {
    readonly kind: "create" | "write" | "remove" | "rename" | "metadata" | "reset" | "rescan";
    readonly inode: bigint;
    readonly source: "host" | "guest";
    readonly origin: bigint;
    readonly path: string;
    readonly oldPath?: string;
    readonly aliases: readonly string[];
}
const kinds = ["directory", "file", "symlink"] as const;
const changes: Readonly<Record<number, P9Change["kind"]>> = {
    1: "create", 2: "write", 3: "remove", 4: "rename", 5: "metadata", 8: "reset", 9: "rescan",
};
const sources = ["host", "guest"] as const;
function kind(reader: Reader): FileKind {
    const value = kinds[reader.u32() - 1];
    if (value === undefined) throw new Error("Invalid filesystem kind");
    return value;
}
function now(): number { return Math.floor(Date.now() / 1000); }
function checked(status: number): void { if (status < 0) throw new FilesystemError(-status); }

// Client objects expose operations on owned storage, without packet or handle access.
export interface Filesystem {
    readFile(path: string): Uint8Array;
    writeFile(path: string, content: Uint8Array | string, origin?: bigint): void;
    mkdir(path: string, origin?: bigint): void;
    remove(path: string, origin?: bigint): void;
    rename(oldPath: string, newPath: string, origin?: bigint): void;
    listDirectory(path?: string): readonly DirectoryEntry[];
    listFiles(): readonly string[];
    stat(path: string): FileStat;
    symlink(path: string, target: string, origin?: bigint): void;
    readlink(path: string): string;
    link(existing: string, path: string, origin?: bigint): void;
    clear(): void;
    reset(): void;
    setAttributes(path: string, attributes: FileAttributes, origin?: bigint): void;
    subscribe(listener: (change: P9Change) => void): () => void;
}
export interface BlockDisk {
    readonly capacitySectors: bigint;
    read(sector: bigint, length: number): Uint8Array | Promise<Uint8Array>;
    write(sector: bigint, bytes: Uint8Array): void;
    reset(): void;
}

// A facade references its VM-owned tree; calls always finish before returning.
/** @internal */
export class FilesystemHandle {
    readonly client: Filesystem = Object.freeze({
        readFile: this.readFile.bind(this), writeFile: this.writeFile.bind(this),
        mkdir: this.mkdir.bind(this), remove: this.remove.bind(this), rename: this.rename.bind(this),
        listDirectory: this.listDirectory.bind(this), listFiles: this.listFiles.bind(this),
        stat: this.stat.bind(this), symlink: this.symlink.bind(this), readlink: this.readlink.bind(this),
        link: this.link.bind(this), clear: this.clear.bind(this), reset: this.reset.bind(this),
        setAttributes: this.setAttributes.bind(this), subscribe: this.subscribe.bind(this),
    });
    private readonly listeners = new Set<(change: P9Change) => void>();
    private closed = false;
    private constructor(private readonly runtime: StorageRuntime, readonly handle: number) {}

    static open(runtime: StorageRuntime, name: string): FilesystemHandle {
        const handle = runtime.withBytes(name, (address, length) => runtime.exports.riscbox_fs_get(address, length));
        if (handle === 0) throw new FilesystemError(-runtime.exports.riscbox_fs_status());
        const existing = runtime.filesystems.get(handle);
        if (existing) return existing;
        const filesystem = new FilesystemHandle(runtime, handle);
        runtime.filesystems.set(handle, filesystem);
        return filesystem;
    }
    private snapshot(): Uint8Array {
        return this.runtime.bytes(this.runtime.exports.riscbox_fs_data_address(), this.runtime.exports.riscbox_fs_data_length());
    }
    private call(operation: number, body = new Writer(), origin = 0n): { status: number; bytes: Uint8Array } {
        if (this.closed) throw new FilesystemError(9);
        if (typeof origin !== "bigint") throw new TypeError("origin must be bigint");
        const packet = new Writer().u32(operation).u64(now()).u64(origin).raw(body.finish()).finish();
        const status = this.runtime.withBytes(packet, (address, length) => this.runtime.exports.riscbox_fs_call(this.handle, address, length));
        const bytes = this.snapshot();
        checked(status); return { status, bytes };
    }
    private operation(operation: number, body = new Writer(), origin = 0n): Uint8Array {
        const { bytes } = this.call(operation, body, origin);
        this.runtime.filesystemChanged(); return bytes;
    }
    readFile(path: string): Uint8Array { return this.operation(1, new Writer().str(path)); }
    writeFile(path: string, content: Uint8Array | string, origin = 0n): void {
        const bytes = typeof content === "string" ? new TextEncoder().encode(content) : content;
        this.operation(2, new Writer().str(path).blob(bytes), origin);
    }
    mkdir(path: string, origin = 0n): void { this.operation(3, new Writer().str(path), origin); }
    remove(path: string, origin = 0n): void { this.operation(4, new Writer().str(path), origin); }
    rename(oldPath: string, newPath: string, origin = 0n): void { this.operation(5, new Writer().str(oldPath).str(newPath), origin); }
    listDirectory(path = ""): readonly DirectoryEntry[] {
        const reader = new Reader(this.operation(6, new Writer().str(path)));
        const count = reader.u32(); const entries: DirectoryEntry[] = [];
        for (let index = 0; index < count; index++) entries.push({ name: reader.str(), inode: reader.u64(), kind: kind(reader), cookie: reader.u64() });
        reader.end(); return entries;
    }
    listFiles(): readonly string[] {
        const reader = new Reader(this.operation(7));
        const count = reader.u32(); const paths: string[] = [];
        for (let index = 0; index < count; index++) paths.push(reader.str());
        reader.end(); return paths;
    }
    stat(path: string): FileStat {
        const reader = new Reader(this.operation(8, new Writer().str(path)));
        const inode = reader.u64(); const fileKind = kind(reader);
        const mode = reader.u32(), uid = reader.u32(), gid = reader.u32(), version = reader.u32(), linkCount = reader.u32();
        const size = reader.u64();
        const time = (): FileTime => ({ seconds: reader.u64(), nanoseconds: reader.u32() });
        const result = { inode, kind: fileKind, mode, uid, gid, version, linkCount, size, atime: time(), mtime: time(), ctime: time() };
        reader.end(); return result;
    }
    symlink(path: string, target: string, origin = 0n): void { this.operation(9, new Writer().str(path).str(target), origin); }
    readlink(path: string): string { return new TextDecoder().decode(this.operation(10, new Writer().str(path))); }
    link(existing: string, path: string, origin = 0n): void { this.operation(11, new Writer().str(existing).str(path), origin); }
    clear(): void { this.operation(15); }
    reset(): void { this.operation(12); }

    // Restored attributes preserve nanoseconds; inode identity and ctime are new.
    setAttributes(path: string, attributes: FileAttributes, origin = 0n): void {
        const { mode, uid, gid, atime, mtime } = attributes;
        if (typeof atime.seconds !== "bigint" || typeof mtime.seconds !== "bigint")
            throw new TypeError("filesystem epoch seconds must be bigint");
        if (mode > 0o7777 || atime.nanoseconds >= 1_000_000_000 || mtime.nanoseconds >= 1_000_000_000)
            throw new RangeError("invalid filesystem attributes");
        this.operation(14, new Writer().str(path).u32(mode).u32(uid).u32(gid)
            .u64(atime.seconds).u32(atime.nanoseconds).u64(mtime.seconds).u32(mtime.nanoseconds), origin);
    }

    subscribe(listener: (change: P9Change) => void): () => void {
        if (this.closed) throw new FilesystemError(9);
        if (typeof listener !== "function") throw new TypeError("listener must be a function");
        if (this.listeners.size === 0) this.call(13, new Writer().u32(1));
        this.listeners.add(listener);
        this.runtime.serviceStorage();
        return () => {
            this.listeners.delete(listener);
            if (!this.closed && this.listeners.size === 0) this.call(13, new Writer().u32(0));
        };
    }

    invalidate(): void { this.closed = true; this.listeners.clear(); }

    poll(): void {
        if (this.closed) return;
        // Capture recipients with each copied event, then leave the synchronous service path.
        for (;;) {
            const status = this.runtime.exports.riscbox_fs_next_change(this.handle);
            checked(status);
            if (status === 0) break;
            const reader = new Reader(this.snapshot());
            const changeKind = changes[reader.u32()]; const inode = reader.u64();
            const source = sources[reader.u32()]; const origin = reader.u64(); const path = reader.str();
            const oldPath = reader.u32() === 0 ? undefined : reader.str();
            const aliases: string[] = []; const count = reader.u32();
            for (let index = 0; index < count; index++) aliases.push(reader.str());
            reader.end();
            if (changeKind === undefined || source === undefined) throw new Error("Invalid filesystem event");
            const change: P9Change = { kind: changeKind, inode, source, origin, path, aliases, ...(oldPath === undefined ? {} : { oldPath }) };
            const recipients = [...this.listeners];
            queueMicrotask(() => {
                for (const listener of recipients) {
                    if (!this.listeners.has(listener)) continue;
                    try { listener(change); } catch (error: unknown) { this.runtime.reportFilesystemError(error); }
                }
            });
        }
    }
}

interface PendingDiskRead {
    readonly resolve: (bytes: Uint8Array) => void;
    readonly reject: (error: unknown) => void;
}

export class BlockError extends Error {
    constructor(readonly errno: number) { super(`Block operation failed (errno ${errno})`); this.name = "BlockError"; }
}

/** @internal */
export class DiskHandle {
    readonly client: BlockDisk;
    private closed = false;
    private readonly pending = new Map<number, PendingDiskRead>();
    constructor(private readonly runtime: StorageRuntime, readonly index: number) {
        const disk = this;
        this.client = Object.freeze({
            read: this.read.bind(this), write: this.write.bind(this),
            reset: this.reset.bind(this),
            get capacitySectors(): bigint { return disk.capacitySectors; },
        });
    }

    private check(): void { if (this.closed) throw new BlockError(9); }
    private checked(status: number): void { if (status < 0) throw new BlockError(-status); }
    private snapshot(): Uint8Array {
        return this.runtime.bytes(this.runtime.exports.riscbox_disk_data_address(), this.runtime.exports.riscbox_disk_data_length());
    }
    private address(sector: bigint): readonly [number, number] {
        this.check();
        if (typeof sector !== "bigint" || sector < 0n || sector > 0xffff_ffff_ffff_ffffn) throw new RangeError("invalid disk sector");
        return [Number(sector & 0xffff_ffffn), Number(sector >> 32n)];
    }

    // Resident reads return directly. A cache miss retains only a Rust request
    // ID and promise callbacks; the eventual result is copied from WASM.
    read(sector: bigint, length: number): Uint8Array | Promise<Uint8Array> {
        const [low, high] = this.address(sector);
        if (!Number.isInteger(length) || length <= 0 || length > 0xffff_ffff || length % 512 !== 0) throw new RangeError("disk reads need whole sectors");
        const status = this.runtime.exports.riscbox_disk_read(this.index, low, high, length);
        this.checked(status);
        if (status === 0) return this.snapshot();
        const promise = new Promise<Uint8Array>((resolve, reject) => this.pending.set(status, { resolve, reject }));
        this.runtime.drainActions();
        return promise;
    }

    write(sector: bigint, bytes: Uint8Array): void {
        const [low, high] = this.address(sector);
        if (!(bytes instanceof Uint8Array)) throw new TypeError("disk write requires Uint8Array");
        if (bytes.length === 0 || bytes.length % 512 !== 0) throw new RangeError("disk writes need whole sectors");
        this.checked(this.runtime.withBytes(bytes, (address, length) => this.runtime.exports.riscbox_disk_write(this.index, low, high, address, length)));
        // A write can make a waiting read entirely resident before its base
        // fetch returns. Poll copied results and dispatch any remaining misses.
        this.runtime.drainActions();
    }

    get capacitySectors(): bigint {
        this.check();
        const low = this.runtime.exports.riscbox_disk_capacity(this.index, 0);
        const high = this.runtime.exports.riscbox_disk_capacity(this.index, 1);
        const capacity = BigInt(low) | (BigInt(high) << 32n);
        if (capacity === 0n) throw new BlockError(16);
        return capacity;
    }

    reset(): void {
        this.check();
        this.checked(this.runtime.exports.riscbox_disk_discard(this.index));
        this.poll();
    }

    poll(): boolean {
        if (this.closed) return false;
        const polled = this.pending.size !== 0;
        for (const [id, pending] of this.pending) {
            const status = this.runtime.exports.riscbox_disk_finish(id);
            if (status === 1) continue;
            this.pending.delete(id);
            if (status < 0) pending.reject(new BlockError(-status));
            else pending.resolve(this.snapshot());
        }
        return polled;
    }

    invalidate(): void {
        this.closed = true;
        for (const pending of this.pending.values()) pending.reject(new BlockError(9));
        this.pending.clear();
    }
}
