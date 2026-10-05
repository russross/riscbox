import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

// Stub remote and revision discovery while exercising the actual release decisions.
function plan(overrides = {}) {
    const directory = mkdtempSync(join(tmpdir(), "riscbox-release-plan-"));
    const output = join(directory, "outputs");
    const calls = join(directory, "calls");
    const commands = {
        cargo: 'printf \'{"packages":[{"name":"riscbox","version":"%s"},{"name":"riscbox-wasm","version":"%s"}]}\\n\' "$CURRENT" "$WASM_VERSION"',
        git: `
            echo "$*" >> "$CALLS"
            if [ "$1" = show ]; then
                printf '[workspace.package]\\nversion = "%s"\\n' "$PREVIOUS"
            elif [ "$TAGGED_SHA" != absent ]; then
                echo "$TAGGED_SHA"
            else
                exit 1
            fi`,
        gh: 'echo "$*" >> "$CALLS"; test "$API_FAILURE" = false || exit 1; printf "%s\\n" "$RELEASE_TAGS"',
    };
    try {
        for (const [name, body] of Object.entries(commands)) {
            writeFileSync(join(directory, name), `#!/bin/sh\nset -eu\n${body}\n`, { mode: 0o755 });
        }
        writeFileSync(output, "");
        writeFileSync(calls, "");
        const result = spawnSync(resolve(".github/scripts/release-plan.sh"), [], {
            encoding: "utf8",
            env: {
                ...process.env,
                PATH: `${directory}:${process.env.PATH}`,
                GITHUB_OUTPUT: output, CALLS: calls,
                EVENT_NAME: "workflow_dispatch", RELEASE_MODE: "test",
                GITHUB_REF: "refs/heads/experiment", GITHUB_SHA: "selected",
                GITHUB_REPOSITORY: "russross/riscbox", BEFORE_SHA: "previous",
                CURRENT: "2026.9.31", WASM_VERSION: "2026.9.31", PREVIOUS: "2026.9.30",
                TAGGED_SHA: "absent", RELEASE_TAGS: "", API_FAILURE: "false",
                ...overrides,
            },
        });
        return {
            status: result.status, stderr: result.stderr,
            outputs: Object.fromEntries(readFileSync(output, "utf8").trim().split("\n").filter(Boolean).map(line => line.split("="))),
            calls: readFileSync(calls, "utf8"),
        };
    } finally {
        rmSync(directory, { recursive: true, force: true });
    }
}

// Test builds remain independent of existing releases, even on another branch.
test("manual test builds a tagged version on another branch without publication", () => {
    const result = plan({ TAGGED_SHA: "other", RELEASE_TAGS: "v2026.9.31" });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.outputs.build, "true");
    assert.equal(result.outputs.publish, "false");
    assert.equal(result.calls, "");
});

test("unchanged push skips builds and remote discovery", () => {
    const result = plan({ EVENT_NAME: "push", GITHUB_REF: "refs/heads/main", PREVIOUS: "2026.9.31" });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.outputs.build, "false");
    assert.equal(result.outputs.publish, undefined);
    assert.doesNotMatch(result.calls, /api /);
});

test("increasing version on main publishes a new release", () => {
    const result = plan({ EVENT_NAME: "push", GITHUB_REF: "refs/heads/main" });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.outputs.publish, "true");
    assert.equal(result.outputs.tag, "v2026.9.31");
    assert.equal(result.outputs.release_exists, "false");
});

test("decreasing version fails before remote discovery", () => {
    const result = plan({ EVENT_NAME: "push", GITHUB_REF: "refs/heads/main", PREVIOUS: "2026.10.1" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /must increase/);
    assert.doesNotMatch(result.calls, /api /);
});

// Retries preserve published bytes, and conflicting tags never replace releases.
test("manual publish retries an existing release at the selected commit", () => {
    const result = plan({ RELEASE_MODE: "publish", GITHUB_REF: "refs/heads/main", TAGGED_SHA: "selected", RELEASE_TAGS: "v2026.9.30\nv2026.9.31" });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.outputs.build, "true");
    assert.equal(result.outputs.publish, "true");
    assert.equal(result.outputs.tag_exists, "true");
    assert.equal(result.outputs.release_exists, "true");
});

test("manual publish can complete a tag with no release", () => {
    const result = plan({ RELEASE_MODE: "publish", GITHUB_REF: "refs/heads/main", TAGGED_SHA: "selected" });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.outputs.tag_exists, "true");
    assert.equal(result.outputs.release_exists, "false");
});

