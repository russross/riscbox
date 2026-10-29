#!/bin/sh
set -eu

# Extract the archive unchanged so the demo can also consume a downloaded release.
if [ "$#" -ne 1 ]; then
    echo "usage: $0 RELEASE_ARCHIVE" >&2
    exit 2
fi
archive=$(realpath "$1")
digest=$(sha256sum "$archive" | cut -d ' ' -f 1)
if [ -f build/release.sha256 ] && [ "$(cat build/release.sha256)" = "$digest" ]; then
    exit 0
fi
mkdir -p build
stage=$(mktemp -d build/release.XXXXXX)
trap 'rm -rf "$stage"' EXIT HUP INT TERM
tar -xzf "$archive" -C "$stage"
release=$(find "$stage" -mindepth 1 -maxdepth 1 -type d)
for asset in riscbox.d.ts network.d.ts splitimg.py README.md STORAGE-ABI.md NINEP.md API.md HOWTO.md; do
    test -f "$release/$asset" || { echo "release is missing $asset" >&2; exit 1; }
done
assets=$(find "$release" -mindepth 1 -maxdepth 1 -type d -name 'riscbox-*')
test -d "$assets" || { echo "release asset directory is missing" >&2; exit 1; }
for asset in "$assets"/*.br; do
    brotli -d -k "$asset"
done
for asset in riscbox.js riscbox.wasm linux fw_dynamic.bin u-boot.bin network.js; do
    test -f "$assets/$asset" || { echo "release is missing $asset" >&2; exit 1; }
done
rm -rf build/release
mv "$stage" build/release
basename "$release" > build/release.name
printf '%s\n' "$digest" > build/release.sha256
