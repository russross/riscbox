import * as commonmark from "commonmark";
import Split from "split.js";
import { defaultKeymap } from "@codemirror/commands";
import { cpp } from "@codemirror/lang-cpp";
import { markdown } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { LanguageSupport, StreamLanguage } from "@codemirror/language";
import { gas } from "@codemirror/legacy-modes/mode/gas";
import { shell } from "@codemirror/legacy-modes/mode/shell";
import { Compartment, EditorSelection, EditorState } from "@codemirror/state";
import { EditorView, keymap, ViewUpdate } from "@codemirror/view";
import { basicSetup } from "codemirror";
import { TerminalInputQueue } from "./terminal_input";
import { TerminalView } from "./terminal";
import { populateWorkspace, restoreWorkspace, snapshotWorkspace } from "./workspace";
import type { WorkspaceSnapshot } from "./workspace";
import type { Filesystem, P9Change } from "../../../js/storage";
import type { Riscbox as RiscboxRuntime } from "../../../js/riscbox";

interface ExampleFile { readonly path: string; readonly size: number; }

interface ExampleDescription {
    readonly id: string;
    readonly title: string;
    readonly editable: string;
    readonly documentation?: string;
    readonly files: readonly ExampleFile[];
}

interface ExampleState {
    readonly description: ExampleDescription;
    readonly filesystem: Filesystem;
    snapshot?: WorkspaceSnapshot;
}

interface FileTreeNode {
    isFile: boolean;
    fullPath: string;
    children: Record<string, FileTreeNode>;
}

declare global {
    interface Window {
        Riscbox: typeof RiscboxRuntime;
    }
}

const DOC_PATH = "doc/doc.md";
const decoder = new TextDecoder();
const encoder = new TextEncoder();
const markdownParser = new commonmark.Parser();
const markdownRenderer = new commonmark.HtmlRenderer();
const language = new Compartment();
const editable = new Compartment();

let examples: ExampleState[] = [];
let currentExample: ExampleState | null = null;
let currentPath: string | null = null;
let editor: EditorView;
let programmaticEditorUpdate = false;
let vmController: VmController;

// Revisions distinguish the submitted text from edits made while a write finishes.
let editorRevision = 0;
let flushedRevision = 0;
let editorTimer: number | undefined;
let editorWrites = Promise.resolve();
let conflictPath: string | null = null;
let instructionPaths = new Set([DOC_PATH]);
const EDITOR_FLUSH_DELAY_MS = 30_000;

let viewGeneration = 0;
let openGeneration = 0;
let treeGeneration = 0;
let instructionsGeneration = 0;
let switchQueue = Promise.resolve();
let switching = false;
const EDITOR_ORIGIN = 1n;

function reportUiError(error: unknown): void {
    console.error(error);
    requiredElement("status").textContent = error instanceof Error ? error.message : String(error);
}

function requiredElement(id: string): HTMLElement {
    const result = document.getElementById(id);
    if (!(result instanceof HTMLElement)) {
        throw new Error(`Missing required element: ${id}`);
    }
    return result;
}

function requiredButton(id: string): HTMLButtonElement {
    const result = document.getElementById(id);
    if (!(result instanceof HTMLButtonElement)) {
        throw new Error(`Missing required button: ${id}`);
    }
    return result;
}

function normalizeRelativePath(raw: string): string {
    if (raw.includes("\\")) {
        throw new Error(`Invalid example path: ${JSON.stringify(raw)}`);
    }
    const parts = raw.split("/");
    if (raw === "" || raw.startsWith("/") || raw.includes("\0")
        || parts.some((part) => part === "" || part === "." || part === "..")) {
        throw new Error(`Invalid example path: ${JSON.stringify(raw)}`);
    }
    return parts.join("/");
}

