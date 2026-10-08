#!/bin/sh
set -eu
cd "$(dirname "$0")"
PATH="$PATH:/usr/sbin:/sbin"
export PATH
compiler="${CROSS_COMPILE:-riscv64-linux-gnu-}gcc"
for command in curl sha256sum fakeroot cpio gzip tar timeout qemu-system-riscv64 truncate mkfs.ext4 unzip "$compiler"; do
    command -v "$command" >/dev/null 2>&1 || { echo "missing command: $command" >&2; exit 1; }
done
mkdir -p build/downloads build/sqlite build/games build/programs

# Every source archive has a pinned hash; package versions are recorded inside
# the resulting image and the image hash identifies the complete guest fixture.
download() {
    url=$1 destination=$2 digest=$3
    if [ ! -f "$destination" ]; then
        curl --fail --location --show-error --output "$destination.part" "$url"
        mv "$destination.part" "$destination"
    fi
    printf '%s  %s\n' "$digest" "$destination" | sha256sum --check --status || {
        echo "checksum failed: $destination" >&2; exit 1;
    }
}
download https://dl-cdn.alpinelinux.org/alpine/v3.24/releases/riscv64/alpine-minirootfs-3.24.2-riscv64.tar.gz \
    build/downloads/alpine.tar.gz 57132e6e4f3a4ba9ffdf24e485513ce507e45471e7fd565aeed91c055cd63f7b
download https://www.sqlite.org/2025/sqlite-amalgamation-3500400.zip \
    build/downloads/sqlite-amalgamation.zip 1d3049dd0f830a025a53105fc79fd2ab9431aea99e137809d064d8ee8356b032
download https://www.sqlite.org/2025/sqlite-src-3500400.zip \
    build/downloads/sqlite-src.zip b7b4dc060f36053902fb65b344bbbed592e64b2291a26ac06fe77eec097850e9
unzip -p build/downloads/sqlite-amalgamation.zip sqlite-amalgamation-3500400/sqlite3.c > build/sqlite/sqlite3.c
unzip -p build/downloads/sqlite-amalgamation.zip sqlite-amalgamation-3500400/sqlite3.h > build/sqlite/sqlite3.h
unzip -p build/downloads/sqlite-src.zip sqlite-src-3500400/test/speedtest1.c > build/sqlite/speedtest1.c

# The existing cross toolchain builds fixed standalone RV64GC guest programs.
# Static linkage keeps their libraries independent of Alpine package updates.
"$compiler" -static -march=rv64gc -mabi=lp64d -O2 -std=c11 -Wall -Wextra -Werror \
    src/loop.c -o build/programs/loop
"$compiler" -static -march=rv64gc -mabi=lp64d -O2 -DSQLITE_TEMP_STORE=3 \
    -DSQLITE_THREADSAFE=0 -DSQLITE_OMIT_LOAD_EXTENSION -Ibuild/sqlite \
    build/sqlite/speedtest1.c build/sqlite/sqlite3.c -lm -o build/programs/speedtest1
"$compiler" --version > build/toolchain.txt
printf '%s\n' 'RV64GC/lp64d, static, -O2; SQLITE_TEMP_STORE=3, SQLITE_THREADSAFE=0, SQLITE_OMIT_LOAD_EXTENSION' >> build/toolchain.txt

# Copy source and durable project files, excluding any local game build outputs.
rm -rf build/games
mkdir -p build/games
(cd ../demo/bsd-games-3.3 && find . -type f \( -name '*.c' -o -name '*.h' -o -name Makefile -o -name LICENSE -o -name '*.md' \) -print0 | tar --null -T - -cf -) | tar -xf - -C build/games
kernel=$(cat ../kernel/.asset-name)
firmware=$(cat ../opensbi/.asset-name)
gzip -dc "../kernel/$kernel" > build/linux
gzip -dc "../opensbi/$firmware" > build/fw_dynamic.bin
stage=$(mktemp -d "$(pwd)/build/rootfs.XXXXXX")
output=$(mktemp -d "$(pwd)/build/output.XXXXXX")
trap 'rm -rf "$stage" "$output" build/rootfs.ext4.part' EXIT HUP INT TERM
fakeroot sh -eu -c '
    tar -xpf "$1" -C "$2"
    cp -R "$3/." "$2/"
    cd "$2"
    find . -print0 | cpio --null --quiet -o -H newc | gzip -1 > "$4"
' sh "$(pwd)/build/downloads/alpine.tar.gz" "$stage" "$(pwd)/guest" "$(pwd)/build/setup-initramfs.gz"

# The preparation guest exports a complete filesystem or a clean failure marker.
if ! timeout 600 qemu-system-riscv64 \
    -machine virt -m 512M -smp 1 -nographic -no-reboot \
    -bios build/fw_dynamic.bin -kernel build/linux -initrd build/setup-initramfs.gz \
    -append 'console=ttyS0,115200 rdinit=/sbin/bench-prepare riscv_isa_fallback panic=-1' \
    -netdev user,id=net -device virtio-net-device,netdev=net \
    -fsdev "local,id=source,path=$(pwd),security_model=none,readonly=on" \
    -device virtio-9p-device,fsdev=source,mount_tag=source \
    -fsdev "local,id=output,path=$output,security_model=none" \
    -device virtio-9p-device,fsdev=output,mount_tag=output \
    > build/image-setup.log 2>&1; then
    echo 'QEMU preparation failed; see bench/build/image-setup.log' >&2
    tail -20 build/image-setup.log >&2
    exit 1
fi
if [ ! -f "$output/complete" ] || [ -f "$output/failed" ]; then
    echo 'Guest preparation failed; see bench/build/image-setup.log' >&2
    tail -20 build/image-setup.log >&2
    exit 1
fi

# Source image and split assets remain generated local files, outside releases.
rm -rf "$stage"
mkdir -p "$stage"
fakeroot sh -eu -c '
    tar -xpf "$1" -C "$2"
    truncate -s 96M "$3"
    mkfs.ext4 -q -F -m 0 -d "$2" "$3"
' sh "$output/rootfs.tar" "$stage" "$(pwd)/build/rootfs.ext4.part"
cp "$stage/bench/packages.txt" build/packages.txt
mv build/rootfs.ext4.part build/rootfs.ext4
echo 'Prepared benchmark image: bench/build/rootfs.ext4'
