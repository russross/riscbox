#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
DEMO_DIR="$SCRIPT_DIR/web/demo"
ROOTFS_IMAGE=${ROOTFS_IMAGE:-"$SCRIPT_DIR/build/rootfs.ext4"}
KERNEL_IMAGE=${KERNEL_IMAGE:-"$ROOT_DIR/images/build/kernel/arch/riscv/boot/Image"}
OPENSBI=${OPENSBI:-/usr/lib/riscv64-linux-gnu/opensbi/generic/fw_jump.bin}
RISCBOX=${RISCBOX:-"$ROOT_DIR/riscbox"}
DIOD=${DIOD:-/usr/sbin/diod}
DIOD_DEBUG=${DIOD_DEBUG:-0}

for required_file in "$ROOTFS_IMAGE" "$KERNEL_IMAGE" "$OPENSBI"; do
    if [ ! -f "$required_file" ]; then
        echo "missing required file: $required_file" >&2
        exit 1
    fi
done
for required_command in "$RISCBOX" "$DIOD"; do
    if [ ! -x "$required_command" ]; then
        echo "missing executable: $required_command" >&2
        exit 1
    fi
done
if [ "$(id -u)" -ne 1000 ]; then
    echo "the Risclet image expects the host export user to have uid 1000" >&2
    exit 1
fi

case "$DEMO_DIR" in
    *[!A-Za-z0-9_./-]*)
        echo "demo path contains characters that cannot be passed to the guest: $DEMO_DIR" >&2
        exit 1
        ;;
esac

RUN_DIR=$(mktemp -d)
SOCKET_PATH="$RUN_DIR/diod.sock"
CONFIG_PATH="$RUN_DIR/risclet.cfg"
DIOD_LOG="$RUN_DIR/diod.log"
DIOD_PID=

cleanup() {
    if [ -n "$DIOD_PID" ]; then
        kill "$DIOD_PID" 2>/dev/null || true
        wait "$DIOD_PID" 2>/dev/null || true
    fi
    rm -rf "$RUN_DIR"
}
trap cleanup EXIT HUP INT TERM

cat > "$CONFIG_PATH" <<EOF
{
    version: 1,
    machine: "riscv64",
    memory_size: 256,
    bios: "$OPENSBI",
    kernel: "$KERNEL_IMAGE",
    cmdline: "root=/dev/vda rw rootfstype=ext4 console=hvc0 quiet loglevel=0",
    drive0: { file: "$ROOTFS_IMAGE" },
    fs0: { socket: "$SOCKET_PATH", tag: "risclet" },
    console: "virtio",
}
EOF

"$DIOD" --foreground --no-auth --listen "$SOCKET_PATH" --export "$DEMO_DIR" \
    --logdest stderr --debug "$DIOD_DEBUG" >"$DIOD_LOG" 2>&1 &
DIOD_PID=$!

i=0
while [ ! -S "$SOCKET_PATH" ]; do
    if ! kill -0 "$DIOD_PID" 2>/dev/null; then
        cat "$DIOD_LOG" >&2
        echo "diod exited before creating its socket" >&2
        exit 1
    fi
    i=$((i + 1))
    if [ "$i" -ge 100 ]; then
        cat "$DIOD_LOG" >&2
        echo "timed out waiting for diod socket" >&2
        exit 1
    fi
    sleep 0.05
done

"$RISCBOX" -append "risclet.aname=$DEMO_DIR risclet.uname=$(id -un)" \
    "$CONFIG_PATH"
