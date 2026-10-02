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
            await wait(() => frame.contentWindow.testRuntime && frame.contentDocument.getElementById("status")?.textContent.includes("Example file inputs/test.input: HTTP 403"), "UI ready");
            app = frame.contentWindow;
            const doc = app.document;
            const runtime = app.testRuntime;
            doc.getElementById("vm-reset-button").click();
            await wait(() => doc.getElementById("status").textContent.startsWith("Running") && doc.querySelector('.selected[data-path="reduction_steps.s"]'), "download retry and boot");
            const encoder = new app.TextEncoder();
            const decoder = new TextDecoder();
            const services = [...runtime.filesystems.values()];
            check(services.length === 1, "one VM-owned namespace");
            let workspace;
            for (const filesystem of services) if ((await filesystem.listFiles()).includes("reduction_steps.s")) workspace = filesystem;
            check(workspace !== undefined && runtime.started, "live host access");
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
            await delay(50);
            check(decoder.decode(await workspace.readFile("reduction_steps.s")) === "host update\\n", "typing remains buffered");
            doc.getElementById("instructions-tab-button").focus();
            await wait(async () => decoder.decode(await workspace.readFile("reduction_steps.s")) === "editor update\\n", "blur flushes editor");
            check(events.some(change => change.origin === 1n && change.source === "host"), "editor origin");
            const flushCount = events.filter(change => change.origin === 1n).length;
            content.focus();
            doc.getElementById("instructions-tab-button").focus();
            await delay(50);
            check(events.filter(change => change.origin === 1n).length === flushCount, "clean blur does not rewrite the file");

            step = "explicit Sync";
            content.focus();
            doc.execCommand("selectAll");
            doc.execCommand("insertText", false, "explicit sync");
            await wait(() => !doc.getElementById("sync-button").disabled, "dirty editor enables Sync");
            doc.getElementById("sync-button").click();
            await wait(() => decoder.decode(workspace.readFile("reduction_steps.s")) === "explicit sync\\n", "Sync writes without requiring blur");
            check(doc.getElementById("sync-button").disabled, "successful Sync disables clean control");

            step = "failed editor flush";
            const writeFile = workspace.writeFile.bind(workspace);
            let rejectEditorWrite = true;
            workspace.writeFile = (path, bytes, origin) => {
                if (origin === 1n && rejectEditorWrite) throw new Error("controlled editor write failure");
                return writeFile(path, bytes, origin);
            };
            content.focus();
            doc.execCommand("selectAll");
            doc.execCommand("insertText", false, "retained edit");
            doc.querySelector('.file[data-path="Makefile"] .item-content-wrapper').click();
            await wait(() => doc.getElementById("status").textContent.includes("controlled editor write failure"), "write error visible");
            check(content.textContent.includes("retained edit"), "failed switch retains editor text");
            check(doc.querySelector('.selected[data-path="reduction_steps.s"]'), "failed switch retains selection");
            rejectEditorWrite = false;
            doc.querySelector('.file[data-path="Makefile"] .item-content-wrapper').click();
            await wait(() => doc.querySelector('.selected[data-path="Makefile"]'), "retry allows switching");
            check(decoder.decode(await workspace.readFile("reduction_steps.s")) === "retained edit\\n", "retry flushes retained text");
            workspace.writeFile = writeFile;
            doc.querySelector('.file[data-path="reduction_steps.s"] .item-content-wrapper').click();
            await wait(() => doc.querySelector('.selected[data-path="reduction_steps.s"]'), "editable file reselected");

            step = "editor conflict";
            let conflicts = 0;
            app.confirm = () => { conflicts += 1; return false; };
            content.focus();
            doc.execCommand("selectAll");
            doc.execCommand("insertText", false, "keep editor");
            await workspace.writeFile("reduction_steps.s", "external replacement\\n");
            await wait(() => conflicts === 1, "external change asks before replacing dirty text");
            check(content.textContent.includes("keep editor"), "declined conflict preserves text");
            doc.getElementById("instructions-tab-button").focus();
            await wait(async () => decoder.decode(await workspace.readFile("reduction_steps.s")) === "keep editor\\n", "retained editor wins on flush");
            app.confirm = () => { conflicts += 1; return true; };
            content.focus();
            doc.execCommand("selectAll");
            doc.execCommand("insertText", false, "discard editor");
            await workspace.writeFile("reduction_steps.s", "accept external\\n");
            await wait(() => content.textContent.includes("accept external"), "accepted conflict loads external text");
            check(conflicts === 2, "one decision per conflict");
            doc.getElementById("instructions-tab-button").focus();
            await workspace.writeFile("reduction_steps.s", original);

            step = "debounced editor deadline";
            await runtime.halt();
            await wait(() => content.textContent.includes(".global"), "original editor restored");
            content.focus();
            doc.execCommand("selectAll");
            doc.execCommand("insertText", false, "first deadline edit");
            await delay(15_000);
            doc.execCommand("insertText", false, " later edit");
            await delay(16_000);
            check(decoder.decode(workspace.readFile("reduction_steps.s")) === decoder.decode(original), "later edit postpones autosync");
            await delay(15_000);
            await wait(() => decoder.decode(workspace.readFile("reduction_steps.s")).includes("later edit"), "autosync runs thirty seconds after latest edit");
            check(!decoder.decode(await workspace.readFile("reduction_steps.s")).includes(".global"), "deadline flushed edited content");
            doc.getElementById("instructions-tab-button").focus();
            await workspace.writeFile("reduction_steps.s", original);

            await runtime.boot();
            step = "instruction image dependency";
            const originalDoc = await workspace.readFile("doc/doc.md");
            await workspace.writeFile("doc/picture.svg", '<svg xmlns="http://www.w3.org/2000/svg"><text>first</text></svg>');
            await workspace.writeFile("doc/doc.md", "![picture](picture.svg)\\n");
            await wait(() => doc.querySelector("#instructions-tab-content img"), "instruction image rendered");
            const firstImage = doc.querySelector("#instructions-tab-content img").src;
            await workspace.writeFile("doc/picture.svg", '<svg xmlns="http://www.w3.org/2000/svg"><text>second</text></svg>');
            await wait(() => doc.querySelector("#instructions-tab-content img")?.src !== firstImage, "image-only change refreshes instructions");
            await workspace.writeFile("doc/doc.md", originalDoc);
            await workspace.remove("doc/picture.svg");

            step = "dirty rename";
            await wait(() => content.textContent.includes(".global"), "original content reloaded");
            await workspace.mkdir("moved");
            await wait(() => content.textContent.includes(".global"), "rename source loaded");
            content.focus();
            doc.execCommand("selectAll");
            doc.execCommand("insertText", false, "dirty renamed edit");
            await workspace.rename("reduction_steps.s", "moved/reduction_steps.s");
            await wait(() => doc.querySelector('.selected[data-path="moved/reduction_steps.s"]'), "editor follows file rename");
            await workspace.rename("moved", "renamed");
            await wait(() => doc.querySelector('.selected[data-path="renamed/reduction_steps.s"]'), "editor follows directory rename");
            check(content.textContent.includes("dirty renamed edit"), "rename preserves dirty buffer");
            doc.getElementById("instructions-tab-button").focus();
            await wait(async () => decoder.decode(await workspace.readFile("renamed/reduction_steps.s")) === "dirty renamed edit\\n", "dirty buffer flushes to renamed path");
            await workspace.rename("renamed/reduction_steps.s", "reduction_steps.s");
            await workspace.remove("renamed");
            await wait(() => doc.querySelector('.selected[data-path="reduction_steps.s"]'), "editor follows restored path");

            step = "guest mount and write";
            doc.getElementById("vm-tab-button").click();
            await wait(() => app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "guest login");

            step = "terminal paste backpressure";
            const consoleInput = runtime.consoleInput.bind(runtime);
            const pastedBytes = [];
            let blocked = true;
            runtime.consoleInput = bytes => {
                if (blocked) return 0;
                const accepted = consoleInput(bytes.subarray(0, 127));
                pastedBytes.push(...bytes.subarray(0, accepted));
                return accepted;
            };
            const paste = "#" + " ".repeat(2200) + "\\necho PASTE_''DONE\\n";
            const clipboard = new app.DataTransfer();
            clipboard.setData("text/plain", paste);
            doc.querySelector("#vm-terminal textarea").dispatchEvent(new app.ClipboardEvent("paste", { clipboardData: clipboard, bubbles: true, cancelable: true }));
            await delay(50);
            check(pastedBytes.length === 0, "full FIFO retains paste");
            blocked = false;
            await wait(() => pastedBytes.length >= encoder.encode(paste).length, "complete large paste accepted");
            check(decoder.decode(Uint8Array.from(pastedBytes)).includes("echo PASTE_''DONE"), "paste order retained");
            await wait(() => app.testConsole.includes("PASTE_DONE"), "pasted command executes");
            runtime.consoleInput = consoleInput;

            step = "guest mount and write";
            const residentSize = (await workspace.stat("lib/stepper")).size;
            runtime.consoleInput(encoder.encode("wc -c lib/stepper; echo SOURCE_''LOAD_DONE\\n"));
            await wait(() => app.testConsole.includes(String(residentSize)) && app.testConsole.includes("SOURCE_LOAD_DONE"), "guest resident read");
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
            doc.getElementById("vm-boot-button").click();
            await wait(() => app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "guest reboot login");
            check(decoder.decode(await workspace.readFile("host-file")) === "host visible\\n", "reboot retains namespace");

            step = "clean reset";
            let rejected = false;
            try { workspace.clear(); } catch { rejected = true; }
            check(rejected, "running namespace clear rejected");
            app.testConsole = "";
            doc.getElementById("vm-reset-button").click();
            await wait(() => app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "reset login");
            check(!workspace.listFiles().includes("host-file"), "reset clears custom files");
            check(decoder.decode(workspace.readFile("reduction_steps.s")) === decoder.decode(original), "reset restores cached original");
            check(runtime === app.testRuntime && runtime.filesystems.size === 1, "reset retains runtime and share");

            step = "orderly snapshot switches";
            const buttons = () => [...doc.querySelectorAll(".example-button")];
            workspace.writeFile("retained", "outgoing work");
            workspace.mkdir("empty-directory");
            workspace.link("retained", "retained-alias");
            workspace.symlink("retained-link", "retained");
            const attrs = { mode: 0o751, uid: 1000, gid: 1000,
                atime: { seconds: 123n, nanoseconds: 456 }, mtime: { seconds: 789n, nanoseconds: 123 } };
            workspace.setAttributes("retained", attrs);
            const shutdown = runtime.requestShutdown.bind(runtime);
            let shutdowns = 0;
            runtime.requestShutdown = async () => { shutdowns += 1; await shutdown(); };
            content.focus();
            doc.execCommand("selectAll");
            doc.execCommand("insertText", false, "buffered switch edit");
            await wait(() => !doc.getElementById("sync-button").disabled, "outgoing editor is dirty");
            step = "switch during example download";
            buttons().find(button => button.textContent === "Insertion sort").click();
            await wait(async () => (await (await fetch("/requests")).json()).some(path => path.endsWith("/examples/sort/sort.s")), "sort download pending");
            app.testConsole = "";
            buttons().find(button => button.textContent === "Binary reduction steps").click();
            await wait(() => doc.querySelector('.selected[data-path="reduction_steps.s"]') && app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "latest selection wins after download");
            check(workspace.listFiles().includes("retained") && !workspace.listFiles().includes("sort.s"), "superseded download preserves outgoing namespace");
            const beforeSwitches = shutdowns;
            step = "orderly snapshot switches";
            app.testConsole = "";
            buttons().find(button => button.textContent === "Insertion sort").click();
            await wait(() => doc.querySelector('.selected[data-path="sort.s"]') && app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "switch shuts down and boots sort");
            check(!workspace.listFiles().includes("retained"), "incoming example has independent files");
            workspace.writeFile("sort-work", "sort snapshot");
            app.testConsole = "";
            buttons().find(button => button.textContent === "Binary reduction steps").click();
            await wait(() => doc.querySelector('.selected[data-path="reduction_steps.s"]') && app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "switch restores reduction");
            check(shutdowns === beforeSwitches + 2, "both running switches request orderly shutdown");
            check(decoder.decode(workspace.readFile("reduction_steps.s")) === "buffered switch edit\\n", "switch flushes editor before snapshot");
            check(decoder.decode(workspace.readFile("retained")) === "outgoing work", "outgoing work preserved");
            check(workspace.stat("retained").inode === workspace.stat("retained-alias").inode, "snapshot preserves hard links");
            check(workspace.readlink("retained-link") === "retained", "snapshot preserves symlinks");
            check(workspace.listDirectory("empty-directory").length === 0, "snapshot preserves empty directories");
            check(workspace.stat("retained").mode === 0o751 && workspace.stat("retained").mtime.seconds === 789n, "snapshot preserves executable metadata");
            app.testConsole = "";
            buttons().find(button => button.textContent === "Insertion sort").click();
            await wait(() => doc.querySelector('.selected[data-path="sort.s"]') && app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "incoming sort snapshot restored");
            check(decoder.decode(workspace.readFile("sort-work")) === "sort snapshot", "incoming example retains its own work");
            app.testConsole = "";
            buttons().find(button => button.textContent === "Binary reduction steps").click();
            await wait(() => doc.querySelector('.selected[data-path="reduction_steps.s"]') && app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "reduction restored again");

            step = "stalled shutdown recovery";
            let rejectRetiredShutdown;
            runtime.requestShutdown = () => {
                shutdowns += 1;
                return new Promise((resolve, reject) => { rejectRetiredShutdown = reject; });
            };
            buttons().find(button => button.textContent === "Insertion sort").click();
            await wait(() => doc.getElementById("status").textContent.startsWith("Shutting down"), "waiting for poweroff");
            check(!doc.getElementById("vm-reset-button").disabled, "Reset remains available during shutdown");
            app.testConsole = "";
            doc.getElementById("vm-reset-button").click();
            await wait(() => doc.querySelector('.selected[data-path="reduction_steps.s"]') && app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "Reset interrupts stalled transition");
            check(!workspace.listFiles().includes("retained"), "Reset discards saved snapshot");
            check(decoder.decode(workspace.readFile("reduction_steps.s")) === decoder.decode(original), "Reset restores originals");
            rejectRetiredShutdown(new Error("controlled retired shutdown failure"));
            await delay(50);
            check(runtime.started && doc.getElementById("status").textContent.startsWith("Running"), "late retired shutdown failure cannot fail new VM");
            runtime.requestShutdown = shutdown;

            step = "shutdown";
            await runtime.requestShutdown();
            await wait(() => !runtime.started && doc.getElementById("status").textContent.startsWith("Halted"), "orderly shutdown");
            workspace.writeFile("preserve-me", "preserved");
            const disk = runtime.block(0);
            const baseSector = await disk.read(0n, 512);
            const overlay = baseSector.slice();
            overlay[0] ^= 255;
            disk.write(0n, overlay);

            step = "superseded example switch";
            app.testConsole = "";
            buttons().find(button => button.textContent === "Insertion sort").click();
            buttons().find(button => button.textContent === "Binary reduction steps").click();
            await wait(() => doc.querySelector('.selected[data-path="reduction_steps.s"]') && doc.getElementById("status").textContent.startsWith("Running")
                && !doc.getElementById("vm-boot-button").disabled && runtime.started
                && app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "reduction reselected");
            await delay(500);
            check(!workspace.listFiles().includes("sort.s") && workspace.listFiles().includes("preserve-me"), "superseded switch preserves outgoing files");
            check(runtime === app.testRuntime && runtime.filesystems.size === 1, "switch retains runtime and share");
            await runtime.requestShutdown();
            await wait(() => !runtime.started, "poweroff for retained disk inspection");
            check((await disk.read(0n, 512))[0] === overlay[0], "switch retains disk overlay");
            app.testConsole = "";
            doc.getElementById("vm-reset-button").click();
            await wait(() => app.testConsole.includes("To test your code:") && app.testConsole.includes("$"), "final Reset boot");
            await runtime.requestShutdown();
            await wait(() => !runtime.started, "poweroff after Reset");
            check((await disk.read(0n, 512))[0] === baseSector[0], "Reset discards disk overlay");
            await unsubscribe();
            if (runtime.started) await runtime.halt();
            await runtime.destroy();
            await fetch("/result?status=pass");
        } catch (error) {
            await fetch("/result?status=" + encodeURIComponent(step + ": " + error + "\\n" + (app?.testConsole ?? "")));
        }
        </script>`, directory, {
            timeoutMs: 150_000,
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
        assert.equal(requests.filter(path => path.endsWith("/examples/reduction/lib/stepper")).length, 2);
        // Unselected sort bodies are never requested during initial setup.
        const firstSort = requests.findIndex(path => path.includes("/examples/sort/"));
        const firstInput = requests.findIndex(path => path.includes("/examples/reduction/inputs/test.input"));
        assert.ok(firstSort > firstInput, requests.join("\n"));
    } finally { await rm(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }); }
});
