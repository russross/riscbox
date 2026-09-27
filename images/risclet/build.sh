#!/bin/sh
set -eu

IMAGE_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BIN_DIR=$(CDPATH= cd -- "$IMAGE_DIR/../bin" && pwd)
ROOT_DIR=$(CDPATH= cd -- "$IMAGE_DIR/../.." && pwd)
BUILD_DIR="$IMAGE_DIR/build"
ROOTFS_IMAGE="$BUILD_DIR/rootfs.ext4"
DISK_IMAGE="$BUILD_DIR/risclet.img"
UBOOT_PACKAGE="$BUILD_DIR/u-boot-qemu_2025.01-3+deb13u1_all.deb"
UBOOT_URL=https://deb.debian.org/debian/pool/main/u/u-boot/u-boot-qemu_2025.01-3+deb13u1_all.deb
UBOOT_SHA256=a3751b5d36a411457b2e7a2834cdce0e5d40493e6588a65d51c5603740a0c71c
UBOOT_BINARY="$BUILD_DIR/uboot-package/usr/lib/u-boot/qemu-riscv64_smode/u-boot.bin"
RISCLET_BINARY="$BUILD_DIR/risclet"
RISCLET_URL=https://github.com/russross/risclet/releases/download/v0.4.8/risclet-riscv64gc-unknown-linux-musl
RISCLET_SHA256=ede5c483810c3ed4137a95ee84e62cb0a04f75dcf396899f7f2dbf5fb41361fc

mkdir -p "$BUILD_DIR"
make -C "$ROOT_DIR" kernel
if [ ! -f "$UBOOT_PACKAGE" ]; then
    curl --fail --location --show-error --output "$UBOOT_PACKAGE.part" "$UBOOT_URL"
    mv "$UBOOT_PACKAGE.part" "$UBOOT_PACKAGE"
fi
printf '%s  %s\n' "$UBOOT_SHA256" "$UBOOT_PACKAGE" | \
    sha256sum --check --status || {
        echo "U-Boot package checksum failed: $UBOOT_PACKAGE" >&2
        exit 1
    }
dpkg-deb -x "$UBOOT_PACKAGE" "$BUILD_DIR/uboot-package"
if [ ! -s "$UBOOT_BINARY" ]; then
    echo "missing S-mode U-Boot binary: $UBOOT_BINARY" >&2
    exit 1
fi

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
    "$ROOTFS_IMAGE" "$IMAGE_DIR/setup.sh" "$RISCLET_BINARY" \
    "$BIN_DIR/mount-ephemeral-writes"
"$BIN_DIR/create-erofs-from-ext4" "$ROOTFS_IMAGE" "$BUILD_DIR/rootfs.erofs"
"$IMAGE_DIR/assemble-disk" "$ROOT_DIR/kernel/linux" \
    "$BUILD_DIR/rootfs.erofs" "$DISK_IMAGE"
BOOT_PAYLOAD="$UBOOT_BINARY" BOOT_PAYLOAD_NAME=u-boot.bin \
    "$BIN_DIR/build-distribution" "$IMAGE_DIR" "$DISK_IMAGE"
