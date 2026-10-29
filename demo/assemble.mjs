import { cp, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { execFileSync } from "node:child_process";

try {
    // The release subtree is copied in full. The app only adds its own files beside it.
    const name = (await readFile("build/release.name", "utf8")).trim();
    const release = join("build/release", name);
    await mkdir("dist", { recursive: true });
    await rm("dist/riscbox", { recursive: true, force: true });
    await cp(release, "dist/riscbox", { recursive: true, force: true });
    await cp("web", "dist", { recursive: true, force: true });
    await rm("dist/examples", { recursive: true, force: true });
    await cp("bsd-games-3.3", "dist/examples", {
        recursive: true, force: true,
        filter: async source => (await stat(source)).isDirectory() ||
            /(?:\.(?:c|h|6|md)|\/(?:Makefile|LICENSE))$/.test(source),
    });
    const assets = (await readdir(release)).filter(asset => /^riscbox-[0-9a-f]{8}$/.test(asset));
    if (assets.length !== 1) throw new Error("release must have one asset directory");
    const assetDir = assets[0];
    // Published archives use their release tag; test builds supply their source commit.
    const documentationRef = process.env.DOCUMENTATION_REF || `v${name.slice("riscbox-".length)}`;
    const documentationBase = `https://github.com/russross/riscbox/blob/${encodeURIComponent(documentationRef)}/`;
    const indexPath = "dist/index.html";
    const index = await readFile(indexPath, "utf8");
    await writeFile(indexPath, index.replaceAll("{{DOCUMENTATION_BASE}}", documentationBase)
        .replaceAll("{{ASSET_DIR}}", assetDir));
    await rm("dist/docs", { recursive: true, force: true });

    // Boot assets and the disk splitter come exclusively from the selected release.
    // Splitting uses the packaged executable, with its regular public command line.
    const split = execFileSync(join(release, "splitimg.py"), ["build/rootfs.ext4", "dist", "256"], { encoding: "utf8" });
    const drive = split.trim().split(" ")[0];
    await writeFile("dist/riscbox.cfg", JSON.stringify({
        version: 1, machine: "riscv64", memory_size: 128,
        bios: `riscbox/${assetDir}/fw_dynamic.bin`, kernel: `riscbox/${assetDir}/linux`,
        cmdline: "root=/dev/vda rw rootfstype=ext4 console=hvc0",
        console: "virtio", uart_output: true,
        drive0: { file: `${drive}/blk.txt` },
        fs0: { server: "shared", tag: "shared" },
    }, null, 2) + "\n");

    // A manifest declares file bodies to fetch before an explicit share replacement.
    async function files(directory, prefix = "") {
        const paths = [];
        for (const entry of await readdir(directory, { withFileTypes: true })) {
            const path = prefix + entry.name;
            if (entry.isDirectory()) paths.push(...await files(join(directory, entry.name), `${path}/`));
            else if (entry.isFile()) paths.push(path);
        }
        return paths.sort();
    }
    const trees = [];
    for (const entry of await readdir("dist/examples", { withFileTypes: true })) {
        if (!entry.isDirectory()) continue;
        const id = entry.name;
        trees.push({ id, files: await files(join("dist/examples", id)) });
    }
    await writeFile("dist/examples.json", JSON.stringify(trees, null, 2) + "\n");

    // Keep original URLs usable while supplying compressed siblings for web servers.
    for (const path of await files("dist")) {
        if (path.startsWith("riscbox/") || /\.(?:br|cfg|txt|md|6)$/.test(path) ||
            /(?:^|\/)LICENSE$/.test(path) || path === ".built") continue;
        const source = join("dist", path);
        try {
            if ((await stat(`${source}.br`)).mtimeMs >= (await stat(source)).mtimeMs) continue;
        } catch (error) {
            if (error.code !== "ENOENT") throw error;
        }
        execFileSync("brotli", ["-q", "11", "-f", "-k", source]);
    }
} catch (error) {
    console.error(`Demo assembly failed: ${error.message}`);
    process.exitCode = 1;
}
