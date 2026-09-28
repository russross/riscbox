#!/bin/sh
set -eu

image_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec make -C "$image_dir" all
