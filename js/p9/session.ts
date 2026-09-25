/*
 * Small synchronous 9P2000.L server for browser-hosted teaching VMs.
 *
 * The implementation deliberately favors a direct data model over general
 * filesystem features. Files are bounded by explicit quotas and all accepted 64-bit
 * wire values must fit in their low 32 bits.
 */

import type { SeedEntry, SeedLoader, SeedMetadata, SeedPlugin } from "./seed.js";

const MAX_FILE_SIZE = 256 * 1024 * 1024;
const MAX_MESSAGE_SIZE = 64 * 1024;
const NOTAG = 0xffff;

const RLERROR = 7;
const QTDIR = 0x80;
const QTSYMLINK = 0x02;
const S_IFDIR = 0x4000;
const S_IFREG = 0x8000;
const S_IFLNK = 0xa000;
const O_TRUNC = 0x200;
const O_APPEND = 0x400;
const AT_REMOVEDIR = 0x200;

const EPERM = 1;
const ENOENT = 2;
const EIO = 5;
const EBADF = 9;
const EEXIST = 17;
const ENOTDIR = 20;
const EISDIR = 21;
const EINVAL = 22;
const EFBIG = 27;
const ENOSPC = 28;
const ENOTEMPTY = 39;
const EPROTO = 71;
const EOPNOTSUPP = 95;

export type FileContent = string | Uint8Array;
export type Memory9PEntry = FileContent | Memory9PTree;
export interface Memory9PTree { readonly [name: string]: Memory9PEntry }
export interface Memory9PLimits {
    readonly maxFileBytes?: number;
    readonly maxTreeBytes?: number;
    readonly maxInodes?: number;
    readonly maxDirectoryEntries?: number;
}
export type ChangeSource = string;
export type P9Change =
    | { kind: "create" | "write" | "remove" | "loaded" | "load-error";
        inode: number; path: string; paths: readonly string[]; source: ChangeSource }
    | { kind: "rename"; inode: number; path: string; paths: readonly string[];
        oldPath: string; source: ChangeSource }
    | { kind: "reset"; path: ""; source: ChangeSource };

interface BaseNode {
    id: number; version: number; name: string; parent: DirectoryNode | null;
    mode: number; uid: number; gid: number; atime: number; mtime: number; ctime: number;
    linkCount: number; fidRefs: number;
}
interface DirectoryNode extends BaseNode {
    kind: "directory"; children: Record<string, Node>;
    cookies: Record<string, number>; nextCookie: number;
}
interface ResidentContent { readonly kind: "resident"; readonly bytes: Uint8Array }
interface UnloadedContent {
    readonly kind: "unloaded"; readonly size: number; readonly key: unknown;
    readonly loader: SeedLoader<unknown>;
}
interface LoadingContent extends Omit<UnloadedContent, "kind"> {
    readonly kind: "loading"; readonly promise: Promise<void>;
}
interface FailedContent extends Omit<UnloadedContent, "kind"> {
    readonly kind: "failed"; readonly error: unknown;
}
type FileContentState = ResidentContent | UnloadedContent | LoadingContent | FailedContent;
interface FileNode extends BaseNode { kind: "file"; content: FileContentState; contentRevision: number }
interface SymlinkNode extends BaseNode { kind: "symlink"; target: string }
type Node = DirectoryNode | FileNode | SymlinkNode;
interface Fid { node: Node; flags: number }
interface SharedState {
    nextNodeId: number;
    nextSessionId: number;
    listeners: Set<(change: P9Change) => void>;
    limits: Required<Memory9PLimits>;
    inodeCount: number;
    directoryEntries: number;
    logicalBytes: number;
    locks: ByteRangeLock[];
    engines: Set<ProtocolEngine>;
    root?: DirectoryNode;
}
interface ByteRangeLock {
    inode: number; session: number; type: number; start: number; length: number;
    procId: number; clientId: string;
}
export type P9Outcome =
    | { kind: "reply"; bytes: Uint8Array }
    | { kind: "suppressed" };
export type FileReadState =
    | { readonly kind: "resident"; readonly bytes: Uint8Array }
    | { readonly kind: "not-loaded"; readonly paths: readonly string[] }
    | { readonly kind: "failed"; readonly error: unknown };
interface PendingRequest {
    generation: number;
    resolve: (outcome: P9Outcome) => void;
    expectResponse: () => void;
}

class P9Error extends Error {
    readonly errno: number;
    constructor(errno: number, message: string) {
        super(message);
        this.errno = errno;
    }
}

class Reader {
    private readonly data: Uint8Array;
    private readonly view: DataView;
    private offset: number;
    constructor(data: Uint8Array) {
        this.data = data;
        this.view = new DataView(data.buffer, data.byteOffset, data.byteLength);
        this.offset = 0;
    }

    require(size: number): void {
        if (!Number.isInteger(size) || size < 0 || this.offset + size > this.data.length) {
            throw new P9Error(EPROTO, "truncated 9p message");
        }
    }

    u8(): number {
        this.require(1);
        const value = this.data[this.offset];
        if (value === undefined) throw new P9Error(EPROTO, "truncated 9p message");
        this.offset += 1;
        return value;
    }

    u16(): number {
        this.require(2);
        const value = this.view.getUint16(this.offset, true);
        this.offset += 2;
        return value;
    }

    u32(): number {
        this.require(4);
        const value = this.view.getUint32(this.offset, true);
        this.offset += 4;
        return value;
    }

    u64(): number {
        const low = this.u32();
        const high = this.u32();
        if (high !== 0) {
            throw new P9Error(EFBIG, "64-bit value exceeds the server limit");
        }
        return low;
    }

    string(): string {
        const size = this.u16();
        this.require(size);
        let value;
        try {
            value = new TextDecoder("utf-8", { fatal: true }).decode(
                this.data.subarray(this.offset, this.offset + size),
            );
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            throw new P9Error(EPROTO, `invalid UTF-8 in 9p string: ${message}`);
        }
        this.offset += size;
        return value;
    }

    bytes(size: number): Uint8Array {
        this.require(size);
        const value = this.data.subarray(this.offset, this.offset + size);
        this.offset += size;
        return value;
    }

    done(): void {
        if (this.offset !== this.data.length) {
            throw new P9Error(EPROTO, "trailing data in 9p message");
        }
    }
}

class Writer {
    readonly data: Uint8Array;
    private readonly view: DataView;
    offset: number;
    constructor(capacity: number, type: number, tag: number) {
        if (capacity < 7) {
            throw new P9Error(ENOSPC, "9p reply buffer is too small");
        }
        this.data = new Uint8Array(capacity);
        this.view = new DataView(this.data.buffer);
        this.offset = 4;
        this.u8(type);
        this.u16(tag);
    }

    require(size: number): void {
        if (this.offset + size > this.data.length) {
            throw new P9Error(ENOSPC, "9p reply exceeds the negotiated buffer");
        }
    }

    u8(value: number): void {
        this.require(1);
        this.data[this.offset++] = value;
    }

    u16(value: number): void {
        this.require(2);
        this.view.setUint16(this.offset, value, true);
        this.offset += 2;
    }

