#!/bin/sh
set -eu

# A queued build may finish after main advances to a different runtime version.
gh api "repos/$GITHUB_REPOSITORY/contents/Cargo.toml?ref=main" --jq .content | base64 --decode > main-Cargo.toml
latest=$(python3 -c 'import tomllib; print(tomllib.load(open("main-Cargo.toml", "rb"))["workspace"]["package"]["version"])')
current=true
if [ "$BUILT_VERSION" != "$latest" ]; then current=false; fi

# Manual runs must also use the current application sources on main.
if [ "$GITHUB_EVENT_NAME" = workflow_dispatch ]; then
    main_sha=$(gh api "repos/$GITHUB_REPOSITORY/commits/main" --jq .sha)
    if [ "$GITHUB_SHA" != "$main_sha" ]; then current=false; fi
fi
echo "current=$current" >> "$GITHUB_OUTPUT"
if [ "$current" = false ]; then echo 'Skipping superseded demo build'; fi
