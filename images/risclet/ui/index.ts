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
import { FitAddon, init as initializeGhostty, Terminal } from "ghostty-web";
import { basicSetup } from "codemirror";
import { Filesystem, createHttpsSeedPlugin, type FilesystemRuntime, type HttpsSeedFile, type P9Change } from "../../../js/p9";

interface ExampleDescription {
    readonly id: string;
    readonly title: string;
    readonly editable: string;
    readonly documentation?: string;
    readonly files: readonly HttpsSeedFile[];
}

interface ExampleState {
    readonly description: ExampleDescription;
    readonly filesystem: Filesystem;
}

interface FileTreeNode {
    isFile: boolean;
    fullPath: string;
    children: Record<string, FileTreeNode>;
}

interface FramebufferGeometry {
    readonly x: number;
    readonly y: number;
    readonly width: number;
    readonly height: number;
    readonly stride: number;
}

interface ResolvedVmConfig {
    readonly version: number;
    readonly machine: string;
    readonly memory_size: number;
}

interface RiscboxRuntime extends FilesystemRuntime {
    readonly started: boolean;
    startResolved(config: ResolvedVmConfig, memoryMiB: number): number;
    consoleInput(bytes: Uint8Array): void;
    consoleResize(columns: number, rows: number): void;
    boot(): Promise<void>;
    reset(): Promise<void>;
    halt(): Promise<void>;
    destroy(): Promise<void>;
}

interface RiscboxOptions {
    readonly debugTiming?: boolean;
    readonly consoleWrite: (text: string | Uint8Array) => void;
    readonly consoleReset?: () => void;
    readonly onVmStarted: () => void;
    readonly onVmHalted?: (cause: string) => void;
    readonly onVmReset?: (cause: string) => void;
    readonly onError: (error: unknown) => void;
    readonly framebufferRefresh?: (bytes: Uint8Array, geometry: FramebufferGeometry) => void;
}

interface RiscboxApi {
    instantiate(bytes: ArrayBuffer, options: RiscboxOptions): Promise<RiscboxRuntime>;
    loadResolvedConfig(url: string): Promise<ResolvedVmConfig>;
}

declare global {
    interface Window {
        Riscbox: RiscboxApi;
    }
}

const DOC_PATH = "doc/doc.md";
const SHOW_CURSOR = "\x1b[?25h";
const TERMINAL_THEME = {
    background: "#000000",
    foreground: "#c0c0c0",
    black: "#000000",
    red: "#ff0000",
    green: "#00ff00",
    yellow: "#ffff00",
    blue: "#0000ff",
    magenta: "#ff00ff",
    cyan: "#00ffff",
    white: "#ffffff",
    brightBlack: "#808080",
    brightRed: "#ff8080",
    brightGreen: "#80ff80",
    brightYellow: "#ffff80",
    brightBlue: "#8080ff",
    brightMagenta: "#ff80ff",
    brightCyan: "#80ffff",
    brightWhite: "#ffffff",
} as const;
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