    u32(value: number): void {
        this.require(4);
        this.view.setUint32(this.offset, value, true);
        this.offset += 4;
    }

    u64(value: number): void {
        if (!Number.isInteger(value) || value < 0 || value > 0xffffffff) {
            throw new P9Error(EFBIG, "value exceeds the server's 32-bit limit");
        }
        this.u32(value);
        this.u32(0);
    }

    string(value: string): void {
        const encoded = new TextEncoder().encode(value);
        if (encoded.length > 0xffff) {
            throw new P9Error(EINVAL, "9p string is too long");
        }
        this.u16(encoded.length);
        this.bytes(encoded);
    }

    bytes(value: Uint8Array): void {
        this.require(value.length);
        this.data.set(value, this.offset);
        this.offset += value.length;
    }

    qid(node: Node): void {
        this.u8(qidType(node));
        this.u32(node.version);
        this.u64(node.id);
    }

    finish(): Uint8Array {
        this.view.setUint32(0, this.offset, true);
        return this.data.slice(0, this.offset);
    }
}

function nowSeconds(): number {
    return Math.floor(Date.now() / 1000);
}

function qidType(node: Node): number {
    if (node.kind === "directory") {
        return QTDIR;
    }
    if (node.kind === "symlink") {
        return QTSYMLINK;
    }
    return 0;
}

function nodeMode(node: Node): number {
    if (node.kind === "directory") {
        return S_IFDIR | node.mode;
    }
    if (node.kind === "symlink") {
        return S_IFLNK | node.mode;
    }
    return S_IFREG | node.mode;
}

function nodeSize(node: Node): number {
    if (node.kind === "file") {
        return node.content.kind === "resident" ? node.content.bytes.length : node.content.size;
    }
    if (node.kind === "symlink") {
        return new TextEncoder().encode(node.target).length;
    }
    return 0;
}

function validateName(name: string): void {
    if (name === "" || name === "." || name === ".." || name.includes("/") || name.includes("\0")) {
        throw new P9Error(EINVAL, `invalid file name ${JSON.stringify(name)}`);
    }
}

function normalizedParts(path: string): string[] {
    if (typeof path !== "string" || path.includes("\\") || path.includes("\0")) {
        throw new P9Error(EINVAL, `invalid path ${JSON.stringify(path)}`);
    }
    const relative = path.startsWith("/") ? path.slice(1) : path;
    if (relative === "") {
        return [];
    }
    const parts = relative.split("/");
    for (const part of parts) {
        validateName(part);
    }
    return parts;
}

function copyContent(content: FileContent): Uint8Array {
    if (typeof content === "string") {
        return new TextEncoder().encode(content);
    }
    if (content instanceof Uint8Array) {
        return content.slice();
    }
    throw new TypeError("file content must be a string or Uint8Array");
}

function rangesOverlap(leftStart: number, leftLength: number, rightStart: number, rightLength: number): boolean {
    const leftEnd = leftLength === 0 ? Number.POSITIVE_INFINITY : leftStart + leftLength;
    const rightEnd = rightLength === 0 ? Number.POSITIVE_INFINITY : rightStart + rightLength;
    return leftStart < rightEnd && rightStart < leftEnd;
}

export class ProtocolEngine {
    private readonly sharedState: SharedState;
    private readonly sessionId: number;
    readonly fids: Map<number, Fid>;
    private msize: number;
    constructor(
        tree: Memory9PTree = {},
        sharedState: SharedState | null = null,
        limits: Memory9PLimits = {},
    ) {
        const resolvedLimits = {
            maxFileBytes: limits.maxFileBytes ?? MAX_FILE_SIZE,
            maxTreeBytes: limits.maxTreeBytes ?? 1024 * 1024 * 1024,
            maxInodes: limits.maxInodes ?? 2 ** 20,
            maxDirectoryEntries: limits.maxDirectoryEntries ?? 2 ** 20,
        };
        for (const [name, value] of Object.entries(resolvedLimits)) {
            if (!Number.isSafeInteger(value) || value < 0) {
                throw new TypeError(`${name} must be a nonnegative safe integer`);
            }
        }
        this.sharedState = sharedState ?? {
            nextNodeId: 1,
            nextSessionId: 1,
            listeners: new Set(),
            limits: resolvedLimits,
            inodeCount: 0,
            directoryEntries: 0,
            logicalBytes: 0,
            locks: [],
            engines: new Set(),
        };
        this.sessionId = this.sharedState.nextSessionId++;
        this.fids = new Map();
        this.sharedState.engines.add(this);
        this.msize = MAX_MESSAGE_SIZE;
        if (sharedState === null) {
            this.root = this.createDirectory("", null, 0o755);
            this.loadTree(tree);
        }
    }

    get nextNodeId(): number {
        return this.sharedState.nextNodeId;
    }

    set nextNodeId(value: number) {
        this.sharedState.nextNodeId = value;
    }

    get listeners(): Set<(change: P9Change) => void> {
        return this.sharedState.listeners;
    }

    get root(): DirectoryNode {
        const root = this.sharedState.root;
        if (root === undefined) throw new Error("filesystem root is not initialized");
        return root;
    }

    set root(value: DirectoryNode) {
        this.sharedState.root = value;
    }

    fork(): ProtocolEngine {
        return new ProtocolEngine({}, this.sharedState);
    }

    checkCapacity(inodes: number, entries: number, bytes: number): void {
        const state = this.sharedState;
        if (state.inodeCount + inodes > state.limits.maxInodes
            || state.directoryEntries + entries > state.limits.maxDirectoryEntries
            || state.logicalBytes + bytes > state.limits.maxTreeBytes) {
            throw new P9Error(ENOSPC, "filesystem quota exceeded");
        }
    }

    register(node: Node): Node {
        this.checkCapacity(1, 0, node.kind === "file" ? nodeSize(node) : 0);
        this.sharedState.inodeCount += 1;
        if (node.kind === "file") this.sharedState.logicalBytes += nodeSize(node);
        return node;
    }

    createDirectory(name: string, parent: DirectoryNode | null, mode: number): DirectoryNode {
        if (parent !== null) this.checkCapacity(0, 1, 0);
        const time = nowSeconds();
        return this.register({
            kind: "directory",
            id: this.nextNodeId++,
            version: 0,
            name,
            parent,
            mode: mode & 0o7777,
            uid: 1000,
            gid: 1000,
            atime: time,
            mtime: time,
            ctime: time,
            linkCount: parent === null ? 1 : 0,
            fidRefs: 0,
            children: Object.create(null),
            cookies: Object.create(null),
            nextCookie: 1,
        }) as DirectoryNode;
    }

    createFile(name: string, parent: DirectoryNode, content: FileContent, mode = 0o644): FileNode {
        this.checkCapacity(0, 1, 0);
        const data = copyContent(content);
        if (data.length > this.sharedState.limits.maxFileBytes) {
            throw new P9Error(EFBIG, `file ${JSON.stringify(name)} exceeds the file limit`);
        }
        const time = nowSeconds();
        return this.register({
            kind: "file",
            id: this.nextNodeId++,
            version: 0,
            name,
            parent,
            mode: mode & 0o7777,
            uid: 1000,
            gid: 1000,
            atime: time,
            mtime: time,
            ctime: time,
            linkCount: 0,
            fidRefs: 0,
            content: { kind: "resident", bytes: data },
            contentRevision: 0,
        }) as FileNode;
    }

