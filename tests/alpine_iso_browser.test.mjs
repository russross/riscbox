import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { runChromePage } from "./chrome.mjs";

// Use real browser scheduling and host-array or split HTTP transport for ISO media.
// The media remains external because installation images are not source assets.
test("Alpine ISO boots through U-Boot EFI in Chrome", {
    skip: !process.env.RISCBOX_ALPINE_ISO,
}, async () => {
    const iso = await readFile(process.env.RISCBOX_ALPINE_ISO);
    const directory = await mkdtemp(join(tmpdir(), "riscbox-iso-browser-"));
    const http = process.env.RISCBOX_ISO_TRANSPORT === "http";
    const blockSize = 512 * 1024;
    try {
        await runChromePage(`<!doctype html><pre id="console"></pre>
        <script src="/js/riscbox.js"></script><script type="module">
        import { ArrayBlockProvider } from "/build/js/block/array.js";
        const encoder = new TextEncoder();
        let output = "";
        let pending = new Uint8Array();
        let loggedIn = false;
        let commandSent = false;
        let runtime;
        const report = status => fetch("/result?status=" + encodeURIComponent(status));
        try {
            const media = new Uint8Array(await (await fetch("/test.iso")).arrayBuffer());
            const provider = new ArrayBlockProvider(media);
            runtime = await Riscbox.instantiate(
                await (await fetch("/target/wasm32-unknown-unknown/release/riscbox_wasm.wasm")).arrayBuffer(), {
                blockProviders: new Map([[1, provider]]),
                consoleWrite(text) {
                    output += text;
                    document.getElementById("console").textContent = output;
                    if (!loggedIn && output.includes("login:")) {
                        pending = encoder.encode("root\\r");
                        loggedIn = true;
                    }
                    if (loggedIn && !commandSent && output.includes(":~#")) {
                        pending = encoder.encode("mkdir -p /mnt/iso /mnt/fat; " +
                            "mount -t iso9660 -o ro /dev/vda /mnt/iso && " +
                            "test -s /mnt/iso/boot/grub/grub.cfg && " +
                            "mount -t vfat -o ro,loop /mnt/iso/boot/grub/efi.img /mnt/fat && " +
                            "test -s /mnt/fat/efi/boot/bootriscv64.efi && " +
                            "echo ISO_BOOT_OK; poweroff -f\\r");
                        commandSent = true;
                    }
                },
                onVmHalted(cause) {
                    clearInterval(input);
                    const passed = cause === "guest-poweroff" &&
                        output.split(/\\r?\\n/).some(line => line.trim() === "ISO_BOOT_OK");
                    report(passed ? "pass" : cause + "\\n" + output);
                },
                onError(error) { report(error.message + "\\n" + output); },
            });

            // Console callbacks collect input; a later task submits it safely.
            // Partial FIFO acceptance retains the unsubmitted bytes for retry.
            const input = setInterval(() => {
                if (pending.length) pending = pending.slice(runtime.consoleInput(pending));
            }, 10);
            runtime.startResolved({ version: 1, machine: "riscv64", memory_size: 512,
                console: "uart", bios: "/opensbi/fw_dynamic.bin", kernel: "/uboot/u-boot.bin",
                drive0: ${http ? '{ file: "/iso/blk.txt" }' : '{ provider: 1, capacity_sectors: media.length / 512 }'} });
        } catch (error) { report(error.message + "\\n" + output); }
        </script>`, directory, {
            timeoutMs: 300_000,
            response(url) {
                if (url.pathname === "/test.iso") return { status: 200, body: iso };
                if (url.pathname === "/iso/blk.txt") return {
                    status: 200,
                    body: `{block_size:512,n_block:${Math.ceil(iso.length / blockSize)}}`,
                };
                const block = /^\/iso\/blk(\d{9})\.bin$/.exec(url.pathname);
                if (!block) return undefined;
                const start = Number(block[1]) * blockSize;
                if (start >= iso.length) return { status: 404, body: "outside ISO" };
                const bytes = Buffer.alloc(blockSize);
                iso.copy(bytes, 0, start, Math.min(start + blockSize, iso.length));
                return { status: 200, body: bytes };
            },
        });
    } finally {
        await rm(directory, { recursive: true, force: true });
    }
});
