// The deployed facade shares resident bytes and enforces platform lifetimes.
function checkFacade(condition, message) { if (!condition) throw Error(message); }
async function rejects(operation, message) {
    try { await operation(); } catch { return; }
    throw Error(message);
}
function bytesEqual(actual, expected, message) {
    checkFacade(actual.length === expected.length && actual.every((byte, index) => byte === expected[index]), message);
}
const delay = () => new Promise(resolve => setTimeout(resolve, 0));

export async function facadeRegression(runtime, firmware) {
    const config = { version: 1, machine: "riscv64", memory_size: 32, bios: "firmware.bin", console: "uart",
        fs0: { server: "share", tag: "shared" } };
    let transcript = "";
    runtime.options.consoleWrite = text => { transcript += text; };
    runtime.options.fetch = async () => new Response(firmware);
    await runtime.prepareResolved(config);
    const share = runtime.filesystem("share");
    const events = [];
    const unsubscribe = share.subscribe(event => events.push(event));
    share.writeFile("file", "initial");
    share.writeFile("file", "old", 0x0123456789abcdefn);
    checkFacade(!(share.readFile("file") instanceof Promise), "resident API is synchronous");
    await delay();
    checkFacade(events.some(event => event.kind === "write" && event.origin === 0x0123456789abcdefn), "origin notification");
    share.setAttributes("file", { mode: 0o751, uid: 123, gid: 456,
        atime: { seconds: 789n, nanoseconds: 123456789 }, mtime: { seconds: 900n, nanoseconds: 987654321 } }, 42n);
    const attributes = share.stat("file");
    checkFacade(attributes.mode === 0o751 && attributes.uid === 123 && attributes.gid === 456
        && attributes.mtime.seconds === 900n && attributes.mtime.nanoseconds === 987654321, "typed attributes restored");
    await delay();
    checkFacade(events.some(event => event.kind === "metadata" && event.origin === 42n), "attribute origin notification");
    await rejects(() => share.setAttributes("file", { ...attributes, mode: 0o10000 }), "invalid mode rejected");
    await rejects(() => share.setAttributes("file", { ...attributes, mtime: { seconds: 0n, nanoseconds: 1_000_000_000 } }), "invalid timestamp rejected");
    await runtime.boot();
    runtime.cancelWakeup();
    await rejects(() => share.clear(), "running share clear must fail");
    for (let run = 0; run < 20 && !transcript.includes("ABI GUEST PASS"); run++) {
        await runtime.runQuantum(); runtime.cancelWakeup();
    }
    checkFacade(transcript.includes("ABI GUEST PASS"), "guest reads the synchronous host file");
    share.writeFile("host", "while running");
    checkFacade(new TextDecoder().decode(share.readFile("host")) === "while running", "concurrent host sharing");
    await runtime.reset(); runtime.cancelWakeup();
    checkFacade(new TextDecoder().decode(share.readFile("host")) === "while running", "reboot retains tree");
    await runtime.halt();
    share.clear();
    checkFacade(share.listFiles().length === 0, "clear after halt");
    unsubscribe();
    await runtime.destroy();
    await rejects(() => share.readFile("file"), "destroy invalidates facade");

    // Array input and exported reads are copies; writes belong entirely to Rust.
    const original = new Uint8Array(2048).fill(2);
    runtime.options.fetch = async url => new Response(url.endsWith("blk.txt")
        ? "{block_size:1,n_block:4}" : Uint8Array.from([0x73, 0, 0x50, 0x10]));
    let loads = 0;
    let complete;
    runtime.options.fetchBlock = async () => { loads++; return new Promise(resolve => { complete = resolve; }); };
    await runtime.prepareResolved({ ...config, drive0: { bytes: original }, drive1: { file: "disk/blk.txt" } });
    const array = runtime.block(0);
    const http = runtime.block(1);
    await rejects(() => runtime.prepareResolved(config), "second preparation cannot replace a live platform");
    checkFacade(runtime.block(1) === http, "failed preparation preserves disk identities");
    original.fill(8);
    checkFacade(array.capacitySectors === 4n, "array capacity");
    bytesEqual(array.read(0n, 512), new Uint8Array(512).fill(2), "input copied into Rust");
    const copy = array.read(0n, 512); copy.fill(7);
    checkFacade(array.read(0n, 512)[0] === 2, "read returns a copy");
    array.write(1n, copy);
    checkFacade(array.read(1n, 512)[0] === 7, "array write-through");
    await rejects(() => array.read(4n, 512), "capacity checked");
    http.write(1n, new Uint8Array(512).fill(9));
    checkFacade(http.read(1n, 512)[0] === 9 && loads === 0, "unfetched writes and reads stay synchronous");
    const first = http.read(0n, 1024);
    const second = http.read(0n, 512);
    checkFacade(first instanceof Promise && second instanceof Promise, "cache misses are asynchronous");
    await delay();
    checkFacade(loads === 1, "duplicate misses share one fetch");
    await rejects(() => runtime.boot(), "pending host reads prevent boot");
    http.write(0n, new Uint8Array(512).fill(6));
    complete(new Uint8Array(1024).fill(3));
    const combined = await first;
    checkFacade(combined[0] === 6 && combined[512] === 9, "late base completion preserves writes");
    checkFacade((await second)[0] === 6, "joined read completes");
    await delay();
    http.discardChanges();
    checkFacade(http.read(0n, 1024)[0] === 3 && loads === 1, "discard retains immutable cache");
    await runtime.boot(); runtime.cancelWakeup();
    await rejects(() => array.read(0n, 512), "running disk reads blocked");
    await rejects(() => array.write(0n, copy), "running disk writes blocked");
    await runtime.halt();
    checkFacade(array.read(1n, 512)[0] === 7, "array survives forced halt");

    // Retired host reads reject promptly; their late network responses are ignored.
    const pending = http.read(2n, 512);
    const cancelled = rejects(() => pending, "discard must retire reads");
    await delay();
    const obsolete = complete;
    http.discardChanges();
    await cancelled;
    obsolete(new Uint8Array(1024).fill(4));
    await delay();
    runtime.options.fetchBlock = async () => { throw Error("backing unavailable"); };
    await rejects(() => http.read(2n, 512), "backing errors reject host reads");
    runtime.options.fetchBlock = async () => new Uint8Array(511);
    await rejects(() => http.read(2n, 512), "incorrect response lengths reject host reads");
    runtime.options.fetchBlock = async () => new Uint8Array(1024).fill(5);
    checkFacade((await http.read(2n, 512))[0] === 5, "failed chunks can retry");
    runtime.options.fetchBlock = () => new Promise(resolve => { complete = resolve; });
    const overwritten = http.read(6n, 512);
    http.write(6n, new Uint8Array(512).fill(8));
    checkFacade((await Promise.race([overwritten, new Promise((_, reject) => setTimeout(() => reject(Error("resident write did not settle reader")), 1000))]))[0] === 8,
        "writes satisfy waiting reads without fetching base bytes");
    complete(new Uint8Array(1024).fill(5));
    await delay();
    checkFacade(http.read(6n, 512)[0] === 8, "late base cannot replace write-satisfied data");
    await runtime.coldReset();
    checkFacade(array.read(1n, 512)[0] === 7, "cold reset leaves storage policy explicit");
    await runtime.destroy();
    await rejects(() => array.read(0n, 512), "destroy invalidates disk facades");
    await rejects(() => share.listFiles(), "old share cannot bind to replacement VM");
    return 1;
}
