import { WTerm } from "@wterm/dom";
import { GhosttyCore } from "@wterm/ghostty";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

const element = id => document.getElementById(id);
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });
let runtime;
let workspace;
let phase = "absent";
let busy = false;
let currentTree = "arithmetic";
let editorPath = null;
let trees = [];

// The editor is a copied host buffer. Only the explicit read/save controls touch 9p.
const editor = new EditorView({
    state: EditorState.create({
        doc: "Choose a file, then click Copy to editor.\n",
        extensions: [EditorView.contentAttributes.of({ "aria-label": "Copied file editor" })],
    }),
    parent: element("editor"),
});
const core = await GhosttyCore.load({
    wasmPath: "https://cdn.jsdelivr.net/npm/@wterm/ghostty@0.5.4/wasm/ghostty-vt.wasm",
});
const terminal = new WTerm(element("terminal"), {
    core, cols: 80, rows: 25, autoResize: false,
    onData: text => queueInput(encoder.encode(text)),
    onBinary: bytes => queueInput(bytes),
});
await terminal.init();

// Terminal input has an explicit bounded handoff; retain unaccepted bytes for retry.
let pendingInput = new Uint8Array();
let inputTimer = null;
function clearInput() {
    pendingInput = new Uint8Array();
    clearTimeout(inputTimer);
    inputTimer = null;
}
function queueInput(bytes) {
    if (phase !== "running") return;
    const joined = new Uint8Array(pendingInput.length + bytes.length);
    joined.set(pendingInput); joined.set(bytes, pendingInput.length);
    pendingInput = joined;
    flushInput();
}
function flushInput() {
    clearTimeout(inputTimer);
    inputTimer = null;
    if (phase !== "running" || pendingInput.length === 0) return;
    const accepted = runtime.consoleInput(pendingInput);
    if (accepted < 0 || accepted > pendingInput.length) {
        clearInput(); reportError(new Error("console input rejected")); return;
    }
    pendingInput = pendingInput.slice(accepted);
    if (pendingInput.length) inputTimer = setTimeout(flushInput, 10);
}

function log(text) {
    element("events").textContent += `${text}\n`;
    element("events").scrollTop = element("events").scrollHeight;
}
function reportError(error) {
    const message = error instanceof Error ? error.message : String(error);
    element("message").textContent = message;
    log(`error: ${message}`);
}
function setPhase(value) {
    phase = value;
    element("state").textContent = phase;
    updateControls();
}

// Enable operations according to their API preconditions, without implicit shutdowns.
function updateControls() {
    element("vm-controls").disabled = busy;
    element("share-controls").disabled = busy || phase !== "halted";
    element("prepare").disabled = phase !== "absent";
    element("boot").disabled = phase !== "halted";
    for (const id of ["shutdown", "reboot", "halt", "reset"]) {
        element(id).disabled = phase !== "running";
    }
    element("image-reset").disabled = phase !== "running" && phase !== "halted";
    element("destroy").disabled = phase !== "halted" && phase !== "preparing";
    for (const id of ["refresh-tree", "files", "read-file", "download-file"]) {
        element(id).disabled = busy || !workspace;
    }
    element("save-file").disabled = busy || !workspace || editorPath === null;
    element("clear-terminal").disabled = false;
}
async function action(name, operation) {
    if (busy) return;
    busy = true; updateControls();
    element("message").textContent = "";
    log(name);
    try { await operation(); }
    catch (error) { reportError(error); }
    finally { busy = false; updateControls(); }
}

// Preparation leaves a halted VM so the host can populate storage before booting.
async function prepare() {
    setPhase("preparing");
    try {
        await runtime.prepareFromUrl(new URL("riscbox.cfg", location.href).href);
        workspace = runtime.filesystem("workspace");
        setPhase("halted");
    } catch (error) {
        await runtime.destroy();
        setPhase("absent");
        throw error;
    }
}
async function loadTree(id) {
    const tree = trees.find(tree => tree.id === id);
    if (!tree) throw new Error(`unknown tree ${id}`);
    // Fetch all bodies before clearing the share, so a failed download preserves it.
    const bodies = await Promise.all(tree.files.map(async path => {
        const response = await fetch(`examples/${id}/${path}`);
        if (!response.ok) throw new Error(`HTTP ${response.status}: ${path}`);
        return { path, bytes: new Uint8Array(await response.arrayBuffer()) };
    }));
    workspace.clear();
    workspace.setAttributes("", {
        mode: 0o755, uid: 1000, gid: 1000,
        atime: { seconds: BigInt(Math.floor(Date.now() / 1000)), nanoseconds: 0 },
        mtime: { seconds: BigInt(Math.floor(Date.now() / 1000)), nanoseconds: 0 },
    });
    const directories = new Set();
    for (const { path, bytes } of bodies) {
        const parts = path.split("/");
        for (let count = 1; count < parts.length; count++) {
            const directory = parts.slice(0, count).join("/");
            if (!directories.has(directory)) {
                workspace.mkdir(directory);
                workspace.setAttributes(directory, { ...workspace.stat(directory), uid: 1000, gid: 1000 });
                directories.add(directory);
            }
        }
        workspace.writeFile(path, bytes);
        const attributes = workspace.stat(path);
        workspace.setAttributes(path, { ...attributes, uid: 1000, gid: 1000 });
    }
    currentTree = id;
    refreshTree();
    element("message").textContent = `Loaded ${id}; boot when ready.`;
}