function parseExample(value: unknown): ExampleDescription {
    if (typeof value !== "object" || value === null
        || !("id" in value) || typeof value.id !== "string"
        || !("title" in value) || typeof value.title !== "string"
        || !("editable" in value) || typeof value.editable !== "string"
        || !("files" in value) || !Array.isArray(value.files)) {
        throw new Error("Invalid example manifest record");
    }
    const id = normalizeRelativePath(value.id);
    if (id.includes("/")) throw new Error("Example ID must be one path component");
    const files = value.files.map((file: unknown): ExampleFile => {
        if (typeof file !== "object" || file === null
            || !("path" in file) || typeof file.path !== "string"
            || !("size" in file) || typeof file.size !== "number"
            || !Number.isInteger(file.size) || file.size < 0 || file.size > 0xffff_ffff) {
            throw new Error("Invalid example file record");
        }
        return { path: normalizeRelativePath(file.path), size: file.size };
    });
    const editablePath = normalizeRelativePath(value.editable);
    if (!files.some(file => file.path === editablePath)) throw new Error("Example editable file is missing");
    const documentation = "documentation" in value ? value.documentation : undefined;
    if (documentation !== undefined && typeof documentation !== "string") throw new Error("Invalid documentation path");
    return { id, title: value.title, editable: editablePath, files,
        ...(documentation === undefined ? {} : { documentation: normalizeRelativePath(documentation) }) };
}

function softTab(view: EditorView): boolean {
    if (view.state.readOnly) {
        return false;
    }
    const tabSize = 4;
    const transaction = view.state.changeByRange((range) => {
        const line = view.state.doc.lineAt(range.from);
        const column = range.from - line.from;
        const spaces = tabSize - (column % tabSize);
        const insert = " ".repeat(spaces === 0 ? tabSize : spaces);
        return {
            changes: { from: range.from, to: range.to, insert },
            range: EditorSelection.cursor(range.from + insert.length),
        };
    });
    view.dispatch(transaction);
    return true;
}

function languageFor(filename: string): LanguageSupport | null {
    const extension = filename.split(".").pop();
    switch (extension) {
        case "c":
        case "h":
            return cpp();
        case "s":
        case "S":
            return new LanguageSupport(StreamLanguage.define(gas));
        case "md":
            return markdown();
        case "py":
            return python();
        default:
            break;
    }
    return filename.endsWith("Makefile")
        ? new LanguageSupport(StreamLanguage.define(shell))
        : null;
}

function editorTextFromFile(content: Uint8Array): string {
    const text = decoder.decode(content);
    return text.endsWith("\n") ? text.slice(0, -1) : text;
}

function fileContentFromEditor(): Uint8Array {
    const text = editor.state.doc.toString();
    return encoder.encode(text === "" ? "" : `${text}\n`);
}

function isBinaryFile(content: Uint8Array): boolean {
    return content.includes(0);
}

function resetEditor(content: string, canEdit: boolean, filename: string): void {
    programmaticEditorUpdate = true;
    editor.dispatch({
        changes: editor.state.doc.toString() === content
            ? undefined
            : { from: 0, to: editor.state.doc.length, insert: content },
        effects: [
            editable.reconfigure([
                EditorView.editable.of(canEdit),
                EditorState.readOnly.of(!canEdit),
            ]),
            language.reconfigure(languageFor(filename) ?? []),
        ],
    });
    programmaticEditorUpdate = false;
}

function clearEditor(): void {
    cancelEditorTimer();
    flushedRevision = editorRevision;
    conflictPath = null;
    currentPath = null;
    resetEditor("", false, "");
    updateSyncButton();
}

async function openFile(path: string): Promise<void> {
    const example = currentExample;
    const view = viewGeneration;
    const request = ++openGeneration;
    const revision = editorRevision;
    if (example === null) {
        return;
    }
    let content: Uint8Array;
    try {
        content = example.filesystem.readFile(path);
    } catch (error: unknown) {
        if (view !== viewGeneration || request !== openGeneration || revision !== editorRevision) return;
        clearEditor();
        void renderFileTree().catch(reportUiError);
        reportUiError(error);
        return;
    }
    if (view !== viewGeneration || request !== openGeneration || example !== currentExample
        || revision !== editorRevision || editorIsDirty()) return;
    conflictPath = null;
    currentPath = path;
    if (isBinaryFile(content)) {
        resetEditor("This file appears to be a binary file and cannot be displayed in the editor.", false, path);
    } else {
        resetEditor(editorTextFromFile(content), true, path);
    }
    void renderFileTree().catch(reportUiError);
    editor.focus();
}

function editorIsDirty(): boolean {
    return editorRevision !== flushedRevision;
}

