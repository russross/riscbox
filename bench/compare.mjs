#!/usr/bin/env node
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

// Reports accept only comparable fixtures and workloads. Repetition counts may
// differ, but all samples must agree on correctness and fixed work performed.
export function summarize(record) {
    const workloads = ["loop", "sqlite", "compile"];
    if (record.schema !== 1 || !Array.isArray(record.samples) ||
        !Number.isSafeInteger(record.options?.repetitions) || record.options.repetitions < 1 ||
        record.samples.length !== workloads.length * record.options.repetitions ||
        record.samples.some(sample => !workloads.includes(sample.workload))) {
        throw new Error("unsupported or empty benchmark result");
    }
    return workloads.map(workload => {
        const samples = record.samples.filter(sample => sample.workload === workload);
        const count = record.options.counts?.[workload];
        if (!Number.isSafeInteger(count) || count < 1 ||
            samples.length !== record.options.repetitions || samples.some(sample =>
            !Number.isFinite(sample.elapsedMs) || sample.elapsedMs <= 0 ||
            sample.count !== count || typeof sample.signature !== "string" || !sample.signature ||
            sample.signature !== samples[0].signature)) {
            throw new Error(`invalid ${workload} samples`);
        }
        const times = samples.map(sample => sample.elapsedMs).sort((a, b) => a - b);
        const middle = Math.floor(times.length / 2);
        const medianMs = times.length % 2 ? times[middle] : (times[middle - 1] + times[middle]) / 2;
        return { workload, medianMs, minimumMs: times[0], maximumMs: times.at(-1), signature: samples[0].signature };
    });
}

export function compare(baseline, candidate) {
    for (const [label, a, b] of [
        ["image", baseline.fixture.imageSha256, candidate.fixture.imageSha256],
        ["workload version", baseline.fixture.workloadVersion, candidate.fixture.workloadVersion],
        ["machine configuration", JSON.stringify(baseline.fixture.config), JSON.stringify(candidate.fixture.config)],
        ["cache policy", baseline.cachePolicy, candidate.cachePolicy],
        ["browser", baseline.browser, candidate.browser],
        ["host", JSON.stringify(baseline.host), JSON.stringify(candidate.host)],
        ["quantum duration", baseline.options.quantumMs, candidate.options.quantumMs],
        ["profiling mode", baseline.options.profile, candidate.options.profile],
    ]) {
        if (a === undefined || a !== b) throw new Error(`cannot compare different ${label}`);
    }
    const before = summarize(baseline);
    const after = summarize(candidate);
    return before.map((sample, index) => {
        const other = after[index];
        if (baseline.options.counts[sample.workload] !== candidate.options.counts[sample.workload]) {
            throw new Error(`cannot compare different ${sample.workload} counts`);
        }
        if (sample.signature !== other.signature) throw new Error(`${sample.workload} correctness mismatch`);
        return { workload: sample.workload, baselineMs: sample.medianMs, candidateMs: other.medianMs,
            speedup: sample.medianMs / other.medianMs, changePercent: (other.medianMs / sample.medianMs - 1) * 100 };
    });
}

// Keeping comparison independent permits archived results from arbitrary builds.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
    try {
        if (process.argv.length !== 4) throw new Error("usage: node bench/compare.mjs BASELINE.json CANDIDATE.json");
        const baseline = JSON.parse(await readFile(process.argv[2], "utf8"));
        const candidate = JSON.parse(await readFile(process.argv[3], "utf8"));
        console.log(`${baseline.runtime.label} → ${candidate.runtime.label}`);
        console.log("Workload    Baseline(s)  Candidate(s)  Time change  Speedup");
        for (const row of compare(baseline, candidate)) {
            console.log(`${row.workload.padEnd(10)}  ${(row.baselineMs / 1000).toFixed(3).padStart(11)}  ${(row.candidateMs / 1000).toFixed(3).padStart(12)}  ${row.changePercent.toFixed(1).padStart(10)}%  ${row.speedup.toFixed(3)}x`);
        }
        console.log(`WASM bytes: ${baseline.runtime.wasmBytes} → ${candidate.runtime.wasmBytes}; gzip: ${baseline.runtime.wasmGzipBytes} → ${candidate.runtime.wasmGzipBytes}`);
    } catch (error) {
        console.error(`Comparison failed: ${error.message}`);
        process.exitCode = 1;
    }
}
