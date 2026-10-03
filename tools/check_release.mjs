#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";

// Validate the deployable archive itself, rather than the build tree beside it.
async function checkRelease(archive) {
    const name = basename(archive).replace(/\.tar\.gz$/, "");
    const files = execFileSync("tar", ["-tzf", archive], { encoding: "utf8" }).trim().split("\n");
    for (const file of files) assert(file.startsWith(`${name}/`) && !file.split("/").includes(".."), `invalid archive path ${file}`);
    for (const file of ["riscbox.wasm", "riscbox.js", "riscbox.d.ts", "splitimg.py", "README.md",
        "API.md", "HOWTO.md", "STORAGE-ABI.md", "NINEP.md", "CHANGELOG.md", "LICENSE",
        "network/index.js", "network/index.d.ts", "network/README.md"]) {
        assert(files.includes(`${name}/${file}`), `missing ${file}`);
    }
    for (const prefix of ["linux-", "fw_dynamic.bin-", "u-boot.bin-"]) {
        assert.equal(files.filter(file => file.startsWith(`${name}/${prefix}`) && file.endsWith(".gz")).length, 1,
            `expected one ${prefix} payload`);
    }
    assert(!files.some(file => /internal|build\/|demo\/|images\//.test(file)), "development assets in archive");

    // Extraction checks executable metadata, documentation links, and actual WASM loading.
    const directory = await mkdtemp(join(tmpdir(), "riscbox-release-"));
    try {
        execFileSync("tar", ["-xzf", archive, "-C", directory]);
        const root = join(directory, name);
        const details = execFileSync("tar", ["-tvzf", archive, `${name}/splitimg.py`], { encoding: "utf8" });
        assert(details.startsWith("-rwx"), "splitter must be executable");
        for (const file of files.filter(file => file.endsWith(".md"))) {
            const source = await readFile(join(directory, file), "utf8");
            for (const match of source.matchAll(/\]\(([^)]+)\)/g)) {
                const link = match[1].split("#")[0];
                if (!link || link.includes(":")) continue;
                const target = resolve(join(directory, file, ".."), link);
                await readFile(target);
            }
        }
        const { Riscbox } = createRequire(import.meta.url)(join(root, "riscbox.js"));
        const declarations = await readFile(join(root, "riscbox.d.ts"), "utf8");
        for (const internal of ["StorageRuntime", "StorageExports", "FilesystemHandle", "DiskHandle", "runQuantum", "hostImports"]) {
            assert(!declarations.includes(internal), `internal declaration ${internal}`);
        }
        const module = await WebAssembly.compile(await readFile(join(root, "riscbox.wasm")));
        const exports = WebAssembly.Module.exports(module).map(entry => entry.name);
        assert(!exports.includes("riscbox_start") && !exports.includes("riscbox_start_resolved"), "unused startup exports");
        const runtime = await Riscbox.instantiate(module);
        assert.equal(runtime.state, "empty");
        assert.equal(runtime.runQuantum, undefined);
        assert.equal(runtime.exports, undefined);
        await runtime.destroy();
    } finally { await rm(directory, { recursive: true, force: true }); }
    console.log(`Release archive checked: ${archive}`);
}

try {
    if (process.argv.length !== 3) throw new Error("usage: check_release.mjs ARCHIVE.tar.gz");
    await checkRelease(resolve(process.argv[2]));
} catch (error) {
    console.error(`Release check failed: ${error.message}`);
    process.exitCode = 1;
}
