#!/bin/sh
set -eu

apk add --no-cache make python3 tzdata
ln -sf /usr/share/zoneinfo/America/Denver /etc/localtime
printf 'America/Denver\n' > /etc/timezone

cat > /etc/init.d/rcS <<'EOF'
#!/bin/sh
mount -t proc proc /proc
mount -t sysfs sysfs /sys
[ -c /dev/null ] || mount -t devtmpfs devtmpfs /dev
mkdir -p /dev/pts /run
mount -t devpts devpts /dev/pts
mount -t tmpfs tmpfs /run
hostname riscbox
ip link set lo up 2>/dev/null || true
aname=
uname=student
cache=mmap
for option in $(cat /proc/cmdline); do
    case "$option" in
        risclet.aname=*) aname=${option#risclet.aname=} ;;
        risclet.uname=*) uname=${option#risclet.uname=} ;;
        risclet.cache=*) cache=${option#risclet.cache=} ;;
    esac
done
case "$cache" in
    mmap|none) ;;
    *) echo "invalid risclet.cache value: $cache" >&2; exit 1 ;;
esac
mount_options=trans=virtio,version=9p2000.L,cache=$cache,access=1000,uname=$uname
if [ -n "$aname" ]; then
    mount_options="$mount_options,aname=$aname"
fi
mount -t 9p -o "$mount_options" risclet /home/student
EOF
chmod 755 /etc/init.d/rcS

cat > /etc/init.d/autologin-student <<'EOF'
#!/bin/sh
exec /bin/login -f student
EOF
chmod 755 /etc/init.d/autologin-student

cat > /etc/init.d/getty-console <<'EOF'
#!/bin/sh
case " $(cat /proc/cmdline) " in
    *" console=hvc0 "*) console=hvc0 ;;
    *) console=ttyS0 ;;
esac
exec /sbin/getty -n -l /etc/init.d/autologin-student -L 115200 "$console" vt100
EOF
chmod 755 /etc/init.d/getty-console

printf 'student:x:1000:1000:Student:/home/student:/bin/sh\n' >> /etc/passwd
printf 'student:x:1000:student\n' >> /etc/group
printf 'student::0:0:99999:7:::\n' >> /etc/shadow
mkdir -p /home/student /usr/local/bin /etc/profile.d
chown 1000:1000 /home/student

cat > /etc/motd <<'EOF'
Note: "grind" is not available on this VM, but "make" is.

To test your code:

    make                (run with testing)
    risclet run         (run without testing)
    risclet             (run the debugger)

EOF
cat > /etc/profile.d/risclet.sh <<'EOF'
HISTFILE=/tmp/student-history
export HISTFILE
EOF
cp /mnt/setup/risclet /usr/local/bin/risclet
chmod 755 /usr/local/bin/risclet
cat > /usr/local/bin/update-grind <<'EOF'
#!/bin/sh
exit 0
EOF
chmod 755 /usr/local/bin/update-grind
