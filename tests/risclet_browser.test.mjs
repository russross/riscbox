import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { runChromePage } from "./chrome.mjs";

// Exercise the actual deployed UI and optimized runtime; only the HTTP test
// server instruments callbacks and supplies a controlled source failure/delay.
test("Risclet shares Rust files with its host UI across image and VM lifetimes", async () => {
    const directory = await mkdtemp(join(tmpdir(), "riscbox-risclet-browser-"));
    const requests = [];
    let failInput = true;
    try {
        await runChromePage(`<!doctype html><iframe id="app" src="/images/risclet/dist/index.html?example=reduction" style="width:1200px;height:800px"></iframe>
        <script type="module">
        const frame = document.getElementById("app");
        const check = (condition, message) => { if (!condition) throw new Error(message); };
        const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
        const wait = async (condition, message) => {
            for (let attempt = 0; attempt < 1000; attempt++) { if (await condition()) return; await delay(10); }
            throw new Error(message);
        };
        let step = "initialize";
        let app;
        try {
            await wait(() => frame.contentWindow.testRuntime && frame.contentDocument.getElementById("status")?.textContent.startsWith("Ready"), "UI ready");
            app = frame.contentWindow;
            const doc = app.document;
            const runtime = app.testRuntime;
            const encoder = new app.TextEncoder();
            const decoder = new TextDecoder();
            const services = [...runtime.filesystems.values()];
            check(services.length === 2, "both namespaces exist before boot");
            let workspace;
            for (const filesystem of services) if ((await filesystem.listFiles()).includes("reduction_steps.s")) workspace = filesystem;
            check(workspace !== undefined && !runtime.started, "preboot host access");
            const original = await workspace.readFile("reduction_steps.s");
            check(original.length > 0, "initial HTTP source");
            const events = [];
            const unsubscribe = await workspace.subscribe(change => events.push(change));
            step = "host and editor";
            await workspace.writeFile("reduction_steps.s", "host update\\n");
            await wait(() => doc.querySelector(".cm-content")?.textContent.includes("host update"), "host notification updates editor");
            const content = doc.querySelector(".cm-content");
            content.focus();
            doc.execCommand("selectAll");
            doc.execCommand("insertText", false, "editor update");
            await wait(async () => decoder.decode(await workspace.readFile("reduction_steps.s")) === "editor update\\n", "editor promise writes through");
            check(events.some(change => change.origin === 1n && change.source === "host"), "editor origin");
            await workspace.writeFile("reduction_steps.s", original);
            await workspace.mkdir("moved");
            await workspace.rename("reduction_steps.s", "moved/reduction_steps.s");
            await wait(() => doc.querySelector('.selected[data-path="moved/reduction_steps.s"]'), "editor follows file rename");
            await workspace.rename("moved", "renamed");
            await wait(() => doc.querySelector('.selected[data-path="renamed/reduction_steps.s"]'), "editor follows directory rename");
            await workspace.rename("renamed/reduction_steps.s", "reduction_steps.s");
            await workspace.remove("renamed");
            await wait(() => doc.querySelector('.selected[data-path="reduction_steps.s"]'), "editor follows restored path");

            step = "HTTP failure";
            try { await workspace.readFile("inputs/test.input"); throw new Error("HTTP failure accepted"); }
            catch (error) { check(error.errno === 5, "HTTP failure maps to EIO"); }
            await workspace.retrySource("inputs/test.input");
            check((await workspace.readFile("inputs/test.input")).length > 0, "HTTP retry");

            step = "guest mount and write";
            doc.getElementById("vm-tab-button").click();
            await wait(() => app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "guest login");
            const lazySize = (await workspace.stat("lib/stepper")).size;
            runtime.consoleInput(encoder.encode("wc -c lib/stepper; echo SOURCE_''LOAD_DONE\\n"));
            await wait(() => app.testConsole.includes(String(lazySize)) && app.testConsole.includes("SOURCE_LOAD_DONE"), "guest HTTP-backed lazy read");
            runtime.consoleInput(encoder.encode("printf 'guest new\\n' > guest-file\\n"));
            await wait(async () => (await workspace.listFiles()).includes("guest-file")
                && decoder.decode(await workspace.readFile("guest-file")) === "guest new\\n", "guest creates a shared file");
            runtime.consoleInput(encoder.encode("printf 'guest update\\n' > reduction_steps.s\\n"));
            await wait(async () => decoder.decode(await workspace.readFile("reduction_steps.s")) === "guest update\\n", "guest write visible to host");
            await wait(() => doc.querySelector(".cm-content")?.textContent.includes("guest update"), "guest notification updates editor");
            check(events.some(change => change.source === "guest" && change.path === "reduction_steps.s"), "guest notification");
            await workspace.writeFile("reduction_steps.s", "host replacement\\n");
            runtime.consoleInput(encoder.encode("cat reduction_steps.s; echo EXISTING_''READ_DONE\\n"));
            await wait(() => app.testConsole.includes("host replacement") && app.testConsole.includes("EXISTING_READ_DONE"), "guest rereads host replacement");
            await workspace.writeFile("host-file", "host visible\\n");
            runtime.consoleInput(encoder.encode("cat host-file; echo HOST_''READ_DONE\\n"));
            await wait(() => app.testConsole.includes("host visible") && app.testConsole.includes("HOST_READ_DONE"), "guest reads host write");

            step = "guest reboot";
            app.testConsole = "";
            await runtime.requestReboot();
            await wait(() => app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "guest reboot login");
            check(decoder.decode(await workspace.readFile("host-file")) === "host visible\\n", "reboot retains namespace");

            step = "namespace reset and remount";
            await workspace.reset();
            await workspace.writeFile("reset-file", "reset visible\\n");
            runtime.consoleInput(encoder.encode("cat /home/student/reset-file; echo STALE_''READ_DONE\\n"));
            await wait(() => app.testConsole.includes("Bad file descriptor") && app.testConsole.includes("STALE_READ_DONE"), "old guest fids fail after namespace reset");
            app.testConsole = "";
            await runtime.reset();
            await wait(() => app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "reset remount login");
            runtime.consoleInput(encoder.encode("cat reset-file; echo RESET_''READ_DONE\\n"));
            await wait(() => app.testConsole.includes("reset visible") && app.testConsole.includes("RESET_READ_DONE"), "namespace remount coherent");

            step = "shutdown";
            await runtime.requestShutdown();
            await wait(() => !runtime.started && doc.getElementById("status").textContent.startsWith("Halted"), "orderly shutdown");
            check(decoder.decode(await workspace.readFile("reset-file")) === "reset visible\\n", "halted host access");

            step = "switch during lazy read";
            const buttons = () => [...doc.querySelectorAll(".example-button")];
            buttons().find(button => button.textContent === "Insertion sort").click();
            await wait(() => doc.querySelector('.file[data-path="sort.s"]'), "sort selected");
            await wait(async () => (await (await fetch("/requests")).json()).some(path => path.endsWith("/examples/sort/sort.s")), "sort read pending");
            buttons().find(button => button.textContent === "Binary reduction steps").click();
            await wait(() => doc.querySelector('.file[data-path="reset-file"]'), "reduction reselected");
            await delay(500);
            check(!doc.querySelector('.file[data-path="sort.s"]'), "late sort view ignored");
            check(runtime === app.testRuntime && runtime.filesystems.size === 2, "one runtime retains handles");
            check(decoder.decode(await workspace.readFile("reset-file")) === "reset visible\\n", "destroy and rebind retain namespace");
            await unsubscribe();
            if (runtime.started) await runtime.halt();
            await runtime.destroy();
            await fetch("/result?status=pass");
        } catch (error) {
            await fetch("/result?status=" + encodeURIComponent(step + ": " + error + "\\n" + (app?.testConsole ?? "")));
        }
        </script>`, directory, {
            timeoutMs: 60_000,
            onRequest(url) { requests.push(url.pathname); },
            async response(url) {
                if (url.pathname === "/requests") return { status: 200, body: JSON.stringify(requests) };
                if (url.pathname.endsWith("/examples/reduction/inputs/test.input") && failInput) {
                    failInput = false; return { status: 403, body: "denied" };
                }
                if (url.pathname.endsWith("/examples/sort/sort.s")) await new Promise(resolve => setTimeout(resolve, 300));
            },
            transform(path, bytes) {
                if (!path.endsWith("/risclet/dist/riscbox.js")) return bytes;
                return Buffer.concat([bytes, Buffer.from(`
                    const instantiate = Riscbox.instantiate.bind(Riscbox);
                    Riscbox.instantiate = async (bytes, options) => {
                        window.testConsole = "";
                        const write = options.consoleWrite;
                        options.consoleWrite = text => { window.testConsole += typeof text === "string" ? text : new TextDecoder().decode(text); write(text); };
                        const runtime = await instantiate(bytes, options);
                        window.testRuntime = runtime;
                        return runtime;
                    };
                `)]);
            },
        });
        assert.equal(failInput, false);
        assert.equal(requests.filter(path => path.endsWith("/examples/reduction/lib/stepper")).length, 1);
        // Unselected sort bodies are never requested during initial setup.
        const firstSort = requests.findIndex(path => path.includes("/examples/sort/"));
        const firstInput = requests.findIndex(path => path.includes("/examples/reduction/inputs/test.input"));
        assert.ok(firstSort > firstInput, requests.join("\n"));
    } finally { await rm(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }); }
});
