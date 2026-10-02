import { resolve } from "node:path";
import { runChromePage as runSharedChromePage } from "../client-core/tests/chrome.mjs";

// Repository probes serve runtime and image assets alongside the shared fixtures.
export function runChromePage(html, directory, options = {}) {
    return runSharedChromePage(html, directory, { ...options, root: resolve(import.meta.dirname, "..") });
}
