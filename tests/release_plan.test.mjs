import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

// Run the real scripts with isolated manifests and observable remote commands.
function runScript(script, overrides = {}, metadata = {}) {
    const directory = mkdtempSync(join(tmpdir(), "riscbox-release-"));
    const output = join(directory, "outputs");
    const calls = join(directory, "calls");
    const release = { tagName: "v2026.9.31", isDraft: false,
        assets: [{ name: "riscbox-2026.9.31.tar.gz" }], ...metadata };
    const commands = {
        cargo: 'printf \'{"packages":[{"name":"riscbox","version":"%s"},{"name":"riscbox-wasm","version":"%s"}]}\\n\' "$CURRENT" "$WASM_VERSION"',
        git: `echo "$*" >> "$CALLS"
            if [ "$1" = show-ref ]; then test "$TAG_EXISTS" = true
            elif [ "$1" = ls-remote ]; then test "$REMOTE_TAG" = true
            else echo 'unexpected git operation' >&2; exit 1; fi`,
        gh: `echo "$*" >> "$CALLS"
            test "$API_FAILURE" = false || exit 1
            if [ "$1 $2" = 'release view' ]; then cat "$METADATA"
            elif [ "$1 $2" = 'release download' ]; then
                test "$DOWNLOAD_FAILURE" = false || exit 1
                printf 'posted bytes' > build/releases/riscbox-2026.9.31.tar.gz
            elif [ "$1" = api ]; then
                case "$2" in
                    */contents/*) printf '[workspace.package]\\nversion = "%s"\\n' "$MAIN_VERSION" | base64 ;;
                    */commits/main) echo "$MAIN_SHA" ;;
                    *) exit 1 ;;
                esac
            else echo 'unexpected GitHub operation' >&2; exit 1; fi`,
    };
    try {
        for (const [name, body] of Object.entries(commands)) {
            writeFileSync(join(directory, name), `#!/bin/sh\nset -eu\n${body}\n`, { mode: 0o755 });
        }
        writeFileSync(join(directory, "Cargo.toml"), '[workspace.package]\nversion = "2026.9.31"\n');
        writeFileSync(join(directory, "metadata"), JSON.stringify(release));
        writeFileSync(output, "");
        writeFileSync(calls, "");
        const result = spawnSync(resolve(`.github/scripts/${script}.sh`), [], {
            cwd: directory, encoding: "utf8",
            env: { ...process.env, PATH: `${directory}:${process.env.PATH}`,
                GITHUB_OUTPUT: output, CALLS: calls, METADATA: join(directory, "metadata"),
                GITHUB_REF: "refs/heads/main", GITHUB_SHA: "selected",
                GITHUB_EVENT_NAME: "push", GITHUB_REPOSITORY: "russross/riscbox",
                CURRENT: "2026.9.31", WASM_VERSION: "2026.9.31", TAG_EXISTS: "false",
                REMOTE_TAG: "true", API_FAILURE: "false", DOWNLOAD_FAILURE: "false",
                BUILT_VERSION: "2026.9.31", MAIN_VERSION: "2026.9.31", MAIN_SHA: "selected",
                ...overrides },
        });
        const outputs = Object.fromEntries(readFileSync(output, "utf8").trim().split("\n").filter(Boolean).map(line => line.split("=")));
        return { status: result.status, stderr: result.stderr, outputs,
            calls: readFileSync(calls, "utf8"),
            archive: outputs.archive ? readFileSync(join(directory, outputs.archive), "utf8") : undefined };
    } finally {
        rmSync(directory, { recursive: true, force: true });
    }
}

// Tag existence gates all builds; no prior push revision or release API is needed.
test("a tagged version stops even when its tag names an older commit", () => {
    const result = runScript("release-plan", { TAG_EXISTS: "true" });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(result.outputs, { build: "false" });
    assert.equal(result.calls, "show-ref --verify --quiet refs/tags/v2026.9.31\n");
});

test("an untagged version builds, including a corrected push at the same version", () => {
    const result = runScript("release-plan");
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(result.outputs, { build: "true", version: "2026.9.31", tag: "v2026.9.31" });
    assert.doesNotMatch(result.calls, /api|show /);
});

test("release planning rejects another branch and mismatched crate versions", () => {
    for (const overrides of [{ GITHUB_REF: "refs/heads/experiment" }, { WASM_VERSION: "2026.9.30" }]) {
        const result = runScript("release-plan", overrides);
        assert.notEqual(result.status, 0);
        assert.deepEqual(result.outputs, {});
        assert.equal(result.calls, "");
    }
});

// Demo builds pin the source version rather than selecting a changing latest release.
test("demo downloads the published workspace version", () => {
    const result = runScript("demo-release");
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.outputs.version, "2026.9.31");
    assert.equal(result.outputs.tag, "v2026.9.31");
    assert.equal(result.archive, "posted bytes");
    assert.match(result.calls, /ls-remote --exit-code --tags origin refs\/tags\/v2026.9.31/);
    assert.match(result.calls, /release view .* -- v2026.9.31/);
    assert.match(result.calls, /release download v2026.9.31 .* --pattern riscbox-2026.9.31.tar.gz/);
});

test("demo rejects drafts, different versions, and missing archives before download", () => {
    for (const metadata of [{ isDraft: true }, { tagName: "v2026.9.30" }, { assets: [] },
        { assets: [{ name: "riscbox-2026.9.30.tar.gz" }] }]) {
        const result = runScript("demo-release", {}, metadata);
        assert.notEqual(result.status, 0);
        assert.deepEqual(result.outputs, {});
        assert.doesNotMatch(result.calls, /release download/);
    }
});

test("demo rejects another branch, missing tags, and remote failures", () => {
    for (const overrides of [{ GITHUB_REF: "refs/heads/experiment" }, { REMOTE_TAG: "false" },
        { API_FAILURE: "true" }, { DOWNLOAD_FAILURE: "true" }]) {
        const result = runScript("demo-release", overrides);
        assert.notEqual(result.status, 0);
        assert.deepEqual(result.outputs, {});
    }
});

// Freshness gates publication after a queued build finishes, for either entry point.
test("demo freshness accepts the current runtime for automatic and manual runs", () => {
    for (const event of ["push", "workflow_dispatch"]) {
        const result = runScript("demo-freshness", { GITHUB_EVENT_NAME: event });
        assert.equal(result.status, 0, result.stderr);
        assert.equal(result.outputs.current, "true");
    }
});

test("demo freshness rejects a superseded runtime version", () => {
    const result = runScript("demo-freshness", { MAIN_VERSION: "2026.10.1" });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.outputs.current, "false");
});

test("manual demo freshness rejects superseded sources at the same version", () => {
    const result = runScript("demo-freshness", { GITHUB_EVENT_NAME: "workflow_dispatch", MAIN_SHA: "newer" });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.outputs.current, "false");
});

test("freshness API failure supplies no deployment decision", () => {
    const result = runScript("demo-freshness", { API_FAILURE: "true" });
    assert.notEqual(result.status, 0);
    assert.deepEqual(result.outputs, {});
});
