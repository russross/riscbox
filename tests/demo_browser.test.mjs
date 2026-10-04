import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { runChromePage } from "./chrome.mjs";

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
    await click('refresh-tree');
    doc.getElementById('files').value = 'Makefile';
    const initial = app.editor.state.doc.toString();
    check(initial.includes('Choose a file'), 'selection must not copy');
    await click('read-file');
    const original = app.editor.state.doc.toString();
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
    const retired = app.workspace;
    await click('destroy');
    let invalid = false;
    try { retired.listDirectory(''); } catch { invalid = true; }
    check(invalid, 'destroy invalidates share');
    await click('prepare');
    await click('share-reset');
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
