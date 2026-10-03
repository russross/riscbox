import Split from "split.js";
import { EditorSession } from "../../../client-core/editor-session";
import { renderInstructions } from "../../../client-core/instructions";
import { renderFileTree as renderSharedFileTree } from "../../../client-core/workspace-view";
import { changeAffectsPath } from "../../../client-core/workspace";
import { VmSession } from "../../../client-core/vm-session";
import type { VmImage, VmTarget, VmTransition } from "../../../client-core/vm-session";
import type { Riscbox } from "@riscbox/runtime";
import type { P9Change } from "@riscbox/storage";
import { loadExampleDescriptions, loadExampleFiles } from "./examples";
import type { ExampleDescription } from "./examples";

interface ExampleState extends VmTarget { readonly description: ExampleDescription; }
declare global { interface Window { Riscbox: typeof Riscbox; } }

const DOC_PATH = "doc/doc.md";
const switchTransition: VmTransition = {
    workspace: "snapshot", poweroff: "orderly", discardDiskChanges: false, boot: true,
};
const image: VmImage = {
    configUrl: new URL("riscbox.cfg", window.location.href).href,
    runtimeUrl: new URL("riscbox.js", window.location.href).href,
    wasmUrl: new URL("riscbox.wasm", window.location.href).href,
    memoryMiB: 256, shareName: "default",
};
let examples: ExampleState[] = [];
let currentExample: ExampleState | null = null;
let editor: EditorSession;
let vm: VmSession<ExampleState>;
let viewGeneration = 0;
let instructionsGeneration = 0;
let instructionPaths = new Set([DOC_PATH]);
let switchQueue = Promise.resolve();
let switching = false;

function requiredElement(id: string): HTMLElement {
    const result = document.getElementById(id);
    if (!(result instanceof HTMLElement)) throw new Error(`Missing required element: ${id}`);
    return result;
}
function requiredButton(id: string): HTMLButtonElement {
    const result = requiredElement(id);
    if (!(result instanceof HTMLButtonElement)) throw new Error(`Missing required button: ${id}`);
    return result;
}
function reportUiError(error: unknown): void {
    console.error(error);
    requiredElement("status").textContent = error instanceof Error ? error.message : String(error);
}

// Controls reflect shared state without participating in VM lifecycle.
function updateControls(): void {
    requiredButton("sync-button").disabled = switching || !editor.dirty;
    if (vm === undefined) return;
    const boot = requiredButton("vm-boot-button");
    boot.disabled = switching || vm.target === undefined || vm.state === "loading" || vm.state === "stopping";
    boot.textContent = vm.state === "running" || vm.state === "failed" ? "Reboot VM" : "Boot VM";
    requiredButton("vm-reset-button").disabled = vm.target === undefined || vm.state === "loading";
}
function renderFileTree(): void {
    const paths = currentExample === null ? [] : vm.filesystem.listFiles();
    if (editor.path !== null && !paths.includes(editor.path) && !editor.dirty) editor.clear();
    renderSharedFileTree(requiredElement("file-tree-pane"), paths, {
        selectedPath: editor.path, priority: () => 0,
        onSelect: path => {
            if (switching) return;
            void editor.flush("selection").then(() => openFile(path)).catch(reportUiError);
        },
    });
}
function openFile(path: string): void {
    if (currentExample === null) return;
    try { editor.open(vm.filesystem, path); }
    catch (error: unknown) { editor.clear(); reportUiError(error); }
    renderFileTree();
}

function selectTab(name: "instructions" | "vm"): void {
    const selected = name === "instructions" && !requiredButton("instructions-tab-button").hidden ? "instructions" : "vm";
    for (const button of document.querySelectorAll<HTMLButtonElement>(".tab-button")) {
        button.classList.toggle("active", button.id === `${selected}-tab-button`);
    }
    for (const content of document.querySelectorAll<HTMLElement>(".tab-content")) {
        content.classList.toggle("active", content.id === `${selected}-tab-content`);
    }
    if (selected === "vm") {
        vm.fit();
        void editor.flush("interaction").then(() => vm.bootIfInactive()).catch(reportUiError);
    }
}
async function updateInstructions(): Promise<void> {
    const view = viewGeneration;
    const request = ++instructionsGeneration;
    const example = currentExample;
    const dependencies = new Set([DOC_PATH]);
    let rendered: string;
    try { rendered = example === null ? "" : await renderInstructions(vm.filesystem, dependencies); }
    catch (error: unknown) {
        if (view !== viewGeneration || request !== instructionsGeneration) return;
        instructionPaths = dependencies;
        throw error;
    }
    if (view !== viewGeneration || request !== instructionsGeneration || example !== currentExample) return;
    instructionPaths = dependencies;
    const button = requiredButton("instructions-tab-button");
    const content = requiredElement("instructions-tab-content");
    button.hidden = rendered === "";
    content.innerHTML = rendered;
    if (rendered === "" && content.classList.contains("active")) selectTab("vm");
}
function handleFilesystemChange(change: P9Change): void {
    if (switching || currentExample === null) return;
    try {
        editor.handleChange(change);
        if (change.kind !== "write") renderFileTree();
        if ([...instructionPaths].some(path => changeAffectsPath(change, path))) {
            void updateInstructions().catch(reportUiError);
        }
    } catch (error: unknown) { reportUiError(error); }
}

