#!/bin/sh
set -eu

# Demo deployment selects published assets independently of the current Cargo version.
test "$GITHUB_REF" = refs/heads/main || { echo 'Demo deployment requires main' >&2; exit 1; }
requested=${DEMO_RELEASE_TAG:-}
if [ -n "$requested" ]; then
    release=$(gh release view --repo "$GITHUB_REPOSITORY" --json tagName,isDraft,assets -- "$requested")
else
    release=$(gh release view --repo "$GITHUB_REPOSITORY" --json tagName,isDraft,assets)
fi
test "$(printf '%s\n' "$release" | jq -er '.isDraft == false')" = true || {
    echo 'Demo deployment requires a published release' >&2; exit 1;
}
tag=$(printf '%s\n' "$release" | jq -er '.tagName')
case "$tag" in
    ''|*[!A-Za-z0-9.+-]*) echo "Unsupported release tag: $tag" >&2; exit 1 ;;
esac
case "$tag" in
    v[0-9]*) version=${tag#v} ;;
    *) echo "Unsupported release tag: $tag" >&2; exit 1 ;;
esac

# Pin the resolved tag for download so a changing latest release cannot mix assets.
asset="riscbox-$version.tar.gz"
printf '%s\n' "$release" | jq -e --arg asset "$asset" 'any(.assets[]; .name == $asset)' >/dev/null || {
    echo "Release $tag is missing $asset" >&2; exit 1;
}
mkdir -p build/releases
gh release download "$tag" --repo "$GITHUB_REPOSITORY" --pattern "$asset" --dir build/releases --clobber
{
    echo "tag=$tag"
    echo "archive=build/releases/$asset"
} >> "$GITHUB_OUTPUT"
