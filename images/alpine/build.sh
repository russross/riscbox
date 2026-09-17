#!/bin/sh
set -eu

IMAGE_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BIN_DIR=$(CDPATH= cd -- "$IMAGE_DIR/../bin" && pwd)
BUILD_DIR="$IMAGE_DIR/build"
ROOTFS_IMAGE="$BUILD_DIR/rootfs.ext4"

mkdir -p "$BUILD_DIR"
"$BIN_DIR/build-kernel"
"$BIN_DIR/create-alpine-ext4" 64 "$ROOTFS_IMAGE"
"$BIN_DIR/run-image-setup" "$ROOTFS_IMAGE" "$IMAGE_DIR/setup.sh"
"$BIN_DIR/build-distribution" "$IMAGE_DIR" "$ROOTFS_IMAGE"
