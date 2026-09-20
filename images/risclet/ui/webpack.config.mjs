import path from "node:path";
import { fileURLToPath } from "node:url";

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
            { test: /\.ts$/, use: "ts-loader", exclude: /node_modules/ },
            { test: /\.css$/i, use: ["style-loader", "css-loader"] },
        ],
    },
    resolve: {
        extensions: [".ts", ".js"],
        extensionAlias: { ".js": [".ts", ".js"] },
        modules: [path.resolve(directory, "node_modules"), "node_modules"],
    },
};
