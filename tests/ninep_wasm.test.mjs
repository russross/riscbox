import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

test("9p namespace and protocol execute on raw WASM without operating-system imports", async () => {
    const directory = await mkdtemp(join(tmpdir(), "riscbox-ninep-"));
    try {
        const wasm = join(directory, "namespace.wasm");
        const compiled = spawnSync("rustc", [
            "--edition=2024", "--target", "wasm32-unknown-unknown",
            "--crate-type", "cdylib", "-O",
            join(import.meta.dirname, "fixtures/ninep_wasm.rs"), "-o", wasm,
        ], { encoding: "utf8" });
        assert.equal(compiled.status, 0, compiled.stderr);
        const bytes = await readFile(wasm);
        const module = await WebAssembly.compile(bytes);
        assert.deepEqual(WebAssembly.Module.imports(module), []);
        const instance = await WebAssembly.instantiate(module, {});
        assert.equal(instance.exports.namespace_regression(), 1);
        assert.equal(instance.exports.protocol_regression(), 1);

        // make test also invokes this probe in Chrome. A promise continuation
        // enters WASM only after the previous activation has returned.
        if (process.env.RISCBOX_TEST_BROWSER === "1") {
            const html = join(directory, "probe.html");
            await writeFile(html, `<!doctype html><body>pending<script type="module">
                try {
                    const bytes = Uint8Array.from(atob("${bytes.toString("base64")}"), c => c.charCodeAt(0));
                    const { instance } = await WebAssembly.instantiate(bytes, {});
                    if (instance.exports.namespace_regression() !== 1) throw Error("first call");
                    if (instance.exports.protocol_regression() !== 1) throw Error("protocol call");
                    await Promise.resolve();
                    if (instance.exports.namespace_regression() !== 1) throw Error("second call");
                    if (instance.exports.protocol_regression() !== 1) throw Error("protocol continuation");
                    document.body.textContent = "NINEP_WASM_PASS";
                } catch (error) { document.body.textContent = "FAIL: " + error; }
            </script>`);
            const chrome = spawnSync("google-chrome", [
                "--headless=new", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
                `--user-data-dir=${join(directory, "chrome")}`,
                "--virtual-time-budget=5000", "--dump-dom", `file://${html}`,
            ], { encoding: "utf8", timeout: 20_000 });
            assert.equal(chrome.status, 0, chrome.stderr);
            assert.match(chrome.stdout, /<body>NINEP_WASM_PASS<\/body>/);
        }
    } finally {
        await rm(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    }
});
