#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
IMAGE_DIR="$ROOT_DIR/image"
ROOTFS_ARCHIVE=${ROOTFS_ARCHIVE:-"$IMAGE_DIR/alpine-minirootfs-3.24.1-riscv64.tar.gz"}
ALPINE_ISO=${ALPINE_ISO:-"$IMAGE_DIR/alpine-standard-3.24.1-riscv64.iso"}
BUILD_DIR=${BUILD_DIR:-"$IMAGE_DIR/build"}
ROOTFS_DIR="$BUILD_DIR/rootfs"
IMAGE_PATH=${IMAGE_PATH:-"$IMAGE_DIR/rootfs.ext4"}
IMAGE_SIZE_MB=${IMAGE_SIZE_MB:-32}
MKFS_EXT4=${MKFS_EXT4:-/usr/sbin/mkfs.ext4}
QEMU_RISCV64=${QEMU_RISCV64:-qemu-riscv64}
ROOT_PASSWORD=${ROOT_PASSWORD:-root}
RISCLET_URL=https://github.com/russross/risclet/releases/download/v0.4.7/risclet-riscv64gc-unknown-linux-musl
RISCLET_SHA256=7268ce6837980b8da2514535290559ac5bcb1efe9fff0941c08952361c11e860

require_file() {
    if [ ! -f "$1" ]; then
        echo "missing required file: $1" >&2
        exit 1
    fi
}

require_file "$ROOTFS_ARCHIVE"
require_file "$ALPINE_ISO"

if [ ! -x "$MKFS_EXT4" ]; then
    echo "mkfs.ext4 not found at $MKFS_EXT4; set MKFS_EXT4 to its path" >&2
    exit 1
fi
if ! command -v "$QEMU_RISCV64" >/dev/null 2>&1; then
    echo "RISC-V user emulator not found: $QEMU_RISCV64" >&2
    exit 1
fi
if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required to download image contents" >&2
    exit 1
fi

archive_release=$(tar -xOf "$ROOTFS_ARCHIVE" ./usr/lib/os-release |
    sed -n 's/^VERSION_ID=//p' | tr -d '"')
iso_release=$(dd if="$ALPINE_ISO" bs=2048 skip=31 count=1 2>/dev/null |
    tr -cd '[:print:]\n' | sed -n 's/.*Alpine Linux \([0-9][0-9.]*\).*/\1/p' | head -n 1)
if [ "$archive_release" != "3.24.1" ]; then
    echo "expected Alpine 3.24.1 minirootfs, found $archive_release" >&2
    exit 1
fi
if [ -n "$iso_release" ] && [ "$iso_release" != "$archive_release" ]; then
    echo "ISO release $iso_release does not match minirootfs $archive_release" >&2
    exit 1
fi

mkdir -p "$BUILD_DIR"
rm -rf "$ROOTFS_DIR"
mkdir -p "$ROOTFS_DIR"
tar -xpf "$ROOTFS_ARCHIVE" -C "$ROOTFS_DIR"

cp /etc/resolv.conf "$ROOTFS_DIR/etc/resolv.conf"
"$QEMU_RISCV64" -L "$ROOTFS_DIR" "$ROOTFS_DIR/sbin/apk" \
    --root "$ROOTFS_DIR" \
    --arch riscv64 \
    --repositories-file /dev/null \
    --no-cache \
    --no-scripts \
    add \
    --repository https://dl-cdn.alpinelinux.org/alpine/v3.24/main \
    --repository https://dl-cdn.alpinelinux.org/alpine/v3.24/community \
    fbdebug \
    fbkeyboard \
    libgcc
rm -f "$ROOTFS_DIR/etc/resolv.conf"

mkdir -p "$ROOTFS_DIR/etc/init.d" "$ROOTFS_DIR/dev" "$ROOTFS_DIR/proc" \
    "$ROOTFS_DIR/sys" "$ROOTFS_DIR/run" "$ROOTFS_DIR/tmp"
chmod 1777 "$ROOTFS_DIR/tmp"

cat > "$ROOTFS_DIR/etc/init.d/rcS" <<'EOF'
#!/bin/sh
mount -t proc proc /proc
mount -t sysfs sysfs /sys
[ -c /dev/null ] || mount -t devtmpfs devtmpfs /dev
mount -t tmpfs tmpfs /run
hostname riscbox
ip link set lo up 2>/dev/null || true
mount -t 9p -o trans=virtio,version=9p2000.L risclet /home/student
EOF
chmod 755 "$ROOTFS_DIR/etc/init.d/rcS"

cat > "$ROOTFS_DIR/etc/init.d/getty-console" <<'EOF'
#!/bin/sh
case " $(cat /proc/cmdline) " in
    *" console=hvc0 "*) console=hvc0 ;;
    *) console=ttyS0 ;;
esac
exec /sbin/getty -L 115200 "$console" vt100
EOF
chmod 755 "$ROOTFS_DIR/etc/init.d/getty-console"

cat > "$ROOTFS_DIR/etc/inittab" <<'EOF'
::sysinit:/etc/init.d/rcS
::respawn:/etc/init.d/getty-console
::ctrlaltdel:/sbin/reboot
::shutdown:/bin/umount -a -r
EOF

cat > "$ROOTFS_DIR/etc/fstab" <<'EOF'
/dev/vda  /      ext4  defaults,noatime  0 1
proc      /proc  proc  defaults           0 0
sysfs     /sys   sysfs defaults           0 0
devpts    /dev/pts devpts gid=5,mode=620  0 0
tmpfs     /run   tmpfs defaults           0 0
EOF

printf 'riscbox\n' > "$ROOTFS_DIR/etc/hostname"
root_hash=$(openssl passwd -6 -salt riscbox "$ROOT_PASSWORD")
sed -i "s|^root:[^:]*:|root:${root_hash}:|" "$ROOTFS_DIR/etc/shadow"

printf 'student:x:1000:1000:Student:/home/student:/bin/sh\n' >> \
    "$ROOTFS_DIR/etc/passwd"
printf 'student:x:1000:student\n' >> "$ROOTFS_DIR/etc/group"
printf 'student::0:0:99999:7:::\n' >> "$ROOTFS_DIR/etc/shadow"
mkdir -p "$ROOTFS_DIR/home/student" "$ROOTFS_DIR/usr/local/bin"
curl --fail --location --silent --show-error \
    --output "$ROOTFS_DIR/usr/local/bin/risclet" "$RISCLET_URL"
printf '%s  %s\n' "$RISCLET_SHA256" \
    "$ROOTFS_DIR/usr/local/bin/risclet" | sha256sum --check --status
chmod 755 "$ROOTFS_DIR/usr/local/bin/risclet"

chown 1000:1000 "$ROOTFS_DIR/home/student"

rm -f "$IMAGE_PATH"
truncate -s "${IMAGE_SIZE_MB}M" "$IMAGE_PATH"
"$MKFS_EXT4" -q -F -L riscbox-root -d "$ROOTFS_DIR" "$IMAGE_PATH"

actual_size=$(stat -c %s "$IMAGE_PATH")
expected_size=$((IMAGE_SIZE_MB * 1024 * 1024))
if [ "$actual_size" -ne "$expected_size" ]; then
    echo "image has $actual_size bytes, expected $expected_size" >&2
    exit 1
fi

echo "created $IMAGE_PATH (${IMAGE_SIZE_MB} MiB, Alpine $archive_release)"
