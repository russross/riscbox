import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { runChromePage } from "./chrome.mjs";

// Documentation stays navigable as rendered HTML, with the reference typography.
test("demo documentation renders styled HTML and links between guides", { timeout: 60_000 }, async () => {
    const directory = await mkdtemp(join(tmpdir(), "riscbox-demo-docs-"));
    try {
        await runChromePage(`<!doctype html><iframe src="/riscbox/docs/riscbox/API.html" style="width:1000px;height:700px"></iframe><script type="module">
const frame = document.querySelector('iframe');
const check = (condition, message) => { if (!condition) throw new Error(message); };
try {
    await new Promise(resolve => frame.addEventListener('load', resolve));
    const doc = frame.contentDocument;
    await doc.fonts.ready;
    check(doc.querySelector('h1')?.textContent.includes('API'), 'rendered API heading');
    check(frame.contentWindow.getComputedStyle(doc.body).fontSize === '17px', 'reference body typography');
    check(frame.contentWindow.getComputedStyle(doc.querySelector('h1')).color === 'rgb(0, 77, 0)', 'reference heading color');
    for (const family of ['CMU Serif', 'CMU Sans Serif', 'CMU Typewriter Text']) {
        check([...doc.fonts].some(font => font.family === family && font.status === 'loaded'), 'CDN font loaded: ' + family);
    }
    frame.style.width = '350px';
    check(frame.contentWindow.getComputedStyle(doc.documentElement).fontSize === '17px', 'reference root typography on mobile');
    check(doc.body.getBoundingClientRect().width <= 350, 'mobile document fits viewport');

    // Follow every local guide link from the app and from its rendered documents.
    const index = new DOMParser().parseFromString(await (await fetch('/riscbox/index.html')).text(), 'text/html');
    const pending = [...index.querySelectorAll('a[href]')].map(link => new URL(link.getAttribute('href'), location.origin + '/riscbox/'));
    const visited = new Set();
    while (pending.length) {
        const url = pending.pop();
        check(!url.pathname.endsWith('.md'), 'local link still targets Markdown: ' + url);
        if (visited.has(url.pathname)) continue;
        visited.add(url.pathname);
        const response = await fetch(url);
        check(response.ok, 'missing documentation: ' + url);
        const page = new DOMParser().parseFromString(await response.text(), 'text/html');
        check(page.querySelector('h1'), 'missing rendered content: ' + url);
        for (const link of page.querySelectorAll('a[href]')) {
            const target = new URL(link.getAttribute('href'), url);
            if (target.origin === location.origin && target.pathname.startsWith('/riscbox/docs/')) pending.push(target);
        }
    }
    check(visited.has('/riscbox/docs/examples/README.html'), 'rendered provenance');
    check(visited.has('/riscbox/docs/riscbox/STORAGE-ABI.html'), 'linked storage guide');
    await fetch('/result?status=pass');
} catch (error) { await fetch('/result?status=' + encodeURIComponent(error.stack ?? String(error))); }
</script>`, directory, { root: resolve(import.meta.dirname, "../demo/dist"), basePath: "/riscbox", timeoutMs: 50_000 });
    } finally { await rm(directory, { recursive: true, force: true }); }
});

