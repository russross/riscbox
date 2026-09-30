import type { SeedEntry, SeedMetadata, SeedPlugin } from "./seed.js";
import { Reader, Writer } from "./wire.js";

export interface FilesystemExports {
    riscbox_fs_create(address: number, length: number): number;
    riscbox_fs_bind(handle: number, address: number, length: number): number;
    riscbox_fs_call(handle: number, address: number, length: number): number;
    riscbox_fs_close(handle: number): number;
    riscbox_fs_complete_load(handle: number, address: number, length: number): number;
    riscbox_fs_next_change(handle: number): number;
    riscbox_fs_status(): number;
    riscbox_fs_data_address(): number;
    riscbox_fs_data_length(): number;
}
export interface FilesystemService {
    dispatch(ticket: Uint8Array): void;
    poll(): void;
}
export interface FilesystemRuntime {
    readonly exports: FilesystemExports;
    readonly filesystems: Map<number, FilesystemService>;
    withBytes<Value>(bytes: Uint8Array | string, call: (address: number, length: number) => Value): Value;
    bytes(address: number, length: number): Uint8Array;
    serviceFilesystems(): void;
    filesystemChanged(): void;
    reportFilesystemError(error: unknown): void;
}
export interface FilesystemLimits {
    readonly maxFileBytes?: number;
    readonly maxTreeBytes?: number;
    readonly maxInodes?: number;
    readonly maxDirectoryEntries?: number;
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
export interface P9Change {
    readonly kind: "create" | "write" | "remove" | "rename" | "metadata" | "loaded" | "load-error" | "reset" | "rescan";
    readonly inode: bigint;
    readonly source: "host" | "guest" | "loader";
    readonly origin: bigint;
    readonly path: string;
    readonly oldPath?: string;
    readonly aliases: readonly string[];
}
interface PendingRead { readonly resolve: (bytes: Uint8Array) => void; readonly reject: (error: unknown) => void; }
interface Source { load(signal: AbortSignal): Promise<Uint8Array>; }
const kinds = ["directory", "file", "symlink"] as const;
const changes = ["create", "write", "remove", "rename", "metadata", "loaded", "load-error", "reset", "rescan"] as const;
const sources = ["host", "guest", "loader"] as const;
function kind(reader: Reader): FileKind {
    const value = kinds[reader.u32() - 1];
    if (value === undefined) throw new Error("Invalid filesystem kind");
    return value;
}
function now(): number { return Math.floor(Date.now() / 1000); }
function checked(status: number): void { if (status < 0) throw new FilesystemError(-status); }
function metadata(writer: Writer, value: SeedMetadata): void {
    const fields = [value.mode, value.uid, value.gid, value.atime, value.mtime, value.ctime];
    writer.u32(fields.reduce<number>((mask, field, index) => field === undefined ? mask : mask | (1 << index), 0));
    for (const [index, field] of fields.entries()) {
        if (field !== undefined) { if (index < 3) writer.u32(field); else writer.u64(field); }
    }
}

// The facade owns promises and source objects; Rust owns namespace and protocol state.
export class Filesystem implements FilesystemService {
    private readonly pending = new Map<number, PendingRead>();
    private readonly listeners = new Set<(change: P9Change) => void>();
    private readonly loads = new Set<AbortController>();
    private sourceTable = new Map<number, Source>();
    private nextSource = 1;
    private closed = false;
    private constructor(private readonly runtime: FilesystemRuntime, readonly handle: number) {}

