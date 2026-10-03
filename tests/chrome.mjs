import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, join, resolve } from "node:path";

// Browser probes use real event-loop timing and report their result over HTTP.
// Each invocation owns its server, Chrome process, and temporary browser profile.
export async function runChromePage(html, directory, options = {}) {
    const root = options.root ?? resolve(import.meta.dirname, "..");
    let finish;
    const result = new Promise(resolveResult => { finish = resolveResult; });
    const server = createServer(async (request, response) => {
        const url = new URL(request.url, "http://localhost");
        options.onRequest?.(url);
        if (url.pathname === "/result") {
            finish(url.searchParams.get("status")); response.end("received"); return;
        }
        if (url.pathname === "/probe.html") {
            response.setHeader("Content-Type", "text/html"); response.end(html); return;
        }
        const path = resolve(root, `.${decodeURIComponent(url.pathname)}`);
        if (!path.startsWith(`${root}/`)) { response.writeHead(403).end(); return; }
        try {
            const override = await options.response?.(url);
            if (override !== undefined) { response.writeHead(override.status).end(override.body); return; }
            let bytes = await readFile(path);
            if (options.transform) bytes = await options.transform(path, bytes);
            const types = { ".html": "text/html", ".css": "text/css", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".svg": "image/svg+xml", ".json": "application/json" };
            response.setHeader("Content-Type", types[extname(path)] ?? "application/octet-stream");
            response.end(bytes);
        } catch { response.writeHead(404).end(); }
    });
    await new Promise(resolveListen => server.listen(0, "127.0.0.1", resolveListen));

    // A display selects headed Chrome; each probe has an isolated temporary profile.
    const flags = process.env.DISPLAY ? [] : ["--headless=new"];
    const chrome = spawn("google-chrome", [...flags, "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
        ...(options.chromeArgs ?? []),
        `--user-data-dir=${join(directory, "chrome")}`, `http://127.0.0.1:${server.address().port}/probe.html`],
        { stdio: ["ignore", "ignore", "pipe"] });
    const closed = new Promise(resolveClose => chrome.once("close", resolveClose));
    let errors = "";
    chrome.stderr.on("data", bytes => { errors += bytes.toString(); });
    chrome.on("error", error => finish(String(error)));
    const timeout = setTimeout(() => finish("timeout"), options.timeoutMs ?? 20_000);
    try { assert.equal(await result, "pass", errors); }
    finally {
        clearTimeout(timeout);
        if (chrome.exitCode === null && chrome.signalCode === null) chrome.kill("SIGTERM");
        await closed;
        server.closeAllConnections();
        await new Promise(resolveClose => server.close(resolveClose));
    }
}