let viewGeneration = 0;
let openGeneration = 0;
let treeGeneration = 0;
let instructionsGeneration = 0;
let switchQueue = Promise.resolve();
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
    const files = value.files.map((file: unknown): HttpsSeedFile => {
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
    currentPath = null;
    resetEditor("", false, "");
}

async function openFile(path: string): Promise<void> {
    const example = currentExample;
    const view = viewGeneration;
    const request = ++openGeneration;
    if (example === null) {
        return;
    }
    let content: Uint8Array;
    try {
        content = await example.filesystem.readFile(path);
    } catch (error: unknown) {
        if (view !== viewGeneration || request !== openGeneration) return;
        clearEditor();
        void renderFileTree().catch(reportUiError);
        reportUiError(error);
        return;
    }
    if (view !== viewGeneration || request !== openGeneration || example !== currentExample) return;
    currentPath = path;
    if (isBinaryFile(content)) {
        resetEditor("This file appears to be a binary file and cannot be displayed in the editor.", false, path);
    } else {
        resetEditor(editorTextFromFile(content), true, path);
    }
    void renderFileTree().catch(reportUiError);
    editor.focus();
}

async function syncEditor(): Promise<void> {
    if (currentExample === null || currentPath === null || editor.state.readOnly) {
        return;
    }
    await currentExample.filesystem.writeFile(currentPath, fileContentFromEditor(), EDITOR_ORIGIN);
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
    const paths = example === null ? [] : await example.filesystem.listFiles();
    if (view !== viewGeneration || request !== treeGeneration || example !== currentExample) return;
    if (currentPath !== null && !paths.includes(currentPath)) {
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

async function renderInstructions(filesystem: Filesystem): Promise<string> {
    if (!(await filesystem.listFiles()).includes(DOC_PATH)) {
        return "";
    }
    const document = markdownParser.parse(decoder.decode(await filesystem.readFile(DOC_PATH)));
    const documentUrl = new URL(DOC_PATH, "https://workspace.invalid/");
    const walker = document.walker();
    let event = walker.next();
    while (event !== null) {
        if (event.entering && event.node.type === "image" && event.node.destination !== null) {
            const url = new URL(event.node.destination, documentUrl);
            if (url.origin === documentUrl.origin) {
                const path = decodeURIComponent(url.pathname.replace(/^\//, ""));
                const content = await filesystem.readFile(path);
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
        vmController.bootIfInactive();
    }
}

async function updateInstructions(): Promise<void> {
    const view = viewGeneration;
    const request = ++instructionsGeneration;
    const example = currentExample;
    const button = requiredButton("instructions-tab-button");
    const content = requiredElement("instructions-tab-content");
    let rendered: string;
    try {
        rendered = example === null ? "" : await renderInstructions(example.filesystem);
    } catch (error: unknown) {
        if (view !== viewGeneration || request !== instructionsGeneration) return;
        throw error;
    }
    if (view !== viewGeneration || request !== instructionsGeneration || example !== currentExample) return;
    button.hidden = rendered === "";
    content.innerHTML = rendered;
    if (rendered === "" && content.classList.contains("active")) {
        selectTab("vm");
    }
}

async function handleFilesystemChange(example: ExampleState, change: P9Change): Promise<void> {
    if (example !== currentExample) {
        return;
    }
    if (change.kind === "rename" && change.oldPath === currentPath) {
        currentPath = change.path;
    }
    if (change.kind !== "write") {
        void renderFileTree().catch(reportUiError);
    }
    if (change.path === DOC_PATH
        || (change.kind === "rename" && change.oldPath === DOC_PATH)
        || change.kind === "reset" || change.kind === "rescan") {
        void updateInstructions().catch(reportUiError);
    }
    if (currentPath !== null && !(change.source === "host" && change.origin === EDITOR_ORIGIN)
        && (change.path === currentPath
            || change.aliases.includes(currentPath)
            || change.kind === "reset" || change.kind === "rescan"
            || (change.kind === "rename" && change.oldPath === currentPath))) {
        const view = viewGeneration;
        const path = currentPath;
        const paths = await example.filesystem.listFiles();
        if (view !== viewGeneration || example !== currentExample || currentPath !== path) return;
        if (paths.includes(currentPath)) {
            await openFile(currentPath);
        } else {
            clearEditor();
        }
    }
}

class VmController {
    private readonly bootButton: HTMLButtonElement;
    private readonly fitAddon = new FitAddon();
    private readonly terminal: Terminal;
    private runtime: RiscboxRuntime | undefined;
    private target: ExampleState | undefined;
    private generation = 0;
    private state: "ready" | "loading" | "running" | "halted" | "failed" = "ready";

    constructor(host: HTMLElement, bootButton: HTMLButtonElement) {
        this.bootButton = bootButton;
        this.terminal = new Terminal({
            convertEol: false,
            cursorBlink: true,
            fontFamily: '"Latin Modern Mono", monospace',
            fontSize: 18,
            scrollback: 1000,
            theme: TERMINAL_THEME,
        });
        this.terminal.loadAddon(this.fitAddon);
        this.terminal.open(host);
        this.fitAddon.fit();
        this.terminal.onData((text: string): void => {
            this.runtime?.consoleInput(encoder.encode(text));
        });
        this.terminal.onResize(({ cols, rows }): void => {
            this.runtime?.consoleResize(cols, rows);
        });
        this.bootButton.addEventListener("click", (): void => {
            if (this.state === "ready") {
                void this.boot();
            } else if (this.state === "halted") {
                void this.bootRetained();
            } else {
                void this.reboot();
            }
        });
        new ResizeObserver((): void => this.fit()).observe(host);
        this.updateControls();
    }

    async setTarget(target: ExampleState): Promise<void> {
        await this.stop();
        this.target = target;
        this.resetTerminal();
        this.bootButton.hidden = false;
        this.state = "ready";
        this.updateControls();
    }

    fit(): void {
        this.fitAddon.fit();
    }

    bootIfInactive(): void {
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

    private async stop(): Promise<void> {
        this.generation += 1;
        const runtime = this.runtime;
        this.state = "ready";
        if (runtime) {
            if (runtime.started) await runtime.halt();
            await runtime.destroy();
        }
        this.state = "ready";
    }

    private resetTerminal(): void {
        this.terminal.reset();
        this.terminal.scrollToBottom();
        this.terminal.write(SHOW_CURSOR);
    }

    private async reboot(): Promise<void> {
        if (this.state === "running" && this.runtime) {
            try {
                await this.runtime.reset();
            } catch (error: unknown) {
                this.fail(error instanceof Error ? error.message : String(error));
            }
            return;
        }
        await this.stop();
        this.resetTerminal();
        await this.boot();
    }

    private async bootRetained(): Promise<void> {
        if (!this.runtime) {
            this.fail("VM is unavailable");
            return;
        }
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
            const config = await window.Riscbox.loadResolvedConfig(new URL("riscbox.cfg", window.location.href).href);
            if (generation !== this.generation) return;
            await target.filesystem.bind("default");
            if (generation !== this.generation) return;
            this.fit();
            const result = runtime.startResolved(config, 256);
            if (result !== 0) {
                throw new Error("Riscbox rejected the VM configuration");
            }
        } catch (error: unknown) {
            if (generation === this.generation) {
                this.fail(error instanceof Error ? error.message : String(error));
            }
        }
    }

    // The runtime and host namespaces exist before any guest starts. VM teardown
    // releases machine state while each example retains its filesystem handle.
    async prepareRuntime(): Promise<RiscboxRuntime> {
        const response = await fetch("riscbox.wasm", { cache: "no-cache" });
        if (!response.ok) throw new Error(`WASM request failed with status ${response.status}`);
        const runtime = await window.Riscbox.instantiate(await response.arrayBuffer(), {
            debugTiming: true,
            consoleWrite: (text): void => { this.terminal.write(text); },
            consoleReset: (): void => this.resetTerminal(),
            onVmStarted: (): void => this.markRunning(),
            onVmHalted: (): void => {
                this.state = "halted";
                this.updateControls();
                requiredElement("status").textContent = `Halted · ${this.target?.description.title ?? "VM"}`;
            },
            onVmReset: (): void => this.markRunning(),
            onError: (error): void => this.fail(error instanceof Error ? error.message : String(error)),
        });
        this.runtime = runtime;
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

    private fail(message: string): void {
        this.state = "failed";
        this.terminal.writeln(`\r\n${message}`);
        requiredElement("status").textContent = `VM failed · ${message}`;
        this.updateControls();
    }

    private updateControls(): void {
        this.bootButton.disabled = this.target === undefined || this.state === "loading";
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
    return Promise.all(descriptions.map(async (description): Promise<ExampleState> => {
        const files = description.files.map(file => ({ ...file, path: normalizeRelativePath(file.path) }));
        const base = new URL(`examples/${encodeURIComponent(description.id)}/`, window.location.href);
        const filesystem = await Filesystem.create(runtime);
        await filesystem.installSeed(createHttpsSeedPlugin({ files }, base));
        const state = { description, filesystem };
        await filesystem.subscribe((change: P9Change): void => {
            void handleFilesystemChange(state, change).catch(reportUiError);
        });
        return state;
    }));
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

function switchExample(example: ExampleState): Promise<void> {
    const generation = ++viewGeneration;
    // Serialize machine teardown and selection while old body reads can finish
    // independently. Only the newest view request may update the visible panes.
    const selection = switchQueue.then(async () => {
        if (generation !== viewGeneration) return false;
        await syncEditor();
        await vmController.setTarget(example);
        return generation === viewGeneration;
    });
    switchQueue = selection.then(() => undefined, reportUiError);
    return selection.then(async selected => {
        if (selected) await showExample(example, generation);
    });
}

async function showExample(example: ExampleState, generation: number): Promise<void> {
    currentExample = example;
    currentPath = null;
    renderMenu();
    clearEditor();
    await Promise.all([renderFileTree(), updateInstructions()]);
    if (generation !== viewGeneration) return;
    const paths = await example.filesystem.listFiles();
    if (generation !== viewGeneration) return;
    const preferred = paths.includes(example.description.editable)
        ? example.description.editable
        : paths[0];
    if (preferred !== undefined) {
        await openFile(preferred);
    }
    if (generation !== viewGeneration) return;
    requiredElement("status").textContent = `Ready · ${example.description.title}`;
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
    await initializeGhostty();
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
                    if (update.docChanged && !programmaticEditorUpdate) {
                        void syncEditor().catch(reportUiError);
                    }
                }),
            ],
        }),
        parent: requiredElement("editor-pane"),
    });
    vmController = new VmController(requiredElement("vm-terminal"), requiredButton("vm-boot-button"));
    requiredButton("instructions-tab-button").addEventListener("click", (): void => selectTab("instructions"));
    requiredButton("vm-tab-button").addEventListener("click", (): void => selectTab("vm"));
    const runtime = await vmController.prepareRuntime();
    examples = await loadExamples(runtime);
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
