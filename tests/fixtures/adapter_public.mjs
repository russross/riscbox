// Exercise client contracts against an actual raw-WASM instance through its adapter.
export async function publicAdapterRegression(Adapter, wasm, firmware) {
    const check = (condition, message) => { if (!condition) throw new Error(message); };
    async function rejects(operation, message) {
        try { await operation(); } catch { return; }
        throw new Error(message);
    }
    const config = { version: 1, machine: "riscv64", memory_size: 32, bios: "firmware.bin",
        console: "virtio", input_device: "virtio", drive0: { capacity_sectors: 4 },
        fs0: { server: "share", tag: "shared" } };
    let releaseConfig;
    const client = await Adapter.instantiate(wasm, { fetch: async url => {
        if (url.endsWith("cancel.cfg")) return new Promise(resolve => { releaseConfig = resolve; });
        return new Response(firmware);
    } });
    check(client.state === "empty", "initial state");
    for (const name of ["exports", "options", "filesystems", "runQuantum", "bytes", "withBytes", "drainActions", "start"]) {
        check(!(name in client), "internal or removed member exposed: " + name);
    }
    await rejects(() => client.boot(), "empty VM boot");
    await rejects(() => client.consoleInput(Uint8Array.of(1)), "input before boot");
    await rejects(() => client.filesystem("share"), "share before prepare");
    await rejects(() => Adapter.instantiate(wasm, { timesliceMs: 5 }), "unknown option");
    for (const override of [{ machine: "other" }, { console: "other" }, { memory_size: 1.5 },
        { fs0: { server: "share", tag: 1 } }, { input_device: "other" },
        { drive0: undefined, drive1: { capacity_sectors: 4 } }]) {
        await rejects(() => Adapter.loadResolvedConfig("https://host/config",
            "", async () => new Response(JSON.stringify({ ...config, ...override }))), "typed configuration shape");
    }
    await rejects(() => client.prepareResolved(config, -1), "invalid RAM override");
    await rejects(() => client.prepareResolved({ version: 1 }), "invalid configuration");
    check(client.state === "empty", "failed preparation cleans up");

    // Cancellation cannot let an old configuration replace or destroy a newer VM.
    const cancelled = client.prepareFromUrl("https://host/cancel.cfg");
    const observedCancellation = rejects(() => cancelled, "cancelled load must reject");
    check(client.state === "preparing", "download reserves the VM");
    await rejects(() => client.prepareResolved(config), "overlapping preparation");
    await client.destroy();
    await client.prepareResolved(config);
    releaseConfig(new Response(JSON.stringify(config)));
    await observedCancellation;
    check(client.state === "halted", "old cancellation preserves new VM");
    const share = client.filesystem("share"), disk = client.block(0);
    check(client.filesystem("share") === share && client.block(0) === disk, "facade identities");
    for (const name of ["handle", "runtime", "poll", "invalidate"]) check(!(name in share), "share internals");
    for (const name of ["index", "runtime", "poll", "invalidate"]) check(!(name in disk), "disk internals");
    await rejects(() => share.readFile(1), "path must be string");
    await rejects(() => share.subscribe(null), "listener must be function");
    await rejects(() => share.writeFile("file", "text", 1), "origin must be bigint");
    await rejects(() => disk.write(0n, "text"), "disk writes require bytes");
    share.writeFile("file", "old");
    disk.write(0n, new Uint8Array(512).fill(7));
    check(disk.read(0n, 512)[0] === 7 && disk.capacitySectors === 4n, "powered-off disk access");
    await rejects(() => client.reset(), "reset requires running VM");
    await rejects(() => client.halt(), "halt requires running VM");
    await rejects(() => client.requestReboot(), "soft reboot requires running VM");

    // Public execution is automatic; calls reject invalid states and scalar truncation.
    await client.boot();
    check(client.state === "running" && client.started, "boot updates state before return");
    await rejects(() => client.boot(), "boot cannot silently reset running VM");
    await rejects(() => client.destroy(), "destroy cannot force halt");
    await rejects(() => client.coldReset(), "cold reset requires halt");
    await rejects(() => share.clear(), "clear requires halt");
    await rejects(() => disk.read(0n, 512), "host disk reads require halt");
    await rejects(() => client.consoleInput("text"), "console requires bytes");
    await rejects(() => client.consoleResize(0, 25), "nonzero terminal dimensions");
    await rejects(() => client.keyEvent(true, 65536), "key code truncation");
    await rejects(() => client.pointerEvent(-1, 0, 0), "negative pointer coordinate");
    await rejects(() => client.wheelEvent(0.5), "fractional wheel input");
    check(client.consoleResize(80, 25) === 0, "terminal dimensions");
    check(client.keyEvent(true, 30) === 0 && client.pointerEvent(10, 20, 1) === 0, "input devices");
    check(client.wheelEvent(-1) === 0, "wheel input");
    await client.reset();
    await client.halt();
    await client.coldReset();
    check(client.state === "halted" && share.readFile("file").length > 0, "cold reset retains share");
    check(disk.read(0n, 512)[0] === 7, "cold reset retains array disk");
    await client.destroy();
    check(client.state === "empty" && !client.started, "destroy clears state");
    await rejects(() => share.listFiles(), "destroyed share");
    await rejects(() => disk.read(0n, 512), "destroyed disk");
    const noInput = { ...config, console: "uart" };
    delete noInput.input_device;
    await client.prepareResolved(noInput);
    await client.boot();
    await rejects(() => client.consoleResize(80, 25), "UART has no resize interface");
    await rejects(() => client.keyEvent(true, 30), "keyboard requires configured input");
    await rejects(() => client.pointerEvent(0, 0, 0), "tablet requires configured input");
    await rejects(() => client.wheelEvent(1), "wheel requires configured input");
    await rejects(() => client.networkInput(Uint8Array.of(1)), "network requires configured device");
    await client.halt();
    await client.destroy();
    return 1;
}
