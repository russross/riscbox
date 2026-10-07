import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { runChromePage } from "./chrome.mjs";

// Real focus events gate requests while delayed response bodies remain in flight.
test("demo prefetch pauses on terminal focus and resumes after blur", async () => {
    const directory = await mkdtemp(join(tmpdir(), "riscbox-demo-prefetch-"));
    try {
        await runChromePage(`<!doctype html><div id="terminal"><textarea></textarea><button>Inside</button></div><button id="outside">Outside</button><script type="module">
import { DiskPrefetch, prefetchImage } from '/demo/web/disk-prefetch.js';
const check = (condition, message) => { if (!condition) throw new Error(message); };
const settle = () => new Promise(resolve => setTimeout(resolve, 20));
const realFetch = window.fetch;
try {
    // Headed Chrome must receive window focus before native focus events are observable.
    window.focus();
    for (let attempt = 0; !document.hasFocus() && attempt < 100; attempt++) await settle();
    check(document.hasFocus(), 'Chrome document has focus');
    const calls = [], bodies = [];
    window.fetch = async (url, options) => {
        calls.push({ url: String(url), cache: options.cache });
        if (String(url).endsWith('riscbox.cfg')) return { ok: true, json: async () => ({ drive0: { file: 'drive-12345678/blk.txt' } }) };
        if (String(url).endsWith('blk.txt')) return { ok: true, text: async () => '{ block_size: 512, n_block: 5, }' };
        return { arrayBuffer: () => new Promise(resolve => bodies.push(resolve)) };
    };
    const host = document.getElementById('terminal');
    const prefetch = prefetchImage(new URL('/image/riscbox.cfg', location.href), host);
    await settle();
    check(!host.contains(document.activeElement), 'terminal starts unfocused');
    check(calls.length === 4 && bodies.length === 2, 'two concurrent bodies');
    check(calls[2].url.endsWith('/image/drive-12345678/blk000000000.bin'), 'relative chunk URL');
    check(calls[0].cache === 'no-cache' && calls.slice(1).every(call => call.cache === 'force-cache'), 'cache policy');

    // Completing both transfers while focused must leave the remaining chunks untouched.
    host.querySelector('textarea').focus();
    bodies.splice(0).forEach(resolve => resolve(new ArrayBuffer(0)));
    await settle();
    check(calls.length === 4, 'focus pauses subsequent requests: ' + calls.length + ', paused=' + prefetch.paused + ', active=' + document.activeElement.tagName + ', document focus=' + document.hasFocus());
    host.querySelector('button').focus();
    await settle();
    check(calls.length === 4, 'focus within terminal remains paused');
    document.getElementById('outside').focus();
    await settle();
    check(calls.length === 6 && bodies.length === 2, 'blur resumes two transfers');
    bodies.splice(0).forEach(resolve => resolve(new ArrayBuffer(0)));
    await settle();
    check(calls.length === 7 && bodies.length === 1, 'last chunk requested once');
    bodies.pop()(new ArrayBuffer(0));
    await settle();
    await prefetch.start(new URL('/image/riscbox.cfg', location.href));
    check(calls.length === 7, 'completed prefetch does not restart');

    // Focus before startup also gates metadata, and failed chunks do not stop warmup.
    calls.length = 0;
    host.querySelector('textarea').focus();
    prefetchImage(new URL('/image/riscbox.cfg', location.href), host);
    await settle();
    check(calls.length === 0, 'initial focus gates metadata');
    window.fetch = async (url) => {
        calls.push(String(url));
        if (String(url).endsWith('riscbox.cfg')) return { ok: true, json: async () => ({ drive0: { file: 'drive/blk.txt' } }) };
        if (String(url).endsWith('blk.txt')) return { ok: true, text: async () => '{ block_size: 512, n_block: 3 }' };
        throw new Error('offline');
    };
    document.getElementById('outside').focus();
    await settle();
    check(calls.length === 5, 'failed chunks continue');
    for (const metadata of ['invalid', '{ block_size: 0, n_block: 3 }', '{ block_size: 512, n_block: 9007199254740992 }']) {
        let requests = 0;
        window.fetch = async url => {
            requests++;
            return String(url).endsWith('riscbox.cfg')
                ? { ok: true, json: async () => ({ drive0: { file: 'drive/blk.txt' } }) }
                : { ok: true, text: async () => metadata };
        };
        await new DiskPrefetch().start(new URL('/image/riscbox.cfg', location.href));
        check(requests === 2, 'malformed metadata does not request chunks');
    }
    await realFetch('/result?status=pass');
} catch (error) { await realFetch('/result?status=' + encodeURIComponent(error.stack ?? String(error))); }
</script>`, directory);
    } finally { await rm(directory, { recursive: true, force: true }); }
});

