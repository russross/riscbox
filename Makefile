#
# Riscbox
# 
# Copyright (c) 2016-2018 Fabrice Bellard
#
# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to deal
# in the Software without restriction, including without limitation the rights
# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
# copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:
#
# The above copyright notice and this permission notice shall be included in
# all copies or substantial portions of the Software.
#
# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
# THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
# THE SOFTWARE.
#

# if set, network filesystem is enabled. libcurl and libcrypto
# (openssl) must be installed.
CONFIG_FS_NET=y
# SDL support (optional)
CONFIG_SDL=y
# user space network redirector
CONFIG_SLIRP=y

CC=clang
STRIP=strip
CPPFLAGS=-D_FILE_OFFSET_BITS=64 -D_LARGEFILE_SOURCE
CPPFLAGS+=-D_GNU_SOURCE -DCONFIG_VERSION=\"$(shell cat VERSION)\"
CFLAGS=-O2 -g -Wall -MMD
DEBUG_CFLAGS=-O1 -g3 -Wall -Wextra -Werror -Wformat=2 -Wshadow -MMD \
    -fno-omit-frame-pointer -fsanitize=address,undefined \
    -fno-sanitize-recover=all
LDFLAGS=
DEBUG_LDFLAGS=-fsanitize=address,undefined -fno-sanitize-recover=all

bindir=/usr/local/bin
INSTALL=install

PROGS+= riscbox
ifdef CONFIG_FS_NET
PROGS+=build_filelist splitimg
endif

all: release

release: $(PROGS)

debug: riscbox-debug

wasm:
	$(MAKE) -f Makefile.js

EMU_OBJS:=virtio.o pci.o fs.o p9_socket.o cutils.o iomem.o simplefb.o \
    json.o machine.o riscbox.o uart16550.o goldfish_rtc.o

ifdef CONFIG_SLIRP
CPPFLAGS+=-DCONFIG_SLIRP
EMU_OBJS+=$(addprefix slirp/, bootp.o ip_icmp.o mbuf.o slirp.o tcp_output.o cksum.o ip_input.o misc.o socket.o tcp_subr.o udp.o if.o ip_output.o sbuf.o tcp_input.o tcp_timer.o)
endif

EMU_OBJS+=fs_disk.o
EMU_LIBS=-lrt
ifdef CONFIG_FS_NET
CPPFLAGS+=-DCONFIG_FS_NET
EMU_OBJS+=fs_net.o fs_wget.o fs_utils.o block_net.o
EMU_LIBS+=-lcurl -lcrypto
endif # CONFIG_FS_NET
ifdef CONFIG_SDL
EMU_LIBS+=-lSDL
EMU_OBJS+=sdl.o
CPPFLAGS+=-DCONFIG_SDL
endif

EMU_OBJS+=riscv_machine.o softfp.o riscv_cpu64.o
DEBUG_OBJS:=$(addprefix build/debug/,$(EMU_OBJS))

PORT_TEST_CFLAGS=-O1 -g3 -Wall -Wextra -Werror -Wformat=2 -Wshadow \
    -fno-omit-frame-pointer -fsanitize=address,undefined \
    -fno-sanitize-recover=all

riscbox: $(EMU_OBJS)
	$(CC) $(LDFLAGS) -o $@ $^ $(EMU_LIBS)

riscv_cpu64.o: riscv_cpu.c
	$(CC) $(CPPFLAGS) $(CFLAGS) -c -o $@ $<

riscbox-debug: $(DEBUG_OBJS)
	$(CC) $(DEBUG_LDFLAGS) -o $@ $^ $(EMU_LIBS)

build/debug/riscv_cpu64.o: riscv_cpu.c
	mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(DEBUG_CFLAGS) -c -o $@ $<

build/debug/%.o: %.c
	mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(DEBUG_CFLAGS) -c -o $@ $<

build/tests/physical_memory_c: tests/physical_memory_c.c iomem.c cutils.c \
    iomem.h cutils.h
	mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(PORT_TEST_CFLAGS) -I. -o $@ \
		tests/physical_memory_c.c iomem.c cutils.c

build/tests/cpu_foundation_c: tests/cpu_foundation_c.c iomem.c cutils.c softfp.c \
    riscv_cpu.c riscv_cpu_template.h riscv_cpu_priv.h riscv_cpu.h iomem.h cutils.h
	mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(PORT_TEST_CFLAGS) -I. -o $@ \
		tests/cpu_foundation_c.c iomem.c cutils.c softfp.c

build/tests/platform_foundation_c: tests/platform_foundation_c.c \
    riscv_machine.c riscv_machine_test.h uart16550.c goldfish_rtc.c simplefb.c \
    riscv_cpu.c iomem.c cutils.c softfp.c
	mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(PORT_TEST_CFLAGS) -ffunction-sections -I. \
		-DRISCV_MACHINE_TEST -Wl,--gc-sections -o $@ \
		tests/platform_foundation_c.c riscv_machine.c uart16550.c \
		goldfish_rtc.c simplefb.c riscv_cpu.c iomem.c cutils.c softfp.c

build/tests/virtio_c: tests/virtio_c.c virtio.c virtio.h iomem.c cutils.c
	mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(PORT_TEST_CFLAGS) -ffunction-sections -I. \
		-Wl,--gc-sections -o $@ tests/virtio_c.c iomem.c cutils.c

test-port: build/tests/physical_memory_c build/tests/cpu_foundation_c \
    build/tests/platform_foundation_c build/tests/virtio_c
	./build/tests/physical_memory_c
	./build/tests/cpu_foundation_c
	./build/tests/platform_foundation_c
	./build/tests/virtio_c
	cargo test
	cargo clippy --all-targets -- -D warnings
	cargo build --target wasm32-unknown-unknown

build_filelist: build_filelist.o fs_utils.o cutils.o
	$(CC) $(LDFLAGS) -o $@ $^ -lm

splitimg: splitimg.o
	$(CC) $(LDFLAGS) -o $@ $^

install: $(PROGS)
	$(STRIP) $(PROGS)
	$(INSTALL) -m755 $(PROGS) "$(DESTDIR)$(bindir)"

%.o: %.c
	$(CC) $(CPPFLAGS) $(CFLAGS) -c -o $@ $<

clean:
	rm -rf build
	rm -rf target
	rm -f *.o *.d *~ $(PROGS) riscbox-debug slirp/*.o slirp/*.d slirp/*~
	rm -f js/riscbox-wasm.js js/riscbox-wasm.wasm

-include $(wildcard *.d)
-include $(wildcard slirp/*.d)
-include $(wildcard build/debug/*.d)
-include $(wildcard build/debug/slirp/*.d)

.PHONY: all release debug wasm clean install test-port
