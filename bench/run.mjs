#!/usr/bin/env node
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir, cpus, platform, release } from "node:os";
import { dirname, extname, join, resolve } from "node:path";
import { parseArgs } from "node:util";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const bench = dirname(fileURLToPath(import.meta.url));
const root = resolve(bench, "..");
const hash = bytes => createHash("sha256").update(bytes).digest("hex");

// Arguments describe independent runs; workload counts never adapt to a build.
function argumentsForRun() {
    const { values } = parseArgs({ options: {
        runtime: { type: "string" }, output: { type: "string" }, label: { type: "string" },
        profile: { type: "boolean", default: false }, help: { type: "boolean", default: false },
        "loop-count": { type: "string", default: "150000000" },
        "sqlite-size": { type: "string", default: "10" },
        "compile-count": { type: "string", default: "1" },
        repetitions: { type: "string", default: "3" },
        "quantum-ms": { type: "string", default: "20" },
        "timeout-seconds": { type: "string", default: "300" },
    } });
    if (values.help) {
        console.log("Usage: node bench/run.mjs [--runtime DIR] [--label TEXT] [--output FILE]\n" +
            "  [--loop-count N] [--sqlite-size N] [--compile-count N] [--repetitions N]\n" +
            "  [--quantum-ms N] [--timeout-seconds N] [--profile]\n" +
            "DIR contains riscbox.js and riscbox.wasm. Defaults use the current local build.");
        return;
    }
    function positive(name, maximum) {
        const value = Number(values[name]);
        if (!Number.isSafeInteger(value) || value < 1 || value > maximum) {
            throw new Error(`--${name} must be an integer from 1 to ${maximum}`);
        }
        return value;
    }
    return { values, page: {
        counts: { loop: positive("loop-count", 1000000000), sqlite: positive("sqlite-size", 1000), compile: positive("compile-count", 100) },
        repetitions: positive("repetitions", 30), quantumMs: positive("quantum-ms", 100),
        timeoutMs: positive("timeout-seconds", 3600) * 1000, profile: values.profile,
    } };
}

// DevTools' pipe transport keeps Chrome private and needs no websocket library.
// Commands correlate by ID; a closed process rejects every outstanding request.
function devTools(chrome) {
    let nextId = 1;
    let buffer = "";
    const pending = new Map();
    const fail = error => {
        for (const entry of pending.values()) { clearTimeout(entry.timer); entry.reject(error); }
        pending.clear();
    };
    chrome.once("close", () => fail(new Error("Chrome closed")));
    chrome.once("error", fail);
    chrome.stdio[3].on("error", fail);
    chrome.stdio[4].setEncoding("utf8");
    chrome.stdio[4].on("data", data => {
        buffer += data;
        let boundary;
        while ((boundary = buffer.indexOf("\0")) !== -1) {
            let message;
            try { message = JSON.parse(buffer.slice(0, boundary)); }
            catch { fail(new Error("invalid Chrome DevTools response")); return; }
            buffer = buffer.slice(boundary + 1);
            const entry = pending.get(message.id);
            if (!entry) continue;
            pending.delete(message.id);
            clearTimeout(entry.timer);
            if (message.error) entry.reject(new Error(message.error.message));
            else entry.resolve(message.result);
        }
    });
    return (method, params = {}, sessionId) => new Promise((resolveCommand, reject) => {
        const id = nextId++;
        const timer = setTimeout(() => {
            pending.delete(id);
            reject(new Error(`Chrome DevTools timeout: ${method}`));
        }, 30000);
        pending.set(id, { resolve: resolveCommand, reject, timer });
        chrome.stdio[3].write(JSON.stringify({ id, method, params, sessionId }) + "\0");
    });
}

