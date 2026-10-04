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
