/*
 * Small synchronous 9P2000.L server for browser-hosted teaching VMs.
 *
 * The implementation deliberately favors a direct data model over general
 * filesystem features. Files are bounded at 16 MiB and all accepted 64-bit
 * wire values must fit in their low 32 bits.
 */

const MAX_FILE_SIZE = 16 * 1024 * 1024;
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

class P9Error extends Error {
    constructor(errno, message) {
        super(message);
        this.errno = errno;
    }
}

class Reader {
    constructor(data) {
        this.data = data;
        this.view = new DataView(data.buffer, data.byteOffset, data.byteLength);
        this.offset = 0;
    }

    require(size) {
        if (!Number.isInteger(size) || size < 0 || this.offset + size > this.data.length) {
            throw new P9Error(EPROTO, "truncated 9p message");
        }
    }

    u8() {
        this.require(1);
        return this.data[this.offset++];
    }

    u16() {
        this.require(2);
        const value = this.view.getUint16(this.offset, true);
        this.offset += 2;
        return value;
    }

    u32() {
        this.require(4);
        const value = this.view.getUint32(this.offset, true);
        this.offset += 4;
        return value;
    }

    u64() {
        const low = this.u32();
        const high = this.u32();
        if (high !== 0) {
            throw new P9Error(EFBIG, "64-bit value exceeds the server limit");
        }
        return low;
    }

    string() {
        const size = this.u16();
        this.require(size);
        let value;
        try {
            value = new TextDecoder("utf-8", { fatal: true }).decode(
                this.data.subarray(this.offset, this.offset + size),
            );
        } catch (error) {
            throw new P9Error(EPROTO, `invalid UTF-8 in 9p string: ${error.message}`);
        }
        this.offset += size;
        return value;
    }

    bytes(size) {
        this.require(size);
        const value = this.data.subarray(this.offset, this.offset + size);
        this.offset += size;
        return value;
    }

    done() {
        if (this.offset !== this.data.length) {
            throw new P9Error(EPROTO, "trailing data in 9p message");
        }
    }
}

class Writer {
    constructor(capacity, type, tag) {
        if (capacity < 7) {
            throw new P9Error(ENOSPC, "9p reply buffer is too small");
        }
        this.data = new Uint8Array(capacity);
        this.view = new DataView(this.data.buffer);
        this.offset = 4;
        this.u8(type);
        this.u16(tag);
    }

    require(size) {
        if (this.offset + size > this.data.length) {
            throw new P9Error(ENOSPC, "9p reply exceeds the negotiated buffer");
        }
    }

    u8(value) {
        this.require(1);
        this.data[this.offset++] = value;
    }

    u16(value) {
        this.require(2);
        this.view.setUint16(this.offset, value, true);
        this.offset += 2;
    }

    u32(value) {
        this.require(4);
        this.view.setUint32(this.offset, value, true);
        this.offset += 4;
    }

    u64(value) {
        if (!Number.isInteger(value) || value < 0 || value > 0xffffffff) {
            throw new P9Error(EFBIG, "value exceeds the server's 32-bit limit");
        }
        this.u32(value);
        this.u32(0);
    }

    string(value) {
        const encoded = new TextEncoder().encode(value);
        if (encoded.length > 0xffff) {
            throw new P9Error(EINVAL, "9p string is too long");
        }
        this.u16(encoded.length);
        this.bytes(encoded);
    }

    bytes(value) {
        this.require(value.length);
        this.data.set(value, this.offset);
        this.offset += value.length;
    }

    qid(node) {
        this.u8(qidType(node));
        this.u32(node.version);
        this.u64(node.id);
    }

    finish() {
        this.view.setUint32(0, this.offset, true);
        return this.data.slice(0, this.offset);
    }
}

function nowSeconds() {
    return Math.floor(Date.now() / 1000);
}

function qidType(node) {
    if (node.kind === "directory") {
        return QTDIR;
    }
    if (node.kind === "symlink") {
        return QTSYMLINK;
    }
    return 0;
}

