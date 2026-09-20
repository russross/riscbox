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
import { Memory9PServer, P9Change } from "../../../js/p9";

interface ExampleDescription {
    readonly id: string;
    readonly title: string;
    readonly editable: string;
    readonly documentation?: string;
    readonly files: readonly string[];
}

interface ExampleState {
    readonly description: ExampleDescription;
    readonly filesystem: Memory9PServer;
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

interface RiscboxRuntime {
    start(configUrl: string, memoryMiB: number): number;
    run(): void;
    consoleInput(bytes: Uint8Array): void;
    consoleResize(columns: number, rows: number): void;
}

interface RiscboxOptions {
    readonly p9Servers: ReadonlyMap<string, {
        connect(): {
            request(bytes: Uint8Array, replyCapacity: number): Promise<
                | { readonly kind: "reply"; readonly bytes: Uint8Array }
                | { readonly kind: "suppressed" }
            >;
            close(): void;
        };
    }>;
    readonly consoleWrite: (text: string | Uint8Array) => void;
    readonly onVmStarted: () => void;
    readonly onError: (error: unknown) => void;
    readonly schedule: (milliseconds: number) => void;
    readonly framebufferRefresh?: (bytes: Uint8Array, geometry: FramebufferGeometry) => void;
}

interface RiscboxApi {
    instantiate(bytes: ArrayBuffer, options: RiscboxOptions): Promise<RiscboxRuntime>;
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
    const trimmed = raw.trim();
    const parts = trimmed.split("/");
    if (trimmed === "" || trimmed.startsWith("/")
        || parts.some((part) => part === "" || part === "." || part === "..")) {
        throw new Error(`Invalid example path: ${JSON.stringify(raw)}`);
    }
    return parts.join("/");
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

function openFile(path: string): void {
    const example = currentExample;
    if (example === null) {
        return;
    }
    let content: Uint8Array;
    try {
        content = example.filesystem.readFile(path);
    } catch {
        clearEditor();
        renderFileTree();
        return;
    }
    currentPath = path;
    if (isBinaryFile(content)) {
        resetEditor("This file appears to be a binary file and cannot be displayed in the editor.", false, path);
    } else {
        resetEditor(editorTextFromFile(content), true, path);
    }
    renderFileTree();
    editor.focus();
}

function syncEditor(): void {
    if (currentExample === null || currentPath === null || editor.state.readOnly) {
        return;
    }
    currentExample.filesystem.writeFile(currentPath, fileContentFromEditor(), "editor");
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
                syncEditor();
                openFile(item.fullPath);
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

function renderFileTree(): void {
    const pane = requiredElement("file-tree-pane");
    const paths = currentExample?.filesystem.listFiles() ?? [];
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

function renderInstructions(): string {
    const filesystem = currentExample?.filesystem;
    if (filesystem === undefined || !filesystem.listFiles().includes(DOC_PATH)) {
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
        vmController.bootIfInactive();
    }
}

function updateInstructions(): void {
    const button = requiredButton("instructions-tab-button");
    const content = requiredElement("instructions-tab-content");
    const rendered = renderInstructions();
    button.hidden = rendered === "";
    content.innerHTML = rendered;
    if (rendered === "" && content.classList.contains("active")) {
        selectTab("vm");
    }
}

function handleFilesystemChange(example: ExampleState, change: P9Change): void {
    if (example !== currentExample) {
        return;
    }
    if (change.kind === "rename" && change.oldPath === currentPath) {
        currentPath = change.path;
    }
    if (change.kind !== "write") {
        renderFileTree();
    }
    if (change.path === DOC_PATH
        || (change.kind === "rename" && change.oldPath === DOC_PATH)
        || change.kind === "reset") {
        updateInstructions();
    }
    if (currentPath !== null && change.source !== "editor"
        && (change.path === currentPath
            || (change.kind === "rename" && change.oldPath === currentPath))) {
        const paths = example.filesystem.listFiles();
        if (paths.includes(currentPath)) {
            openFile(currentPath);
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
    private timer: number | undefined;
    private generation = 0;
    private state: "ready" | "loading" | "running" | "failed" = "ready";

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
            } else {
                void this.reboot();
            }
        });
        new ResizeObserver((): void => this.fit()).observe(host);
        this.updateControls();
    }

    setTarget(target: ExampleState): void {
        this.stop();
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
        } else if (this.state === "failed") {
            void this.reboot();
        } else if (this.state === "running") {
            this.terminal.focus();
        }
    }

    private stop(): void {
        this.generation += 1;
        if (this.timer !== undefined) {
            window.clearTimeout(this.timer);
        }
        this.timer = undefined;
        this.runtime = undefined;
        this.state = "ready";
    }

    private resetTerminal(): void {
        this.terminal.reset();
        this.terminal.write(SHOW_CURSOR);
    }