    createSeedFile(
        name: string,
        parent: DirectoryNode,
        entry: Extract<SeedEntry<unknown>, { readonly kind: "file" }>,
        loader: SeedLoader<unknown>,
    ): FileNode {
        this.checkCapacity(0, 1, 0);
        if (entry.size > this.sharedState.limits.maxFileBytes) {
            throw new P9Error(EFBIG, `file ${JSON.stringify(name)} exceeds the file limit`);
        }
        const time = nowSeconds();
        return this.register({
            kind: "file",
            id: this.nextNodeId++,
            version: 0,
            name,
            parent,
            mode: (entry.mode ?? 0o644) & 0o7777,
            uid: entry.uid ?? 1000,
            gid: entry.gid ?? 1000,
            atime: entry.atime ?? time,
            mtime: entry.mtime ?? time,
            ctime: entry.ctime ?? time,
            linkCount: 0,
            fidRefs: 0,
            content: { kind: "unloaded", size: entry.size, key: entry.key, loader },
            contentRevision: 0,
        }) as FileNode;
    }

    createSymlink(name: string, parent: DirectoryNode, target: string): SymlinkNode {
        this.checkCapacity(0, 1, 0);
        const time = nowSeconds();
        return this.register({
            kind: "symlink",
            id: this.nextNodeId++,
            version: 0,
            name,
            parent,
            mode: 0o777,
            uid: 1000,
            gid: 1000,
            atime: time,
            mtime: time,
            ctime: time,
            linkCount: 0,
            fidRefs: 0,
            target,
        }) as SymlinkNode;
    }

    loadTree(tree: Memory9PTree): void {
        if (tree === null || typeof tree !== "object" || tree instanceof Uint8Array) {
            throw new TypeError("the 9p tree root must be an object");
        }
        const oldRoot = this.root;
        const oldNextNodeId = this.nextNodeId;
        const oldInodes = this.sharedState.inodeCount;
        const oldEntries = this.sharedState.directoryEntries;
        const oldBytes = this.sharedState.logicalBytes;
        let newRoot: DirectoryNode;
        let newInodes: number;
        let newEntries: number;
        let newBytes: number;
        try {
            this.sharedState.inodeCount = 0;
            this.sharedState.directoryEntries = 0;
            this.sharedState.logicalBytes = 0;
            this.root = this.createDirectory("", null, 0o755);
            this.addTree(this.root, tree);
            newRoot = this.root;
            newInodes = this.sharedState.inodeCount;
            newEntries = this.sharedState.directoryEntries;
            newBytes = this.sharedState.logicalBytes;
        } catch (error) {
            this.root = oldRoot;
            this.nextNodeId = oldNextNodeId;
            this.sharedState.inodeCount = oldInodes;
            this.sharedState.directoryEntries = oldEntries;
            this.sharedState.logicalBytes = oldBytes;
            throw error;
        }
        this.root = oldRoot;
        this.sharedState.inodeCount = oldInodes;
        this.sharedState.directoryEntries = oldEntries;
        this.sharedState.logicalBytes = oldBytes;
        for (const engine of this.sharedState.engines) engine.clearFids();
        this.root = newRoot;
        this.sharedState.inodeCount = newInodes;
        this.sharedState.directoryEntries = newEntries;
        this.sharedState.logicalBytes = newBytes;
        this.sharedState.locks = [];
        this.emit({ kind: "reset", path: "", source: "host" });
    }

    loadFiles(files: Readonly<Record<string, FileContent>>): void {
        const tree = Object.create(null);
        for (const [path, content] of Object.entries(files)) {
            const parts = normalizedParts(path);
            if (parts.length === 0) {
                throw new P9Error(EINVAL, "a file path may not name the root");
            }
            let directory = tree;
            for (const part of parts.slice(0, -1)) {
                const value = directory[part];
                if (value === undefined) {
                    const created: Memory9PTree = Object.create(null);
                    directory[part] = created;
                    directory = created;
                } else if (value === null || typeof value !== "object" || value instanceof Uint8Array) {
                    throw new P9Error(EINVAL, `path crosses a file at ${JSON.stringify(part)}`);
                } else {
                    directory = value;
                }
            }
            const name = parts.at(-1);
            if (name === undefined) throw new P9Error(EINVAL, "a file path may not name the root");
            directory[name] = content;
        }
        this.loadTree(tree);
    }

    addTree(parent: DirectoryNode, tree: Memory9PTree): void {
        for (const [name, value] of Object.entries(tree)) {
            validateName(name);
            let node;
            if (typeof value === "string" || value instanceof Uint8Array) {
                node = this.createFile(name, parent, value);
            } else if (value !== null && typeof value === "object") {
                node = this.createDirectory(name, parent, 0o755);
                this.addTree(node, value);
            } else {
                throw new TypeError(`invalid tree entry ${JSON.stringify(name)}`);
            }
            this.addEntry(parent, name, node);
        }
    }

    loadSeed<Key>(plugin: SeedPlugin<Key>): void {
        const entries = plugin.entries as readonly SeedEntry<unknown>[];
        if (!Array.isArray(entries) || plugin.loader === null || typeof plugin.loader?.load !== "function") {
            throw new TypeError("invalid seed plugin");
        }
        this.loadTree({});
        const paths = new Set<string>();
        const sharedFiles = new Map<string, {
            readonly node: FileNode;
            readonly entry: Extract<SeedEntry<unknown>, { readonly kind: "file" }>;
        }>();
        for (const entry of entries) {
            const parts = normalizedParts(entry.path);
            if (parts.length === 0 || parts.join("/") !== entry.path || paths.has(entry.path)) {
                throw new TypeError(`invalid or duplicate seed path ${JSON.stringify(entry.path)}`);
            }
            paths.add(entry.path);
            const { parent, name } = this.ensureParent(entry.path);
            const existing = parent.children[name];
            if (entry.kind === "directory") {
                if (existing !== undefined && existing.kind !== "directory") {
                    throw new TypeError(`seed path crosses a non-directory at ${JSON.stringify(entry.path)}`);
                }
                const directory = existing ?? this.createDirectory(name, parent, entry.mode ?? 0o755);
                if (existing === undefined) this.addEntry(parent, name, directory);
                this.applySeedMetadata(directory, entry);
                continue;
            }
            if (existing !== undefined) throw new TypeError(`duplicate seed path ${JSON.stringify(entry.path)}`);
            if (entry.kind === "symlink") {
                const symlink = this.createSymlink(name, parent, entry.target);
                this.applySeedMetadata(symlink, entry);
                this.addEntry(parent, name, symlink);
                continue;
            }
            const shared = entry.inodeKey === undefined ? undefined : sharedFiles.get(entry.inodeKey);
            let file: FileNode;
            if (shared === undefined) {
                file = this.createSeedFile(name, parent, entry, plugin.loader as SeedLoader<unknown>);
                if (entry.inodeKey !== undefined) sharedFiles.set(entry.inodeKey, { node: file, entry });
            } else {
                const original = shared.entry;
                if (entry.size !== original.size || entry.key !== original.key
                    || entry.mode !== original.mode || entry.uid !== original.uid
                    || entry.gid !== original.gid || entry.atime !== original.atime
                    || entry.mtime !== original.mtime || entry.ctime !== original.ctime) {
                    throw new TypeError(`inconsistent shared seed inode ${JSON.stringify(entry.inodeKey)}`);
                }
                file = shared.node;
            }
            this.addEntry(parent, name, file);
        }
    }

