import {
    P9Error,
    P9Session,
    ProtocolEngine,
    type FileContent,
    type Memory9PLimits,
    type Memory9PTree,
    type P9Change,
} from "./session.js";
import type { SeedPlugin } from "./seed.js";

export type FilesystemErrorCode =
    | "not-found" | "not-directory" | "is-directory" | "already-exists"
    | "invalid" | "too-large" | "no-space" | "not-empty" | "permission" | "io";

export interface FilesystemError {
    readonly code: FilesystemErrorCode;
    readonly message: string;
    readonly errno?: number;
    readonly cause?: unknown;
}

export type SyncResult<Value> =
    | { readonly kind: "ok"; readonly value: Value }
    | { readonly kind: "not-loaded"; readonly paths: readonly string[] }
    | { readonly kind: "error"; readonly error: FilesystemError };

function filesystemError(error: unknown): FilesystemError {
    if (!(error instanceof P9Error)) {
        return { code: "io", message: error instanceof Error ? error.message : String(error), cause: error };
    }
    const codes: Readonly<Record<number, FilesystemErrorCode>> = {
        1: "permission", 2: "not-found", 5: "io", 17: "already-exists",
        20: "not-directory", 21: "is-directory", 22: "invalid",
        27: "too-large", 28: "no-space", 39: "not-empty",
    };
    return { code: codes[error.errno] ?? "io", message: error.message, errno: error.errno };
}

function attempt<Value>(operation: () => Value): SyncResult<Value> {
    try {
        return { kind: "ok", value: operation() };
    } catch (error) {
        return { kind: "error", error: filesystemError(error) };
    }
}

export class MemoryFilesystem<Key = never> {
    private readonly engine: ProtocolEngine;

    constructor(
        tree: Memory9PTree = {},
        limits: Memory9PLimits = {},
        seedPlugin?: SeedPlugin<Key>,
    ) {
        if (seedPlugin !== undefined && Object.keys(tree).length !== 0) {
            throw new TypeError("resident tree and seed plugin are mutually exclusive");
        }
        this.engine = new ProtocolEngine(tree, null, limits);
        if (seedPlugin !== undefined) this.engine.loadSeed(seedPlugin);
    }

    static fromSeed<Key>(plugin: SeedPlugin<Key>, limits: Memory9PLimits = {}): MemoryFilesystem<Key> {
        return new MemoryFilesystem({}, limits, plugin);
    }

    connect(): P9Session { return new P9Session(this.engine.fork()); }
    loadTree(tree: Memory9PTree): void { this.engine.loadTree(tree); }
    loadFiles(files: Readonly<Record<string, FileContent>>): void { this.engine.loadFiles(files); }
    subscribe(listener: (change: P9Change) => void): () => void { return this.engine.subscribe(listener); }

    readFile(path: string): SyncResult<Uint8Array> {
        try {
            const state = this.engine.readFileState(path);
            if (state.kind === "resident") return { kind: "ok", value: state.bytes };
            if (state.kind === "not-loaded") return state;
            return { kind: "error", error: filesystemError(state.error) };
        } catch (error) {
            return { kind: "error", error: filesystemError(error) };
        }
    }

    writeFile(path: string, content: FileContent, source = "host"): SyncResult<void> {
        return attempt(() => this.engine.writeFile(path, content, source));
    }

    remove(path: string, source = "host"): SyncResult<void> {
        return attempt(() => this.engine.remove(path, source));
    }

    rename(oldPath: string, newPath: string, source = "host"): SyncResult<void> {
        return attempt(() => this.engine.rename(oldPath, newPath, source));
    }

    listFiles(): SyncResult<readonly string[]> { return attempt(() => this.engine.listFiles()); }

    async load(paths: readonly string[], retry = false): Promise<SyncResult<void>> {
        try {
            await this.engine.loadPaths(paths, retry);
            return { kind: "ok", value: undefined };
        } catch (error) {
            return { kind: "error", error: filesystemError(error) };
        }
    }

    async readFileAsync(path: string, retry = false): Promise<SyncResult<Uint8Array>> {
        const loaded = await this.load([path], retry);
        return loaded.kind === "ok" ? this.readFile(path) : loaded;
    }
}

export class Memory9PServer<Key = never> extends MemoryFilesystem<Key> {}
