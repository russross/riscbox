import { SeedBuilder, type SeedPlugin } from "./seed.js";

export interface HttpsSeedFile {
    readonly path: string;
    readonly size: number;
    readonly source?: string;
    readonly inodeKey?: string;
}

export interface HttpsSeedManifest {
    readonly files: readonly HttpsSeedFile[];
}

export function createHttpsSeedPlugin(
    manifest: HttpsSeedManifest,
    baseUrl: URL,
): SeedPlugin<string> {
    const builder = new SeedBuilder<string>();
    for (const file of manifest.files) {
        const source = new URL(file.source ?? file.path, baseUrl).href;
        builder.addFile(file.path, file.size, source,
            file.inodeKey === undefined ? {} : { inodeKey: file.inodeKey });
    }
    return Object.freeze({
        entries: builder.finish(),
        loader: {
            async load(url: string, signal: AbortSignal): Promise<Uint8Array> {
                const response = await fetch(url, { signal });
                if (!response.ok) throw new Error(`HTTP ${response.status} loading ${url}`);
                return new Uint8Array(await response.arrayBuffer());
            },
        },
    });
}

export interface TarSeedKey {
    readonly offset: number;
    readonly size: number;
}

function tarString(block: Uint8Array, offset: number, length: number): string {
    const field = block.subarray(offset, offset + length);
    const end = field.indexOf(0);
    return new TextDecoder().decode(end < 0 ? field : field.subarray(0, end));
}

function tarOctal(block: Uint8Array, offset: number, length: number): number {
    const value = tarString(block, offset, length).trim();
    if (!/^[0-7]*$/.test(value)) throw new TypeError("invalid tar numeric field");
    return value === "" ? 0 : Number.parseInt(value, 8);
}

export function createTarSeedPlugin(archive: Uint8Array): SeedPlugin<TarSeedKey> {
    const builder = new SeedBuilder<TarSeedKey>();
    let offset = 0;
    while (offset + 512 <= archive.length) {
        const header = archive.subarray(offset, offset + 512);
        if (header.every((byte) => byte === 0)) break;
        const name = tarString(header, 0, 100).replace(/\/$/, "");
        const prefix = tarString(header, 345, 155);
        const path = prefix === "" ? name : `${prefix}/${name}`;
        const size = tarOctal(header, 124, 12);
        const type = header[156] ?? 0;
        const bodyOffset = offset + 512;
        if (bodyOffset + size > archive.length) throw new TypeError("truncated tar entry");
        if (type === 0 || type === 48) {
            builder.addFile(path, size, Object.freeze({ offset: bodyOffset, size }), { inodeKey: path });
        } else if (type === 49) {
            builder.addHardLink(path, tarString(header, 157, 100));
        } else if (type === 53) {
            builder.addDirectory(path);
        } else if (type === 50) {
            builder.addSymlink(path, tarString(header, 157, 100));
        }
        offset = bodyOffset + Math.ceil(size / 512) * 512;
    }
    return Object.freeze({
        entries: builder.finish(),
        loader: {
            async load(key: TarSeedKey): Promise<Uint8Array> {
                return archive.slice(key.offset, key.offset + key.size);
            },
        },
    });
}