    applySeedMetadata(node: Node, metadata: SeedMetadata): void {
        if (metadata.mode !== undefined) node.mode = metadata.mode & 0o7777;
        if (metadata.uid !== undefined) node.uid = metadata.uid;
        if (metadata.gid !== undefined) node.gid = metadata.gid;
        if (metadata.atime !== undefined) node.atime = metadata.atime;
        if (metadata.mtime !== undefined) node.mtime = metadata.mtime;
        if (metadata.ctime !== undefined) node.ctime = metadata.ctime;
    }

    addEntry(parent: DirectoryNode, name: string, node: Node, cookie?: number): void {
        this.checkCapacity(0, 1, 0);
        parent.children[name] = node;
        parent.cookies[name] = cookie ?? parent.nextCookie++;
        node.linkCount += 1;
        if (node.kind === "directory") {
            node.parent = parent;
            node.name = name;
        }
        this.sharedState.directoryEntries += 1;
    }

    removeEntry(parent: DirectoryNode, name: string): Node {
        const node = this.child(parent, name);
        delete parent.children[name];
        delete parent.cookies[name];
        this.sharedState.directoryEntries -= 1;
        node.linkCount -= 1;
        if (node.kind === "directory") node.parent = null;
        this.collect(node);
        return node;
    }

    collect(node: Node): void {
        if (node.linkCount !== 0 || node.fidRefs !== 0) return;
        this.sharedState.inodeCount -= 1;
        if (node.kind === "file") this.sharedState.logicalBytes -= nodeSize(node);
        this.sharedState.locks = this.sharedState.locks.filter((lock) => lock.inode !== node.id);
    }

    resize(node: FileNode, size: number): void {
        if (size > this.sharedState.limits.maxFileBytes) {
            throw new P9Error(EFBIG, "file exceeds the file limit");
        }
        const oldSize = nodeSize(node);
        const growth = size - oldSize;
        this.checkCapacity(0, 0, growth);
        const data = new Uint8Array(size);
        if (node.content.kind === "resident") {
            data.set(node.content.bytes.subarray(0, size));
        } else if (size !== 0) {
            throw new P9Error(EIO, "file content is not loaded");
        }
        node.content = { kind: "resident", bytes: data };
        node.contentRevision += 1;
        this.sharedState.logicalBytes += growth;
    }

    subscribe(listener: (change: P9Change) => void): () => void {
        this.listeners.add(listener);
        return () => {
            this.listeners.delete(listener);
        };
    }

    emit(change: P9Change): void {
        for (const listener of this.listeners) {
            listener(change);
        }
    }

    inodeChange(node: Node, path = this.pathOf(node)): {
        readonly inode: number; readonly path: string; readonly paths: readonly string[];
    } {
        return { inode: node.id, path, paths: this.pathsOf(node) };
    }

    pathOf(node: Node): string {
        return this.pathsOf(node)[0] ?? "";
    }

    pathsOf(target: Node): string[] {
        if (target === this.root) return [""];
        const paths: string[] = [];
        const visit = (directory: DirectoryNode, prefix: string): void => {
            for (const [name, node] of Object.entries(directory.children)) {
                const path = prefix === "" ? name : `${prefix}/${name}`;
                if (node === target) paths.push(path);
                if (node.kind === "directory") visit(node, path);
            }
        };
        visit(this.root, "");
        return paths.sort();
    }

    lookup(path: string): Node {
        let node: Node = this.root;
        for (const part of normalizedParts(path)) {
            if (node.kind !== "directory") {
                throw new P9Error(ENOTDIR, "path crosses a non-directory");
            }
            const child: Node | undefined = node.children[part];
            if (child === undefined) {
                throw new P9Error(ENOENT, `file not found: ${path}`);
            }
            node = child;
        }
        return node;
    }

    ensureParent(path: string): { parent: DirectoryNode; name: string } {
        const parts = normalizedParts(path);
        const name = parts.pop();
        if (name === undefined) {
            throw new P9Error(EINVAL, "operation may not replace the root");
        }
        let parent = this.root;
        for (const part of parts) {
            let child = parent.children[part];
            if (child === undefined) {
                child = this.createDirectory(part, parent, 0o755);
                this.addEntry(parent, part, child);
                this.touch(parent);
            }
            if (child.kind !== "directory") {
                throw new P9Error(ENOTDIR, "path crosses a non-directory");
            }
            parent = child;
        }
        return { parent, name };
    }

    readFile(path: string): Uint8Array {
        const node = this.lookup(path);
        if (node.kind !== "file") {
            throw new P9Error(EISDIR, `${path} is not a regular file`);
        }
        if (node.content.kind !== "resident") throw new P9Error(EIO, "file content is not loaded");
        return node.content.bytes.slice();
    }

    readFileState(path: string): FileReadState {
        const node = this.lookup(path);
        if (node.kind !== "file") throw new P9Error(EISDIR, `${path} is not a regular file`);
        if (node.content.kind === "resident") {
            return { kind: "resident", bytes: node.content.bytes.slice() };
        }
        if (node.content.kind === "failed") return { kind: "failed", error: node.content.error };
        return { kind: "not-loaded", paths: this.pathsOf(node) };
    }

    async loadPaths(paths: readonly string[], retry = false): Promise<void> {
        const nodes = new Set<FileNode>();
        for (const path of paths) {
            const node = this.lookup(path);
            if (node.kind !== "file") throw new P9Error(EISDIR, `${path} is not a regular file`);
            nodes.add(node);
        }
        await Promise.all([...nodes].map((node) => this.loadNode(node, retry)));
    }

    async loadNode(node: FileNode, retry = false): Promise<void> {
        if (node.content.kind === "resident") return;
        if (node.content.kind === "loading") return node.content.promise;
        if (node.content.kind === "failed" && !retry) throw node.content.error;
        const source = node.content;
        const revision = node.contentRevision;
        let loading: LoadingContent;
        let loaded: Promise<Uint8Array>;
        try {
            loaded = source.loader.load(source.key, new AbortController().signal);
        } catch (error) {
            loaded = Promise.reject(error);
        }
        const promise = loaded.then((bytes) => {
            if (!(bytes instanceof Uint8Array) || bytes.length !== source.size) {
                throw new P9Error(EIO, "seed loader returned an unexpected length");
            }
            if (node.content !== loading || node.contentRevision !== revision
                || (node.linkCount === 0 && node.fidRefs === 0)) return;
            node.content = { kind: "resident", bytes };
            this.emit({ kind: "loaded", ...this.inodeChange(node), source: "loader" });
        }).catch((error: unknown) => {
            if (node.content === loading && node.contentRevision === revision
                && (node.linkCount !== 0 || node.fidRefs !== 0)) {
                node.content = { ...source, kind: "failed", error };
                this.emit({ kind: "load-error", ...this.inodeChange(node), source: "loader" });
            }
            throw error;
        });
        loading = { ...source, kind: "loading", promise };
        node.content = loading;
        return promise;
    }

