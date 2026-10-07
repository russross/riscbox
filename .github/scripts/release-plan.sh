#!/bin/sh
set -eu

# Both crates inherit the workspace version and must agree before publication.
test "$GITHUB_REF" = refs/heads/main || { echo 'Publishing requires main' >&2; exit 1; }
metadata=$(cargo metadata --locked --no-deps --format-version 1)
current=$(printf '%s\n' "$metadata" | jq -er '.packages[] | select(.name == "riscbox") | .version')
wasm=$(printf '%s\n' "$metadata" | jq -er '.packages[] | select(.name == "riscbox-wasm") | .version')
test "$current" = "$wasm" || { echo 'Rust package versions differ' >&2; exit 1; }
tag="v$current"

# Untagged versions retry on every main push, including corrections after failed builds.
if git show-ref --verify --quiet "refs/tags/$tag"; then
    echo "Cargo workspace version $current is already tagged"
    echo 'build=false' >> "$GITHUB_OUTPUT"
    exit 0
fi
{
    echo 'build=true'
    echo "tag=$tag"
    echo "version=$current"
} >> "$GITHUB_OUTPUT"
