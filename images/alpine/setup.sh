#!/bin/sh
set -eu

apk add --no-cache alpine-base ca-certificates tzdata
ln -sf /usr/share/zoneinfo/UTC /etc/localtime
printf 'UTC\n' > /etc/timezone
cat > /etc/motd <<'EOF'
Riscbox Alpine image

The root account has no password. Set one before using this image outside an
isolated teaching environment.
EOF