// The page serves only the assembled application, including its unchanged release tree.
test("release demo boots, shares files, compiles games, and exercises lifecycle controls", { timeout: 300_000 }, async () => {
    const directory = await mkdtemp(join(tmpdir(), "riscbox-demo-"));
    try {
        await runChromePage(`<!doctype html><iframe src="/riscbox/index.html"></iframe><script type="module">
const frame = document.querySelector('iframe');
const sleep = () => new Promise(resolve => setTimeout(resolve, 100));
const check = (condition, message) => { if (!condition) throw new Error(message); };
let app;
async function until(condition, message) {
    const deadline = performance.now() + 60000;
    while (!await condition()) {
        if (performance.now() > deadline) throw new Error(message + ': ' + frame.contentDocument?.getElementById('message')?.textContent);
        await sleep();
    }
}
try {
    await until(() => frame.contentDocument?.getElementById('state')?.textContent === 'running', 'startup');
    app = await frame.contentWindow.eval('import("/riscbox/app.js")');
    const doc = frame.contentDocument;
    let captured = '';
    async function text() {
        try {
            captured = (await app.terminal.readText()).split('\\n').map(line => line.trim()).join('\\n');
            return captured;
        }
        catch (error) { if (error.name === 'AbortError') return captured; throw error; }
    }
    async function prompt() { await until(async () => (await text()).includes('demo@riscbox:'), 'login'); }
    async function click(id) {
        check(!doc.getElementById(id).disabled, id + ' disabled');
        if (['boot', 'reset', 'reboot', 'image-reset'].includes(id)) {
            doc.getElementById('clear-terminal').click();
            captured = '';
        }
        doc.getElementById(id).click();
        await until(() => !doc.getElementById('vm-controls').disabled, id + ' completion');
        if (['boot', 'reset', 'reboot', 'image-reset'].includes(id)) await prompt();
    }
    let serial = 0;
    async function command(command, expected) {
        const marker = 'DEMO_OK_' + ++serial;
        const start = 'DEMO_BEGIN_' + serial;
        app.terminal.onData('echo ' + start + '; ' + command + '; echo; echo ' + marker + '\\r');
        await until(async () => (await text()).includes('\\n' + marker), command);
        const output = captured.slice(captured.indexOf('\\n' + start + '\\n') + start.length + 2, captured.lastIndexOf('\\n' + marker));
        check(output.includes(expected), 'missing output ' + expected + ': ' + output);
        return output;
    }
    await prompt();
    const initialOutput = await command('id; doas id; mount | grep overlay; make; ./arithmetic </dev/null', 'uid=1000(demo)');
    check(initialOutput.includes('uid=0(root)'), 'passwordless doas');
    check(initialOutput.includes('type overlay'), 'tmpfs overlays');
    check(initialOutput.includes(' ='), 'arithmetic compiled and executed');
    const files = doc.getElementById('files');
    const hasFile = path => [...files.options].some(option => option.value === path);
    check(!doc.getElementById('refresh-tree'), 'tree has no manual refresh control');
    await until(() => hasFile('arithmetic'), 'guest build updates tree');
    doc.getElementById('files').value = 'Makefile';
    const initial = app.editor.state.doc.toString();
    check(initial.includes('Choose a file'), 'selection must not copy');
    await click('read-file');
    const original = app.editor.state.doc.toString();
    await command('mkdir -p nested/deeper; echo guest > nested/deeper/file.txt', '');
    await until(() => hasFile('nested/deeper/file.txt'), 'guest creation updates tree');
    const nestedFile = [...files.options].find(option => option.value === 'nested/deeper/file.txt');
    check(nestedFile.textContent.startsWith('\\u00a0'.repeat(4)), 'nested file indentation');
    check(frame.contentWindow.getComputedStyle(nestedFile).whiteSpace === 'pre', 'indentation retains whitespace');
    check(files.value === 'Makefile', 'guest changes preserve selection');
    check(app.editor.state.doc.toString() === original, 'guest changes do not copy into editor');
    await command('mv nested/deeper/file.txt nested/deeper/renamed.txt', '');
    await until(() => hasFile('nested/deeper/renamed.txt') && !hasFile('nested/deeper/file.txt'), 'guest rename updates tree');
    files.value = 'nested/deeper/renamed.txt';
    await command('rm -r nested', '');
    await until(() => !hasFile('nested'), 'guest removal updates tree');
    check(files.value === '', 'removed selection is cleared');
    app.workspace.writeFile('host-created.txt', 'host');
    await until(() => hasFile('host-created.txt'), 'host writes update tree');
    files.value = 'Makefile';
    app.editor.dispatch({changes: {from: 0, insert: '# explicit save\\n'}});
    check(new TextDecoder().decode(app.workspace.readFile('Makefile')) === original, 'edit must not save');
    await click('save-file');
    await command('head -1 Makefile; echo retained > /home/demo/marker', '# explicit save');
    await click('reboot');
    const rebootOutput = await command('test ! -e /home/demo/marker && echo TMPFS_CLEAN; head -1 Makefile', 'TMPFS_CLEAN');
    check(rebootOutput.includes('# explicit save'), 'reboot retains 9p edits');
    await click('halt');
    check(doc.getElementById('state').textContent === 'halted', 'halt');
    const disk = app.runtime.block(0);
    const originalSector = await disk.read(0n, 512);
    const changedSector = originalSector.slice();
    changedSector[0] ^= 1;
    disk.write(0n, changedSector);
    doc.getElementById('source-tree').value = 'number';
    await click('load-tree');
    await click('boot');
    await command('make; ./number 123', 'one hundred twenty-three.');
    await click('shutdown');
    await until(() => doc.getElementById('state').textContent === 'halted', 'soft shutdown');
    doc.getElementById('source-tree').value = 'wump';
    await click('load-tree');
    await click('boot');
    await command("make; printf 'n\\\\nq\\\\n' | ./wump", 'Wumpus');
    await click('reset');
    await command('test -f wump && echo SHARE_RETAINED', 'SHARE_RETAINED');
    await click('image-reset');
    const resetOutput = await command('test ! -e /home/demo/marker && echo IMAGE_CLEAN; test -f wump && echo SHARE_KEPT', 'IMAGE_CLEAN');
    check(resetOutput.includes('SHARE_KEPT'), 'image reset retains share');
    await click('halt');
    check((await disk.read(0n, 512))[0] === originalSector[0], 'image reset discards HTTP overlay');
    await click('share-reset');
    check(!app.workspace.listDirectory('').some(entry => entry.name === 'wump'), 'share reset removes built binary');
    await click('share-clear');
    check(app.workspace.listDirectory('').length === 0, 'share clear');
    await until(() => files.options.length === 0, 'share clear updates tree');
    const retired = app.workspace;
    await click('destroy');
    let invalid = false;
    try { retired.listDirectory(''); } catch { invalid = true; }
    check(invalid, 'destroy invalidates share');
    await click('prepare');
    await click('share-reset');
    await until(() => hasFile('Makefile'), 'replacement VM has a tree subscription');
    await click('boot');
    await command('echo RECREATED', 'RECREATED');
    await click('halt');
    await click('destroy');
    await fetch('/result?status=pass');
} catch (error) {
    let output = '';
    try { output = await app?.terminal.readText(); } catch {}
    await fetch('/result?status=' + encodeURIComponent((error?.stack ?? String(error)) + '\\n' + output));
}
</script>`, directory, { root: resolve(import.meta.dirname, "../demo/dist"), basePath: "/riscbox", timeoutMs: 290_000 });
    } finally { await rm(directory, { recursive: true, force: true }); }
});