function nodeMode(node) {
    if (node.kind === "directory") {
        return S_IFDIR | node.mode;
    }
    if (node.kind === "symlink") {
        return S_IFLNK | node.mode;
    }
    return S_IFREG | node.mode;
}

function nodeSize(node) {
    if (node.kind === "file") {
        return node.data.length;
    }
    if (node.kind === "symlink") {
        return new TextEncoder().encode(node.target).length;
    }
    return 0;
}

function validateName(name) {
    if (name === "" || name === "." || name === ".." || name.includes("/") || name.includes("\0")) {
        throw new P9Error(EINVAL, `invalid file name ${JSON.stringify(name)}`);
    }
}

function normalizedParts(path) {
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

function copyContent(content) {
    if (typeof content === "string") {
        return new TextEncoder().encode(content);
    }
    if (content instanceof Uint8Array) {
        return content.slice();
    }
    throw new TypeError("file content must be a string or Uint8Array");
}

class Memory9PServer {
    constructor(tree = {}) {
        this.nextNodeId = 1;
        this.listeners = new Set();
        this.fids = new Map();
        this.msize = MAX_MESSAGE_SIZE;
        this.root = this.createDirectory("", null, 0o755);
        this.loadTree(tree);
    }

    createDirectory(name, parent, mode) {
        const time = nowSeconds();
        return {
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
            children: Object.create(null),
        };
    }

    createFile(name, parent, content, mode = 0o644) {
        const data = copyContent(content);
        if (data.length > MAX_FILE_SIZE) {
            throw new P9Error(EFBIG, `file ${JSON.stringify(name)} exceeds 16 MiB`);
        }
        const time = nowSeconds();
        return {
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
            data,
        };
    }

    createSymlink(name, parent, target) {
        const time = nowSeconds();
        return {
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
            target,
        };
    }

    loadTree(tree) {
        if (tree === null || typeof tree !== "object" || tree instanceof Uint8Array) {
            throw new TypeError("the 9p tree root must be an object");
        }
        this.root.children = Object.create(null);
        this.fids.clear();
        this.addTree(this.root, tree);
        this.touch(this.root);
        this.emit({ kind: "reset", path: "", source: "host" });
    }

    loadFiles(files) {
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
                    directory[part] = Object.create(null);
                } else if (value === null || typeof value !== "object" || value instanceof Uint8Array) {
                    throw new P9Error(EINVAL, `path crosses a file at ${JSON.stringify(part)}`);
                }
                directory = directory[part];
            }
            directory[parts[parts.length - 1]] = content;
        }
        this.loadTree(tree);
    }

    addTree(parent, tree) {
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
            parent.children[name] = node;
        }
    }

    subscribe(listener) {
        this.listeners.add(listener);
        return () => {
            this.listeners.delete(listener);
        };
    }

    emit(change) {
        for (const listener of this.listeners) {
            listener(change);
        }
    }

    pathOf(node) {
        const parts = [];
        let current = node;
        while (current !== this.root && current.parent !== null) {
            parts.push(current.name);
            current = current.parent;
        }
        return parts.reverse().join("/");
    }

    lookup(path) {
        let node = this.root;
        for (const part of normalizedParts(path)) {
            if (node.kind !== "directory") {
                throw new P9Error(ENOTDIR, "path crosses a non-directory");
            }
            const child = node.children[part];
            if (child === undefined) {
                throw new P9Error(ENOENT, `file not found: ${path}`);
            }
            node = child;
        }
        return node;
    }

    ensureParent(path) {
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
                parent.children[part] = child;
                this.touch(parent);
            }
            if (child.kind !== "directory") {
                throw new P9Error(ENOTDIR, "path crosses a non-directory");
            }
            parent = child;
        }
        return { parent, name };
    }

    readFile(path) {
        const node = this.lookup(path);
        if (node.kind !== "file") {
            throw new P9Error(EISDIR, `${path} is not a regular file`);
        }
        return node.data.slice();
    }

    writeFile(path, content, source = "host") {
        const data = copyContent(content);
        if (data.length > MAX_FILE_SIZE) {
            throw new P9Error(EFBIG, `file ${JSON.stringify(path)} exceeds 16 MiB`);
        }
        const { parent, name } = this.ensureParent(path);
        const existing = parent.children[name];
        let kind = "write";
        if (existing === undefined) {
            parent.children[name] = this.createFile(name, parent, data);
            kind = "create";
        } else {
            if (existing.kind !== "file") {
                throw new P9Error(EISDIR, `${path} is not a regular file`);
            }
            existing.data = data;
            this.touch(existing);
        }
        this.touch(parent);
        this.emit({ kind, path: normalizedParts(path).join("/"), source });
    }

    remove(path, source = "host") {
        const node = this.lookup(path);
        if (node === this.root || node.parent === null) {
            throw new P9Error(EPERM, "cannot remove the root");
        }
        if (node.kind === "directory" && Object.keys(node.children).length !== 0) {
            throw new P9Error(ENOTEMPTY, "directory is not empty");
        }
        const oldPath = this.pathOf(node);
        const parent = node.parent;
        delete parent.children[node.name];
        node.parent = null;
        this.touch(parent);
        this.emit({ kind: "remove", path: oldPath, source });
    }

    rename(oldPath, newPath, source = "host") {
        const node = this.lookup(oldPath);
        if (node === this.root || node.parent === null) {
            throw new P9Error(EPERM, "cannot rename the root");
        }
        const destination = this.ensureParent(newPath);
        this.moveNode(node.parent, node.name, destination.parent, destination.name, source);
    }

    listFiles() {
        const result = [];
        const visit = (directory) => {
            for (const node of Object.values(directory.children)) {
                if (node.kind === "directory") {
                    visit(node);
                } else if (node.kind === "file") {
                    result.push(this.pathOf(node));
                }
            }
        };
        visit(this.root);
        return result.sort();
    }

    snapshot() {
        const result = Object.create(null);
        for (const path of this.listFiles()) {
            result[path] = this.readFile(path);
        }
        return result;
    }

    touch(node) {
        const time = nowSeconds();
        node.version = (node.version + 1) >>> 0;
        node.mtime = time;
        node.ctime = time;
    }

    fid(number) {
        const fid = this.fids.get(number);
        if (fid === undefined) {
            throw new P9Error(EBADF, `unknown fid ${number}`);
        }
        return fid;
    }

    directory(node) {
        if (node.kind !== "directory") {
            throw new P9Error(ENOTDIR, "fid does not name a directory");
        }
        return node;
    }

    child(directory, name) {
        validateName(name);
        const child = directory.children[name];
        if (child === undefined) {
            throw new P9Error(ENOENT, `file not found: ${name}`);
        }
        return child;
    }

    request(request, replyCapacity) {
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

    handle(type, tag, reader, writer) {
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
            case 70: throw new P9Error(EOPNOTSUPP, "hard links are not supported");
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

    statfs(reader, writer) {
        this.fid(reader.u32());
        writer.u32(0x01021997);
        writer.u32(4096);
        writer.u64(0x100000);
        writer.u64(0x0f0000);
        writer.u64(0x0f0000);
        writer.u64(0x100000);
        writer.u64(0x0f0000);
        writer.u64(1);
        writer.u32(255);
    }

    lopen(reader, writer) {
        const fid = this.fid(reader.u32());
        const flags = reader.u32();
        if ((flags & O_TRUNC) !== 0) {
            if (fid.node.kind !== "file") {
                throw new P9Error(EISDIR, "cannot truncate a directory");
            }
            fid.node.data = new Uint8Array();
            this.touch(fid.node);
            this.emit({ kind: "write", path: this.pathOf(fid.node), source: "guest" });
        }
        fid.flags = flags;
        writer.qid(fid.node);
        writer.u32(Math.max(0, this.msize - 24));
    }

    lcreate(reader, writer) {
        const fid = this.fid(reader.u32());
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
        directory.children[name] = node;
        this.touch(directory);
        fid.node = node;
        fid.flags = flags;
        this.emit({ kind: "create", path: this.pathOf(node), source: "guest" });
        writer.qid(node);
        writer.u32(Math.max(0, this.msize - 24));
    }

    symlink(reader, writer) {
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
        directory.children[name] = node;
        this.touch(directory);
        this.emit({ kind: "create", path: this.pathOf(node), source: "guest" });
        writer.qid(node);
    }

    readlink(reader, writer) {
        const node = this.fid(reader.u32()).node;
        if (node.kind !== "symlink") {
            throw new P9Error(EINVAL, "fid does not name a symlink");
        }
        writer.string(node.target);
    }

    getattr(reader, writer) {
        const node = this.fid(reader.u32()).node;
        const requested = reader.u64();
        writer.u64(requested);
        writer.qid(node);
        writer.u32(nodeMode(node));
        writer.u32(node.uid);
        writer.u32(node.gid);
        writer.u64(node.kind === "directory" ? 2 + Object.values(node.children).filter((child) => child.kind === "directory").length : 1);
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

    setattr(reader) {
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
            if (size > MAX_FILE_SIZE) throw new P9Error(EFBIG, "file exceeds 16 MiB");
            const data = new Uint8Array(size);
            data.set(node.data.subarray(0, size));
            node.data = data;
            dataChanged = true;
            this.emit({ kind: "write", path: this.pathOf(node), source: "guest" });
        }
        if ((mask & 0x10) !== 0) node.atime = (mask & 0x80) !== 0 ? atime : time;
        if ((mask & 0x20) !== 0) node.mtime = (mask & 0x100) !== 0 ? mtime : time;
        else if (dataChanged) node.mtime = time;
        node.version = (node.version + 1) >>> 0;
        node.ctime = time;
    }

    readdir(reader, writer) {
        const directory = this.directory(this.fid(reader.u32()).node);
        const offset = reader.u64();
        const count = reader.u32();
        const entries = Object.values(directory.children).sort((left, right) => left.name.localeCompare(right.name));
        const payload = new Writer(Math.min(count + 7, MAX_MESSAGE_SIZE), 0, 0);
        payload.offset = 0;
        for (let index = offset; index < entries.length; index += 1) {
            const node = entries[index];
            const encodedName = new TextEncoder().encode(node.name);
            const entrySize = 13 + 8 + 1 + 2 + encodedName.length;
            if (payload.offset + entrySize > count) break;
            payload.qid(node);
            payload.u64(index + 1);
            payload.u8(node.kind === "directory" ? 4 : node.kind === "symlink" ? 10 : 8);
            payload.string(node.name);
        }
        writer.u32(payload.offset);
        writer.bytes(payload.data.subarray(0, payload.offset));
    }

    fsync(reader) {
        this.fid(reader.u32());
        reader.u32();
    }

    lock(reader, writer) {
        this.fid(reader.u32());
        reader.u8(); reader.u32(); reader.u64(); reader.u64(); reader.u32(); reader.string();
        writer.u8(0);
    }

    getlock(reader, writer) {
        this.fid(reader.u32());
        reader.u8(); reader.u64(); reader.u64(); reader.u32(); reader.string();
        writer.u8(2);
        writer.u64(0);
        writer.u64(0);
        writer.u32(0);
        writer.string("");
    }

    mkdir(reader, writer) {
        const directory = this.directory(this.fid(reader.u32()).node);
        const name = reader.string();
        const mode = reader.u32();
        reader.u32();
        validateName(name);
        if (directory.children[name] !== undefined) throw new P9Error(EEXIST, "file already exists");
        const node = this.createDirectory(name, directory, mode);
        directory.children[name] = node;
        this.touch(directory);
        this.emit({ kind: "create", path: this.pathOf(node), source: "guest" });
        writer.qid(node);
    }

    renameat(reader) {
        const oldDirectory = this.directory(this.fid(reader.u32()).node);
        const oldName = reader.string();
        const newDirectory = this.directory(this.fid(reader.u32()).node);
        const newName = reader.string();
        this.moveNode(oldDirectory, oldName, newDirectory, newName, "guest");
    }

    moveNode(oldDirectory, oldName, newDirectory, newName, source) {
        validateName(oldName);
        validateName(newName);
        const node = this.child(oldDirectory, oldName);
        if (oldDirectory === newDirectory && oldName === newName) {
            return;
        }
        for (let ancestor = newDirectory; ancestor !== null; ancestor = ancestor.parent) {
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
            replaced.parent = null;
        }
        const oldPath = this.pathOf(node);
        delete oldDirectory.children[oldName];
        newDirectory.children[newName] = node;
        node.parent = newDirectory;
        node.name = newName;
        this.touch(oldDirectory);
        if (newDirectory !== oldDirectory) this.touch(newDirectory);
        this.touch(node);
        this.emit({ kind: "rename", path: this.pathOf(node), oldPath, source });
    }

    unlinkat(reader) {
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
        delete directory.children[name];
        node.parent = null;
        this.touch(directory);
        this.emit({ kind: "remove", path, source: "guest" });
    }

    version(tag, reader, writer) {
        if (tag !== NOTAG) throw new P9Error(EPROTO, "Tversion must use NOTAG");
        const requestedSize = reader.u32();
        const version = reader.string();
        if (requestedSize < 256) throw new P9Error(EINVAL, "negotiated msize is too small");
        this.fids.clear();
        this.msize = Math.min(requestedSize, MAX_MESSAGE_SIZE);
        writer.u32(this.msize);
        writer.string(version === "9P2000.L" ? version : "unknown");
    }

    attach(reader, writer) {
        const fidNumber = reader.u32();
        reader.u32();
        reader.string();
        reader.string();
        reader.u32();
        if (this.fids.has(fidNumber)) throw new P9Error(EEXIST, "fid already exists");
        this.fids.set(fidNumber, { node: this.root, flags: 0 });
        writer.qid(this.root);
    }

    walk(reader, writer) {
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
        this.fids.set(newFidNumber, { node, flags: 0 });
        writer.u16(qids.length);
        for (const qid of qids) writer.qid(qid);
    }

    read(reader, writer) {
        const node = this.fid(reader.u32()).node;
        const offset = reader.u64();
        const count = reader.u32();
        if (node.kind !== "file") throw new P9Error(EISDIR, "fid does not name a regular file");
        const available = Math.max(0, Math.min(count, node.data.length - offset, writer.data.length - writer.offset - 4));
        writer.u32(available);
        writer.bytes(node.data.subarray(offset, offset + available));
        node.atime = nowSeconds();
    }

    write(reader, writer) {
        const fid = this.fid(reader.u32());
        let offset = reader.u64();
        const count = reader.u32();
        const data = reader.bytes(count);
        const node = fid.node;
        if (node.kind !== "file") throw new P9Error(EISDIR, "fid does not name a regular file");
        if ((fid.flags & O_APPEND) !== 0) offset = node.data.length;
        if (offset + count > MAX_FILE_SIZE) throw new P9Error(EFBIG, "file exceeds 16 MiB");
        const size = Math.max(node.data.length, offset + count);
        const replacement = new Uint8Array(size);
        replacement.set(node.data);
        replacement.set(data, offset);
        node.data = replacement;
        this.touch(node);
        this.emit({ kind: "write", path: this.pathOf(node), source: "guest" });
        writer.u32(count);
    }

    clunk(reader) {
        const fidNumber = reader.u32();
        this.fid(fidNumber);
        this.fids.delete(fidNumber);
    }
}

export { MAX_FILE_SIZE, Memory9PServer, P9Error };
