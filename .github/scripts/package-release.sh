#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"

# The workspace manifest is the single source for both Rust package versions.
version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')
output=${1:-"build/releases/riscbox-$version.tar.gz"}
mkdir -p "$(dirname "$output")"
output=$(realpath -m "$output")
stage=$(mktemp -d)
trap 'rm -rf "$stage" "$output.part"' EXIT
package="$stage/riscbox-$version"
mkdir -p "$package/network"

# Only deployable runtime files and their documentation enter the archive.
install -m 644 target/wasm32-unknown-unknown/release/riscbox_wasm.wasm "$package/riscbox.wasm"
install -m 644 build/js/riscbox.js build/js/riscbox.d.ts "$package/"
for component in kernel opensbi uboot; do
    asset=$(cat "$component/.asset-name")
    install -m 644 "$component/$asset" "$package/$asset"
done
install -m 644 README.md CHANGELOG.md LICENSE "$package/"
install -m 644 js/network/README.md "$package/network/README.md"

# TypeScript output is pure ESM and includes its declarations for host authors.
for module in network; do
    for source in build/js/"$module"/*.js build/js/"$module"/*.d.ts; do
        test -f "$source"
        install -m 644 "$source" "$package/$module/$(basename "$source")"
    done
done

tar -C "$stage" -cf - "riscbox-$version" | gzip -9 -n > "$output.part"
mv "$output.part" "$output"
echo "$output"