async function run(options) {
    const { values, page } = options;
    const directory = await mkdtemp(join(tmpdir(), "riscbox-bench-"));
    const timestamp = new Date().toISOString().replaceAll(":", "-");
    const output = resolve(values.output ?? join(bench, "results", `${timestamp}.json`));
    let chrome;
    let server;
    let deadline;
    try {
        // Stage exact runtime bytes once so changing a build cannot mix artifacts.
        const runtime = join(directory, "runtime");
        await mkdir(runtime);
        const source = values.runtime ? resolve(values.runtime) : root;
        await cp(values.runtime ? join(source, "riscbox.js") : join(root, "build/js/riscbox.js"), join(runtime, "riscbox.js"), { dereference: true });
        await cp(values.runtime ? join(source, "riscbox.wasm") : join(root, "target/wasm32-unknown-unknown/release/riscbox_wasm.wasm"), join(runtime, "riscbox.wasm"), { dereference: true });
        const wasm = await readFile(join(runtime, "riscbox.wasm"));
        const adapter = await readFile(join(runtime, "riscbox.js"));
        const fixture = JSON.parse(await readFile(join(bench, "dist/fixture.json"), "utf8"));
        await mkdir(dirname(output), { recursive: true });
        let finish;
        let fail;
        const completed = new Promise((resolveRun, reject) => { finish = resolveRun; fail = reject; });
        // Rejections are observed immediately, including during Chrome startup.
        completed.catch(() => {});
        let cacheReady = false;
        let lateChunkRequests = 0;
        let cdp;
        let sessionId;
        server = createServer(async (request, response) => {
            try {
                const url = new URL(request.url, "http://localhost");
                if (url.pathname === "/options") {
                    response.setHeader("Content-Type", "application/json");
                    response.end(JSON.stringify(page)); return;
                }
                if (url.pathname === "/cache-ready") { cacheReady = true; response.end("ready"); return; }
                if (url.pathname === "/progress") {
                    console.log(url.searchParams.get("step")); response.end("received"); return;
                }
                if (url.pathname === "/result") {
                    let body = "";
                    for await (const chunk of request) {
                        body += chunk;
                        if (body.length > 8 * 1024 * 1024) throw new Error("result exceeds 8 MiB");
                    }
                    const result = JSON.parse(body);
                    response.end("received");
                    if (result.error) fail(new Error(`${result.error}\n${result.console}`));
                    else if (lateChunkRequests) fail(new Error("disk chunks reached the server after preload"));
                    else finish(result);
                    return;
                }
                const profile = url.pathname.match(/^\/profile\/(start|stop)\/(loop|sqlite|compile)\/(\d+)$/);
                if (profile) {
                    if (!page.profile) throw new Error("profiling is disabled");
                    if (profile[1] === "start") await cdp("Profiler.start", {}, sessionId);
                    else {
                        const { profile: data } = await cdp("Profiler.stop", {}, sessionId);
                        await writeFile(`${output}.${profile[2]}-${profile[3]}.cpuprofile`, JSON.stringify(data));
                    }
                    response.end("profile saved"); return;
                }
                if (url.pathname === "/") {
                    response.setHeader("Content-Type", "text/html");
                    response.end('<!doctype html><meta charset="utf-8"><title>Riscbox benchmarks</title><h1 id="status">Starting</h1><pre id="console"></pre><script type="module" src="/page.mjs"></script>');
                    return;
                }

                // Immutable chunk responses populate the actual HTTP disk cache.
                // Once warm, any server-side chunk request invalidates the run.
                let assetRoot;
                let relative;
                if (url.pathname.startsWith("/image/")) { assetRoot = join(bench, "dist"); relative = url.pathname.slice(7); }
                else if (url.pathname.startsWith("/runtime/")) { assetRoot = runtime; relative = url.pathname.slice(9); }
                else if (url.pathname === "/page.mjs") { assetRoot = bench; relative = "page.mjs"; }
                else { response.writeHead(404).end(); return; }
                const path = resolve(assetRoot, decodeURIComponent(relative));
                if (!path.startsWith(assetRoot + "/")) { response.writeHead(403).end(); return; }
                if (/\/blk\d+\.bin$/.test(path)) {
                    if (cacheReady) { lateChunkRequests++; response.writeHead(503).end("cache miss"); return; }
                    response.setHeader("Cache-Control", "public, max-age=31536000, immutable");
                } else response.setHeader("Cache-Control", "no-cache");
                const types = { ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".json": "application/json" };
                response.setHeader("Content-Type", types[extname(path)] ?? "application/octet-stream");
                response.end(await readFile(path));
            } catch (error) {
                response.writeHead(500).end(error.message);
                fail(error);
            }
        });
        await new Promise(resolveListen => server.listen(0, "127.0.0.1", resolveListen));
        const flags = process.env.DISPLAY ? [] : ["--headless=new"];
        chrome = spawn(process.env.CHROME ?? "google-chrome", [...flags,
            "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage", "--no-first-run",
            "--no-default-browser-check", "--password-store=basic", "--remote-debugging-pipe",
            "--disk-cache-size=268435456", "--disable-background-timer-throttling",
            "--disable-renderer-backgrounding", `--user-data-dir=${join(directory, "chrome")}`, "about:blank"],
            { stdio: ["ignore", "ignore", "pipe", "pipe", "pipe"] });
        let errors = "";
        chrome.stderr.on("data", chunk => { errors = (errors + chunk).slice(-8000); });
        chrome.once("error", fail);
        chrome.once("close", () => fail(new Error(`Chrome exited before completion\n${errors}`)));
        deadline = setTimeout(() => fail(new Error("benchmark run timed out")), page.timeoutMs * (page.repetitions + 1) * 3 + 120000);
        cdp = devTools(chrome);
        const { targetId } = await cdp("Target.createTarget", { url: "about:blank" });
        ({ sessionId } = await cdp("Target.attachToTarget", { targetId, flatten: true }));
        const chromeVersion = await cdp("Browser.getVersion");
        if (page.profile) {
            await cdp("Profiler.enable", {}, sessionId);
            await cdp("Profiler.setSamplingInterval", { interval: 1000 }, sessionId);
        }
        await cdp("Page.navigate", { url: `http://127.0.0.1:${server.address().port}/` }, sessionId);
        console.log(`Running ${values.label ?? source}; ${page.profile ? "profiling" : "timing"}; results: ${output}`);
        const result = await completed;
        if (result.fixture.imageSha256 !== fixture.imageSha256) throw new Error("fixture changed during run");
        const record = { ...result, recordedAt: new Date().toISOString(), chromeVersion,
            host: { platform: platform(), release: release(), cpu: cpus()[0]?.model },
            runtime: { label: values.label ?? source, source, wasmSha256: hash(wasm), adapterSha256: hash(adapter),
                wasmBytes: wasm.length, wasmGzipBytes: gzipSync(wasm, { level: 9 }).length },
        };
        await writeFile(output, JSON.stringify(record, null, 2) + "\n");
        for (const workload of ["loop", "sqlite", "compile"]) {
            const times = record.samples.filter(sample => sample.workload === workload).map(sample => sample.elapsedMs).sort((a, b) => a - b);
            const middle = Math.floor(times.length / 2);
            const median = times.length % 2 ? times[middle] : (times[middle - 1] + times[middle]) / 2;
            console.log(`${workload}: median ${(median / 1000).toFixed(3)}s, range ${(times[0] / 1000).toFixed(3)}–${(times.at(-1) / 1000).toFixed(3)}s`);
            if (times[0] < 10000) console.log(`  Increase the fixed ${workload} count for longer profiling samples.`);
        }
    } finally {
        clearTimeout(deadline);
        if (chrome && chrome.exitCode === null && chrome.signalCode === null) {
            const closed = new Promise(resolveClose => chrome.once("close", resolveClose));
            chrome.kill("SIGTERM");
            const kill = setTimeout(() => chrome.kill("SIGKILL"), 5000);
            await closed;
            clearTimeout(kill);
        }
        if (server) {
            server.closeAllConnections();
            await new Promise(resolveClose => server.close(resolveClose));
        }
        await rm(directory, { recursive: true, force: true });
    }
}

try {
    const options = argumentsForRun();
    if (options) await run(options);
} catch (error) {
    console.error(`Benchmark failed: ${error.message}`);
    process.exitCode = 1;
}
