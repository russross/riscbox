import { Compartment, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { createTerminal } from "./terminal.js";
import { setupLayout } from "./layout.js";
import { prefetchImage } from "./disk-prefetch.js";

const element = id => document.getElementById(id);
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });
const EDITOR_ORIGIN = 1n;
let runtime;
let workspace;
let unsubscribeTree;
let phase = "absent";
let busy = false;
let editorPath = null;
let trees = [];
let sourceSelection = 0;
let dirty = false;
let replacing = false;
let saveTimer;
let statusTimer;
const access = new Compartment();

// Edits remain buffered until blur or thirty seconds of inactivity.
const editor = new EditorView({
    state: EditorState.create({ extensions: [
        EditorView.contentAttributes.of({ "aria-label": "Shared file editor" }),
        access.of([EditorState.readOnly.of(true), EditorView.editable.of(false)]),
        EditorView.domEventHandlers({ blur: () => { syncEditor(); } }),
        EditorView.updateListener.of(update => {
            if (!update.docChanged || replacing || update.state.readOnly) return;
            dirty = true;
            clearTimeout(saveTimer);
            saveTimer = setTimeout(syncEditor, 30_000);
        }),
    ] }),
    parent: element("editor"),
});
const terminal = createTerminal(element("terminal"), data => {
    queueInput(typeof data === "string" ? encoder.encode(data) : data);
}, ({ cols, rows }) => { if (phase === "running") runtime.consoleResize(cols, rows); });
setupLayout();
terminal.blur();
prefetchImage(new URL("riscbox.cfg", document.baseURI), element("terminal"));

// Programmatic replacement cancels buffered writes before touching the document.
function replaceEditor(path, text = "", writable = false) {
    clearTimeout(saveTimer);
    dirty = false;
    editorPath = path;
    replacing = true;
    try {
        editor.dispatch({
            changes: { from: 0, to: editor.state.doc.length, insert: text },
            effects: access.reconfigure([EditorState.readOnly.of(!writable), EditorView.editable.of(writable)]),
        });
    } finally { replacing = false; }
    element("editor-pane").classList.toggle("empty", !writable);
    for (const [path, row] of rows) row.setAttribute("aria-current", String(path === editorPath));
}
function openFile(path) {
    const bytes = workspace.readFile(path);
    if (bytes.includes(0)) { replaceEditor(null); return; }
    try { replaceEditor(path, decoder.decode(bytes), true); }
    catch { replaceEditor(null); }
}
function syncEditor() {
    clearTimeout(saveTimer);
    if (!dirty || !workspace || editorPath === null || editor.state.readOnly) return;
    try {
        workspace.writeFile(editorPath, editor.state.doc.toString(), EDITOR_ORIGIN);
        dirty = false;
        pulseRow(rows.get(editorPath));
    } catch (error) { reportError(error); }
}

// Repeated pulses replace their predecessor; overlays never capture pointer events.
const animations = new WeakMap();
function pulseRow(row) {
    if (!row) return;
    animations.get(row)?.cancel();
    const animation = row.animate([{ backgroundColor: "#ffd680" }, { backgroundColor: "transparent" }], { duration: 450 });
    animations.set(row, animation);
    return animation;
}
function pulsePane(id) {
    const pane = element(id);
    pane.querySelector(".pulse-overlay")?.remove();
    const overlay = document.createElement("div");
    overlay.className = "pulse-overlay";
    pane.append(overlay);
    const animation = overlay.animate([{ backgroundColor: "#ffcc6655" }, { backgroundColor: "transparent" }], { duration: 450 });
    animation.onfinish = () => overlay.remove();
}

// Terminal input retains unaccepted bytes until the bounded guest FIFO has room.
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
    clearInterval(statusTimer);
    updateStatus();
    if (phase === "running") statusTimer = setInterval(updateStatus, 1_000);
    updateControls();
}