function cancelEditorTimer(): void {
    if (editorTimer !== undefined) window.clearTimeout(editorTimer);
    editorTimer = undefined;
}

// Blur normally flushes edits; the timer is a fallback after typing stops.
function scheduleEditorFlush(): void {
    cancelEditorTimer();
    if (!editorIsDirty()) return;
    editorTimer = window.setTimeout(() => {
        editorTimer = undefined;
        void syncEditor().catch(reportUiError);
    }, EDITOR_FLUSH_DELAY_MS);
}

function syncEditor(): Promise<void> {
    cancelEditorTimer();
    const operation = editorWrites.then(async () => {
        const example = currentExample;
        const path = currentPath;
        if (example === null || path === null || editor.state.readOnly || !editorIsDirty()) return;
        const revision = editorRevision;
        const content = fileContentFromEditor();
        try {
            example.filesystem.writeFile(path, content, EDITOR_ORIGIN);
        } catch (error: unknown) {
            scheduleEditorFlush();
            throw error;
        }
        // Only the acknowledged snapshot is clean; newer edits retain their deadline.
        if (example === currentExample && path === currentPath) {
            flushedRevision = Math.max(flushedRevision, revision);
            conflictPath = null;
            if (editorIsDirty()) scheduleEditorFlush();
            else cancelEditorTimer();
            updateSyncButton();
        }
    });
    editorWrites = operation.catch(() => undefined);
    return operation;
}

function updateSyncButton(): void {
    requiredButton("sync-button").disabled = switching || !editorIsDirty();
}

function buildFileTree(paths: readonly string[]): Record<string, FileTreeNode> {
    const tree: Record<string, FileTreeNode> = {};
    for (const rawPath of paths) {
        const path = normalizeRelativePath(rawPath);
        const parts = path.split("/");
        let level = tree;
        for (let index = 0; index < parts.length; index += 1) {
            const part = parts[index];
            const isFile = index === parts.length - 1;
            level[part] ??= { isFile, fullPath: path, children: {} };
            level = level[part].children;
        }
    }
    return tree;
}

function renderTree(node: Record<string, FileTreeNode>, parent: HTMLElement, depth = 0): void {
    const keys = Object.keys(node).sort((left, right) => {
        const leftNode = node[left];
        const rightNode = node[right];
        return leftNode.isFile === rightNode.isFile
            ? left.localeCompare(right)
            : leftNode.isFile ? 1 : -1;
    });
    for (const key of keys) {
        const item = node[key];
        const listItem = document.createElement("li");
        listItem.classList.add(item.isFile ? "file" : "folder");
        const wrapper = document.createElement("div");
        wrapper.classList.add("item-content-wrapper");
        wrapper.style.setProperty("--tree-depth", String(depth));
        const icon = document.createElement("span");
        icon.classList.add("icon");
        wrapper.append(icon, document.createTextNode(key));
        listItem.append(wrapper);
        if (item.isFile) {
            listItem.dataset.path = item.fullPath;
            listItem.classList.toggle("selected", item.fullPath === currentPath);
            listItem.addEventListener("click", (event: MouseEvent): void => {
                event.stopPropagation();
                if (switching) return;
                void syncEditor().then(() => openFile(item.fullPath)).catch(reportUiError);
            });
        }
        parent.append(listItem);
        if (Object.keys(item.children).length > 0) {
            const children = document.createElement("ul");
            listItem.append(children);
            renderTree(item.children, children, depth + 1);
        }
    }
}

async function renderFileTree(): Promise<void> {
    const view = viewGeneration;
    const request = ++treeGeneration;
    const example = currentExample;
    const pane = requiredElement("file-tree-pane");
    const paths = example === null ? [] : example.filesystem.listFiles();
    if (view !== viewGeneration || request !== treeGeneration || example !== currentExample) return;
    if (currentPath !== null && !paths.includes(currentPath) && !editorIsDirty()) {
        clearEditor();
    }
    const root = document.createElement("ul");
    root.classList.add("file-tree");
    renderTree(buildFileTree(paths), root);
    pane.replaceChildren(root);
}

function bytesToBase64(content: Uint8Array): string {
    const chunks: string[] = [];
    for (let offset = 0; offset < content.length; offset += 32768) {
        chunks.push(String.fromCharCode(...content.subarray(offset, offset + 32768)));
    }
    return btoa(chunks.join(""));
}

