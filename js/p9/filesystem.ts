import {
    P9Session,
    ProtocolEngine,
    type FileContent,
    type Memory9PTree,
    type P9Change,
} from "./session.js";

export class MemoryFilesystem {
    private readonly engine: ProtocolEngine;

    constructor(tree: Memory9PTree = {}) {
        this.engine = new ProtocolEngine(tree);
    }

    connect(): P9Session { return new P9Session(this.engine.fork()); }
    loadTree(tree: Memory9PTree): void { this.engine.loadTree(tree); }
    loadFiles(files: Readonly<Record<string, FileContent>>): void {
        this.engine.loadFiles(files);
    }
    subscribe(listener: (change: P9Change) => void): () => void {
        return this.engine.subscribe(listener);
    }
    readFile(path: string): Uint8Array { return this.engine.readFile(path); }
    writeFile(path: string, content: FileContent, source = "host"): void {
        this.engine.writeFile(path, content, source);
    }
    remove(path: string, source = "host"): void { this.engine.remove(path, source); }
    rename(oldPath: string, newPath: string, source = "host"): void {
        this.engine.rename(oldPath, newPath, source);
    }
    listFiles(): string[] { return this.engine.listFiles(); }
    snapshot(): Record<string, Uint8Array> { return this.engine.snapshot(); }
}

export class Memory9PServer extends MemoryFilesystem {}
