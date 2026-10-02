import type { FileAttributes, Filesystem } from "../../../js/storage";

interface SnapshotEntry { readonly path: string; readonly attributes: FileAttributes; }
type Entry =
    | (SnapshotEntry & { readonly kind: "directory" })
    | (SnapshotEntry & { readonly kind: "file"; readonly bytes: Uint8Array })
    | (SnapshotEntry & { readonly kind: "symlink"; readonly target: string })
    | (SnapshotEntry & { readonly kind: "link"; readonly target: string });
export interface WorkspaceSnapshot { readonly entries: readonly Entry[]; }

// A powered-off namespace is stable while copied bytes and links are captured.
export function snapshotWorkspace(filesystem: Filesystem): WorkspaceSnapshot {
    const entries: Entry[] = [];
    const pathsByInode = new Map<bigint, string>();
    const visit = (path: string): void => {
        const attributes = filesystem.stat(path);
        const target = pathsByInode.get(attributes.inode);
        if (target !== undefined) {
            entries.push({ kind: "link", path, target, attributes });
            return;
        }
        pathsByInode.set(attributes.inode, path);
        switch (attributes.kind) {
            case "directory":
                entries.push({ kind: "directory", path, attributes });
                for (const entry of filesystem.listDirectory(path)) {
                    visit(path === "" ? entry.name : `${path}/${entry.name}`);
                }
                break;
            case "file":
                entries.push({ kind: "file", path, attributes, bytes: filesystem.readFile(path) });
                break;
            case "symlink":
                entries.push({ kind: "symlink", path, attributes, target: filesystem.readlink(path) });
                break;
        }
    };
    visit("");
    return { entries };
}

// Parent directories and link targets precede their children and aliases.
export function restoreWorkspace(filesystem: Filesystem, snapshot: WorkspaceSnapshot): void {
    filesystem.clear();
    for (const entry of snapshot.entries) {
        switch (entry.kind) {
            case "directory": if (entry.path !== "") filesystem.mkdir(entry.path); break;
            case "file": filesystem.writeFile(entry.path, entry.bytes); break;
            case "symlink": filesystem.symlink(entry.path, entry.target); break;
            case "link": filesystem.link(entry.target, entry.path); break;
        }
    }
    // Creation changes parent times. Restore metadata after the whole tree exists.
    for (const entry of snapshot.entries.toReversed()) {
        filesystem.setAttributes(entry.path, entry.attributes);
    }
}

export function populateWorkspace(filesystem: Filesystem, files: ReadonlyMap<string, Uint8Array>): void {
    filesystem.clear();
    const directories = new Set<string>();
    for (const [path, bytes] of files) {
        const parts = path.split("/");
        for (let end = 1; end < parts.length; end++) {
            const directory = parts.slice(0, end).join("/");
            if (!directories.has(directory)) {
                filesystem.mkdir(directory);
                directories.add(directory);
            }
        }
        filesystem.writeFile(path, bytes);
    }
}
