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
    const fetched = [];
    const fetchAssets = async (url, options) => {
        url = String(url);
        fetched.push([url, options.cache]);
        if (url.endsWith(".wasm")) return new Response(wasm, { headers: { "Content-Type": "application/wasm" } });
        if (url.endsWith(".cfg")) return new Response(JSON.stringify(config));
        if (url.endsWith("blk.txt")) return new Response("{block_size:1,n_block:4}");
        return new Response(firmware);
    };
    const setup = { wasmUrl: "https://host/riscbox.wasm", config: { value: config, baseUrl: "https://host/vm/" },
        fetch: fetchAssets };
    await rejects(() => Adapter.prepare({ ...setup, timesliceMs: 5 }), "unknown option");
    for (const source of [{}, { url: "https://host/config", value: config }, { text: 4 }, { value: [] },
        { value: config, baseUrl: "" }, { url: "https://host/config", baseUrl: "https://other/" }]) {
        await rejects(() => Adapter.prepare({ ...setup, config: source }), "explicit config source");
    }
    for (const override of [{ machine: "other" }, { console: "other" }, { memory_size: 1.5 },
        { fs0: { server: "share", tag: 1 } }, { input_device: "other" },
        { drive0: undefined, drive1: { capacity_sectors: 4 } }]) {
        await rejects(() => Adapter.prepare({ ...setup, config: { value: { ...config, ...override } } }), "typed configuration shape");
    }
    await rejects(() => Adapter.prepare({ ...setup, ramMiB: -1 }), "invalid RAM override");
    await rejects(() => Adapter.prepare({ ...setup, blocks: { drive1: { bytes: new Uint8Array(512) } } }), "unknown configured drive");
    await rejects(() => Adapter.prepare({ ...setup, blocks: { drive0: { bytes: new Uint8Array(511) } } }), "partial sector");
    await rejects(() => Adapter.prepare({ ...setup, blocks: { drive0: { bytes: new Uint8Array(512), capacity_sectors: 2 } } }), "capacity mismatch");
    await rejects(() => Adapter.prepare({ ...setup, blocks: { drive0: { provider: {} } } }), "removed provider");

    // Fetching, text parsing, and host values produce the same halted VM and asset URLs.
    const fromUrl = await Adapter.prepare({ ...setup, config: { url: "https://host/vm/config.cfg" } });
    check(fromUrl.state === "halted", "URL preparation halts");
    check(fetched.some(([url, cache]) => url.endsWith("config.cfg") && cache === "no-cache"), "config revalidates cache");
    check(fetched.some(([url, cache]) => url.endsWith(".wasm") && cache === "no-cache"), "WASM revalidates cache");
    check(fetched.some(([url]) => url === "https://host/vm/firmware.bin"), "relative boot asset resolution");
    await fromUrl.destroy();
    const text = "// configuration\n" + JSON.stringify(config).replace('"version":', "version:").slice(0, -1) + ",}";
    const fromText = await Adapter.prepare({ ...setup, config: { text, baseUrl: "https://host/vm/" },
        blocks: { drive0: { file: "../disk-abcd1234/blk.txt" } } });
    check(fromText.state === "halted", "text preparation halts");
    check(fetched.some(([url, cache]) => url === "https://host/disk-abcd1234/blk.txt" && cache === "force-cache"),
        "HTTP override resolves against config base and retains immutable cache policy");
    await fromText.destroy();

    // The original load failure survives cancellation of its sibling download.
    async function rejectsWith(operation, expected) {
        try { await operation(); } catch (error) {
            check(String(error).includes(expected), "unexpected error: " + error); return;
        }
        throw new Error("expected error: " + expected);
    }
    await rejectsWith(() => Adapter.prepare({ ...setup, config: { url: "https://host/pending.cfg" },
        fetch: async (url, options) => {
            if (String(url).endsWith(".wasm")) return new Response("unavailable", { status: 503 });
            return new Promise((resolve, reject) => options.signal.addEventListener("abort",
                () => reject(options.signal.reason), { once: true }));
        } }), "WASM HTTP 503");
    await rejectsWith(() => Adapter.prepare({ ...setup, config: { url: "https://host/missing.cfg" },
        fetch: async (url, options) => String(url).endsWith(".cfg")
            ? new Response("missing", { status: 404 }) : fetchAssets(url, options) }), "configuration HTTP 404");
    await rejects(() => Adapter.prepare({ ...setup, fetch: async (url, options) => String(url).endsWith(".wasm")
        ? new Response(wasm, { headers: { "Content-Type": "application/octet-stream" } }) : fetchAssets(url, options) }),
        "incorrect WASM MIME type");
    await rejectsWith(() => Adapter.prepare({ ...setup, config: { text: "{broken" } }), "configuration");

    // Cancellation rejects before a client escapes, including after its machine begins preparing.
    const controller = new AbortController();
    let bootFetchStarted;
    const bootFetch = new Promise(resolve => { bootFetchStarted = resolve; });
    let destructionNotices = 0;
    const cancelled = Adapter.prepare({ ...setup, signal: controller.signal, onVmDestroyed: () => { destructionNotices++; },
        fetch: async (url, options) => {
            if (String(url).endsWith(".wasm")) return fetchAssets(url, options);
            return new Promise((resolve, reject) => {
                options.signal.addEventListener("abort", () => reject(options.signal.reason), { once: true });
                bootFetchStarted();
            });
        } });
    const observedCancellation = rejects(() => cancelled, "cancelled preparation must reject");
    await bootFetch;
    controller.abort();
    await observedCancellation;
    check(destructionNotices === 0, "cancelled preparation has no public client lifecycle");
    await rejects(() => Adapter.prepare({ ...setup, signal: controller.signal }), "already aborted preparation");

    const original = new Uint8Array(2048).fill(2);
    const completedSignal = new AbortController();
    let destroyedState;
    const clientPromise = Adapter.prepare({ ...setup, blocks: { drive0: { bytes: original } },
        signal: completedSignal.signal,
        onVmDestroyed: () => { destroyedState = client.state; } });
    original.fill(8);
    const client = await clientPromise;
    completedSignal.abort();
    check(client.state === "halted", "initial client state");
    for (const name of ["exports", "options", "filesystems", "runQuantum", "bytes", "withBytes", "drainActions", "start",
        "prepareResolved", "prepareFromUrl", "startResolved", "startFromUrl"]) {
        check(!(name in client), "internal or removed member exposed: " + name);
    }
    check(Adapter.instantiate === undefined && Adapter.loadResolvedConfig === undefined, "removed factories");
    await rejects(() => client.consoleInput(Uint8Array.of(1)), "input before boot");
    const share = client.filesystem("share"), disk = client.block(0);
    check(disk.read(0n, 512)[0] === 2, "block input copied before downloads");
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
    await rejects(() => share.reset(), "filesystem reset requires halt");
    const root = share.stat("").inode;
    share.clear();
    check(share.listFiles().length === 0 && share.stat("").inode === root, "live clear preserves root");
    share.writeFile("file", "replacement");
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
    check(client.state === "destroyed" && !client.started && destroyedState === "destroyed", "destroy is terminal before notification");
    await rejects(() => client.boot(), "destroyed VM cannot boot");
    await rejects(() => client.destroy(), "destroyed VM cannot destroy twice");
    await rejects(() => client.networkCarrier(true), "destroyed VM carrier");
    await rejects(() => client.networkInput(Uint8Array.of(1)), "destroyed VM ingress");
    await rejects(() => share.listFiles(), "destroyed share");
    await rejects(() => disk.read(0n, 512), "destroyed disk");
    const noInput = { ...config, console: "uart" };
    delete noInput.input_device;
    const uart = await Adapter.prepare({ ...setup, config: { value: noInput } });
    await uart.boot();
    await rejects(() => uart.consoleResize(80, 25), "UART has no resize interface");
    await rejects(() => uart.keyEvent(true, 30), "keyboard requires configured input");
    await rejects(() => uart.pointerEvent(0, 0, 0), "tablet requires configured input");
    await rejects(() => uart.wheelEvent(1), "wheel requires configured input");
    await rejects(() => uart.networkInput(Uint8Array.of(1)), "network requires configured device");
    await uart.halt();
    await uart.destroy();
    if (typeof document === "object") {
        const defaultWasm = await Adapter.prepare({ ...setup, wasmUrl: undefined });
        check(fetched.some(([url, cache]) => url === new URL("/build/js/riscbox.wasm", location.href).href && cache === "no-cache"),
            "default WASM is beside adapter script");
        await defaultWasm.destroy();
    }
    return 1;
}
