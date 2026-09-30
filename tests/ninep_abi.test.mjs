import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { createRequire } from "node:module";
import { abiRegression } from "./fixtures/ninep_abi.mjs";
import { facadeRegression } from "./fixtures/ninep_facade.mjs";
import { runChromePage } from "./chrome.mjs";
import { Filesystem, SeedBuilder } from "../build/js/p9/index.js";
const { Riscbox } = createRequire(import.meta.url)("../js/riscbox.js");

test("deployed filesystem ABI owns bytes and handles through async and VM lifetimes", async () => {
    const directory = await mkdtemp(join(tmpdir(), "riscbox-ninep-abi-"));
    try {
        const target = join(directory, "target");
        const build = spawnSync("cargo", ["build", "--locked", "--release", "--target", "wasm32-unknown-unknown",
            "-p", "riscbox-wasm", "--target-dir", target], { encoding: "utf8", timeout: 60_000 });
        assert.equal(build.status, 0, build.stderr);
        const bytes = await readFile(join(target, "wasm32-unknown-unknown/release/riscbox_wasm.wasm"));
        const elf = join(directory, "probe.elf");
        const binary = join(directory, "probe.bin");
        const compile = spawnSync("riscv64-linux-gnu-gcc", ["-nostdlib", "-nostartfiles", "-static", "-fno-pie", "-no-pie",
            "-march=rv64ima", "-mabi=lp64", "-Wl,--build-id=none", `-Wl,-T,${join(import.meta.dirname, "network_probe.ld")}`,
            join(import.meta.dirname, "ninep_abi_probe.S"), "-o", elf], { encoding: "utf8" });
        assert.equal(compile.status, 0, compile.stderr);
        const extract = spawnSync("riscv64-linux-gnu-objcopy", ["-O", "binary", "-j", ".text", elf, binary], { encoding: "utf8" });
        assert.equal(extract.status, 0, extract.stderr);
        const firmware = new Uint8Array(await readFile(binary));
        const module = await WebAssembly.compile(bytes);
        assert.deepEqual(WebAssembly.Module.imports(module), [{ module: "riscbox_host", name: "random_fill", kind: "function" }]);
        let instance;
        instance = await WebAssembly.instantiate(module, { riscbox_host: { random_fill(address, length) {
            new Uint8Array(instance.exports.memory.buffer, address, length).fill(42); return 0;
        } } });
        assert.equal(await abiRegression(instance.exports, firmware), 1);
        const runtime = new Riscbox(instance.exports, { onError: error => { throw error; } });
        try { assert.equal(await facadeRegression(Filesystem, SeedBuilder, runtime, firmware), 1); }
        finally { runtime.cancelWakeup(); }

        if (process.env.RISCBOX_TEST_BROWSER === "1") {
            const source = (await readFile(join(import.meta.dirname, "fixtures/ninep_abi.mjs"), "utf8"))
                .replace("export async function abiRegression", "async function abiRegression");
            const facade = (await readFile(join(import.meta.dirname, "fixtures/ninep_facade.mjs"), "utf8"))
                .replace("export async function facadeRegression", "async function facadeRegression");
            await runChromePage(`<!doctype html><body>pending<script src="/js/riscbox.js"></script><script type="module">
                import { Filesystem, SeedBuilder } from "/build/js/p9/index.js";
                ${source}
                ${facade}
                try {
                    const bytes = Uint8Array.from(atob("${bytes.toString("base64")}"), value => value.charCodeAt(0));
                    let instance;
                    ({ instance } = await WebAssembly.instantiate(bytes, { riscbox_host: { random_fill(address, length) {
                        new Uint8Array(instance.exports.memory.buffer, address, length).fill(42); return 0;
                    } } }));
                    const firmware = Uint8Array.from(atob("${Buffer.from(firmware).toString("base64")}"), value => value.charCodeAt(0));
                    await abiRegression(instance.exports, firmware);
                    const runtime = new Riscbox(instance.exports, { onError: error => { throw error; } });
                    await facadeRegression(Filesystem, SeedBuilder, runtime, firmware);
                    await fetch("/result?status=pass");
                } catch (error) { await fetch("/result?status=" + encodeURIComponent(String(error))); }
            </script>`, directory);
        }
    } finally {
        await rm(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    }
});
