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
for asset in riscbox.js riscbox.wasm riscbox.d.ts splitimg.py README.md STORAGE-ABI.md NINEP.md DEPLOYMENT.md; do
    test -f "$release/$asset" || { echo "release is missing $asset" >&2; exit 1; }
done
rm -rf build/release
mv "$stage" build/release
basename "$release" > build/release.name
printf '%s\n' "$digest" > build/release.sha256