function renderMenu(): void {
    const menu = requiredElement("menu-items");
    menu.replaceChildren();
    const label = document.createElement("span");
    label.className = "menu-label";
    label.textContent = examples.length === 1 ? "Example:" : "Examples:";
    menu.append(label);
    for (const example of examples) {
        const button = document.createElement("button");
        button.className = "example-button";
        button.textContent = example.description.title;
        button.disabled = example === currentExample;
        button.addEventListener("click", () => { void switchExample(example).catch(reportUiError); });
        menu.append(button);
    }
}

// Selection generations reject stale downloads. Reset can release a stalled halt.
function switchExample(example: ExampleState, reset = false): Promise<void> {
    const generation = ++viewGeneration;
    for (const button of document.querySelectorAll<HTMLButtonElement>(".example-button")) button.disabled = false;
    const stopped = reset ? vm.forceHalt() : Promise.resolve();
    const selection = switchQueue.then(async () => {
        if (generation !== viewGeneration) return;
        await stopped;
        if (!reset) await editor.flush("transition");
        switching = true;
        updateControls();
        const wasReadOnly = editor.readOnly;
        editor.setReadOnly(true);
        let selected = false;
        try {
            const isCurrent = (): boolean => generation === viewGeneration;
            const changed = reset ? await vm.reset(isCurrent)
                : await vm.setTarget(example, switchTransition, isCurrent);
            if (changed) {
                await showExample(example, generation);
                selected = true;
            }
        } finally {
            switching = false;
            if (!selected) editor.setReadOnly(wasReadOnly);
            updateControls();
        }
    });
    switchQueue = selection.then(() => undefined, reportUiError);
    return selection;
}

async function showExample(example: ExampleState, generation: number): Promise<void> {
    currentExample = example;
    renderMenu();
    editor.clear();
    renderFileTree();
    await updateInstructions();
    if (generation !== viewGeneration) return;
    const paths = vm.filesystem.listFiles();
    const preferred = paths.includes(example.description.editable) ? example.description.editable : paths[0];
    if (preferred !== undefined) openFile(preferred);
    if (generation !== viewGeneration) return;
    requiredElement("status").textContent = `Running · ${example.description.title}`;
    selectTab(paths.includes(DOC_PATH) ? "instructions" : "vm");
    const url = new URL(window.location.href);
    url.searchParams.set("example", example.description.id);
    window.history.replaceState(null, "", url);
}

async function initialize(): Promise<void> {
    Split(["#file-tree-pane", "#editor-pane", "#info-pane"], {
        sizes: [10, 45, 45], gutterSize: 8, cursor: "grabbing", onDrag: () => vm.fit(),
    });
    editor = new EditorSession(requiredElement("editor-pane"), {
        canEdit: () => true, onChange: updateControls, onSynced: () => {}, onError: reportUiError,
        confirmDiscard: () => window.confirm("The filesystem changed this file while you have unflushed edits. Discard your edits and use the filesystem version? Keeping your edits will replace the filesystem version on the next flush."),
    });
    vm = new VmSession(requiredElement("vm-terminal"), {
        loadRuntime: async () => window.Riscbox,
        flushEditor: () => editor.flush("interaction"), canInteract: () => !switching,
        beforeReplace: () => { editor.clear(); currentExample = null; },
        afterSnapshot: async () => {}, onFilesystemChange: handleFilesystemChange,
        onError: reportUiError,
        onStateChange: state => {
            updateControls();
            const title = vm.target?.description.title ?? "VM";
            requiredElement("status").textContent = state === "stopping"
                ? "Shutting down VM · Reset can force recovery" : `${state[0].toUpperCase()}${state.slice(1)} · ${title}`;
        },
    });
    requiredButton("vm-reset-button").addEventListener("click", () => {
        if (vm.target !== undefined) void switchExample(vm.target, true).catch(reportUiError);
    });
    requiredButton("vm-boot-button").addEventListener("click", () => {
        void editor.flush("interaction").then(() => vm.state === "running" || vm.state === "failed"
            ? vm.reboot() : vm.boot()).catch(reportUiError);
    });
    requiredButton("sync-button").addEventListener("click", () => { void editor.flush().catch(reportUiError); });
    requiredButton("instructions-tab-button").addEventListener("click", () => selectTab("instructions"));
    requiredButton("vm-tab-button").addEventListener("click", () => selectTab("vm"));
    await vm.prepareImage(image);
    examples = (await loadExampleDescriptions()).map(description => ({
        description, image, loadFiles: () => loadExampleFiles(description),
    }));
    renderMenu();
    if (examples.length === 0) throw new Error("No examples are configured");
    const requested = new URL(window.location.href).searchParams.get("example");
    await switchExample(examples.find(example => example.description.id === requested) ?? examples[0]);
}

document.addEventListener("DOMContentLoaded", () => {
    void initialize().catch(error => { console.error("Could not start the Risclet demo", error); reportUiError(error); });
});
