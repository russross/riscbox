import { Memory9PServer } from "./p9.js";

const DOC_PATH = "doc/doc.md";
const decoder = new TextDecoder();
const encoder = new TextEncoder();
const element = (selector) => document.querySelector(selector);
const ui = {
  example: element("#example"), files: element("#files"), editor: element("#editor"),
  editorTitle: element("#editor-title"), status: element("#status"),
  instructions: element("#instructions"), instructionsTab: element("#instructions-tab"),
  vmTab: element("#vm-tab"), vm: element("#vm"), terminal: element("#terminal"),
};
let currentPath = null;
let runtime = null;

function escapeHtml(text) {
  return text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

function renderMarkdown(source) {
  const codeBlocks = [];
  let text = escapeHtml(source).replace(/```([^\n]*)\n([\s\S]*?)```/g, (_, language, code) => {
    const index = codeBlocks.push(`<pre><code data-language="${language.trim()}">${code}</code></pre>`) - 1;
    return `\u0000${index}\u0000`;
  });
  text = text.replace(/^(.+)\n={3,}$/gm, "<h1>$1</h1>").replace(/^(.+)\n-{3,}$/gm, "<h2>$1</h2>")
    .replace(/^### (.+)$/gm, "<h3>$1</h3>").replace(/^## (.+)$/gm, "<h2>$1</h2>")
    .replace(/^# (.+)$/gm, "<h1>$1</h1>").replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  text = text.split(/\n{2,}/).map((part) => {
    if (/^<(?:h[1-3]|pre)/.test(part) || /^\u0000\d+\u0000$/.test(part)) return part;
    if (/^(?:\*   |[-*] )/m.test(part)) return `<ul>${part.split("\n").map((line) => `<li>${line.replace(/^(?:\*   |[-*] )/, "")}</li>`).join("")}</ul>`;
    return `<p>${part.replaceAll("\n", "<br>")}</p>`;
  }).join("\n");
  return text.replace(/\u0000(\d+)\u0000/g, (_, index) => codeBlocks[Number(index)]);
}

function selectTab(name) {
  const instructions = name === "instructions" && !ui.instructionsTab.hidden;
  ui.instructionsTab.setAttribute("aria-selected", String(instructions));
  ui.vmTab.setAttribute("aria-selected", String(!instructions));
  ui.instructions.hidden = !instructions;
  ui.vm.hidden = instructions;
  if (!instructions) ui.terminal.focus();
}

function updateInstructions(server) {
  const exists = server.listFiles().includes(DOC_PATH);
  ui.instructionsTab.hidden = !exists;
  if (exists) ui.instructions.innerHTML = renderMarkdown(decoder.decode(server.readFile(DOC_PATH)));
  else { ui.instructions.replaceChildren(); selectTab("vm"); }
}

function fileTree(paths) {
  const root = {};
  for (const path of paths) {
    let node = root;
    for (const part of path.split("/")) node = node[part] ??= {};
    node.$path = path;
  }
  return root;
}

function renderTreeNode(node, parent, depth, server) {
  const names = Object.keys(node).filter((name) => name !== "$path").sort((left, right) => {
    const leftFile = "$path" in node[left]; const rightFile = "$path" in node[right];
    return leftFile === rightFile ? left.localeCompare(right) : leftFile ? 1 : -1;
  });
  for (const name of names) {
    const child = node[name]; const item = document.createElement("li");
    if (child.$path) {
      const button = document.createElement("button");
      button.type = "button"; button.textContent = name; button.dataset.path = child.$path;
      button.style.setProperty("--depth", depth); button.classList.toggle("selected", child.$path === currentPath);
      button.addEventListener("click", () => openFile(server, child.$path)); item.append(button);
    } else {
      const label = document.createElement("span"); label.className = "folder-label"; label.textContent = name;
      label.style.setProperty("--depth", depth); item.append(label);
      const list = document.createElement("ul"); renderTreeNode(child, list, depth + 1, server); item.append(list);
    }
    parent.append(item);
  }
}

function renderFileTree(server) {
  const paths = server.listFiles();
  if (currentPath !== null && !paths.includes(currentPath)) {
    currentPath = null; ui.editor.value = ""; ui.editor.disabled = true; ui.editorTitle.textContent = "Select a file";
  }
  const list = document.createElement("ul"); list.className = "file-tree";
  renderTreeNode(fileTree(paths), list, 0, server); ui.files.replaceChildren(list);
}

function openFile(server, path) {
  currentPath = path;
  try { ui.editor.value = decoder.decode(server.readFile(path)); ui.editor.disabled = false; }
  catch { currentPath = null; ui.editor.value = ""; ui.editor.disabled = true; }
  ui.editorTitle.textContent = currentPath ?? "Select a file"; renderFileTree(server); ui.editor.focus();
}

async function loadExamples() {
  const response = await fetch("examples/examples.json");
  if (!response.ok) throw new Error(`could not load examples: HTTP ${response.status}`);
  return response.json();
}

async function loadExample(example) {
  const entries = await Promise.all(example.files.map(async (path) => {
    const response = await fetch(`examples/${example.id}/${path}`);
    if (!response.ok) throw new Error(`could not load ${path}: HTTP ${response.status}`);
    return [path, new Uint8Array(await response.arrayBuffer())];
  }));
  const server = new Memory9PServer();
  server.loadFiles(Object.fromEntries(entries));
  return server;
}

function consoleText(event) {
  const special = new Map([["Enter", "\r"], ["Backspace", "\x7f"], ["Tab", "\t"], ["Escape", "\x1b"], ["ArrowUp", "\x1b[A"], ["ArrowDown", "\x1b[B"], ["ArrowRight", "\x1b[C"], ["ArrowLeft", "\x1b[D"], ["Delete", "\x1b[3~"]]);
  if (event.ctrlKey && !event.altKey && !event.metaKey && event.key.length === 1) return String.fromCharCode(event.key.toUpperCase().charCodeAt(0) & 31);
  return special.get(event.key) ?? (!event.altKey && !event.metaKey && event.key.length === 1 ? event.key : null);
}

async function main() {
  const examples = await loadExamples();
  for (const example of examples) ui.example.add(new Option(example.title, example.id));
  const requested = new URL(location.href).searchParams.get("example");
  const example = examples.find(({ id }) => id === requested) ?? examples[0]; ui.example.value = example.id;
  ui.example.addEventListener("change", () => { const url = new URL(location.href); url.searchParams.set("example", ui.example.value); location.href = url.href; });
  const server = await loadExample(example); renderFileTree(server); updateInstructions(server);
  const preferred = server.listFiles().includes(example.editable) ? example.editable : server.listFiles()[0];
  if (preferred) openFile(server, preferred);
  server.subscribe((change) => {
    if (change.kind === "rename" && change.oldPath === currentPath) currentPath = change.path;
    renderFileTree(server);
    if (change.path === DOC_PATH || change.oldPath === DOC_PATH || change.kind === "reset") updateInstructions(server);
    if (currentPath !== null && change.source === "guest" && (change.path === currentPath || change.oldPath === currentPath) && server.listFiles().includes(currentPath)) {
      ui.editor.value = decoder.decode(server.readFile(currentPath));
    }
  });
  const syncEditor = () => { if (currentPath !== null) server.writeFile(currentPath, ui.editor.value, "editor"); };
  ui.editor.addEventListener("input", syncEditor); ui.editor.addEventListener("blur", syncEditor);
  ui.instructionsTab.addEventListener("click", () => selectTab("instructions")); ui.vmTab.addEventListener("click", () => selectTab("vm"));
  ui.terminal.addEventListener("keydown", (event) => { const text = consoleText(event); if (text === null || runtime === null) return; event.preventDefault(); runtime.consoleInput(encoder.encode(text)); });
  const response = await fetch("./riscbox.wasm"); if (!response.ok) throw new Error(`WASM request failed with status ${response.status}`);
  runtime = await Riscbox.instantiate(await response.arrayBuffer(), {
    p9Server: server,
    consoleWrite(text) { ui.terminal.append(document.createTextNode(text)); ui.terminal.scrollTop = ui.terminal.scrollHeight; },
    onVmStarted() { ui.status.textContent = `Running · ${example.title}`; },
    onError(error) { ui.status.textContent = String(error); console.error(error); },
  });
  ui.status.textContent = "Loading VM assets…";
  if (runtime.start(new URL("riscbox.cfg", location.href).href, 256) !== 0) throw new Error("Riscbox rejected the VM configuration");
}

main().catch((error) => { ui.status.textContent = `Could not start the demo: ${error}`; console.error(error); });