    writeFile(path: string, content: FileContent, source: ChangeSource = "host"): void {
        const data = copyContent(content);
        if (data.length > this.sharedState.limits.maxFileBytes) {
            throw new P9Error(EFBIG, `file ${JSON.stringify(path)} exceeds the file limit`);
        }
        const { parent, name } = this.ensureParent(path);
        const existing = parent.children[name];
        let kind: "write" | "create" = "write";
        if (existing === undefined) {
            this.addEntry(parent, name, this.createFile(name, parent, data));
            kind = "create";
        } else {
            if (existing.kind !== "file") {
                throw new P9Error(EISDIR, `${path} is not a regular file`);
            }
            const oldSize = nodeSize(existing);
            const growth = data.length - oldSize;
            this.checkCapacity(0, 0, growth);
            existing.contentRevision += 1;
            existing.content = { kind: "resident", bytes: data };
            this.sharedState.logicalBytes += growth;
            this.touch(existing);
        }
        this.touch(parent);
        const changed = parent.children[name];
        if (changed === undefined) throw new Error("created file is missing");
        this.emit({ kind, ...this.inodeChange(changed, normalizedParts(path).join("/")), source });
    }

    remove(path: string, source: ChangeSource = "host"): void {
        const node = this.lookup(path);
        const parts = normalizedParts(path);
        const name = parts.pop();
        if (node === this.root || name === undefined) {
            throw new P9Error(EPERM, "cannot remove the root");
        }
        if (node.kind === "directory" && Object.keys(node.children).length !== 0) {
            throw new P9Error(ENOTEMPTY, "directory is not empty");
        }
        const oldPath = this.pathOf(node);
        const parentNode = this.lookup(parts.join("/"));
        const parent = this.directory(parentNode);
        this.removeEntry(parent, name);
        this.touch(parent);
        this.emit({
            kind: "remove", inode: node.id, path: oldPath,
            paths: [oldPath, ...this.pathsOf(node)], source,
        });
    }

    rename(oldPath: string, newPath: string, source: ChangeSource = "host"): void {
        const node = this.lookup(oldPath);
        const oldParts = normalizedParts(oldPath);
        const oldName = oldParts.pop();
        if (node === this.root || oldName === undefined) {
            throw new P9Error(EPERM, "cannot rename the root");
        }
        const destination = this.ensureParent(newPath);
        this.moveNode(this.directory(this.lookup(oldParts.join("/"))), oldName, destination.parent, destination.name, source);
    }

    listFiles(): string[] {
        const result: string[] = [];
        const visit = (directory: DirectoryNode, prefix: string): void => {
            for (const [name, node] of Object.entries(directory.children)) {
                const path = prefix === "" ? name : `${prefix}/${name}`;
                if (node.kind === "directory") {
                    visit(node, path);
                } else if (node.kind === "file") {
                    result.push(path);
                }
            }
        };
        visit(this.root, "");
        return result.sort();
    }

    snapshot(): Record<string, Uint8Array> {
        const result: Record<string, Uint8Array> = Object.create(null);
        for (const path of this.listFiles()) {
            result[path] = this.readFile(path);
        }
        return result;
    }

    touch(node: Node): void {
        const time = nowSeconds();
        node.version = (node.version + 1) >>> 0;
        node.mtime = time;
        node.ctime = time;
    }

    fid(number: number): Fid {
        const fid = this.fids.get(number);
        if (fid === undefined) {
            throw new P9Error(EBADF, `unknown fid ${number}`);
        }
        return fid;
    }

    setFid(number: number, node: Node, flags = 0): void {
        this.dropFid(number);
        node.fidRefs += 1;
        this.fids.set(number, { node, flags });
    }

    dropFid(number: number): void {
        const fid = this.fids.get(number);
        if (fid === undefined) return;
        this.fids.delete(number);
        fid.node.fidRefs -= 1;
        this.collect(fid.node);
    }

    clearFids(): void {
        for (const number of [...this.fids.keys()]) this.dropFid(number);
    }

    resetSession(): void {
        this.clearFids();
        this.sharedState.locks = this.sharedState.locks.filter(
            (lock) => lock.session !== this.sessionId,
        );
    }

    close(): void {
        this.resetSession();
        this.sharedState.engines.delete(this);
    }

    directory(node: Node): DirectoryNode {
        if (node.kind !== "directory") {
            throw new P9Error(ENOTDIR, "fid does not name a directory");
        }
        return node;
    }

    child(directory: DirectoryNode, name: string): Node {
        validateName(name);
        const child = directory.children[name];
        if (child === undefined) {
            throw new P9Error(ENOENT, `file not found: ${name}`);
        }
        return child;
    }

    request(request: Uint8Array, replyCapacity: number): Uint8Array {
        let type = 0;
        let tag = NOTAG;
        try {
            if (!(request instanceof Uint8Array) || !Number.isInteger(replyCapacity)) {
                throw new P9Error(EPROTO, "invalid 9p request arguments");
            }
            const reader = new Reader(request);
            const declaredSize = reader.u32();
            type = reader.u8();
            tag = reader.u16();
            if (declaredSize !== request.length || declaredSize < 7 || declaredSize > this.msize) {
                throw new P9Error(EPROTO, "invalid 9p message size");
            }
            const writer = new Writer(Math.min(replyCapacity, this.msize), type + 1, tag);
            this.handle(type, tag, reader, writer);
            reader.done();
            return writer.finish();
        } catch (error) {
            const errno = error instanceof P9Error ? error.errno : EIO;
            if (!(error instanceof P9Error)) {
                console.error("Memory9PServer request failed", error);
            }
            const writer = new Writer(replyCapacity, RLERROR, tag);
            writer.u32(errno);
            return writer.finish();
        }
    }

    residentRequest(request: Uint8Array): boolean {
        if (request.length < 11) return true;
        const view = new DataView(request.buffer, request.byteOffset, request.byteLength);
        const type = request[4];
        if (type !== 116 && type !== 118 && type !== 26) return true;
        try {
            const node = this.fid(view.getUint32(7, true)).node;
            return node.kind !== "file" || node.content.kind === "resident"
                || node.content.kind === "failed";
        } catch {
            return true;
        }
    }

