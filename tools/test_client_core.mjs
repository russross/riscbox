import { cp, mkdtemp, readdir, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

// Shared browser fixtures need writable build output beside the mounted source.
const root = fileURLToPath(new URL("../", import.meta.url));
const source = join(root, "client-core");
const temporary = await mkdtemp(join(tmpdir(), "riscbox-client-core-"));
const copy = join(temporary, "client-core");
try {
    await cp(source, copy, {
        recursive: true,
        filter: path => path !== join(source, "node_modules")
            && !path.startsWith(join(source, "build", "test-")),
    });
    await symlink(join(source, "node_modules"), join(copy, "node_modules"), "dir");

    // Supply matching local runtime assets regardless of the temporary location.
    const selected = process.argv.slice(2);
    const tests = selected.length > 0 ? selected
        : (await readdir(join(copy, "tests"))).filter(name => name.endsWith(".test.mjs"))
            .sort().map(name => `tests/${name}`);
    const child = spawn(process.execPath, ["--experimental-websocket", "--test", ...tests], {
        cwd: copy, stdio: "inherit",
        env: {
            ...process.env,
            RISCBOX_CLIENT_RUNTIME: process.env.RISCBOX_CLIENT_RUNTIME ?? join(root, "build/js/riscbox.js"),
            RISCBOX_CLIENT_WASM: process.env.RISCBOX_CLIENT_WASM
                ?? join(root, "target/wasm32-unknown-unknown/release/riscbox_wasm.wasm"),
        },
    });
    process.exitCode = await new Promise((resolve, reject) => {
        child.once("error", reject);
        child.once("exit", code => resolve(code ?? 1));
    });
} catch (error) {
    console.error(`Client-core tests failed: ${error instanceof Error ? error.message : String(error)}`);
    process.exitCode = 1;
} finally {
    await rm(temporary, { recursive: true, force: true });
}