function imageMimeType(path: string): string | null {
    const extension = path.split(".").pop()?.toLowerCase();
    switch (extension) {
        case "gif": return "image/gif";
        case "jpg":
        case "jpeg": return "image/jpeg";
        case "png": return "image/png";
        case "svg": return "image/svg+xml";
        default: return null;
    }
}

async function renderInstructions(filesystem: Filesystem, dependencies: Set<string>): Promise<string> {
    if (!(filesystem.listFiles()).includes(DOC_PATH)) {
        return "";
    }
    const document = markdownParser.parse(decoder.decode(filesystem.readFile(DOC_PATH)));
    const documentUrl = new URL(DOC_PATH, "https://workspace.invalid/");
    const walker = document.walker();
    let event = walker.next();
    while (event !== null) {
        if (event.entering && event.node.type === "image" && event.node.destination !== null) {
            const url = new URL(event.node.destination, documentUrl);
            if (url.origin === documentUrl.origin) {
                const path = decodeURIComponent(url.pathname.replace(/^\//, ""));
                dependencies.add(path);
                const content = filesystem.readFile(path);
                const mimeType = imageMimeType(path);
                if (mimeType === null) {
                    throw new Error(`Instruction image has an unsupported type: ${path}`);
                }
                event.node.destination = `data:${mimeType};base64,${bytesToBase64(content)}`;
            }
        }
        event = walker.next();
    }
    return markdownRenderer.render(document);
}

function selectTab(name: "instructions" | "vm"): void {
    const instructionsButton = requiredButton("instructions-tab-button");
    const selected = name === "instructions" && !instructionsButton.hidden ? "instructions" : "vm";
    for (const button of document.querySelectorAll<HTMLButtonElement>(".tab-button")) {
        button.classList.toggle("active", button.id === `${selected}-tab-button`);
    }
    for (const content of document.querySelectorAll<HTMLElement>(".tab-content")) {
        content.classList.toggle("active", content.id === `${selected}-tab-content`);
    }
    if (selected === "vm") {
        vmController.fit();
        void syncEditor().then(() => vmController.bootIfInactive()).catch(reportUiError);
    }
}

async function updateInstructions(): Promise<void> {
    const view = viewGeneration;
    const request = ++instructionsGeneration;
    const example = currentExample;
    const button = requiredButton("instructions-tab-button");
    const content = requiredElement("instructions-tab-content");
    let rendered: string;
    const dependencies = new Set([DOC_PATH]);
    try {
        rendered = example === null ? "" : await renderInstructions(example.filesystem, dependencies);
    } catch (error: unknown) {
        if (view !== viewGeneration || request !== instructionsGeneration) return;
        instructionPaths = dependencies;
        throw error;
    }
    if (view !== viewGeneration || request !== instructionsGeneration || example !== currentExample) return;
    instructionPaths = dependencies;
    button.hidden = rendered === "";
    content.innerHTML = rendered;
    if (rendered === "" && content.classList.contains("active")) {
        selectTab("vm");
    }
}

function changeAffectsPath(change: P9Change, path: string): boolean {
    if (change.kind === "reset" || change.kind === "rescan") return true;
    const names = [change.path, ...change.aliases];
    if (change.oldPath !== undefined) names.push(change.oldPath);
    return names.some(name => path === name || path.startsWith(`${name}/`));
}

async function handleFilesystemChange(example: ExampleState, change: P9Change): Promise<void> {
    if (switching || example !== currentExample) {
        return;
    }
    // Directory renames move every displayed child path. Alias events also
    // invalidate an open editor even when the guest wrote through another link.
    const followsRename = change.kind === "rename" && change.oldPath !== undefined && currentPath !== null
        && (currentPath === change.oldPath || currentPath.startsWith(`${change.oldPath}/`));
    if (followsRename && change.oldPath !== undefined && currentPath !== null) {
        currentPath = change.path + currentPath.slice(change.oldPath.length);
    }
    if (change.kind !== "write") {
        void renderFileTree().catch(reportUiError);
    }
    if ([...instructionPaths].some(path => changeAffectsPath(change, path))) {
        void updateInstructions().catch(reportUiError);
    }
    if (currentPath !== null && !(change.source === "host" && change.origin === EDITOR_ORIGIN)
        && changeAffectsPath(change, currentPath)) {
        // Structural notifications preserve a dirty buffer at its renamed path.
        // Content replacement requires an explicit decision before discarding it.
        if (editorIsDirty()) {
            if (followsRename || change.kind === "metadata") return;
            if (conflictPath === currentPath) return;
            if (!window.confirm("The filesystem changed this file while you have unflushed edits. Discard your edits and use the filesystem version? Keeping your edits will replace the filesystem version on the next flush.")) {
                conflictPath = currentPath;
                return;
            }
            cancelEditorTimer();
            flushedRevision = editorRevision;
            conflictPath = null;
            updateSyncButton();
        }
        const view = viewGeneration;
        const path = currentPath;
        const paths = example.filesystem.listFiles();
        if (view !== viewGeneration || example !== currentExample || currentPath !== path) return;
        if (editorIsDirty()) return;
        if (paths.includes(currentPath)) {
            await openFile(currentPath);
        } else {
            clearEditor();
        }
    }
}

class VmController {
    private readonly bootButton: HTMLButtonElement;
    private readonly resetButton: HTMLButtonElement;
    private readonly terminal: TerminalView;
    private runtime: RiscboxRuntime | undefined;
    private target: ExampleState | undefined;
    private generation = 0;
    private inputGeneration = 0;
    private state: "ready" | "loading" | "stopping" | "running" | "halted" | "failed" = "ready";
    private workspaceLoaded = false;
    private shutdownWaiter: { resolve(): void; reject(error: Error): void } | undefined;
    private readonly input = new TerminalInputQueue(bytes => {
        if (this.state !== "running" || this.runtime === undefined) return 0;
        return this.runtime.consoleInput(bytes);
    });

    constructor(host: HTMLElement, bootButton: HTMLButtonElement) {
        this.bootButton = bootButton;
        const resetButton = requiredElement("vm-reset-button");
        if (!(resetButton instanceof HTMLButtonElement)) throw new Error("Reset control must be a button");
        this.resetButton = resetButton;
        resetButton.addEventListener("click", () => {
            if (this.target) void switchExample(this.target, true).catch(reportUiError);
        });
        this.terminal = new TerminalView(host, {
            onData: text => this.sendInput(encoder.encode(text)),
            onBinary: bytes => this.sendInput(bytes),
            onResize: (cols, rows) => this.runtime?.consoleResize(cols, rows),
        });
        this.bootButton.addEventListener("click", (): void => {
            void syncEditor().then(async () => {
                if (this.state === "ready") await this.boot();
                else if (this.state === "halted") await this.bootRetained();
                else await this.reboot();
            }).catch(reportUiError);
        });
        new ResizeObserver((): void => this.fit()).observe(host);
        this.updateControls();
    }

    private sendInput(bytes: Uint8Array): void {
        if (this.state !== "running" || switching) return;
        const generation = this.inputGeneration;
        const copied = bytes.slice();
        void syncEditor().then(() => {
            if (generation === this.inputGeneration && this.state === "running" && !switching) {
                this.input.enqueue(copied);
            }
        }).catch(reportUiError);
    }

    async setTarget(target: ExampleState, reset: boolean, isCurrent: () => boolean): Promise<boolean> {
        const runtime = this.runtime;
        if (!runtime) throw new Error("Runtime is unavailable");
        const previous = this.target;
        if (previous === undefined) this.target = target;
        this.clearInput();
        try {
            // Downloads finish before retiring the outgoing namespace. A failed
            // download leaves its bytes and VM available for recovery.
            const files = reset || target.snapshot === undefined
                ? await loadExampleFiles(target.description) : undefined;
            if (!isCurrent()) return false;
            if (reset) {
                await this.forceHalt();
                await runtime.coldReset();
                runtime.block(0).discardChanges();
            } else {
                await this.shutdown();
            }
            if (!isCurrent()) return false;
            if (!reset && previous !== undefined && this.workspaceLoaded) {
                previous.snapshot = snapshotWorkspace(previous.filesystem);
            }
            // Retired editor views cannot react to namespace replacement events.
            clearEditor();
            currentExample = null;
            this.target = target;
            this.workspaceLoaded = false;
            if (reset) target.snapshot = undefined;
            if (target.snapshot !== undefined) restoreWorkspace(target.filesystem, target.snapshot);
            else if (files !== undefined) populateWorkspace(target.filesystem, files);
            else throw new Error("Example files are unavailable");
            this.workspaceLoaded = true;
            this.resetTerminal();
            this.bootButton.hidden = false;
            this.state = "ready";
            await this.boot();
            return isCurrent();
        } catch (error: unknown) {
            if (isCurrent()) this.fail(error instanceof Error ? error.message : String(error));
            throw error;
        }
    }

    fit(): void {
        this.terminal.fit();
    }

    // Poweroff completion, rather than input delivery, makes snapshotting safe.
    private async shutdown(): Promise<void> {
        const runtime = this.runtime;
        if (runtime === undefined || !runtime.started) return;
        this.state = "stopping";
        this.updateControls();
        requiredElement("status").textContent = "Shutting down VM · Reset can force recovery";
        const stopped = new Promise<void>((resolve, reject) => { this.shutdownWaiter = { resolve, reject }; });
        const waiter = this.shutdownWaiter;
        void runtime.requestShutdown().catch((error: unknown) => {
            if (this.shutdownWaiter !== waiter) return;
            waiter?.reject(error instanceof Error ? error : new Error(String(error)));
            this.shutdownWaiter = undefined;
        });
        await stopped;
    }

    async forceHalt(): Promise<void> {
        this.clearInput();
        if (this.runtime?.started) await this.runtime.halt();
    }

    bootIfInactive(): void {
        if (switching) return;
        if (this.state === "ready") {
            void this.boot();
        } else if (this.state === "halted") {
            void this.bootRetained();
        } else if (this.state === "failed") {
            void this.reboot();
        } else if (this.state === "running") {
            this.terminal.focus();
        }
    }

    private resetTerminal(): void {
        this.clearInput();
        this.terminal.clear();
    }

    private async reboot(): Promise<void> {
        if (!this.runtime) throw new Error("Runtime is unavailable");
        this.clearInput();
        if (this.runtime.started) {
            this.state = "stopping";
            this.updateControls();
            requiredElement("status").textContent = "Rebooting VM · Reset can force recovery";
            try { await this.runtime.requestReboot(); }
            catch (error: unknown) {
                this.fail(error instanceof Error ? error.message : String(error));
                throw error;
            }
        }
        else await this.bootRetained();
    }

    private async bootRetained(): Promise<void> {
        if (!this.runtime) {
            this.fail("VM is unavailable");
            return;
        }
        this.state = "loading";
        this.updateControls();
        try {
            await this.runtime.boot();
        } catch (error: unknown) {
            this.fail(error instanceof Error ? error.message : String(error));
        }
    }

    private async boot(): Promise<void> {
        const target = this.target;
        if (target === undefined || this.state === "loading" || this.state === "running") {
            return;
        }
        const generation = ++this.generation;
        this.state = "loading";
        this.updateControls();
        requiredElement("status").textContent = `Loading VM · ${target.description.title}`;
        try {
            const runtime = this.runtime;
            if (runtime === undefined) throw new Error("Runtime is unavailable");
            if (generation !== this.generation) return;
            this.fit();
            await runtime.boot();
        } catch (error: unknown) {
            if (generation === this.generation) {
                this.fail(error instanceof Error ? error.message : String(error));
            }
        }
    }

    // One prepared platform owns the disk and share used by every example.
    async prepareRuntime(): Promise<RiscboxRuntime> {
        await this.terminal.ready;
        const response = await fetch("riscbox.wasm", { cache: "no-cache" });
        if (!response.ok) throw new Error(`WASM request failed with status ${response.status}`);
        const runtime = await window.Riscbox.instantiate(await response.arrayBuffer(), {
            debugTiming: true,
            consoleWrite: (text): void => { this.terminal.write(text); },
            consoleReset: (): void => this.resetTerminal(),
            onVmStarted: (): void => this.markRunning(),
            onVmHalted: (): void => {
                this.clearInput();
                const waiter = this.shutdownWaiter;
                this.shutdownWaiter = undefined;
                if (waiter !== undefined) { waiter.resolve(); return; }
                this.state = "halted";
                this.updateControls();
                requiredElement("status").textContent = `Halted · ${this.target?.description.title ?? "VM"}`;
            },
            onVmReset: (): void => this.markRunning(),
            onError: (error): void => this.fail(error instanceof Error ? error.message : String(error)),
        });
        this.runtime = runtime;
        const config = await window.Riscbox.loadResolvedConfig(new URL("riscbox.cfg", window.location.href).href);
        await runtime.prepareResolved(config, 256);
        return runtime;
    }

    private markRunning(): void {
        this.state = "running";
        this.updateControls();
        requiredElement("status").textContent = `Running · ${this.target?.description.title ?? "VM"}`;
        this.fit();
        this.runtime?.consoleResize(this.terminal.cols, this.terminal.rows);
        this.terminal.focus();
    }

    // Retire input waiting for an editor flush as well as already queued bytes.
    private clearInput(): void {
        this.inputGeneration += 1;
        this.input.clear();
    }

    private fail(message: string): void {
        this.clearInput();
        this.shutdownWaiter?.reject(new Error(message));
        this.shutdownWaiter = undefined;
        this.state = "failed";
        this.terminal.writeln(`\r\n${message}`);
        requiredElement("status").textContent = `VM failed · ${message}`;
        this.updateControls();
    }

    updateControls(): void {
        this.bootButton.disabled = switching || this.target === undefined || this.state === "loading" || this.state === "stopping";
        this.resetButton.disabled = this.target === undefined || this.state === "loading";
        this.bootButton.textContent = this.state === "running" || this.state === "failed"
            ? "Reboot VM"
            : "Boot VM";
    }
}

async function loadExamples(runtime: RiscboxRuntime): Promise<ExampleState[]> {
    const manifestResponse = await fetch("examples/examples.json");
    if (!manifestResponse.ok) {
        throw new Error(`Could not load examples: HTTP ${manifestResponse.status}`);
    }
    const manifest: unknown = await manifestResponse.json();
    if (!Array.isArray(manifest)) throw new Error("Example manifest must be an array");
    const descriptions = manifest.map(parseExample);
    const filesystem = runtime.filesystem("default");
    filesystem.subscribe((change: P9Change): void => {
        if (currentExample !== null) void handleFilesystemChange(currentExample, change).catch(reportUiError);
    });
    return descriptions.map(description => ({ description, filesystem }));
}

// Example downloads belong to the application. A populated share contains all
// bytes before boot, so guest and editor operations are always synchronous.
const exampleFiles = new Map<string, ReadonlyMap<string, Uint8Array>>();
async function loadExampleFiles(description: ExampleDescription): Promise<ReadonlyMap<string, Uint8Array>> {
    const cached = exampleFiles.get(description.id);
    if (cached) return cached;
    const base = new URL(`examples/${encodeURIComponent(description.id)}/`, window.location.href);
    const entries = await Promise.all(description.files.map(async (file): Promise<readonly [string, Uint8Array]> => {
        const path = file.path.split("/").map(encodeURIComponent).join("/");
        const response = await fetch(new URL(path, base));
        if (!response.ok) throw new Error(`Example file ${file.path}: HTTP ${response.status}`);
        const bytes = new Uint8Array(await response.arrayBuffer());
        if (bytes.length !== file.size) throw new Error(`Example file ${file.path}: incorrect size`);
        return [file.path, bytes];
    }));
    const files = new Map(entries);
    exampleFiles.set(description.id, files);
    return files;
}

function renderMenu(): void {
    const menu = requiredElement("menu-items");
    menu.replaceChildren();
    const label = document.createElement("span");
    label.classList.add("menu-label");
    label.textContent = examples.length === 1 ? "Example:" : "Examples:";
    menu.append(label);
    for (const example of examples) {
        const button = document.createElement("button");
        button.classList.add("example-button");
        button.textContent = example.description.title;
        button.disabled = example === currentExample;
        button.addEventListener("click", (): void => { void switchExample(example).catch(reportUiError); });
        menu.append(button);
    }
}

function switchExample(example: ExampleState, reset = false): Promise<void> {
    const generation = ++viewGeneration;
    for (const button of document.querySelectorAll<HTMLButtonElement>(".example-button")) {
        button.disabled = false;
    }
    // Forced recovery bypasses the queue to release an outstanding shutdown.
    // Namespace mutation still waits for the retired transition to finish.
    const stopped = reset ? vmController.forceHalt() : Promise.resolve();
    const selection = switchQueue.then(async () => {
        if (generation !== viewGeneration) return;
        await stopped;
        if (!reset) await syncEditor();
        switching = true;
        updateSyncButton();
        vmController.updateControls();
        const wasReadOnly = editor.state.readOnly;
        editor.dispatch({ effects: editable.reconfigure([
            EditorView.editable.of(false), EditorState.readOnly.of(true),
        ]) });
        let selected = false;
        try {
            if (await vmController.setTarget(example, reset, () => generation === viewGeneration)) {
                await showExample(example, generation);
                selected = true;
            }
        } finally {
            switching = false;
            // Successful selection sets the new editor's access mode itself.
            if (!selected) {
                editor.dispatch({ effects: editable.reconfigure([
                    EditorView.editable.of(!wasReadOnly), EditorState.readOnly.of(wasReadOnly),
                ]) });
            }
            updateSyncButton();
            vmController.updateControls();
        }
    });
    switchQueue = selection.then(() => undefined, reportUiError);
    return selection;
}

async function showExample(example: ExampleState, generation: number): Promise<void> {
    currentExample = example;
    currentPath = null;
    renderMenu();
    clearEditor();
    await Promise.all([renderFileTree(), updateInstructions()]);
    if (generation !== viewGeneration) return;
    const paths = example.filesystem.listFiles();
    if (generation !== viewGeneration) return;
    const preferred = paths.includes(example.description.editable)
        ? example.description.editable
        : paths[0];
    if (preferred !== undefined) {
        await openFile(preferred);
    }
    if (generation !== viewGeneration) return;
    requiredElement("status").textContent = `Running · ${example.description.title}`;
    if (paths.includes(DOC_PATH)) {
        selectTab("instructions");
    } else {
        selectTab("vm");
    }
    const url = new URL(window.location.href);
    url.searchParams.set("example", example.description.id);
    window.history.replaceState(null, "", url);
}

async function initialize(): Promise<void> {
    Split(["#file-tree-pane", "#editor-pane", "#info-pane"], {
        sizes: [10, 45, 45],
        gutterSize: 8,
        cursor: "grabbing",
        onDrag: (): void => vmController.fit(),
    });
    editor = new EditorView({
        state: EditorState.create({
            extensions: [
                basicSetup,
                keymap.of([{ key: "Tab", run: softTab }, ...defaultKeymap]),
                language.of([]),
                editable.of([
                    EditorView.editable.of(false),
                    EditorState.readOnly.of(true),
                ]),
                EditorView.domEventHandlers({ blur: (): void => { void syncEditor().catch(reportUiError); } }),
                EditorView.updateListener.of((update: ViewUpdate): void => {
                    if (update.docChanged && !programmaticEditorUpdate && !editor.state.readOnly) {
                        editorRevision += 1;
                        scheduleEditorFlush();
                        updateSyncButton();
                    }
                }),
            ],
        }),
        parent: requiredElement("editor-pane"),
    });
    vmController = new VmController(requiredElement("vm-terminal"), requiredButton("vm-boot-button"));
    requiredButton("sync-button").addEventListener("click", (): void => { void syncEditor().catch(reportUiError); });
    requiredButton("instructions-tab-button").addEventListener("click", (): void => selectTab("instructions"));
    requiredButton("vm-tab-button").addEventListener("click", (): void => selectTab("vm"));
    const runtime = await vmController.prepareRuntime();
    examples = await loadExamples(runtime);
    renderMenu();
    if (examples.length === 0) {
        throw new Error("No examples are configured");
    }
    const requested = new URL(window.location.href).searchParams.get("example");
    await switchExample(examples.find((example) => example.description.id === requested) ?? examples[0]);
}

document.addEventListener("DOMContentLoaded", (): void => {
    void initialize().catch((error: unknown): void => {
        console.error("Could not start the Risclet demo", error);
        requiredElement("status").textContent = error instanceof Error ? error.message : String(error);
    });
});
