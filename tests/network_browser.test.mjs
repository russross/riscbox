import assert from "node:assert/strict";
import { createHash, randomBytes } from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawn, spawnSync } from "node:child_process";
import test from "node:test";

const root = resolve(import.meta.dirname, "..");
const fixture = Uint8Array.of(
    0x02, 0, 0, 0, 0, 1, 0x02, 0, 0, 0, 0, 2, 0x88, 0xb5, 0x52, 0x58,
);
const expectedTransmit = Uint8Array.of(
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02, 0, 0, 0, 0, 1, 0x88, 0xb5, 0x54, 0x58,
);

function compileProbe(directory) {
    const elf = join(directory, "network-probe.elf");
    const binary = join(directory, "network-probe.bin");
    const compile = spawnSync("riscv64-linux-gnu-gcc", [
        "-nostdlib", "-nostartfiles", "-static", "-fno-pie", "-no-pie",
        "-march=rv64ima", "-mabi=lp64",
        "-Wl,--build-id=none", `-Wl,-T,${join(root, "tests/network_probe.ld")}`,
        join(root, "tests/network_probe.S"), "-o", elf,
    ], { encoding: "utf8" });
    assert.equal(compile.status, 0, compile.stderr);
    const extract = spawnSync("riscv64-linux-gnu-objcopy", ["-O", "binary", "-j", ".text", elf, binary], {
        encoding: "utf8",
    });
    assert.equal(extract.status, 0, extract.stderr);
    return binary;
}

function encodeFrame(payload) {
    const header = payload.length < 126
        ? Buffer.from([0x82, payload.length])
        : Buffer.from([0x82, 126, payload.length >>> 8, payload.length & 0xff]);
    return Buffer.concat([header, payload]);
}

function decodeFrames(buffer) {
    const frames = [];
    let offset = 0;
    while (buffer.length - offset >= 2) {
        const first = buffer[offset];
        const second = buffer[offset + 1];
        let length = second & 0x7f;
        let header = 2;
        if (length === 126) {
            if (buffer.length - offset < 4)
                break;
            length = buffer.readUInt16BE(offset + 2);
            header = 4;
        } else if (length === 127) {
            throw new Error("test stub does not accept 64-bit WebSocket lengths");
        }
        const masked = (second & 0x80) !== 0;
        const maskLength = masked ? 4 : 0;
        if (buffer.length - offset < header + maskLength + length)
            break;
        const maskOffset = offset + header;
        const payloadOffset = maskOffset + maskLength;
        const payload = Buffer.from(buffer.subarray(payloadOffset, payloadOffset + length));
        if (masked) {
            for (let index = 0; index < payload.length; index++)
                payload[index] ^= buffer[maskOffset + index % 4];
        }
        frames.push({ opcode: first & 0x0f, payload });
        offset = payloadOffset + length;
    }
    return { frames, remaining: buffer.subarray(offset) };
}

function page(port) {
    return `<!doctype html>
<script src="/riscbox.js"></script>
<script type="module">
import { WebSocketNetwork } from "/network/index.js";
const report = (value) => fetch("/result", { method: "POST", body: value });
let output = "";
const network = new WebSocketNetwork("ws://127.0.0.1:${port}/network", {
    onError: (error) => report("adapter: " + error.message),
});
try {
    const response = await fetch("/riscbox.wasm");
    const runtime = await Riscbox.instantiate(await response.arrayBuffer(), {
        consoleWrite(text) {
            output += text;
            if (output.includes("NETWORK PASS")) report("pass");
            if (output.includes("NETWORK FAIL")) report("probe failure");
        },
        networkWrite: network.transmit,
        onError: (error) => report("runtime: " + error.message),
    });
    network.attach(runtime);
    network.connect();
    runtime.start("/riscbox.cfg", 32, "", 0, 0, true);
} catch (error) {
    report("startup: " + error.message);
}
</script>`;
}

