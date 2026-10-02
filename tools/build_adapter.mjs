#!/usr/bin/env node
// Combine the handwritten runtime and compiled storage facade without dependencies.
import { readFile, writeFile } from "node:fs/promises";

const storage = await readFile("build/js/storage.js", "utf8");
const adapter = await readFile("js/riscbox.js", "utf8");
const declarations = await readFile("build/js/storage.d.ts", "utf8");
const runtimeTypes = await readFile("js/riscbox.d.ts", "utf8");

// The storage source has no imports. Its exported classes are installed inside
// a private scope before the runtime exposes its unified public surface.
await writeFile("build/js/riscbox.js", `(function (root) {\n${storage.replace(/^export /gm, "")}\nroot.RiscboxStorage = { Filesystem, FilesystemError, BlockDisk, BlockError };\n}(globalThis));\n${adapter}`);
await writeFile("build/js/riscbox.d.ts", declarations + "\n" + runtimeTypes.replace(/^import type .*;\n/gm, ""));
