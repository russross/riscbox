interface ExampleFile { readonly path: string; readonly size: number; }

export interface ExampleDescription {
    readonly id: string;
    readonly title: string;
    readonly editable: string;
    readonly documentation?: string;
    readonly files: readonly ExampleFile[];
}

function normalizeRelativePath(raw: string): string {
    if (raw.includes("\\")) {
        throw new Error(`Invalid example path: ${JSON.stringify(raw)}`);
    }
    const parts = raw.split("/");
    if (raw === "" || raw.startsWith("/") || raw.includes("\0")
        || parts.some((part) => part === "" || part === "." || part === "..")) {
        throw new Error(`Invalid example path: ${JSON.stringify(raw)}`);
    }
    return parts.join("/");
}

function parseExample(value: unknown): ExampleDescription {
    if (typeof value !== "object" || value === null
        || !("id" in value) || typeof value.id !== "string"
        || !("title" in value) || typeof value.title !== "string"
        || !("editable" in value) || typeof value.editable !== "string"
        || !("files" in value) || !Array.isArray(value.files)) {
        throw new Error("Invalid example manifest record");
    }
    const id = normalizeRelativePath(value.id);
    if (id.includes("/")) throw new Error("Example ID must be one path component");
    const files = value.files.map((file: unknown): ExampleFile => {
        if (typeof file !== "object" || file === null
            || !("path" in file) || typeof file.path !== "string"
            || !("size" in file) || typeof file.size !== "number"
            || !Number.isInteger(file.size) || file.size < 0 || file.size > 0xffff_ffff) {
            throw new Error("Invalid example file record");
        }
        return { path: normalizeRelativePath(file.path), size: file.size };
    });
    const editablePath = normalizeRelativePath(value.editable);
    if (!files.some(file => file.path === editablePath)) throw new Error("Example editable file is missing");
    const documentation = "documentation" in value ? value.documentation : undefined;
    if (documentation !== undefined && typeof documentation !== "string") throw new Error("Invalid documentation path");
    return { id, title: value.title, editable: editablePath, files,
        ...(documentation === undefined ? {} : { documentation: normalizeRelativePath(documentation) }) };
}

// Example downloads belong to the application. A populated share contains all
// bytes before boot, so guest and editor operations are always synchronous.
const exampleFiles = new Map<string, ReadonlyMap<string, Uint8Array>>();
export async function loadExampleFiles(description: ExampleDescription): Promise<ReadonlyMap<string, Uint8Array>> {
    const cached = exampleFiles.get(description.id);
    if (cached) return cached;
    const base = new URL(`examples/${encodeURIComponent(description.id)}/`, window.location.href);
    const entries = await Promise.all(description.files.map(async (file): Promise<readonly [string, Uint8Array]> => {
        const path = file.path.split("/").map(encodeURIComponent).join("/");
        const response = await fetch(new URL(path, base));
        if (!response.ok) throw new Error(`Example file ${file.path}: HTTP ${response.status}`);
        const bytes = new Uint8Array(await response.arrayBuffer());
        if (bytes.length !== file.size) throw new Error(`Example file ${file.path}: incorrect size`);
        return [file.path, bytes];
    }));
    const files = new Map(entries);
    exampleFiles.set(description.id, files);
    return files;
}

export async function loadExampleDescriptions(): Promise<ExampleDescription[]> {
    const response = await fetch("examples/examples.json");
    if (!response.ok) throw new Error(`Could not load examples: HTTP ${response.status}`);
    const manifest: unknown = await response.json();
    if (!Array.isArray(manifest)) throw new Error("Example manifest must be an array");
    return manifest.map(parseExample);
}