// Clock and CPU uptime use elapsed seconds, with compact clock notation once
// a minute has passed. Status sampling stops after the final halt reading.
function formatUptime(seconds) {
    const whole = Math.floor(seconds);
    if (whole < 60) return `${whole}s`;
    const padded = value => String(value).padStart(2, "0");
    const minutes = Math.floor(whole / 60) % 60;
    const remainder = padded(whole % 60);
    if (whole < 3_600) return `${padded(minutes)}:${remainder}`;
    return `${padded(Math.floor(whole / 3_600))}:${padded(minutes)}:${remainder}`;
}
function updateStatus() {
    const speed = runtime && (runtime.state === "running" || runtime.state === "halted")
        ? runtime.speed()
        : { mcycles1s: 0, mcycles5s: 0, mcycles15s: 0, uptimeSeconds: 0, cyclingSeconds: 0 };
    element("speed-1s").textContent = speed.mcycles1s.toFixed(2);
    element("speed-5s").textContent = speed.mcycles5s.toFixed(2);
    element("speed-15s").textContent = speed.mcycles15s.toFixed(2);
    element("clock-time").textContent = formatUptime(speed.uptimeSeconds);
    element("cpu-time").textContent = formatUptime(speed.cyclingSeconds);
}

// Lifecycle controls keep their existing preconditions; source selection stays enabled.
function updateControls() {
    element("vm-controls").disabled = busy;
    element("prepare").disabled = phase !== "absent";
    element("boot").disabled = phase !== "halted";
    for (const id of ["shutdown", "reboot", "halt", "reset"]) element(id).disabled = phase !== "running";
    element("image-reset").disabled = phase !== "running" && phase !== "halted";
    element("destroy").disabled = phase !== "halted";
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
async function prepare() {
    setPhase("preparing");
    try {
        runtime = await Riscbox.prepare({
            config: { url: "riscbox.cfg" },
            consoleWrite: text => terminal.write(text),
            onVmStarted: () => {
                element("message").textContent = "";
                log("onVmStarted"); setPhase("running"); runtime.consoleResize(terminal.cols, terminal.rows);
            },
            onVmHalted: cause => { clearInput(); log(`onVmHalted: ${cause}`); setPhase("halted"); },
            onVmReset: cause => { clearInput(); log(`onVmReset: ${cause}`); setPhase("running"); runtime.consoleResize(terminal.cols, terminal.rows); },
            onVmDestroyed: () => { clearInput(); log("onVmDestroyed"); },
            onError: error => { clearInput(); reportError(error); },
        });
        workspace = runtime.filesystem("shared");
        subscribeTree();
        refreshTree();
        setPhase("halted");
        await loadTree(element("source-tree").value);
    } catch (error) {
        unsubscribeTree?.(); unsubscribeTree = null;
        workspace = null;
        clearView();
        if (runtime?.state === "halted") await runtime.destroy();
        runtime = null;
        setPhase("absent");
        throw error;
    }
}

// A selection clears immediately; only the newest fetch may populate the current share.
async function loadTree(id) {
    const selection = ++sourceSelection;
    const share = workspace;
    if (!share) return;
    share.clear();
    clearRows();
    if (editorPath !== null) pulsePane("editor-pane");
    replaceEditor(null);
    if (id === "") return;
    const tree = trees.find(tree => tree.id === id);
    if (!tree) throw new Error(`unknown tree ${id}`);
    const bodies = await Promise.all(tree.files.map(async path => {
        const response = await fetch(`examples/${id}/${path}`);
        if (!response.ok) throw new Error(`HTTP ${response.status}: ${path}`);
        return { path, bytes: new Uint8Array(await response.arrayBuffer()) };
    }));
    if (selection !== sourceSelection || workspace !== share) return;
    share.setAttributes("", {
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
                share.mkdir(directory);
                share.setAttributes(directory, { ...share.stat(directory), uid: 1000, gid: 1000 });
                directories.add(directory);
            }
        }
        share.writeFile(path, bytes);
        share.setAttributes(path, { ...share.stat(path), uid: 1000, gid: 1000 });
    }
    log(`Loaded ${id}`);
}

// Notifications are coalesced per browser microtask, with origin retained for editor writes.
function subscribeTree() {
    const share = workspace;
    let pending = [];
    unsubscribeTree = share.subscribe(change => {
        pending.push(change);
        if (pending.length !== 1) return;
        queueMicrotask(() => {
            const changes = pending;
            pending = [];
            if (workspace !== share) return;
            try { refreshTree(changes); }
            catch (error) { reportError(error); }
        });
    });
}
const rows = new Map();
function clearView() {
    replaceEditor(null);
    clearRows();
}
function clearRows() {
    for (const row of rows.values()) animations.get(row)?.cancel();
    rows.clear();
    element("files").replaceChildren();
}
function affects(change, path) {
    return change.kind === "reset" || change.kind === "rescan" ||
        [change.path, change.oldPath, ...change.aliases].some(changed =>
            changed !== undefined && (changed === "" || path === changed || path.startsWith(`${changed}/`)));
}

