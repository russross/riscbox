import assert from "node:assert/strict";
import test from "node:test";
import { compare, summarize } from "./compare.mjs";

// Synthetic records exercise reporting semantics without running the emulator.
function record(times) {
    return { schema: 1, fixture: { imageSha256: "image", workloadVersion: 1, config: { memory_size: 256 } },
        cachePolicy: "warm", browser: "Chrome", host: { cpu: "test" },
        options: { quantumMs: 20, profile: false, repetitions: times.length, counts: { loop: 1, sqlite: 1, compile: 1 } },
        samples: ["loop", "sqlite", "compile"].flatMap(workload => times.map(elapsedMs => ({
            workload, count: 1, elapsedMs, signature: workload,
        }))),
    };
}

test("medians and comparison use elapsed work rather than guest clock reports", () => {
    assert.equal(summarize(record([30, 10, 20]))[0].medianMs, 20);
    assert.equal(summarize(record([10, 20]))[0].medianMs, 15);
    const rows = compare(record([30, 10, 20]), record([5, 15, 10]));
    assert.equal(rows[0].speedup, 2);
    assert.equal(rows[0].changePercent, -50);
});

test("incompatible cache, fixture, workload, or correctness results are rejected", () => {
    const baseline = record([10, 20, 30]);
    for (const alter of [
        value => { value.fixture.imageSha256 = "other"; },
        value => { value.cachePolicy = "cold"; },
        value => { value.options.counts.loop = 2; value.samples.filter(s => s.workload === "loop").forEach(s => { s.count = 2; }); },
        value => { value.samples[0].signature = "wrong"; },
        value => { value.samples[0].elapsedMs = 0; },
        value => { value.options.profile = true; },
        value => { value.samples.push({ workload: "unknown", count: 1, elapsedMs: 10, signature: "unknown" }); },
        value => { value.samples.pop(); },
    ]) {
        const candidate = structuredClone(baseline);
        alter(candidate);
        assert.throws(() => compare(baseline, candidate));
    }
});