    static async create(runtime: FilesystemRuntime, limits: FilesystemLimits = {}): Promise<Filesystem> {
        const config = new Writer().u32(limits.maxFileBytes ?? 256 * 1024 * 1024)
            .u32(limits.maxTreeBytes ?? 1024 * 1024 * 1024).u32(limits.maxInodes ?? 2 ** 20)
            .u32(limits.maxDirectoryEntries ?? 2 ** 20).u64(now()).finish();
        const handle = runtime.withBytes(config, (address, length) => runtime.exports.riscbox_fs_create(address, length));
        if (handle === 0) throw new FilesystemError(-runtime.exports.riscbox_fs_status());
        const filesystem = new Filesystem(runtime, handle);
        runtime.filesystems.set(handle, filesystem);
        return filesystem;
    }
    private snapshot(): Uint8Array {
        return this.runtime.bytes(this.runtime.exports.riscbox_fs_data_address(), this.runtime.exports.riscbox_fs_data_length());
    }
    private call(operation: number, body = new Writer(), origin = 0n): { status: number; bytes: Uint8Array } {
        if (this.closed) throw new FilesystemError(9);
        const packet = new Writer().u32(operation).u64(now()).u64(origin).raw(body.finish()).finish();
        const status = this.runtime.withBytes(packet, (address, length) => this.runtime.exports.riscbox_fs_call(this.handle, address, length));
        const bytes = this.snapshot();
        checked(status); return { status, bytes };
    }
    private operation(operation: number, body = new Writer(), origin = 0n): Uint8Array {
        const { bytes } = this.call(operation, body, origin);
        this.runtime.filesystemChanged(); return bytes;
    }
    async bind(key: string): Promise<void> {
        if (this.closed) throw new FilesystemError(9);
        checked(this.runtime.withBytes(key, (address, length) => this.runtime.exports.riscbox_fs_bind(this.handle, address, length)));
    }
    async readFile(path: string): Promise<Uint8Array> {
        const result = this.call(1, new Writer().str(path));
        if (result.status === 0) { this.runtime.filesystemChanged(); return result.bytes; }
        const reader = new Reader(result.bytes);
        const request = reader.u32(); reader.end();
        const promise = new Promise<Uint8Array>((resolve, reject) => this.pending.set(request, { resolve, reject }));
        this.runtime.filesystemChanged(); return promise;
    }
    async writeFile(path: string, content: Uint8Array | string, origin = 0n): Promise<void> {
        const bytes = typeof content === "string" ? new TextEncoder().encode(content) : content;
        this.operation(2, new Writer().str(path).blob(bytes), origin);
    }
    async mkdir(path: string, origin = 0n): Promise<void> { this.operation(3, new Writer().str(path), origin); }
    async remove(path: string, origin = 0n): Promise<void> { this.operation(4, new Writer().str(path), origin); }
    async rename(oldPath: string, newPath: string, origin = 0n): Promise<void> { this.operation(5, new Writer().str(oldPath).str(newPath), origin); }
    async listDirectory(path = ""): Promise<readonly DirectoryEntry[]> {
        const reader = new Reader(this.operation(6, new Writer().str(path)));
        const count = reader.u32(); const entries: DirectoryEntry[] = [];
        for (let index = 0; index < count; index++) entries.push({ name: reader.str(), inode: reader.u64(), kind: kind(reader), cookie: reader.u64() });
        reader.end(); return entries;
    }
    async listFiles(): Promise<readonly string[]> {
        const reader = new Reader(this.operation(7));
        const count = reader.u32(); const paths: string[] = [];
        for (let index = 0; index < count; index++) paths.push(reader.str());
        reader.end(); return paths;
    }
    async stat(path: string): Promise<FileStat> {
        const reader = new Reader(this.operation(8, new Writer().str(path)));
        const inode = reader.u64(); const fileKind = kind(reader);
        const mode = reader.u32(), uid = reader.u32(), gid = reader.u32(), version = reader.u32(), linkCount = reader.u32();
        const size = reader.u64();
        const time = (): FileTime => ({ seconds: reader.u64(), nanoseconds: reader.u32() });
        const result = { inode, kind: fileKind, mode, uid, gid, version, linkCount, size, atime: time(), mtime: time(), ctime: time() };
        reader.end(); return result;
    }
    async symlink(path: string, target: string, origin = 0n): Promise<void> { this.operation(9, new Writer().str(path).str(target), origin); }
    async readlink(path: string): Promise<string> { return new TextDecoder().decode(this.operation(10, new Writer().str(path))); }
    async link(existing: string, path: string, origin = 0n): Promise<void> { this.operation(11, new Writer().str(existing).str(path), origin); }
    async reset(): Promise<void> {
        this.operation(12);
        this.sourceTable.clear();
        for (const controller of this.loads) controller.abort();
    }
    async retrySource(path: string): Promise<void> { this.operation(17, new Writer().str(path)); }