    async prepare(request: Uint8Array): Promise<void> {
        if (request.length < 11) return;
        const view = new DataView(request.buffer, request.byteOffset, request.byteLength);
        const type = request[4];
        if (type !== 116 && type !== 118 && type !== 26) return;
        const node = this.fid(view.getUint32(7, true)).node;
        if (node.kind !== "file") return;
        if (type === 26) {
            if (request.length < 35 || (view.getUint32(11, true) & 8) === 0) return;
            const sizeLow = view.getUint32(27, true);
            const sizeHigh = view.getUint32(31, true);
            if (sizeLow === 0 && sizeHigh === 0) return;
        }
        await this.loadNode(node);
    }

    handle(type: number, tag: number, reader: Reader, writer: Writer): void {
        switch (type) {
            case 8: this.statfs(reader, writer); return;
            case 12: this.lopen(reader, writer); return;
            case 14: this.lcreate(reader, writer); return;
            case 16: this.symlink(reader, writer); return;
            case 18: throw new P9Error(EOPNOTSUPP, "mknod is not supported");
            case 22: this.readlink(reader, writer); return;
            case 24: this.getattr(reader, writer); return;
            case 26: this.setattr(reader); return;
            case 30: throw new P9Error(EOPNOTSUPP, "extended attributes are not supported");
            case 40: this.readdir(reader, writer); return;
            case 50: this.fsync(reader); return;
            case 52: this.lock(reader, writer); return;
            case 54: this.getlock(reader, writer); return;
            case 70: this.link(reader); return;
            case 72: this.mkdir(reader, writer); return;
            case 74: this.renameat(reader); return;
            case 76: this.unlinkat(reader); return;
            case 100: this.version(tag, reader, writer); return;
            case 104: this.attach(reader, writer); return;
            case 108: reader.u16(); return;
            case 110: this.walk(reader, writer); return;
            case 116: this.read(reader, writer); return;
            case 118: this.write(reader, writer); return;
            case 120: this.clunk(reader); return;
            default: throw new P9Error(EOPNOTSUPP, `unsupported 9p operation ${type}`);
        }
    }

    statfs(reader: Reader, writer: Writer): void {
        this.fid(reader.u32());
        const blocks = Math.floor(this.sharedState.limits.maxTreeBytes / 4096);
        const freeBlocks = Math.floor(
            (this.sharedState.limits.maxTreeBytes - this.sharedState.logicalBytes) / 4096,
        );
        writer.u32(0x01021997);
        writer.u32(4096);
        writer.u64(blocks);
        writer.u64(freeBlocks);
        writer.u64(freeBlocks);
        writer.u64(this.sharedState.limits.maxInodes);
        writer.u64(this.sharedState.limits.maxInodes - this.sharedState.inodeCount);
        writer.u64(1);
        writer.u32(255);
    }

    lopen(reader: Reader, writer: Writer): void {
        const fid = this.fid(reader.u32());
        const flags = reader.u32();
        if ((flags & O_TRUNC) !== 0) {
            if (fid.node.kind !== "file") {
                throw new P9Error(EISDIR, "cannot truncate a directory");
            }
            this.resize(fid.node, 0);
            this.touch(fid.node);
            this.emit({ kind: "write", ...this.inodeChange(fid.node), source: "guest" });
        }
        fid.flags = flags;
        writer.qid(fid.node);
        writer.u32(Math.max(0, this.msize - 24));
    }

    lcreate(reader: Reader, writer: Writer): void {
        const fidNumber = reader.u32();
        const fid = this.fid(fidNumber);
        const directory = this.directory(fid.node);
        const name = reader.string();
        const flags = reader.u32();
        const mode = reader.u32();
        reader.u32();
        validateName(name);
        if (directory.children[name] !== undefined) {
            throw new P9Error(EEXIST, "file already exists");
        }
        const node = this.createFile(name, directory, new Uint8Array(), mode);
        this.addEntry(directory, name, node);
        this.touch(directory);
        this.setFid(fidNumber, node, flags);
        this.emit({ kind: "create", ...this.inodeChange(node), source: "guest" });
        writer.qid(node);
        writer.u32(Math.max(0, this.msize - 24));
    }

    symlink(reader: Reader, writer: Writer): void {
        const directory = this.directory(this.fid(reader.u32()).node);
        const name = reader.string();
        const target = reader.string();
        reader.u32();
        validateName(name);
        if (new TextEncoder().encode(target).length > 0xffff) {
            throw new P9Error(EINVAL, "symlink target is too long");
        }
        if (directory.children[name] !== undefined) {
            throw new P9Error(EEXIST, "file already exists");
        }
        const node = this.createSymlink(name, directory, target);
        this.addEntry(directory, name, node);
        this.touch(directory);
        this.emit({ kind: "create", ...this.inodeChange(node), source: "guest" });
        writer.qid(node);
    }

    readlink(reader: Reader, writer: Writer): void {
        const node = this.fid(reader.u32()).node;
        if (node.kind !== "symlink") {
            throw new P9Error(EINVAL, "fid does not name a symlink");
        }
        writer.string(node.target);
    }

    getattr(reader: Reader, writer: Writer): void {
        const node = this.fid(reader.u32()).node;
        const requested = reader.u64();
        writer.u64(requested);
        writer.qid(node);
        writer.u32(nodeMode(node));
        writer.u32(node.uid);
        writer.u32(node.gid);
        writer.u64(node.kind === "directory"
            ? 2 + Object.values(node.children).filter((child) => child.kind === "directory").length
            : node.linkCount);
        writer.u64(0);
        writer.u64(nodeSize(node));
        writer.u64(4096);
        writer.u64(Math.ceil(nodeSize(node) / 512));
        writer.u64(node.atime); writer.u64(0);
        writer.u64(node.mtime); writer.u64(0);
        writer.u64(node.ctime); writer.u64(0);
        writer.u64(0); writer.u64(0);
        writer.u64(0);
        writer.u64(node.version);
    }

    setattr(reader: Reader): void {
        const node = this.fid(reader.u32()).node;
        const mask = reader.u32();
        const mode = reader.u32();
        const uid = reader.u32();
        const gid = reader.u32();
        const size = reader.u64();
        const atime = reader.u64();
        reader.u64();
        const mtime = reader.u64();
        reader.u64();
        const time = nowSeconds();
        let dataChanged = false;
        if ((mask & 1) !== 0) node.mode = mode & 0o7777;
        if ((mask & 2) !== 0) node.uid = uid;
        if ((mask & 4) !== 0) node.gid = gid;
        if ((mask & 8) !== 0) {
            if (node.kind !== "file") throw new P9Error(EISDIR, "cannot resize a directory");
            this.resize(node, size);
            dataChanged = true;
            this.emit({ kind: "write", ...this.inodeChange(node), source: "guest" });
        }
        if ((mask & 0x10) !== 0) node.atime = (mask & 0x80) !== 0 ? atime : time;
        if ((mask & 0x20) !== 0) node.mtime = (mask & 0x100) !== 0 ? mtime : time;
        else if (dataChanged) node.mtime = time;
        node.version = (node.version + 1) >>> 0;
        node.ctime = time;
    }