// Relisting reuses live rows; deleted rows remain disabled until their pulse finishes.
function refreshTree(changes = []) {
    const entries = new Map();
    function list(path = "", depth = 0) {
        for (const entry of workspace.listDirectory(path)) {
            const full = path ? `${path}/${entry.name}` : entry.name;
            entries.set(full, { ...entry, depth });
            if (entry.kind === "directory") list(full, depth + 1);
        }
    }
    list();
    // An empty namespace snaps away in one update; individual deletions retain
    // their short pulse while files elsewhere in the tree remain visible.
    if (entries.size === 0) clearRows();
    for (const [path, row] of rows) {
        if (entries.has(path) || row.classList.contains("deleted")) continue;
        row.disabled = true;
        row.classList.add("deleted");
        const animation = pulseRow(row);
        animation.onfinish = () => {
            if (!row.classList.contains("deleted")) return;
            row.remove(); rows.delete(path);
        };
    }
    for (const [path, entry] of entries) {
        let row = rows.get(path);
        const created = !row;
        if (!row) {
            row = document.createElement("button");
            row.className = "file-row";
            row.dataset.path = path;
            row.onclick = () => {
                syncEditor();
                try { openFile(path); editor.focus(); } catch (error) { reportError(error); }
            };
            rows.set(path, row);
        }
        row.classList.remove("deleted");
        row.disabled = entry.kind !== "file";
        row.textContent = `${"\u00a0\u00a0".repeat(entry.depth)}${entry.name}${entry.kind === "directory" ? "/" : ""}`;
        row.setAttribute("aria-current", String(path === editorPath));
        element("files").append(row);
        if (created || changes.some(change => affects(change, path))) pulseRow(row);
    }
    if (editorPath === null) return;
    const external = changes.some(change => !(change.source === "host" && change.origin === EDITOR_ORIGIN) && affects(change, editorPath));
    if (!entries.has(editorPath) || entries.get(editorPath).kind !== "file") {
        replaceEditor(null); pulsePane("editor-pane");
    } else if (external) {
        openFile(editorPath); pulsePane("editor-pane");
    }
}

// Lifecycle operations preserve storage ownership and retire pending source fetches on destroy.
element("prepare").onclick = () => action("Riscbox.prepare()", prepare);
element("boot").onclick = () => action("boot()", () => runtime.boot());
element("shutdown").onclick = () => action("requestShutdown()", () => runtime.requestShutdown());
element("reboot").onclick = () => action("requestReboot()", () => runtime.requestReboot());
element("halt").onclick = () => action("halt()", () => runtime.halt());
element("reset").onclick = () => action("reset()", () => runtime.reset());
element("image-reset").onclick = () => action("halt(); coldReset(); block(0).reset(); boot()", async () => {
    if (phase === "running") await runtime.halt();
    await runtime.coldReset();
    runtime.block(0).reset();
    await runtime.boot();
});
element("destroy").onclick = () => action("destroy()", async () => {
    ++sourceSelection;
    unsubscribeTree?.(); unsubscribeTree = null;
    await runtime.destroy(); runtime = null; workspace = null; setPhase("absent");
    clearView();
});
element("source-tree").onchange = () => { loadTree(element("source-tree").value).catch(reportError); };

// Prepare and populate the selected source before executing the first guest instruction.
try {
    const examples = await fetch("examples.json");
    if (!examples.ok) throw new Error(`Examples HTTP ${examples.status}`);
    trees = await examples.json();
    element("source-tree").replaceChildren(new Option("<clear>", ""), ...trees.map(tree => new Option(tree.id, tree.id)));
    element("source-tree").value = "robots";
    await action("prepare VM; load robots; boot", async () => { await prepare(); await runtime.boot(); });
} catch (error) { reportError(error); }

export { runtime, workspace, terminal, editor, queueInput };
