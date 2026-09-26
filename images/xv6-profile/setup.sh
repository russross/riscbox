#!/bin/sh
set -eu

apk add --no-cache alpine-base build-base doas git
install -m 755 /mnt/setup/mount-ephemeral-writes /sbin/mount-ephemeral-writes
sed -i 's|^/dev/vda  /         ext4   defaults,noatime|/dev/vda  /         erofs  ro,noatime|' /etc/fstab
ln -sf /usr/share/zoneinfo/UTC /etc/localtime
printf 'UTC\n' > /etc/timezone

addgroup test
adduser -D -G test -s /bin/sh test
addgroup test wheel
printf 'permit nopass :wheel\n' > /etc/doas.conf
chmod 600 /etc/doas.conf

su - test -c 'git clone --depth 1 https://github.com/mit-pdos/xv6-riscv.git /home/test/xv6-riscv'
su - test -c 'doas true'

cat > /home/test/profile-build.sh <<'EOF'
#!/bin/sh
set -eu
cd /home/test/xv6-riscv
make -j1 TOOLPREFIX= QEMU=/bin/false kernel/kernel
printf '\nXV6_PROFILE_BUILD_COMPLETE\n'
doas poweroff
EOF
chown test:test /home/test/profile-build.sh
chmod 755 /home/test/profile-build.sh

cat > /etc/init.d/rcS <<'EOF'
#!/bin/sh
set -eu
mount -t proc proc /proc
mount -t sysfs sysfs /sys
[ -c /dev/null ] || mount -t devtmpfs devtmpfs /dev
mkdir -p /dev/pts /run
mount -t devpts devpts /dev/pts
mount -t tmpfs tmpfs /run
/sbin/mount-ephemeral-writes
hostname riscbox-profile
ip link set lo up 2>/dev/null || true
exec su - test -c /home/test/profile-build.sh
EOF
chmod 755 /etc/init.d/rcS

cat > /etc/inittab <<'EOF'
::sysinit:/etc/init.d/rcS
::ctrlaltdel:/sbin/reboot
::shutdown:/bin/umount -a -r
EOF
