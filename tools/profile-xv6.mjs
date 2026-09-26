import { createRequire } from "node:module";
import { readFile } from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";

const require = createRequire(import.meta.url);
const completionMarker = "XV6_PROFILE_BUILD_COMPLETE";
const poweroffMarker = "reboot: Power down";
const linuxPoweroffMarker = "Requesting system poweroff";
const tinyemuPoweroffMarker = "Power off.";

function usage() {
    console.error("usage: node profile-xv6.mjs riscbox|tinyemu CONFIG WASM_OR_JS [TIMEOUT_SECONDS]");
    process.exit(2);
}

const [engine, configArgument, runtimeArgument, timeoutArgument = "1800"] = process.argv.slice(2);
if (!engine || !configArgument || !runtimeArgument) usage();
const timeoutSeconds = Number(timeoutArgument);
if (!Number.isSafeInteger(timeoutSeconds) || timeoutSeconds <= 0) usage();

const configUrl = pathToFileURL(configArgument).href;
let consoleText = "";
let buildCompleted = false;
let poweredOff = false;
let settled = false;
const nativeStdoutWrite = process.stdout.write.bind(process.stdout);

const timeout = setTimeout(() => finish(1, `timed out after ${timeoutSeconds} seconds`), timeoutSeconds * 1000);

function finish(status, message) {
    if (settled) return;
    settled = true;
    clearTimeout(timeout);
    if (message) console.error(message);
    process.exit(status);
}

function consoleWrite(value) {
    const text = typeof value === "string" ? value : new TextDecoder().decode(value);
    nativeStdoutWrite(text);
    consoleText = (consoleText + text).slice(-4096);
    buildCompleted ||= text.includes(completionMarker) || consoleText.includes(completionMarker);
    poweredOff ||= text.includes(poweroffMarker) || text.includes(linuxPoweroffMarker)
        || (engine === "tinyemu" && text.includes(tinyemuPoweroffMarker))
        || consoleText.includes(poweroffMarker) || consoleText.includes(linuxPoweroffMarker)
        || (engine === "tinyemu" && consoleText.includes(tinyemuPoweroffMarker));
    if (buildCompleted && poweredOff) {
        finish(0);
    }
}

process.stdout.write = function write(chunk, ...args) {
    const text = typeof chunk === "string" ? chunk : new TextDecoder().decode(chunk);
    consoleText = (consoleText + text).slice(-4096);
    buildCompleted ||= text.includes(completionMarker) || consoleText.includes(completionMarker);
    poweredOff ||= text.includes(poweroffMarker) || text.includes(linuxPoweroffMarker)
        || (engine === "tinyemu" && text.includes(tinyemuPoweroffMarker))
        || consoleText.includes(poweroffMarker) || consoleText.includes(linuxPoweroffMarker)
        || (engine === "tinyemu" && consoleText.includes(tinyemuPoweroffMarker));
    if (buildCompleted && poweredOff) finish(0);
    return nativeStdoutWrite(chunk, ...args);
};

function localPath(url) {
    const parsed = new URL(url, configUrl);
    if (parsed.protocol !== "file:") throw new Error(`profile requested network URL: ${parsed.href}`);
    return fileURLToPath(parsed);
}

async function localFetch(url) {
    const bytes = await readFile(localPath(url));
    return {
        status: 200,
        ok: true,
        async arrayBuffer() {
            return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
        },
    };
}

class LocalXMLHttpRequest {
    status = 0;
    response = null;
    open(_method, url) {
        this.url = url;
    }
    setRequestHeader() {}
    send() {
        readFile(localPath(this.url)).then((bytes) => {
            this.status = 0;
            this.response = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
            this.onload?.();
        }).catch((error) => {
            this.error = error;
            this.onerror?.();
        });
    }
    abort() {}
}

async function runRiscbox() {
    const adapter = fileURLToPath(new URL("../js/riscbox.js", import.meta.url));
    const { Riscbox } = require(adapter);
    const wasm = await readFile(runtimeArgument);
    const runtime = await Riscbox.instantiate(wasm, {
        fetch: localFetch,
        debugTiming: process.env.RISCBOX_PROFILE_TIMING === "1",
        consoleWrite,
        onError: (error) => finish(1, String(error)),
    });
    if (runtime.start(configUrl, 256) !== 0) finish(1, "Riscbox rejected the configuration");
}

function runTinyemu() {
    globalThis.XMLHttpRequest = LocalXMLHttpRequest;
    globalThis.term = { write: consoleWrite, getSize: () => [80, 25] };
    globalThis.update_downloading = () => {};
    const module = require(runtimeArgument);
    const start = () => module.ccall(
        "vm_start",
        null,
        ["string", "number", "string", "string", "number", "number", "boolean"],
        [configUrl, 256, "", null, 0, 0, false],
    );
    if (module.calledRun) start();
    else module.onRuntimeInitialized = start;
}

process.on("exit", () => {
    if (!buildCompleted || !poweredOff) process.exitCode = 1;
});
process.on("SIGTERM", () => finish(0));
process.on("uncaughtException", (error) => finish(1, error.stack ?? String(error)));
process.on("unhandledRejection", (error) => finish(1, String(error)));

if (engine === "riscbox") await runRiscbox();
else if (engine === "tinyemu") runTinyemu();
else usage();