test("manual publish rejects another branch", () => {
    const result = plan({ RELEASE_MODE: "publish" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /Publishing requires main/);
});

test("manual publish rejects an existing tag on another commit", () => {
    const result = plan({ RELEASE_MODE: "publish", GITHUB_REF: "refs/heads/main", TAGGED_SHA: "other" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /points to another commit/);
});

test("remote API failure does not masquerade as an unpublished release", () => {
    const result = plan({ RELEASE_MODE: "publish", GITHUB_REF: "refs/heads/main", API_FAILURE: "true" });
    assert.notEqual(result.status, 0);
    assert.equal(result.outputs.publish, undefined);
});

test("mismatched crate versions fail in test mode", () => {
    const result = plan({ WASM_VERSION: "2026.9.30" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /versions differ/);
});

// Demo selection owns a read-only release lookup and a pinned archive download, without Cargo or tags.
function demoArchive(overrides = {}, metadata = {}) {
    const directory = mkdtempSync(join(tmpdir(), "riscbox-demo-release-"));
    const output = join(directory, "outputs");
    const calls = join(directory, "calls");
    const release = { tagName: "v2026.9.31", isDraft: false,
        assets: [{ name: "riscbox-2026.9.31.tar.gz" }], ...metadata };
    try {
        writeFileSync(join(directory, "gh"), `#!/bin/sh
set -eu
echo "$*" >> "$CALLS"
test "$API_FAILURE" = false || exit 1
if [ "$1 $2" = 'release view' ]; then
    cat "$METADATA"
elif [ "$1 $2" = 'release download' ]; then
    test "$DOWNLOAD_FAILURE" = false || exit 1
    printf 'posted bytes' > build/releases/riscbox-2026.9.31.tar.gz
else
    echo 'unexpected GitHub operation' >&2; exit 1
fi
`, { mode: 0o755 });
        writeFileSync(join(directory, "metadata"), JSON.stringify(release));
        writeFileSync(output, "");
        writeFileSync(calls, "");
        const result = spawnSync(resolve(".github/scripts/demo-release.sh"), [], {
            cwd: directory, encoding: "utf8",
            env: { ...process.env, PATH: `${directory}:${process.env.PATH}`,
                GITHUB_REF: "refs/heads/main", GITHUB_REPOSITORY: "russross/riscbox",
                GITHUB_OUTPUT: output, CALLS: calls, METADATA: join(directory, "metadata"),
                API_FAILURE: "false", DOWNLOAD_FAILURE: "false", DEMO_RELEASE_TAG: "", ...overrides },
        });
        const outputs = Object.fromEntries(readFileSync(output, "utf8").trim().split("\n").filter(Boolean).map(line => line.split("=")));
        return { status: result.status, stderr: result.stderr, outputs,
            calls: readFileSync(calls, "utf8"),
            archive: result.status === 0 ? readFileSync(join(directory, outputs.archive), "utf8") : undefined };
    } finally { rmSync(directory, { recursive: true, force: true }); }
}

test("demo defaults to the latest posted release and pins its archive download", () => {
    const result = demoArchive();
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.outputs.tag, "v2026.9.31");
    assert.equal(result.outputs.archive, "build/releases/riscbox-2026.9.31.tar.gz");
    assert.equal(result.archive, "posted bytes");
    assert.match(result.calls, /^release view --repo russross\/riscbox --json tagName,isDraft,assets\n/);
    assert.match(result.calls, /release download v2026\.9\.31 --repo russross\/riscbox --pattern riscbox-2026\.9\.31\.tar\.gz/);
});

test("demo can select an explicit published release independently of the source version", () => {
    const result = demoArchive({ DEMO_RELEASE_TAG: "v2026.9.31" });
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.calls, /release view .* -- v2026\.9\.31\n/);
    assert.equal(result.outputs.tag, "v2026.9.31");
});

test("demo deployment rejects another branch before release lookup", () => {
    const result = demoArchive({ GITHUB_REF: "refs/heads/experiment" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /requires main/);
    assert.equal(result.calls, "");
});

test("demo rejects draft releases and missing or mismatched runtime archives", () => {
    for (const metadata of [{ isDraft: true }, { assets: [] },
        { assets: [{ name: "riscbox-2026.9.30.tar.gz" }] }]) {
        const result = demoArchive({}, metadata);
        assert.notEqual(result.status, 0);
        assert.equal(result.outputs.archive, undefined);
        assert.doesNotMatch(result.calls, /release download/);
    }
});

test("demo reports remote lookup or download failures without deployment outputs", () => {
    for (const failure of [{ API_FAILURE: "true" }, { DOWNLOAD_FAILURE: "true" }]) {
        const result = demoArchive(failure);
        assert.notEqual(result.status, 0);
        assert.equal(result.outputs.archive, undefined);
        if (failure.API_FAILURE) assert.doesNotMatch(result.calls, /release download/);
        else assert.match(result.calls, /release download/);
    }
});

test("demo rejects unsafe release tags before writing workflow outputs", () => {
    const result = demoArchive({}, { tagName: "v2026.9.31\narchive=other" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /Unsupported release tag/);
    assert.deepEqual(result.outputs, {});
    assert.doesNotMatch(result.calls, /release download/);
});