    // Install the complete manifest atomically before publishing its opaque source keys.
    async installSeed<Key>(plugin: SeedPlugin<Key>): Promise<void> {
        const body = new Writer().u32(plugin.entries.length);
        const table = new Map<number, Source>();
        const links = new Map<string, string>();
        for (const entry of plugin.entries) {
            body.str(entry.path);
            this.seedEntry(body, entry, plugin, table, links);
            metadata(body, entry);
        }
        this.call(14, body);
        this.sourceTable = table;
        for (const controller of this.loads) controller.abort();
        this.runtime.filesystemChanged();
    }
    private seedEntry<Key>(body: Writer, entry: SeedEntry<Key>, plugin: SeedPlugin<Key>, table: Map<number, Source>, links: Map<string, string>): void {
        if (entry.kind === "directory") { body.u32(1); return; }
        if (entry.kind === "symlink") { body.u32(3).str(entry.target); return; }
        const target = entry.inodeKey === undefined ? undefined : links.get(entry.inodeKey);
        if (target !== undefined) { body.u32(4).str(target); return; }
        const source = this.nextSource++;
        body.u32(2).u32(entry.size).u32(source);
        table.set(source, { load: (signal) => plugin.loader.load(entry.key, signal) });
        if (entry.inodeKey !== undefined) links.set(entry.inodeKey, entry.path);
    }
    async subscribe(listener: (change: P9Change) => void): Promise<() => Promise<void>> {
        if (this.closed) throw new FilesystemError(9);
        if (this.listeners.size === 0) this.call(13, new Writer().u32(1));
        this.listeners.add(listener);
        this.runtime.serviceFilesystems();
        return async () => {
            this.listeners.delete(listener);
            if (!this.closed && this.listeners.size === 0) this.call(13, new Writer().u32(0));
        };
    }
    async close(): Promise<void> {
        if (this.closed) return;
        checked(this.runtime.exports.riscbox_fs_close(this.handle));
        this.closed = true;
        this.runtime.filesystems.delete(this.handle);
        for (const controller of this.loads) controller.abort();
        for (const pending of this.pending.values()) pending.reject(new FilesystemError(9));
        this.pending.clear(); this.listeners.clear(); this.sourceTable.clear();
    }

    // Dispatch is entered after WASM returns; plugin code starts in a microtask.
    dispatch(ticket: Uint8Array): void {
        const view = new DataView(ticket.buffer, ticket.byteOffset, ticket.byteLength);
        const source = this.sourceTable.get(view.getUint32(24, true));
        const size = view.getUint32(28, true);
        const controller = new AbortController(); this.loads.add(controller);
        Promise.resolve().then(() => {
            if (source === undefined) throw new Error("Missing filesystem source");
            return source.load(controller.signal);
        }).then((bytes) => {
            if (!(bytes instanceof Uint8Array) || bytes.length !== size) throw new Error("Source returned incorrect file size");
            return { status: 0, bytes };
        }).catch(() => ({ status: 1, bytes: new Uint8Array() }))
            .then(({ status, bytes }) => this.complete(ticket, status, bytes))
            .catch((error: unknown) => this.runtime.reportFilesystemError(error))
            .finally(() => this.loads.delete(controller));
    }
    private complete(ticket: Uint8Array, status: number, bytes: Uint8Array): void {
        if (this.closed) return;
        const packet = new Writer().raw(ticket).u64(now()).u32(status).blob(bytes).finish();
        checked(this.runtime.withBytes(packet, (address, length) => this.runtime.exports.riscbox_fs_complete_load(this.handle, address, length)));
        this.runtime.filesystemChanged();
    }
    poll(): void {
        if (this.closed) return;
        for (const [request, pending] of this.pending) {
            try {
                const result = this.call(15, new Writer().u32(request));
                if (result.status === 1) continue;
                this.pending.delete(request); pending.resolve(result.bytes);
            } catch (error: unknown) { this.pending.delete(request); pending.reject(error); }
        }
        // Capture recipients with each copied event, then leave the synchronous service path.
        for (;;) {
            const status = this.runtime.exports.riscbox_fs_next_change(this.handle);
            checked(status);
            if (status === 0) break;
            const reader = new Reader(this.snapshot());
            const changeKind = changes[reader.u32() - 1]; const inode = reader.u64();
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