    private async reboot(): Promise<void> {
        this.stop();
        this.resetTerminal();
        await this.boot();
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
            const response = await fetch("riscbox.wasm", { cache: "no-cache" });
            if (!response.ok) {
                throw new Error(`WASM request failed with status ${response.status}`);
            }
            const runtime = await window.Riscbox.instantiate(await response.arrayBuffer(), {
                p9Servers: new Map([["default", target.filesystem]]),
                consoleWrite: (text: string | Uint8Array): void => {
                    if (generation === this.generation) {
                        this.terminal.write(text);
                    }
                },
                onVmStarted: (): void => {
                    if (generation !== this.generation) {
                        return;
                    }
                    this.state = "running";
                    this.updateControls();
                    requiredElement("status").textContent = `Running · ${target.description.title}`;
                    this.fit();
                    runtime.consoleResize(this.terminal.cols, this.terminal.rows);
                    this.terminal.focus();
                },
                onError: (error: unknown): void => {
                    if (generation === this.generation) {
                        this.fail(error instanceof Error ? error.message : String(error));
                    }
                },
                schedule: (milliseconds: number): void => {
                    if (generation !== this.generation) {
                        return;
                    }
                    if (this.timer !== undefined) {
                        window.clearTimeout(this.timer);
                    }
                    this.timer = window.setTimeout((): void => {
                        this.timer = undefined;
                        if (generation === this.generation) {
                            runtime.run();
                        }
                    }, milliseconds);
                },
            });
            if (generation !== this.generation) {
                return;
            }
            this.runtime = runtime;
            this.fit();
            const result = runtime.start(new URL("riscbox.cfg", window.location.href).href, 256);
            if (result !== 0) {
                throw new Error("Riscbox rejected the VM configuration");
            }
        } catch (error: unknown) {
            if (generation === this.generation) {
                this.fail(error instanceof Error ? error.message : String(error));
            }
        }
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

async function loadExamples(): Promise<ExampleState[]> {
    const manifestResponse = await fetch("examples/examples.json");
    if (!manifestResponse.ok) {
        throw new Error(`Could not load examples: HTTP ${manifestResponse.status}`);
    }
    const descriptions: ExampleDescription[] = await manifestResponse.json();
    return Promise.all(descriptions.map(async (description): Promise<ExampleState> => {
        const entries = await Promise.all(description.files.map(async (rawPath) => {
            const path = normalizeRelativePath(rawPath);
            const response = await fetch(`examples/${encodeURIComponent(description.id)}/${path}`);
            if (!response.ok) {
                throw new Error(`Could not load ${path}: HTTP ${response.status}`);
            }
            return [path, new Uint8Array(await response.arrayBuffer())] as const;
        }));
        const filesystem = new Memory9PServer();
        filesystem.loadFiles(Object.fromEntries(entries));
        const state = { description, filesystem };
        filesystem.subscribe((change: P9Change): void => handleFilesystemChange(state, change));
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
        button.addEventListener("click", (): void => switchExample(example));
        menu.append(button);
    }
}

function switchExample(example: ExampleState): void {
    syncEditor();
    currentExample = example;
    currentPath = null;
    vmController.setTarget(example);
    renderMenu();
    renderFileTree();
    updateInstructions();
    clearEditor();
    const paths = example.filesystem.listFiles();
    const preferred = paths.includes(example.description.editable)
        ? example.description.editable
        : paths[0];
    if (preferred !== undefined) {
        openFile(preferred);
    }
    requiredElement("status").textContent = `Ready · ${example.description.title}`;
    if (example.filesystem.listFiles().includes(DOC_PATH)) {
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
                EditorView.domEventHandlers({ blur: (): void => syncEditor() }),
                EditorView.updateListener.of((update: ViewUpdate): void => {
                    if (update.docChanged && !programmaticEditorUpdate) {
                        syncEditor();
                    }
                }),
            ],
        }),
        parent: requiredElement("editor-pane"),
    });
    vmController = new VmController(requiredElement("vm-terminal"), requiredButton("vm-boot-button"));
    requiredButton("instructions-tab-button").addEventListener("click", (): void => selectTab("instructions"));
    requiredButton("vm-tab-button").addEventListener("click", (): void => selectTab("vm"));
    examples = await loadExamples();
    if (examples.length === 0) {
        throw new Error("No examples are configured");
    }
    const requested = new URL(window.location.href).searchParams.get("example");
    switchExample(examples.find((example) => example.description.id === requested) ?? examples[0]);
}

document.addEventListener("DOMContentLoaded", (): void => {
    void initialize().catch((error: unknown): void => {
        console.error("Could not start the Risclet demo", error);
        requiredElement("status").textContent = error instanceof Error ? error.message : String(error);
    });
});
