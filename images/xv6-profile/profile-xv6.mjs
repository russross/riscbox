import { createRequire } from "node:module";
import { readFile } from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";

const require = createRequire(import.meta.url);
const completionMarker = "XV6_PROFILE_BUILD_COMPLETE";
const poweroffMarker = "reboot: Power down";
const linuxPoweroffMarker = "Requesting system poweroff";

function usage() {
    console.error("usage: node profile-xv6.mjs riscbox CONFIG WASM [TIMEOUT_SECONDS]");
    process.exit(2);
}

const [engine, configArgument, runtimeArgument, timeoutArgument = "1800"] = process.argv.slice(2);
if (engine !== "riscbox" || !configArgument || !runtimeArgument) usage();
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
        || consoleText.includes(poweroffMarker) || consoleText.includes(linuxPoweroffMarker);
    if (buildCompleted && poweredOff) {
        finish(0);
    }
}

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

async function runRiscbox() {
    const adapter = fileURLToPath(new URL("../../build/js/riscbox.js", import.meta.url));
    const { Riscbox } = require(adapter);
    const wasm = await readFile(runtimeArgument);
    const runtime = await Riscbox.instantiate(wasm, {
        fetch: localFetch,
        debugTiming: process.env.RISCBOX_PROFILE_TIMING === "1",
        consoleWrite,
        onError: (error) => finish(1, String(error)),
    });
    await runtime.startFromUrl(configUrl, 256);
}

process.on("exit", () => {
    if (!buildCompleted || !poweredOff) process.exitCode = 1;
});
process.on("SIGTERM", () => finish(0));
process.on("uncaughtException", (error) => finish(1, String(error)));
process.on("unhandledRejection", (error) => finish(1, String(error)));

await runRiscbox();
