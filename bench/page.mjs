// The page uses only public client calls; no WASM exports or runtime internals.
const status = document.querySelector("#status");
const consoleView = document.querySelector("#console");
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
let vm;
let transcript = "";
let pending;
let requests = 0;
let bytes = 0;
let failure;
let finishing = false;

// Console markers delimit measured work independently of the guest clock.
function consoleWrite(text) {
    consoleView.textContent += text;
    transcript += text;
    let newline;
    while ((newline = transcript.indexOf("\n")) !== -1) {
        const line = transcript.slice(0, newline).replace(/\r$/, "");
        transcript = transcript.slice(newline + 1);
        if (line.startsWith("@@ERROR")) failure = new Error(line);
        if (!pending) continue;
        if (line === pending.startMarker) {
            if (pending.start) { failure = new Error("duplicate start marker"); continue; }
            pending.start = { time: performance.now(), requests, bytes };
        } else if (line === pending.endMarker) {
            if (!pending.start) { failure = new Error("end before start"); continue; }
            pending.end = { time: performance.now(), requests, bytes };
        } else if (line === "@@RESULT" && pending.end) {
            pending.done = true;
        } else if (pending.end && !pending.done) pending.output.push(line);
        if (line === "@@READY") pending.done = true;
    }
}

async function until(condition, label, timeoutMs) {
    const deadline = performance.now() + timeoutMs;
    while (!condition()) {
        if (failure) throw failure;
        if (performance.now() > deadline) throw new Error(`timeout: ${label}`);
        await sleep(10);
    }
    if (failure) throw failure;
}

// Partial input acceptance is retried without dropping the unaccepted suffix.
async function command(workload, count, timeoutMs) {
    pending = { startMarker: `@@START ${workload} ${count}`, endMarker: `@@END ${workload} ${count}`, output: [] };
    const input = new TextEncoder().encode(`${workload} ${count}\n`);
    let offset = 0;
    await until(() => {
        offset += vm.consoleInput(input.subarray(offset));
        return offset === input.length;
    }, "console input", timeoutMs);
    await until(() => pending.done, workload, timeoutMs);
    const result = {
        workload, count, elapsedMs: pending.end.time - pending.start.time,
        fetchRequests: pending.end.requests - pending.start.requests,
        fetchBytes: pending.end.bytes - pending.start.bytes,
        output: pending.output.join("\n"),
        speed: vm.speed(),
    };
    pending = undefined;
    return result;
}

// Correctness identifiers exclude SQLite's guest-time reports and memory stats.
function signature(result) {
    const patterns = {
        loop: /^loop-checksum=[0-9a-f]{16}$/gm,
        sqlite: /^Verification Hash: \d+ [0-9a-f]{48}$/gm,
        compile: /^[0-9a-f]{64}  \/bench\/games\/\S+$/gm,
    };
    const matches = result.output.match(patterns[result.workload]) ?? [];
    if (matches.length !== (result.workload === "compile" ? 5 : 1)) {
        throw new Error(`missing correctness result: ${result.workload}\n${result.output}`);
    }
    return matches.join("\n");
}

async function profileCommand(action, workload) {
    const response = await fetch(`/profile/${action}/${workload}`, { method: "POST" });
    if (!response.ok) throw new Error(`profile ${action} failed: ${await response.text()}`);
}

async function progress(step) {
    status.textContent = step;
    await fetch(`/progress?step=${encodeURIComponent(step)}`);
}

