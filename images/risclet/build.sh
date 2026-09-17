#!/bin/sh
set -eu

IMAGE_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BIN_DIR=$(CDPATH= cd -- "$IMAGE_DIR/../bin" && pwd)
BUILD_DIR="$IMAGE_DIR/build"
ROOTFS_IMAGE="$BUILD_DIR/rootfs.ext4"
RISCLET_BINARY="$BUILD_DIR/risclet"
RISCLET_URL=https://github.com/russross/risclet/releases/download/v0.4.8/risclet-riscv64gc-unknown-linux-musl
RISCLET_SHA256=ede5c483810c3ed4137a95ee84e62cb0a04f75dcf396899f7f2dbf5fb41361fc

mkdir -p "$BUILD_DIR"
"$BIN_DIR/build-kernel"

if [ ! -f "$RISCLET_BINARY" ]; then
    curl --fail --location --show-error --output "$RISCLET_BINARY.part" \
        "$RISCLET_URL"
    mv "$RISCLET_BINARY.part" "$RISCLET_BINARY"
fi
printf '%s  %s\n' "$RISCLET_SHA256" "$RISCLET_BINARY" | \
    sha256sum --check --status || {
        echo "Risclet binary checksum failed: $RISCLET_BINARY" >&2
        exit 1
    }

"$BIN_DIR/create-alpine-ext4" 64 "$ROOTFS_IMAGE"
"$BIN_DIR/run-image-setup" \
    "$ROOTFS_IMAGE" "$IMAGE_DIR/setup.sh" "$RISCLET_BINARY"
"$BIN_DIR/build-distribution" "$IMAGE_DIR" "$ROOTFS_IMAGE"