// Documentation links share a fixed revision and point to rendered GitHub files.
test("demo documentation links pin GitHub guides to one revision", { timeout: 60_000 }, async () => {
    const directory = await mkdtemp(join(tmpdir(), "riscbox-demo-docs-"));
    try {
        await runChromePage(`<!doctype html><script type="module">
const check = (condition, message) => { if (!condition) throw new Error(message); };
try {
    const index = new DOMParser().parseFromString(await (await fetch('/riscbox/index.html')).text(), 'text/html');
    const links = [...index.querySelectorAll('a[href]')].map(link => new URL(link.href));
    check(links.length === 3, 'three documentation links');
    const paths = ['API.md', 'HOWTO.md', 'demo/bsd-games-3.3/README.md'];
    let revision;
    for (const [i, link] of links.entries()) {
        check(link.origin === 'https://github.com', 'GitHub rendered documentation');
        const prefix = '/russross/riscbox/blob/';
        check(link.pathname.startsWith(prefix), 'repository file view');
        const rest = link.pathname.slice(prefix.length);
        const slash = rest.indexOf('/');
        const ref = rest.slice(0, slash);
        check(/^(v[0-9][^{}]*|[0-9a-f]{40})$/.test(ref), 'release tag or commit');
        revision ??= ref;
        check(ref === revision, 'consistent revision');
        check(rest.slice(slash + 1) === paths[i], 'documentation source path');
    }
    check((await fetch('/riscbox/docs/riscbox/API.html')).status === 404, 'no rendered documentation tree');
    const config = await (await fetch('/riscbox/riscbox.cfg')).json();
    const manifest = await (await fetch(new URL(config.drive0.file, new URL('/riscbox/riscbox.cfg', location.href)))).text();
    check(/block_size: 256,/.test(manifest), 'demo uses 256 KiB disk chunks');
    await fetch('/result?status=pass');
} catch (error) { await fetch('/result?status=' + encodeURIComponent(error.stack ?? String(error))); }
</script>`, directory, { root: resolve(import.meta.dirname, "../demo/dist"), basePath: "/riscbox", timeoutMs: 50_000 });
    } finally { await rm(directory, { recursive: true, force: true }); }
});

