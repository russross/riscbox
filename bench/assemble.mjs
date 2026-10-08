import { cp, mkdir, readFile, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";

try {
    // Assembly owns the guest fixture; runtime selection belongs to the runner.
    await mkdir("dist", { recursive: true });
    const bios = (await readFile("../opensbi/.asset-name", "utf8")).trim();
    const kernel = (await readFile("../kernel/.asset-name", "utf8")).trim();
    await cp(`../opensbi/${bios}`, `dist/${bios}`);
    await cp(`../kernel/${kernel}`, `dist/${kernel}`);
    const split = execFileSync("../tools/splitimg.py", ["build/rootfs.ext4", "dist", "512"], { encoding: "utf8" });
    const drive = split.trim().split(" ")[0];
    const config = {
        version: 1, machine: "riscv64", memory_size: 256,
        bios, kernel, console: "virtio", uart_output: true,
        cmdline: "root=/dev/vda rw rootfstype=ext4 console=hvc0 init=/sbin/bench-init",
        drive0: { file: `${drive}/blk.txt` },
    };
    await writeFile("dist/riscbox.cfg", JSON.stringify(config, null, 2) + "\n");
    const image = await readFile("build/rootfs.ext4");
    await writeFile("dist/fixture.json", JSON.stringify({
        schema: 1, workloadVersion: 1, sqlite: "3.50.4", alpine: "3.24.2",
        imageSha256: createHash("sha256").update(image).digest("hex"),
        packages: (await readFile("build/packages.txt", "utf8")).trim().split("\n"), config,
        toolchain: (await readFile("build/toolchain.txt", "utf8")).trim(),
    }, null, 2) + "\n");
} catch (error) {
    console.error(`Benchmark assembly failed: ${error.message}`);
    process.exitCode = 1;
}
