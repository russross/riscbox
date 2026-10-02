import path from "node:path";
import { fileURLToPath } from "node:url";
import { clientCoreModules, clientCoreRules } from "../../../client-core/build/webpack.mjs";

const directory = path.dirname(fileURLToPath(import.meta.url));

export default {
    mode: "development",
    entry: "./index.ts",
    output: {
        path: path.resolve(directory, "../build/ui"),
        filename: "bundle.js",
    },
    module: {
        rules: [
            ...clientCoreRules(),
            { test: /\.ts$/, use: "ts-loader", exclude: /node_modules/ },
            { test: /\.css$/i, use: ["style-loader", "css-loader"] },
        ],
    },
    resolve: {
        extensions: [".ts", ".js"],
        extensionAlias: { ".js": [".ts", ".js"] },
        modules: [clientCoreModules, path.resolve(directory, "node_modules"), "node_modules"],
    },
};