// A fresh machine gives each sample the same disk overlays and bounded Rust
// cache. Chrome's process and immutable HTTP cache persist across boots.
async function bootGuest(options, configUrl) {
    finishing = false;
    pending = { output: [] };
    vm = await globalThis.Riscbox.prepare({
        config: { url: configUrl }, wasmUrl: "/runtime/riscbox.wasm",
        targetQuantumMs: options.quantumMs, consoleWrite,
        onError: error => { failure = new Error(String(error)); },
        onVmHalted: cause => { if (!finishing) failure = new Error(`unexpected guest halt: ${cause}`); },
        fetchBlock: async ({ url }) => {
            try {
                const response = await fetch(url, { cache: "only-if-cached", mode: "same-origin" });
                if (!response.ok) throw new Error(`cache miss (${response.status})`);
                const data = new Uint8Array(await response.arrayBuffer());
                requests++;
                bytes += data.byteLength;
                return data;
            } catch (error) {
                // The adapter reports transport errors to the guest as disk I/O
                // errors; benchmark cache misses must also stop host measurement.
                failure = new Error(`browser cache read failed: ${url}: ${String(error)}`);
                throw failure;
            }
        },
    });
    await vm.boot();
    await until(() => pending.done, "guest startup", Math.min(options.timeoutMs, 60000));
    pending = undefined;
}

async function closeGuest() {
    finishing = true;
    await vm.halt();
    await vm.destroy();
    vm = undefined;
}

try {
    const options = await (await fetch("/options")).json();
    const fixture = await (await fetch("/image/fixture.json")).json();
    const configUrl = new URL("/image/riscbox.cfg", location.href);
    const config = fixture.config;
    const manifestUrl = new URL(config.drive0.file, configUrl);
    const manifest = await (await fetch(manifestUrl)).text();
    const count = Number(manifest.match(/n_block:\s*(\d+)/)?.[1]);
    const chunkBytes = Number(manifest.match(/block_size:\s*(\d+)/)?.[1]) * 1024;
    if (!Number.isSafeInteger(count) || count < 1 || !Number.isSafeInteger(chunkBytes) || chunkBytes < 512) {
        throw new Error("invalid disk manifest");
    }

    // Warm only the browser cache. Two transfers bound startup memory and I/O;
    // the VM is constructed afterward with its ordinary bounded Rust cache.
    await progress(`Warming ${count} disk chunks`);
    let next = 0;
    async function warm() {
        while (next < count) {
            const chunk = next++;
            const url = new URL(`blk${String(chunk).padStart(9, "0")}.bin`, manifestUrl);
            const response = await fetch(url, { cache: "force-cache" });
            if (!response.ok) throw new Error(`preload failed: ${url}`);
            if ((await response.arrayBuffer()).byteLength !== chunkBytes) throw new Error(`wrong chunk length: ${url}`);
        }
    }
    await Promise.all([warm(), warm()]);
    await fetch("/cache-ready", { method: "POST" });
    await import("/runtime/riscbox.js");

    // Fixed counts make independent builds comparable. Warm-up uses the same
    // complete workload, and measured output is checked against its signature.
    const samples = [];
    for (const workload of ["loop", "sqlite", "compile"]) {
        await progress(`${workload}: warm-up`);
        await bootGuest(options, configUrl);
        const warmup = await command(workload, options.counts[workload], options.timeoutMs);
        const expected = signature(warmup);
        await closeGuest();
        for (let repetition = 1; repetition <= options.repetitions; repetition++) {
            await progress(`${workload}: ${repetition}/${options.repetitions}`);
            await bootGuest(options, configUrl);
            if (options.profile) await profileCommand("start", `${workload}/${repetition}`);
            const sample = await command(workload, options.counts[workload], options.timeoutMs);
            sample.signature = signature(sample);
            if (sample.signature !== expected) throw new Error(`${workload}: correctness result changed`);
            samples.push({ ...sample, repetition });
            if (options.profile) await profileCommand("stop", `${workload}/${repetition}`);
            await closeGuest();
        }
    }
    status.textContent = "Complete";
    await fetch("/result", { method: "POST", body: JSON.stringify({
        schema: 1, fixture, options, browser: navigator.userAgent,
        hardwareConcurrency: navigator.hardwareConcurrency,
        cachePolicy: "browser-warm/fresh-vm/guest-drop-caches/rust-default-16MiB", samples,
    }) });
} catch (error) {
    status.textContent = `Failed: ${error.message}`;
    finishing = true;
    if (vm?.state === "running") await vm.halt().catch(() => {});
    if (vm?.state === "halted") await vm.destroy().catch(() => {});
    await fetch("/result", { method: "POST", body: JSON.stringify({ error: String(error), console: consoleView.textContent }) });
}