    readdir(reader: Reader, writer: Writer): void {
        const directory = this.directory(this.fid(reader.u32()).node);
        const offset = reader.u64();
        const count = reader.u32();
        const entries = Object.entries(directory.children)
            .map(([name, node]) => ({ name, node, cookie: directory.cookies[name] ?? 0 }))
            .filter((entry) => entry.cookie > offset)
            .sort((left, right) => left.cookie - right.cookie);
        const payload = new Writer(Math.min(count + 7, MAX_MESSAGE_SIZE), 0, 0);
        payload.offset = 0;
        for (const entry of entries) {
            const { node, name, cookie } = entry;
            const encodedName = new TextEncoder().encode(name);
            const entrySize = 13 + 8 + 1 + 2 + encodedName.length;
            if (payload.offset + entrySize > count) break;
            payload.qid(node);
            payload.u64(cookie);
            payload.u8(node.kind === "directory" ? 4 : node.kind === "symlink" ? 10 : 8);
            payload.string(name);
        }
        writer.u32(payload.offset);
        writer.bytes(payload.data.subarray(0, payload.offset));
    }

    fsync(reader: Reader): void {
        this.fid(reader.u32());
        reader.u32();
    }

    lock(reader: Reader, writer: Writer): void {
        const node = this.fid(reader.u32()).node;
        if (node.kind !== "file") throw new P9Error(EOPNOTSUPP, "locks require a regular file");
        const type = reader.u8();
        reader.u32();
        const start = reader.u64();
        const length = reader.u64();
        const procId = reader.u32();
        const clientId = reader.string();
        if (type > 2) throw new P9Error(EINVAL, "invalid lock type");
        if (type === 2) {
            this.sharedState.locks = this.sharedState.locks.filter((lock) =>
                !(lock.inode === node.id && lock.session === this.sessionId
                    && lock.procId === procId && lock.clientId === clientId
                    && rangesOverlap(lock.start, lock.length, start, length))
            );
            writer.u8(0);
            return;
        }
        const conflict = this.sharedState.locks.find((lock) =>
            lock.inode === node.id && lock.session !== this.sessionId
            && (lock.type === 1 || type === 1)
            && rangesOverlap(lock.start, lock.length, start, length)
        );
        if (conflict !== undefined) {
            writer.u8(1);
            return;
        }
        this.sharedState.locks.push({
            inode: node.id, session: this.sessionId, type, start, length, procId, clientId,
        });
        writer.u8(0);
    }

    getlock(reader: Reader, writer: Writer): void {
        const node = this.fid(reader.u32()).node;
        if (node.kind !== "file") throw new P9Error(EOPNOTSUPP, "locks require a regular file");
        const type = reader.u8();
        const start = reader.u64();
        const length = reader.u64();
        reader.u32(); reader.string();
        const conflict = this.sharedState.locks.find((lock) =>
            lock.inode === node.id && lock.session !== this.sessionId
            && (lock.type === 1 || type === 1)
            && rangesOverlap(lock.start, lock.length, start, length)
        );
        writer.u8(conflict?.type ?? 2);
        writer.u64(conflict?.start ?? 0);
        writer.u64(conflict?.length ?? 0);
        writer.u32(conflict?.procId ?? 0);
        writer.string(conflict?.clientId ?? "");
    }

    link(reader: Reader): void {
        const directory = this.directory(this.fid(reader.u32()).node);
        const node = this.fid(reader.u32()).node;
        const name = reader.string();
        validateName(name);
        if (node.kind === "directory") throw new P9Error(EPERM, "directory hard links are forbidden");
        if (directory.children[name] !== undefined) throw new P9Error(EEXIST, "file already exists");
        this.addEntry(directory, name, node);
        this.touch(directory);
        this.touch(node);
        const directoryPath = this.pathOf(directory);
        const path = directoryPath === "" ? name : `${directoryPath}/${name}`;
        this.emit({ kind: "create", ...this.inodeChange(node, path), source: "guest" });
    }

    mkdir(reader: Reader, writer: Writer): void {
        const directory = this.directory(this.fid(reader.u32()).node);
        const name = reader.string();
        const mode = reader.u32();
        reader.u32();
        validateName(name);
        if (directory.children[name] !== undefined) throw new P9Error(EEXIST, "file already exists");
        const node = this.createDirectory(name, directory, mode);
        this.addEntry(directory, name, node);
        this.touch(directory);
        this.emit({ kind: "create", ...this.inodeChange(node), source: "guest" });
        writer.qid(node);
    }

    renameat(reader: Reader): void {
        const oldDirectory = this.directory(this.fid(reader.u32()).node);
        const oldName = reader.string();
        const newDirectory = this.directory(this.fid(reader.u32()).node);
        const newName = reader.string();
        this.moveNode(oldDirectory, oldName, newDirectory, newName, "guest");
    }

    moveNode(
        oldDirectory: DirectoryNode,
        oldName: string,
        newDirectory: DirectoryNode,
        newName: string,
        source: ChangeSource,
    ): void {
        validateName(oldName);
        validateName(newName);
        const node = this.child(oldDirectory, oldName);
        if (oldDirectory === newDirectory && oldName === newName) {
            return;
        }
        for (
            let ancestor: DirectoryNode | null = newDirectory;
            ancestor !== null;
            ancestor = ancestor.parent
        ) {
            if (ancestor === node) throw new P9Error(EINVAL, "cannot move a directory into itself");
        }
        const replaced = newDirectory.children[newName];
        if (replaced !== undefined) {
            if (replaced.kind === "directory" && Object.keys(replaced.children).length !== 0) {
                throw new P9Error(ENOTEMPTY, "destination directory is not empty");
            }
            if ((replaced.kind === "directory") !== (node.kind === "directory")) {
                throw new P9Error(replaced.kind === "directory" ? EISDIR : ENOTDIR, "rename type mismatch");
            }
            this.removeEntry(newDirectory, newName);
        }
        const oldPath = this.pathOf(node);
        const oldCookie = oldDirectory.cookies[oldName];
        delete oldDirectory.children[oldName];
        delete oldDirectory.cookies[oldName];
        newDirectory.children[newName] = node;
        newDirectory.cookies[newName] = oldDirectory === newDirectory && oldCookie !== undefined
            ? oldCookie
            : newDirectory.nextCookie++;
        if (node.kind === "directory") {
            node.parent = newDirectory;
            node.name = newName;
        }
        this.touch(oldDirectory);
        if (newDirectory !== oldDirectory) this.touch(newDirectory);
        this.touch(node);
        this.emit({ kind: "rename", ...this.inodeChange(node), oldPath, source });
    }

    unlinkat(reader: Reader): void {
        const directory = this.directory(this.fid(reader.u32()).node);
        const name = reader.string();
        const flags = reader.u32();
        const node = this.child(directory, name);
        const removingDirectory = (flags & AT_REMOVEDIR) !== 0;
        if (removingDirectory !== (node.kind === "directory")) {
            throw new P9Error(removingDirectory ? ENOTDIR : EISDIR, "unlink type mismatch");
        }
        if (node.kind === "directory" && Object.keys(node.children).length !== 0) {
            throw new P9Error(ENOTEMPTY, "directory is not empty");
        }
        const path = this.pathOf(node);
        this.removeEntry(directory, name);
        this.touch(directory);
        this.emit({
            kind: "remove", inode: node.id, path,
            paths: [path, ...this.pathsOf(node)], source: "guest",
        });
    }

