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
assets="$stage/assets"
mkdir -p "$package" "$assets"

# Only deployable runtime files and their documentation enter the archive.
install -m 644 target/wasm32-unknown-unknown/release/riscbox_wasm.wasm "$assets/riscbox.wasm"
install -m 644 build/js/riscbox.js "$assets/riscbox.js"
install -m 644 build/js/network/index.js "$assets/network.js"
install -m 644 kernel/linux opensbi/fw_dynamic.bin uboot/u-boot.bin "$assets/"
install -m 644 build/js/riscbox.d.ts "$package/riscbox.d.ts"
install -m 644 build/js/network/index.d.ts "$package/network.d.ts"
install -m 644 README.md STORAGE-ABI.md NINEP.md API.md HOWTO.md CHANGELOG.md LICENSE "$package/"
install -m 755 tools/splitimg.py "$package/splitimg.py"
install -m 644 js/network/README.md "$package/NETWORK.md"

# The digest covers sorted names and uncompressed bytes, independent of Brotli.
hash=$(cd "$assets" && sha256sum * | sha256sum | cut -c 1-8)
mkdir "$package/riscbox-$hash"
for asset in "$assets"/*; do
    brotli -q 11 -c "$asset" > "$package/riscbox-$hash/$(basename "$asset").br"
done

tar -C "$stage" -cf - "riscbox-$version" | gzip -9 -n > "$output.part"
mv "$output.part" "$output"
echo "$output"
