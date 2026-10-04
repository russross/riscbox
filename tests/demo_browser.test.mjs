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
test("release demo boots, shares files, compiles games, and exercises lifecycle controls", { timeout: 600_000 }, async () => {
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
    // Command delimiters use console output; screen capture can join wrapped lines.
    let rawConsole = '';
    const write = app.terminal.write.bind(app.terminal);
    app.terminal.write = value => { rawConsole += value; return write(value); };
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
        rawConsole = '';
        app.terminal.onData('echo ' + start + '; ' + command + '; printf "\\\\r\\\\n' + marker + '\\\\r\\\\n"\\r');
        await until(() => rawConsole.replace(/\\r/g, '').includes('\\n' + marker + '\\n'), command);
        const console = rawConsole.replace(/\\r/g, '');
        const output = console.slice(console.indexOf('\\n' + start + '\\n') + start.length + 2, console.lastIndexOf('\\n' + marker));
        check(output.includes(expected), 'missing output ' + expected + ': ' + output);
        return output;
    }
    await prompt();
    const initialOutput = await command('id; doas id; mount | grep overlay; make; ./arithmetic </dev/null', 'uid=1000(demo)');
    check(initialOutput.includes('uid=0(root)'), 'passwordless doas');
    check(initialOutput.includes('type overlay'), 'tmpfs overlays');
    check(initialOutput.includes(' ='), 'arithmetic compiled and executed');

    // Compile every self-contained project on resident 9p with the guest compiler.
    const games = await (await fetch('/riscbox/examples.json')).json();
    check(games.length === 18, 'complete BSD games collection');
    check(doc.getElementById('source-tree').options.length === games.length, 'all games selectable');
    const directories = new Set();
    for (const game of games) {
        for (const path of game.files) {
            check(!/\\.(o|d)$/.test(path) && path !== game.id, 'source distribution excludes build products');
            const source = await (await fetch('/riscbox/examples/' + game.id + '/' + path)).text();
            const destination = 'games/' + game.id + '/' + path;
            const parts = destination.split('/');
            for (let count = 1; count < parts.length; count++) {
                const directory = parts.slice(0, count).join('/');
                if (directories.has(directory)) continue;
                app.workspace.mkdir(directory);
                app.workspace.setAttributes(directory, { ...app.workspace.stat(directory), uid: 1000, gid: 1000 });
                directories.add(directory);
            }
            app.workspace.writeFile(destination, source);
            app.workspace.setAttributes(destination, {
                ...app.workspace.stat(destination), uid: 1000, gid: 1000,
                mtime: { seconds: 0n, nanoseconds: 0 },
            });
        }
        const build = await command('make -C games/' + game.id + ' > /tmp/build.log 2>&1 && echo GAME_BUILT; cat /tmp/build.log', 'GAME_BUILT');
        check(!/warning:|error:/.test(build), 'clean TinyCC build: ' + game.id);
    }

    // Real terminal startup covers curses, dictionary access, random state, and timers.
    const screens = {
        adventure: 'Colossal Cave', arithmetic: ' =', atc: 'Time:',
        battlestar: 'B A T T L E S T A R', cribbage: 'Your score:',
        dab: 'human', drop4: 'Level:', gofish: 'Cards:', gomoku: 'Your move',
        hangman: 'Word:', klondike: 'Klondike', robots: 'Commands:',
        sail: 'Round', snake: '@', spirhunt: 'Condition', worm: 'Worm', wump: 'Wumpus',
    };
    await command("printf 'Hello\\\\n' | games/caesar/caesar 13", 'Uryyb');
    for (const [id, expected] of Object.entries(screens)) {
        doc.getElementById('clear-terminal').click();
        captured = '';
        app.terminal.onData('(cd games/' + id + ' && ./' + id + ')\\r');
        await until(async () => (await text()).includes(expected), id + ' playable screen');
        check(!/Fatal error|Segmentation fault|Error:/.test(captured), id + ' startup');
        if (id === 'arithmetic') {
            const problem = captured.match(/(\\d+) ([+-]) (\\d+) =/);
            check(problem, 'arithmetic question');
            const left = Number(problem[1]), right = Number(problem[3]);
            app.terminal.onData(String(problem[2] === '+' ? left + right : left - right) + '\\r');
            await until(async () => (await text()).includes('Right!'), 'arithmetic answer');
        }
        if (id === 'sail') {
            app.terminal.onData('\\r');
            await until(async () => (await text()).includes('Aye aye, Sir'), 'sail scenario');
        }
        const quit = { arithmetic: 'q\\r', hangman: '\\r', wump: 'q\\r' };
        app.terminal.onData(quit[id] ?? '\\u0003');
        await until(async () => (await text()).includes('demo@riscbox:'), id + ' terminal cleanup');
    }
    await command('stty sane', '');
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
    doc.getElementById('source-tree').value = 'caesar';
    await click('load-tree');
    await click('boot');
    await command("make; printf 'Hello\\\\n' | ./caesar 13", 'Uryyb');
    await click('shutdown');
    await until(() => doc.getElementById('state').textContent === 'halted', 'soft shutdown');
    doc.getElementById('source-tree').value = 'wump';
    await click('load-tree');
    await click('boot');
    await command("make; printf 'q\\\\n' | ./wump", 'Wumpus');
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
</script>`, directory, { root: resolve(import.meta.dirname, "../demo/dist"), basePath: "/riscbox", timeoutMs: 590_000 });
    } finally { await rm(directory, { recursive: true, force: true }); }
});
