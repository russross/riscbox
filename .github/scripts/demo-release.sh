#!/bin/sh
set -eu

# The checked-out workspace selects one published runtime, including prereleases.
test "$GITHUB_REF" = refs/heads/main || { echo 'Demo deployment requires main' >&2; exit 1; }
version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')
tag="v$version"
git ls-remote --exit-code --tags origin "refs/tags/$tag"
release=$(gh release view --repo "$GITHUB_REPOSITORY" --json tagName,isDraft,assets -- "$tag")
printf '%s\n' "$release" | jq -e --arg tag "$tag" '.isDraft == false and .tagName == $tag' >/dev/null || {
    echo "Demo deployment requires published release $tag" >&2; exit 1;
}

# The exact versioned asset is the only runtime input to demo assembly.
asset="riscbox-$version.tar.gz"
printf '%s\n' "$release" | jq -e --arg asset "$asset" 'any(.assets[]; .name == $asset)' >/dev/null || {
    echo "Release $tag is missing $asset" >&2; exit 1;
}
mkdir -p build/releases
gh release download "$tag" --repo "$GITHUB_REPOSITORY" --pattern "$asset" --dir build/releases --clobber
{
    echo "version=$version"
    echo "tag=$tag"
    echo "archive=build/releases/$asset"
} >> "$GITHUB_OUTPUT"
