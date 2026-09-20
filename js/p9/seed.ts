export interface SeedMetadata {
    readonly mode?: number;
    readonly uid?: number;
    readonly gid?: number;
    readonly atime?: number;
    readonly mtime?: number;
    readonly ctime?: number;
}

export interface DirectorySeed extends SeedMetadata {
    readonly kind: "directory";
    readonly path: string;
}

export interface SymlinkSeed extends SeedMetadata {
    readonly kind: "symlink";
    readonly path: string;
    readonly target: string;
}

export interface FileSeed<Key> extends SeedMetadata {
    readonly kind: "file";
    readonly path: string;
    readonly size: number;
    readonly key: Key;
    readonly inodeKey?: string;
}

export type SeedEntry<Key> = DirectorySeed | SymlinkSeed | FileSeed<Key>;

export interface SeedLoader<Key> {
    load(key: Key, signal: AbortSignal): Promise<Uint8Array>;
}

export interface SeedPlugin<Key> {
    readonly entries: readonly SeedEntry<Key>[];
    readonly loader: SeedLoader<Key>;
}

interface PendingHardLink {
    readonly path: string;
    readonly inodeKey: string;
}

function normalizedSeedPath(path: string): string {
    if (typeof path !== "string" || path === "" || path.startsWith("/")) {
        throw new TypeError(`invalid seed path ${JSON.stringify(path)}`);
    }
    const parts = path.split("/");
    if (parts.some((part) => part === "" || part === "." || part === ".." || part.includes("\0"))) {
        throw new TypeError(`invalid seed path ${JSON.stringify(path)}`);
    }
    return parts.join("/");
}

function checkedSize(size: number): number {
    if (!Number.isSafeInteger(size) || size < 0) {
        throw new TypeError("seed file size must be a nonnegative safe integer");
    }
    return size;
}

function freezeEntry<Key>(entry: SeedEntry<Key>): SeedEntry<Key> {
    return Object.freeze(entry);
}

export class SeedBuilder<Key> {
    private readonly entries: SeedEntry<Key>[] = [];
    private readonly hardLinks: PendingHardLink[] = [];

    addDirectory(path: string, metadata: SeedMetadata = {}): this {
        this.entries.push({ kind: "directory", path: normalizedSeedPath(path), ...metadata });
        return this;
    }

    addSymlink(path: string, target: string, metadata: SeedMetadata = {}): this {
        if (typeof target !== "string") throw new TypeError("symlink target must be a string");
        this.entries.push({ kind: "symlink", path: normalizedSeedPath(path), target, ...metadata });
        return this;
    }

    addFile(
        path: string,
        size: number,
        key: Key,
        options: SeedMetadata & { readonly inodeKey?: string } = {},
    ): this {
        if (options.inodeKey !== undefined && options.inodeKey === "") {
            throw new TypeError("seed inode key may not be empty");
        }
        this.entries.push({
            kind: "file",
            path: normalizedSeedPath(path),
            size: checkedSize(size),
            key,
            ...options,
        });
        return this;
    }

    addHardLink(path: string, inodeKey: string): this {
        if (inodeKey === "") throw new TypeError("seed inode key may not be empty");
        this.hardLinks.push({ path: normalizedSeedPath(path), inodeKey });
        return this;
    }

    finish(): readonly SeedEntry<Key>[] {
        const paths = new Set<string>();
        const files = new Map<string, FileSeed<Key>>();
        const result: SeedEntry<Key>[] = [];
        for (const entry of this.entries) {
            if (paths.has(entry.path)) throw new TypeError(`duplicate seed path ${JSON.stringify(entry.path)}`);
            paths.add(entry.path);
            if (entry.kind === "file" && entry.inodeKey !== undefined) {
                if (files.has(entry.inodeKey)) {
                    throw new TypeError(`duplicate seed inode key ${JSON.stringify(entry.inodeKey)}`);
                }
                files.set(entry.inodeKey, entry);
            }
            result.push(freezeEntry({ ...entry }));
        }
        for (const link of this.hardLinks) {
            if (paths.has(link.path)) throw new TypeError(`duplicate seed path ${JSON.stringify(link.path)}`);
            const target = files.get(link.inodeKey);
            if (target === undefined) {
                throw new TypeError(`dangling seed inode key ${JSON.stringify(link.inodeKey)}`);
            }
            paths.add(link.path);
            result.push(freezeEntry({ ...target, path: link.path }));
        }
        for (const path of paths) {
            const parts = path.split("/");
            for (let index = 1; index < parts.length; index += 1) {
                const parent = parts.slice(0, index).join("/");
                const entry = result.find((candidate) => candidate.path === parent);
                if (entry !== undefined && entry.kind !== "directory") {
                    throw new TypeError(`seed path crosses a non-directory at ${JSON.stringify(parent)}`);
                }
            }
        }
        return Object.freeze(result);
    }
}
