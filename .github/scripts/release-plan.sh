#!/bin/sh
set -eu

# Manual tests build the selected revision without consulting published tags.
current=$(cargo metadata --locked --no-deps --format-version 1 | jq -er '.packages[] | select(.name == "riscbox") | .version')
wasm=$(cargo metadata --locked --no-deps --format-version 1 | jq -er '.packages[] | select(.name == "riscbox-wasm") | .version')
test "$current" = "$wasm" || { echo 'Rust package versions differ' >&2; exit 1; }
tag="v$current"
publish=false
if [ "$EVENT_NAME" = push ] || [ "$RELEASE_MODE" = publish ]; then
    publish=true
    test "$GITHUB_REF" = refs/heads/main || { echo 'Publishing requires main' >&2; exit 1; }
fi

# Pushes release only when the workspace version increases across the push.
if [ "$EVENT_NAME" = push ]; then
    test "$BEFORE_SHA" != 0000000000000000000000000000000000000000 || { echo 'Previous push revision is unavailable' >&2; exit 1; }
    previous=$(git show "$BEFORE_SHA:Cargo.toml" | python3 -c 'import sys, tomllib; data=tomllib.loads(sys.stdin.read()); print(data["workspace"]["package"]["version"])')
    if [ "$current" = "$previous" ]; then
        echo 'No package version change'
        echo 'build=false' >> "$GITHUB_OUTPUT"
        exit 0
    fi
    .github/scripts/release_version.py "$previous" "$current"
fi

# A retry may reuse a published archive only when its tag names this revision.
tag_exists=false
release_exists=false
if [ "$publish" = true ]; then
    if git rev-parse --verify "refs/tags/$tag^{commit}" >/dev/null 2>&1; then
        tagged=$(git rev-parse "refs/tags/$tag^{commit}")
        test "$tagged" = "$GITHUB_SHA" || { echo "Tag $tag points to another commit" >&2; exit 1; }
        tag_exists=true
    fi
    releases=$(gh api --paginate "repos/$GITHUB_REPOSITORY/releases?per_page=100" --jq '.[].tag_name')
    if printf '%s\n' "$releases" | grep -Fxq "$tag"; then
        test "$tag_exists" = true || { echo "Release $tag has no matching tag" >&2; exit 1; }
        release_exists=true
    fi
fi

# Outputs keep build, archive reuse, and publication decisions explicit in YAML.
{
    echo 'build=true'
    echo "publish=$publish"
    echo "tag=$tag"
    echo "version=$current"
    echo "tag_exists=$tag_exists"
    echo "release_exists=$release_exists"
} >> "$GITHUB_OUTPUT"