test("real WASM exchanges Ethernet frames through Chrome and a local origin", async () => {
    const temporary = await mkdtemp(join(tmpdir(), "riscbox-network-"));
    const probePath = compileProbe(temporary);
    const assets = new Map([
        ["/riscbox.js", [join(root, "js/riscbox.js"), "text/javascript"]],
        ["/network/index.js", [join(root, "build/js/network/index.js"), "text/javascript"]],
        ["/riscbox.wasm", [join(root, "target/wasm32-unknown-unknown/release/riscbox_wasm.wasm"), "application/wasm"]],
        ["/network-probe.bin", [probePath, "application/octet-stream"]],
    ]);
    let transmitted = false;
    let transmitResolve;
    const transmitPromise = new Promise((resolveTransmit) => {
        transmitResolve = resolveTransmit;
    });
    const connections = new Set();
    const requests = [];
    let resultResolve;
    const resultPromise = new Promise((resolveResult) => {
        resultResolve = resolveResult;
    });
    const server = createServer(async (request, response) => {
        requests.push(`${request.method} ${request.url}`);
        if (request.method === "POST" && request.url === "/result") {
            const chunks = [];
            for await (const chunk of request)
                chunks.push(chunk);
            const result = Buffer.concat(chunks).toString();
            response.end("ok");
            resultResolve(result);
            return;
        }
        if (request.url === "/riscbox.cfg") {
            response.setHeader("content-type", "text/plain");
            response.end('{version:1,machine:"riscv64",memory_size:32,bios:"network-probe.bin",console:"uart",eth0:{driver:"user"}}');
            return;
        }
        if (request.url === "/test.html") {
            response.setHeader("content-type", "text/html");
            response.end(page(server.address().port));
            return;
        }
        const asset = assets.get(request.url);
        if (asset !== undefined) {
            response.setHeader("content-type", asset[1]);
            response.end(await readFile(asset[0]));
            return;
        }
        response.statusCode = 404;
        response.end("not found");
    });
    server.on("upgrade", (request, socket) => {
        if (request.url !== "/network") {
            socket.destroy();
            return;
        }
        connections.add(socket);
        socket.on("close", () => connections.delete(socket));
        const key = request.headers["sec-websocket-key"];
        assert.equal(typeof key, "string");
        const accept = createHash("sha1")
            .update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
            .digest("base64");
        socket.write(
            "HTTP/1.1 101 Switching Protocols\r\n" +
            "Upgrade: websocket\r\n" +
            "Connection: Upgrade\r\n" +
            `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
        );
        socket.write(encodeFrame(Buffer.from(fixture)));
        let pending = Buffer.alloc(0);
        socket.on("data", (data) => {
            pending = Buffer.concat([pending, data]);
            const decoded = decodeFrames(pending);
            pending = Buffer.from(decoded.remaining);
            for (const frame of decoded.frames) {
                if (frame.opcode === 2 && Buffer.from(expectedTransmit).equals(frame.payload)) {
                    transmitted = true;
                    transmitResolve();
                }
            }
        });
    });
    await new Promise((resolveListen) => server.listen(0, "127.0.0.1", resolveListen));
    const port = server.address().port;
    const profile = join(temporary, `chrome-${randomBytes(4).toString("hex")}`);
    const chrome = spawn("google-chrome", [
        "--headless=new", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
        `--user-data-dir=${profile}`, `http://127.0.0.1:${port}/test.html`,
    ], { stdio: ["ignore", "pipe", "pipe"] });
    let chromeError = "";
    chrome.stderr.on("data", (data) => {
        chromeError += data.toString();
    });
    const timeout = globalThis.setTimeout(() => resultResolve("timeout"), 20_000);
    try {
        const result = await resultPromise;
        assert.equal(
            result,
            "pass",
            `${chromeError}\nrequests: ${requests.join(", ")}\ntransmitted: ${transmitted}`,
        );
        let transmitTimeout;
        await Promise.race([
            transmitPromise,
            new Promise((resolveTransmit) => {
                transmitTimeout = globalThis.setTimeout(resolveTransmit, 1_000);
            }),
        ]);
        globalThis.clearTimeout(transmitTimeout);
        assert.equal(transmitted, true);
    } finally {
        globalThis.clearTimeout(timeout);
        if (chrome.exitCode === null) {
            chrome.kill("SIGTERM");
            await new Promise((resolveExit) => chrome.once("exit", resolveExit));
        }
        for (const connection of connections)
            connection.destroy();
        await new Promise((resolveClose) => server.close(resolveClose));
        await rm(temporary, { recursive: true, force: true });
    }
});