    version(tag: number, reader: Reader, writer: Writer): void {
        if (tag !== NOTAG) throw new P9Error(EPROTO, "Tversion must use NOTAG");
        const requestedSize = reader.u32();
        const version = reader.string();
        if (requestedSize < 256) throw new P9Error(EINVAL, "negotiated msize is too small");
        this.resetSession();
        this.msize = Math.min(requestedSize, MAX_MESSAGE_SIZE);
        writer.u32(this.msize);
        writer.string(version === "9P2000.L" ? version : "unknown");
    }

    attach(reader: Reader, writer: Writer): void {
        const fidNumber = reader.u32();
        reader.u32();
        reader.string();
        reader.string();
        reader.u32();
        if (this.fids.has(fidNumber)) throw new P9Error(EEXIST, "fid already exists");
        this.setFid(fidNumber, this.root);
        writer.qid(this.root);
    }

    walk(reader: Reader, writer: Writer): void {
        const fidNumber = reader.u32();
        const newFidNumber = reader.u32();
        const count = reader.u16();
        const start = this.fid(fidNumber).node;
        if (newFidNumber !== fidNumber && this.fids.has(newFidNumber)) {
            throw new P9Error(EEXIST, "new fid already exists");
        }
        let node = start;
        const qids = [];
        for (let index = 0; index < count; index += 1) {
            const name = reader.string();
            try {
                if (name === ".") {
                    qids.push(node);
                    continue;
                }
                if (name === "..") {
                    node = node.parent ?? node;
                    qids.push(node);
                    continue;
                }
                node = this.child(this.directory(node), name);
                qids.push(node);
            } catch (error) {
                if (qids.length === 0) throw error;
                for (let remaining = index + 1; remaining < count; remaining += 1) reader.string();
                break;
            }
        }
        this.setFid(newFidNumber, node);
        writer.u16(qids.length);
        for (const qid of qids) writer.qid(qid);
    }

    read(reader: Reader, writer: Writer): void {
        const node = this.fid(reader.u32()).node;
        const offset = reader.u64();
        const count = reader.u32();
        if (node.kind !== "file") throw new P9Error(EISDIR, "fid does not name a regular file");
        if (node.content.kind !== "resident") throw new P9Error(EIO, "file content is not loaded");
        const bytes = node.content.bytes;
        const available = Math.max(0, Math.min(count, bytes.length - offset, writer.data.length - writer.offset - 4));
        writer.u32(available);
        writer.bytes(bytes.subarray(offset, offset + available));
        node.atime = nowSeconds();
    }

    write(reader: Reader, writer: Writer): void {
        const fid = this.fid(reader.u32());
        let offset = reader.u64();
        const count = reader.u32();
        const data = reader.bytes(count);
        const node = fid.node;
        if (node.kind !== "file") throw new P9Error(EISDIR, "fid does not name a regular file");
        if (node.content.kind !== "resident") throw new P9Error(EIO, "file content is not loaded");
        if ((fid.flags & O_APPEND) !== 0) offset = node.content.bytes.length;
        if (offset + count > this.sharedState.limits.maxFileBytes) throw new P9Error(EFBIG, "file exceeds the file limit");
        const size = Math.max(node.content.bytes.length, offset + count);
        this.resize(node, size);
        if (node.content.kind !== "resident") throw new P9Error(EIO, "file content is not loaded");
        node.content.bytes.set(data, offset);
        this.touch(node);
        this.emit({ kind: "write", ...this.inodeChange(node), source: "guest" });
        writer.u32(count);
    }

    clunk(reader: Reader): void {
        const fidNumber = reader.u32();
        this.fid(fidNumber);
        this.dropFid(fidNumber);
    }
}

export class P9Session {
    private readonly protocol: ProtocolEngine;
    private closed: boolean;
    private generation: number;
    private readonly active: Map<number, PendingRequest>;
    constructor(protocol: ProtocolEngine) {
        this.protocol = protocol;
        this.closed = false;
        this.generation = 1;
        this.active = new Map();
    }

    request(request: Uint8Array, replyCapacity: number,
            expectResponse: () => void = () => {}): Promise<P9Outcome> {
        if (this.closed) {
            return Promise.reject(new Error("9p session is closed"));
        }
        let type: number;
        let tag: number;
        try {
            const reader = new Reader(request);
            const size = reader.u32();
            type = reader.u8();
            tag = reader.u16();
            if (size !== request.length || size < 7) {
                throw new P9Error(EPROTO, "invalid 9p message size");
            }
        } catch (error) {
            expectResponse();
            return Promise.resolve({
                kind: "reply",
                bytes: this.protocol.request(request, replyCapacity),
            });
        }
        if (type === 100) {
            this.retireActive();
            this.generation += 1;
            expectResponse();
            return Promise.resolve({
                kind: "reply",
                bytes: this.protocol.request(request, replyCapacity),
            });
        }
        if (type === 108) {
            if (request.length === 9) {
                const oldTag = new DataView(
                    request.buffer, request.byteOffset, request.byteLength,
                ).getUint16(7, true);
                const pending = this.active.get(oldTag);
                if (pending !== undefined) {
                    this.active.delete(oldTag);
                    pending.expectResponse();
                    pending.resolve({ kind: "suppressed" });
                }
            }
            expectResponse();
            return Promise.resolve({
                kind: "reply",
                bytes: this.protocol.request(request, replyCapacity),
            });
        }
        if (this.active.has(tag)) {
            expectResponse();
            const writer = new Writer(replyCapacity, RLERROR, tag);
            writer.u32(EPROTO);
            return Promise.resolve({ kind: "reply", bytes: writer.finish() });
        }
        const generation = this.generation;
        let hinted = false;
        const hint = (): void => {
            if (!hinted) {
                hinted = true;
                expectResponse();
            }
        };
        if (this.protocol.residentRequest(request)) hint();
        return new Promise<P9Outcome>((resolve) => {
            const pending = { generation, resolve, expectResponse: hint };
            this.active.set(tag, pending);
            queueMicrotask(async () => {
                if (this.active.get(tag) !== pending || this.generation !== generation) {
                    return;
                }
                try {
                    await this.protocol.prepare(request);
                } catch {
                    // The protocol request below reports retained loader failures as EIO.
                }
                if (this.active.get(tag) !== pending || this.generation !== generation) {
                    return;
                }
                this.active.delete(tag);
                resolve({
                    kind: "reply",
                    bytes: this.protocol.request(request, replyCapacity),
                });
            });
        });
    }

    retireActive(): void {
        for (const pending of this.active.values()) {
            pending.expectResponse();
            pending.resolve({ kind: "suppressed" });
        }
        this.active.clear();
    }

    close(): void {
        if (this.closed) {
            return;
        }
        this.closed = true;
        this.generation += 1;
        this.retireActive();
        this.protocol.close();
    }
}

export { MAX_FILE_SIZE, P9Error };
