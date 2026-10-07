// Cache warming uses independent requests and never touches the VM's disk overlay.
export class DiskPrefetch {
    started = false;
    paused = false;
    waiting = new Set();

    setPaused(paused) {
        this.paused = paused;
        if (paused) return;
        for (const resume of this.waiting) resume();
        this.waiting.clear();
    }

    // Recheck after waking so a new focus event cannot release a pending request.
    async ready() {
        while (this.paused) await new Promise(resolve => this.waiting.add(resolve));
    }

    async start(configUrl) {
        if (this.started) return;
        this.started = true;
        try {
            await this.ready();
            const response = await fetch(configUrl, { cache: "no-cache" });
            if (!response.ok) return;
            const config = await response.json();
            if (typeof config?.drive0?.file !== "string") return;

            // The demo's split image resolves chunks beside its hash-named manifest.
            const manifestUrl = new URL(config.drive0.file, configUrl);
            await this.ready();
            const manifestResponse = await fetch(manifestUrl, { cache: "force-cache" });
            if (!manifestResponse.ok) return;
            const manifest = await manifestResponse.text();
            const match = /^\s*\{\s*block_size:\s*(\d+),\s*n_block:\s*(\d+),?\s*\}\s*$/.exec(manifest);
            if (!match) return;
            const count = Number(match[2]);
            if (!Number.isSafeInteger(count) || Number(match[1]) <= 0) return;

            // Two workers drain entire bodies before issuing further requests.
            let next = 0;
            const worker = async () => {
                while (next < count) {
                    await this.ready();
                    if (next >= count) return;
                    const url = new URL(`blk${String(next++).padStart(9, "0")}.bin`, manifestUrl);
                    try {
                        const block = await fetch(url, { cache: "force-cache" });
                        await block.arrayBuffer();
                    } catch {
                        // Best-effort cache warming continues after individual transfer failures.
                    }
                }
            };
            await Promise.all([worker(), worker()]);
        } catch {
            // Unavailable metadata leaves ordinary VM loading responsible for the image.
        }
    }
}

// Focus anywhere in the terminal pauses new requests; active transfers finish.
export function prefetchImage(configUrl, host) {
    const prefetch = new DiskPrefetch();
    const update = () => prefetch.setPaused(host.contains(document.activeElement));
    const afterBlur = () => queueMicrotask(update);
    host.addEventListener("focusin", update);
    host.addEventListener("focusout", afterBlur);
    update();
    void prefetch.start(configUrl).finally(() => {
        host.removeEventListener("focusin", update);
        host.removeEventListener("focusout", afterBlur);
    });
    return prefetch;
}