// The page serves only the assembled application, including its unchanged release tree.
test("release demo boots, shares files, compiles games, and exercises lifecycle controls", { timeout: 600_000 }, async () => {
    const directory = await mkdtemp(join(tmpdir(), "riscbox-demo-"));
    try {
        await runChromePage(`<!doctype html><style>iframe { width: 98vw; height: 95vh; border: 0; }</style><iframe src="/riscbox/index.html"></iframe><script type="module">
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
    check(!doc.getElementById('terminal').contains(doc.activeElement), 'VM terminal starts unfocused');
    check(doc.querySelector('h1').textContent === 'Riscbox demo: An Alpine Linux image that shares a live file tree with the host app', 'demo heading');
    check(doc.querySelector('label[for="source-tree"]').textContent === 'Load source tree', 'source label');
    check(doc.getElementById('files-label').textContent === '/shared file tree', 'file tree label');
    check(doc.getElementById('editor-pane').classList.contains('empty'), 'empty editor begins gray');
    const statusIds = ['speed-1s', 'speed-5s', 'speed-15s', 'clock-time', 'cpu-time'];
    const tooltips = ['Mcycles/second average over most recent 1s', 'Mcycles/second average over most recent 5s',
        'Mcycles/second average over most recent 15s', 'Total clock time since boot', 'Total cpu time emulated since boot'];
    check(statusIds.every((id, index) => doc.getElementById(id).title === tooltips[index]), 'status tooltips');

    // Exercise display boundaries through the same periodic update used by the running VM.
    const realSpeed = app.runtime.speed;
    try {
        for (const [seconds, formatted] of [[59.9, '59s'], [60, '01:00'], [3599, '59:59'], [3600, '01:00:00'], [3661.9, '01:01:01']]) {
            app.runtime.speed = () => ({ mcycles1s: 50.123, mcycles5s: 48.27, mcycles15s: 47.11,
                uptimeSeconds: seconds, cyclingSeconds: seconds });
            await until(() => doc.getElementById('clock-time').textContent === formatted, 'clock format ' + formatted);
            check(doc.getElementById('cpu-time').textContent === formatted, 'CPU uptime format');
            check(doc.getElementById('speed-1s').textContent === '50.12', 'speed decimal places');
        }
    } finally { app.runtime.speed = realSpeed; }
    check([...doc.querySelectorAll('details')].every(details => !details.open), 'details begin collapsed');
    const panes = ['tree-pane', 'editor-pane', 'terminal-pane'].map(id => doc.getElementById(id));
    const widths = panes.map(pane => pane.getBoundingClientRect().width);
    check(Math.abs(widths[0] / widths[1] - 10 / 45) < .02, 'initial pane proportions');
    const gutter = doc.querySelector('.gutter');
    check(frame.contentWindow.getComputedStyle(gutter).cursor === 'grab', 'gutter resting cursor');
    const initialCols = app.terminal.cols;
    const terminalGutter = doc.querySelectorAll('.gutter')[1];
    terminalGutter.dispatchEvent(new frame.contentWindow.KeyboardEvent('keydown', {key: 'ArrowLeft'}));
    await until(() => app.terminal.cols > initialCols, 'terminal fits resized pane');
    check(panes[1].getBoundingClientRect().width < widths[1], 'gutter resizes adjacent panes');
    // Command delimiters use console output; screen capture can join wrapped lines.
    let rawConsole = '';
    const write = app.terminal.write.bind(app.terminal);
    app.terminal.write = (value, callback) => { rawConsole += value; return write(value, callback); };
    let captured = '';
    async function text() {
        try {
            captured = (await app.terminal.readText()).split('\\n').map(line => line.trim()).join('\\n');
            return captured;
        }
        catch (error) { if (error.name === 'AbortError') return captured; throw error; }
    }
    async function prompt() { await until(async () => (await text()).includes('riscbox:/shared$'), 'login'); }
    async function click(id) {
        check(!doc.getElementById(id).disabled, id + ' disabled');
        if (['boot', 'reset', 'reboot', 'image-reset'].includes(id)) {
            app.terminal.reset();
            captured = '';
            rawConsole = '';
        }
        doc.getElementById(id).click();
        await until(() => !doc.getElementById('vm-controls').disabled, id + ' completion');
        if (['boot', 'reset', 'image-reset'].includes(id)) {
            check(doc.getElementById('cpu-time').textContent === '0s', 'boot resets displayed uptime');
        }
        // Parser queues can retain older prompts; fresh console output confirms this boot.
        if (['boot', 'reset', 'reboot', 'image-reset'].includes(id)) {
            await until(() => {
                const banner = rawConsole.indexOf('Riscbox demo');
                return banner >= 0 && rawConsole.slice(banner).includes('riscbox:/shared$');
            }, id + ' login');
        }
    }
    let serial = 0;
    async function command(command, expected) {
        const marker = 'DEMO_OK_' + ++serial;
        const start = 'DEMO_BEGIN_' + serial;
        rawConsole = '';
        app.queueInput(new TextEncoder().encode('echo ' + start + '; ' + command + '; printf "\\\\r\\\\n' + marker + '\\\\r\\\\n"\\r'));
        await until(() => rawConsole.replace(/\\r/g, '').includes('\\n' + marker + '\\n'), command);
        const console = rawConsole.replace(/\\r/g, '');
        const output = console.slice(console.indexOf('\\n' + start + '\\n') + start.length + 2, console.lastIndexOf('\\n' + marker));
        check(output.includes(expected), 'missing output ' + expected + ': ' + output);
        return output;
    }
    await prompt();
    await command('stty size', app.terminal.rows + ' ' + app.terminal.cols);
    const initialOutput = await command("id; doas id; mount | grep 'on / type ext4'; make; ./arithmetic </dev/null", 'uid=1000(riscbox)');
    check(initialOutput.includes('uid=0(root)'), 'passwordless doas');
    check(initialOutput.includes('type ext4'), 'writable ext4 root');
    check(initialOutput.includes(' ='), 'arithmetic compiled and executed');

    // Compile every self-contained project on resident 9p with the guest compiler.
    const games = await (await fetch('/riscbox/examples.json')).json();
    check(games.length === 18, 'complete BSD games collection');
    check(doc.getElementById('source-tree').options.length === games.length + 1, 'all games selectable');
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
        app.terminal.reset();
        captured = '';
        app.queueInput(new TextEncoder().encode('(cd games/' + id + ' && ./' + id + ')\\r'));
        await until(async () => (await text()).includes(expected), id + ' playable screen');
        check(!/Fatal error|Segmentation fault|Error:/.test(captured), id + ' startup');
        if (id === 'arithmetic') {
            const problem = captured.match(/(\\d+) ([+-]) (\\d+) =/);
            check(problem, 'arithmetic question');
            const left = Number(problem[1]), right = Number(problem[3]);
            app.queueInput(new TextEncoder().encode(String(problem[2] === '+' ? left + right : left - right) + '\\r'));
            await until(async () => (await text()).includes('Right!'), 'arithmetic answer');
        }
        if (id === 'sail') {
            app.queueInput(new TextEncoder().encode('\\r'));
            await until(async () => (await text()).includes('Aye aye, Sir'), 'sail scenario');
        }
        const quit = { arithmetic: 'q\\r', hangman: '\\r', wump: 'q\\r' };
        // Only output after quitting can confirm that the shell regained input.
        rawConsole = '';
        app.queueInput(new TextEncoder().encode(quit[id] ?? '\\u0003'));
        await until(() => rawConsole.includes('riscbox:/shared$'), id + ' terminal cleanup');
    }
    await command('stty sane', '');
    const files = doc.getElementById('files');
    const row = path => [...files.querySelectorAll('.file-row')].find(row => row.dataset.path === path && !row.classList.contains('deleted'));
    const hasFile = path => Boolean(row(path));
    async function selectTree(id) {
        doc.getElementById('source-tree').value = id;
        doc.getElementById('source-tree').dispatchEvent(new frame.contentWindow.Event('change'));
        check(files.children.length === 0, 'source selection immediately removes old rows without pulses');
        await until(() => id === '' ? app.workspace.listDirectory('').length === 0 : hasFile('Makefile') && hasFile(id + '.c'), 'source selection ' + id);
    }
    check(!doc.getElementById('refresh-tree'), 'tree has no manual refresh control');
    await until(() => hasFile('arithmetic'), 'guest build updates tree');
    row('arithmetic').click();
    check(app.editor.state.doc.length === 0 && app.editor.state.readOnly, 'binary file is not loaded');
    check(doc.getElementById('editor-pane').classList.contains('empty'), 'binary selection grays editor');
    row('Makefile').click();
    check(!doc.getElementById('editor-pane').classList.contains('empty'), 'text selection enables editor appearance');
    const original = app.editor.state.doc.toString();
    check(original.includes('arithmetic'), 'selection copies immediately');
    await command('mkdir -p nested/deeper; echo guest > nested/deeper/file.txt', '');
    await until(() => hasFile('nested/deeper/file.txt'), 'guest creation updates tree');
    const nestedFile = row('nested/deeper/file.txt');
    check(nestedFile.textContent.startsWith('\\u00a0'.repeat(4)), 'nested file indentation');
    check(frame.contentWindow.getComputedStyle(nestedFile).whiteSpace === 'pre', 'indentation retains whitespace');
    check(row('Makefile').getAttribute('aria-current') === 'true', 'guest changes preserve selection');
    check(app.editor.state.doc.toString() === original, 'unrelated guest changes preserve editor');
    await command('mv nested/deeper/file.txt nested/deeper/renamed.txt', '');
    await until(() => hasFile('nested/deeper/renamed.txt') && !hasFile('nested/deeper/file.txt'), 'guest rename updates tree');
    row('nested/deeper/renamed.txt').click();
    app.editor.dispatch({changes: {from: 0, insert: 'discard me'}});
    await command('echo replaced > nested/deeper/renamed.txt', '');
    await until(() => app.editor.state.doc.toString() === 'replaced\\n', 'guest write discards buffered editor edits');
    await command('rm -r nested', '');
    await until(() => !hasFile('nested'), 'guest removal updates tree');
    check(app.editor.state.doc.length === 0 && app.editor.state.readOnly, 'deleted file clears and locks editor');
    check(doc.getElementById('editor-pane').classList.contains('empty'), 'deleted file grays editor');
    app.workspace.writeFile('host-created.txt', 'host');
    await until(() => hasFile('host-created.txt'), 'host writes update tree');
    row('host-created.txt').click();
    app.editor.dispatch({changes: {from: 0, to: app.editor.state.doc.length, insert: 'idle save'}});
    await until(() => new TextDecoder().decode(app.workspace.readFile('host-created.txt')) === 'idle save', 'idle sync');
    app.workspace.writeFile('host-created.txt', 'external replacement');
    await until(() => app.editor.state.doc.toString() === 'external replacement', 'host write updates editor');
    app.workspace.remove('host-created.txt');
    await until(() => app.editor.state.readOnly && app.editor.state.doc.length === 0, 'host deletion locks editor');
    check([...files.children].some(item => item.dataset.path === 'host-created.txt' && item.classList.contains('deleted')), 'deleted file pulses before removal');
    await until(() => ![...files.children].some(item => item.dataset.path === 'host-created.txt'), 'deletion pulse finishes');
    row('Makefile').click();
    app.editor.dispatch({changes: {from: 0, insert: '# automatic save\\n'}});
    check(new TextDecoder().decode(app.workspace.readFile('Makefile')) === original, 'edit stays buffered');
    app.editor.contentDOM.dispatchEvent(new frame.contentWindow.FocusEvent('blur'));
    check(!doc.getElementById('terminal-pane').querySelector('.pulse-overlay'), 'editor save does not pulse VM');
    await until(() => new TextDecoder().decode(app.workspace.readFile('Makefile')).startsWith('# automatic save'), 'blur sync');
    await command('head -1 Makefile; echo retained > /home/riscbox/marker', '# automatic save');
    await click('reboot');
    const rebootOutput = await command('cat /home/riscbox/marker; head -1 Makefile', 'retained');
    check(rebootOutput.includes('# automatic save'), 'reboot retains disk and 9p edits');
    await click('halt');
    check(doc.getElementById('state').textContent === 'halted', 'halt');
    const haltedStatus = statusIds.map(id => doc.getElementById(id).textContent).join(',');
    await new Promise(resolve => setTimeout(resolve, 1200));
    check(statusIds.map(id => doc.getElementById(id).textContent).join(',') === haltedStatus, 'halt freezes status display');
    const disk = app.runtime.block(0);
    const originalSector = await disk.read(0n, 512);
    const changedSector = originalSector.slice();
    changedSector[0] ^= 1;
    disk.write(0n, changedSector);
    await selectTree('caesar');
    await click('boot');
    await command("make; printf 'Hello\\\\n' | ./caesar 13", 'Uryyb');
    await click('shutdown');
    await until(() => doc.getElementById('state').textContent === 'halted', 'soft shutdown');
    await selectTree('wump');
    await click('boot');
    await command("make; printf 'q\\\\n' | ./wump", 'Wumpus');
    await click('reset');
    await command('test -f wump && echo SHARE_RETAINED', 'SHARE_RETAINED');
    await click('image-reset');
    const resetOutput = await command('test ! -e /home/riscbox/marker && echo IMAGE_CLEAN; test -f wump && echo SHARE_KEPT', 'IMAGE_CLEAN');
    check(resetOutput.includes('SHARE_KEPT'), 'image reset retains share');
    row('Makefile').click();
    app.editor.dispatch({changes: {from: 0, insert: 'discard on source change'}});
    await selectTree('caesar');
    check(app.editor.state.readOnly && app.editor.state.doc.length === 0, 'source change clears buffered editor');
    await command("cd /shared; make; printf 'Live\\\\n' | ./caesar 13", 'Yvir');
    await selectTree('wump');
    await command('make; test -f wump && echo LIVE_SOURCE_REPLACED', 'LIVE_SOURCE_REPLACED');
    await command('exec 3<Makefile; mkdir removed; cd removed; echo DIRECTORY_OPEN', 'DIRECTORY_OPEN');
    await selectTree('');
    check(app.workspace.listDirectory('').length === 0, 'live clear empties host namespace');
    const clearOutput = await command('cd /shared; test -z "$(ls -A)" && echo LIVE_TREE_EMPTY; read line <&3 && echo OPEN_FID_RETAINED; exec 3<&-; echo LIVE_FID_CLOSED', 'LIVE_FID_CLOSED');
    check(clearOutput.includes('LIVE_TREE_EMPTY') && clearOutput.includes('OPEN_FID_RETAINED'), 'Linux leaves deleted directory and retains open file');
    await click('halt');
    check((await disk.read(0n, 512))[0] === originalSector[0], 'image reset discards HTTP overlay');
    await selectTree('wump');
    check(!app.workspace.listDirectory('').some(entry => entry.name === 'wump'), 'share reset removes built binary');
    await selectTree('');
    check(app.workspace.listDirectory('').length === 0, 'share clear');
    await until(() => files.children.length === 0, 'share clear updates tree');
    const retired = app.workspace;
    await click('destroy');
    let invalid = false;
    try { retired.listDirectory(''); } catch { invalid = true; }
    check(invalid, 'destroy invalidates share');
    await click('prepare');
    await selectTree('arithmetic');
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
</script>`, directory, { root: resolve(import.meta.dirname, "../demo/dist"), basePath: "/riscbox", timeoutMs: 590_000, chromeArgs: ["--window-size=1800,1000"] });
    } finally { await rm(directory, { recursive: true, force: true }); }
});
