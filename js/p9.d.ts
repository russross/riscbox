export type Memory9PEntry = string | Uint8Array | Memory9PTree;

export interface Memory9PTree {
    readonly [name: string]: Memory9PEntry;
}

export interface P9Endpoint {
    request(request: Uint8Array, replyCapacity: number): Uint8Array;
}

export type P9Change =
    | { readonly kind: "create" | "write" | "remove"; readonly path: string; readonly source: string }
    | { readonly kind: "rename"; readonly path: string; readonly oldPath: string; readonly source: string }
    | { readonly kind: "reset"; readonly path: ""; readonly source: string };

export declare const MAX_FILE_SIZE: number;

export declare class P9Error extends Error {
    readonly errno: number;
    constructor(errno: number, message: string);
}

export declare class Memory9PServer implements P9Endpoint {
    constructor(tree?: Memory9PTree);
    loadTree(tree: Memory9PTree): void;
    loadFiles(files: Readonly<Record<string, string | Uint8Array>>): void;
    subscribe(listener: (change: P9Change) => void): () => void;
    readFile(path: string): Uint8Array;
    writeFile(path: string, content: string | Uint8Array, source?: string): void;
    remove(path: string, source?: string): void;
    rename(oldPath: string, newPath: string, source?: string): void;
    listFiles(): string[];
    snapshot(): Record<string, Uint8Array>;
    request(request: Uint8Array, replyCapacity: number): Uint8Array;
}