// Listing, copying out, and copying back are separate actions with no subscriptions.
function refreshTree() {
    element("files").replaceChildren();
    function list(path = "", depth = 0) {
        for (const entry of workspace.listDirectory(path)) {
            const full = path ? `${path}/${entry.name}` : entry.name;
            const option = document.createElement("option");
            option.value = full;
            option.textContent = `${"  ".repeat(depth)}${entry.name}${entry.kind === "directory" ? "/" : ""}`;
            option.disabled = entry.kind !== "file";
            element("files").append(option);
            if (entry.kind === "directory") list(full, depth + 1);
        }
    }
    list();
}
function selectedFile() {
    const path = element("files").value;
    if (!path || workspace.stat(path).kind !== "file") throw new Error("select a file first");
    return path;
}
function readFile() {
    const path = selectedFile();
    const text = decoder.decode(workspace.readFile(path));
    editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: text } });
    editorPath = path;
    element("editor-path").textContent = path;
}
function downloadFile() {
    const path = selectedFile();
    const url = URL.createObjectURL(new Blob([workspace.readFile(path)]));
    const link = document.createElement("a");
    link.href = url; link.download = path.split("/").pop(); link.click();
    setTimeout(() => URL.revokeObjectURL(url), 0);
}

// Each control spells out the lifecycle calls; reset operations keep their stores distinct.
element("prepare").onclick = () => action("prepareFromUrl()", prepare);
element("boot").onclick = () => action("boot()", () => runtime.boot());
element("shutdown").onclick = () => action("requestShutdown()", () => runtime.requestShutdown());
element("reboot").onclick = () => action("requestReboot()", () => runtime.requestReboot());
element("halt").onclick = () => action("halt()", () => runtime.halt());
element("reset").onclick = () => action("reset()", () => runtime.reset());
element("image-reset").onclick = () => action("halt(); coldReset(); block(0).discardChanges(); boot()", async () => {
    if (phase === "running") await runtime.halt();
    await runtime.coldReset();
    runtime.block(0).discardChanges();
    await runtime.boot();
});
element("destroy").onclick = () => action("destroy()", async () => {
    await runtime.destroy(); workspace = null; setPhase("absent");
});
element("load-tree").onclick = () => action("filesystem.clear(); writeFile() source tree", () => loadTree(element("source-tree").value));
element("share-reset").onclick = () => action("reload current 9p tree", () => loadTree(currentTree));
element("share-clear").onclick = () => action("filesystem.clear()", () => { workspace.clear(); refreshTree(); });
element("refresh-tree").onclick = () => action("filesystem.listDirectory()", refreshTree);
element("read-file").onclick = () => action("filesystem.readFile() → editor", readFile);
element("save-file").onclick = () => action("editor → filesystem.writeFile()", () => {
    workspace.writeFile(editorPath, editor.state.doc.toString());
});
element("download-file").onclick = () => action("filesystem.readFile() → download", downloadFile);
element("clear-terminal").onclick = () => { terminal.write("\x1b[3J\x1b[2J\x1b[H"); log("clear terminal display and history"); };

// Reset notifications retire host input but keep full boot history visible in the terminal.
try {
    const response = await fetch("riscbox/riscbox.wasm");
    if (!response.ok) throw new Error(`WASM HTTP ${response.status}`);
    runtime = await Riscbox.instantiate(await response.arrayBuffer(), {
        consoleWrite: text => terminal.write(text),
        onVmStarted: () => {
            element("message").textContent = "";
            log("onVmStarted"); setPhase("running"); runtime.consoleResize(80, 25);
        },
        onVmHalted: cause => { clearInput(); log(`onVmHalted: ${cause}`); setPhase("halted"); },
        onVmReset: cause => { clearInput(); log(`onVmReset: ${cause}`); setPhase("running"); runtime.consoleResize(80, 25); },
        onVmDestroyed: () => { clearInput(); log("onVmDestroyed"); },
        onError: error => { clearInput(); reportError(error); },
    });
    const examples = await fetch("examples.json");
    if (!examples.ok) throw new Error(`Examples HTTP ${examples.status}`);
    trees = await examples.json();
    await action("prepare VM; load arithmetic; boot", async () => {
        await prepare();
        await loadTree("arithmetic");
        await runtime.boot();
    });
} catch (error) { reportError(error); }

// These ordinary application objects also make the example inspectable in DevTools.
export { runtime, workspace, terminal, editor };
