#!/usr/bin/env node
// Combine the handwritten runtime and compiled storage facade without dependencies.
import { readFile, writeFile } from "node:fs/promises";

const storage = await readFile("build/js/storage.js", "utf8");
const adapter = await readFile("js/riscbox.js", "utf8");
const declarations = await readFile("build/js/storage.d.ts", "utf8");
const runtimeTypes = await readFile("js/riscbox.d.ts", "utf8");

// Runtime and storage share a private scope. Only the client facade is deployed.
const bundle = adapter.replace("    // STORAGE_IMPLEMENTATION", storage.replace(/^export /gm, ""));
await writeFile("build/js/riscbox.js", bundle.replace("    // DEVELOPMENT_EXPORTS", ""));

// Architectural tests get a separate artifact that is never packaged for clients.
await writeFile("build/js/riscbox-internal.js", bundle.replace("    // DEVELOPMENT_EXPORTS",
    `    root.RiscboxRuntime = RiscboxRuntime;\n    if (typeof module === "object" && module.exports) module.exports.RiscboxRuntime = RiscboxRuntime;`));
await writeFile("build/js/riscbox.d.ts", declarations + "\n" + runtimeTypes.replace(/^import type .*;\n/gm, ""));
