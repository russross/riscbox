#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
OUTPUT_DIR=${OUTPUT_DIR:-"$ROOT_DIR/image/risclet-web"}
ROOTFS_IMAGE=${ROOTFS_IMAGE:-"$ROOT_DIR/image/rootfs.ext4"}
KERNEL_IMAGE=${KERNEL_IMAGE:-"$ROOT_DIR/image/build/kernel/arch/riscv/boot/Image"}
OPENSBI=${OPENSBI:-/usr/lib/riscv64-linux-gnu/opensbi/generic/fw_jump.bin}

for required_file in "$ROOT_DIR/splitimg" \
    "$ROOTFS_IMAGE" "$KERNEL_IMAGE" "$OPENSBI"; do
    if [ ! -f "$required_file" ]; then
        echo "missing required file: $required_file" >&2
        exit 1
    fi
done

mkdir -p "$OUTPUT_DIR"
rm -rf "$OUTPUT_DIR/drive" "$OUTPUT_DIR/fs"
mkdir -p "$OUTPUT_DIR/drive"
install -m 644 "$OPENSBI" "$OUTPUT_DIR/fw_jump.bin"
install -m 644 "$KERNEL_IMAGE" "$OUTPUT_DIR/linux"
"$ROOT_DIR/splitimg" "$ROOTFS_IMAGE" "$OUTPUT_DIR/drive"

echo "created Risclet web assets in $OUTPUT_DIR"
