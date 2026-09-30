export async function facadeRegression(Filesystem, SeedBuilder, runtime, firmware) {
    const check = (condition, message) => { if (!condition) throw new Error(message); };
    const decoder = new TextDecoder();
    const filesystem = await Filesystem.create(runtime);
    const changes = [];
    const unsubscribe = await filesystem.subscribe(change => {
        changes.push(change);
        // Listener reentry must happen after the servicing loop and WASM call.
        check(!runtime.servicingFilesystems, "listener reentry boundary");
    });
    await filesystem.mkdir("dir");
    const input = new TextEncoder().encode("content");
    await filesystem.writeFile("dir/file", input, 9007199254740993n);
    input.fill(0);
    check(decoder.decode(await filesystem.readFile("dir/file")) === "content", "host bytes owned");
    await filesystem.link("dir/file", "alias");
    check((await filesystem.stat("alias")).linkCount === 2, "hardlink identity");
    await filesystem.rename("alias", "moved");
    await filesystem.symlink("link", "dir/file");
    check(await filesystem.readlink("link") === "dir/file", "symlink target");
    check((await filesystem.listDirectory()).map(entry => entry.name).sort().join(",") === "dir,link,moved", "directory listing");
    check((await filesystem.listFiles()).includes("moved"), "file listing");
    check(changes.some(change => change.origin === 9007199254740993n && change.kind === "create"), "origin width");

    let resolveLoad;
    let loads = 0;
    const entries = new SeedBuilder().addFile("lazy", 3, "opaque", { inodeKey: "body" }).addHardLink("alias", "body").finish();
    const loader = { load(key, signal) {
        check(key === "opaque" && signal instanceof AbortSignal, "plugin key and signal");
        loads++;
        return new Promise(resolve => { resolveLoad = resolve; });
    } };
    await filesystem.installSeed({ entries, loader });
    check(loads === 0, "no preload");
    const first = filesystem.readFile("lazy");
    const second = filesystem.readFile("alias");
    await Promise.resolve();
    check(loads === 1, "joined source");
    await filesystem.remove("lazy");
    await filesystem.writeFile("lazy", "replacement");
    resolveLoad(new TextEncoder().encode("old"));
    check(decoder.decode(await first) === "old" && decoder.decode(await second) === "old", "host inode pinned");
    check(decoder.decode(await filesystem.readFile("lazy")) === "replacement", "late source respects replacement");

    await filesystem.installSeed({ entries, loader });
    const superseded = filesystem.readFile("lazy");
    await Promise.resolve();
    await filesystem.writeFile("lazy", "host");
    check(decoder.decode(await superseded) === "host", "host write wins load");
    resolveLoad(new TextEncoder().encode("old"));
    await new Promise(resolve => setTimeout(resolve, 0));
    check(decoder.decode(await filesystem.readFile("lazy")) === "host", "old source ignored");

    for (const bad of [() => Promise.reject(new Error("denied")), async () => new Uint8Array(4)]) {
        await filesystem.installSeed({ entries, loader: { load: bad } });
        try { await filesystem.readFile("lazy"); throw new Error("failure accepted"); }
        catch (error) { check(error.errno === 5, "source error mapping"); }
        await filesystem.retrySource("lazy");
    }
    await filesystem.installSeed({ entries, loader });
    const stale = filesystem.readFile("lazy");
    const staleResult = stale.then(() => false, error => error.errno === 116);
    await Promise.resolve();
    await filesystem.reset();
    check(await staleResult, "namespace reset invalidates host read");
    resolveLoad(new TextEncoder().encode("old"));

    // Guest source completion wakes a WFI quantum through the public adapter.
    await filesystem.installSeed({ entries, loader: { async load() {
        loads++;
        await new Promise(resolve => setTimeout(resolve, 5));
        return new TextEncoder().encode("old");
    } } });
    await filesystem.bind("workspace");
    let output = "";
    runtime.options.consoleWrite = text => { output += text; };
    runtime.options.fetch = async () => ({ status: 200, arrayBuffer: async () => firmware.buffer.slice(firmware.byteOffset, firmware.byteOffset + firmware.byteLength) });
    const config = { version: 1, machine: "riscv64", memory_size: 32, bios: "https://host/fw.bin", console: "uart",
        fs0: { server: "workspace", tag: "first" }, fs1: { server: "workspace", tag: "second" } };
    const other = await Filesystem.create(runtime);
    await other.bind("workspace");
    await filesystem.bind("workspace");
    await other.close();
    // Cancelling startup retires its pending response before a new VM can start.
    const fetchFirmware = runtime.options.fetch;
    let lateFirmware;
    runtime.options.fetch = () => new Promise(resolve => { lateFirmware = resolve; });
    runtime.startResolved(config);
    await runtime.destroy();
    lateFirmware(await fetchFirmware());
    await new Promise(resolve => setTimeout(resolve, 0));
    check(!runtime.started, "cancelled startup stays inactive");
    runtime.options.fetch = fetchFirmware;
    runtime.startResolved(config);
    for (let attempt = 0; attempt < 100 && !output.includes("ABI GUEST PASS"); attempt++) await new Promise(resolve => setTimeout(resolve, 5));
    check(output.includes("ABI GUEST PASS") && !output.includes("ABI GUEST FAIL"), "guest async adapter reply");
    try { await filesystem.close(); throw new Error("attached close accepted"); }
    catch (error) { check(error.errno === 16, "attached close rejected"); }
    await filesystem.writeFile("retained", "yes");
    await filesystem.installSeed({ entries, loader });
    const retainedRead = filesystem.readFile("lazy");
    await Promise.resolve();
    await runtime.reset();
    resolveLoad(new TextEncoder().encode("old"));
    check(decoder.decode(await retainedRead) === "old", "VM reset retains host source work");
    check((await filesystem.listFiles()).includes("lazy"), "VM reset retains namespace");
    await filesystem.reset();
    await filesystem.writeFile("after-reset", "yes");
    await runtime.halt();
    await runtime.destroy();
    check(decoder.decode(await filesystem.readFile("after-reset")) === "yes", "destroy retains host access");
    runtime.startResolved(config);
    await new Promise(resolve => setTimeout(resolve, 10));
    await runtime.halt(); await runtime.destroy();
    await unsubscribe(); await filesystem.close();
    try { await filesystem.readFile("after-reset"); throw new Error("closed access accepted"); }
    catch (error) { check(error.errno === 9, "closed error"); }
    return 1;
}
