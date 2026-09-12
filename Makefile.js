#
# Riscbox emulator
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

# Build the WebAssembly version of Riscbox
EMCC=emcc
EMCPPFLAGS=-D_FILE_OFFSET_BITS=64 -D_LARGEFILE_SOURCE -DCONFIG_FS_NET
EMCFLAGS=-O3 -Wall -Wextra -Werror -Wformat=2 -Wshadow -MMD \
    -fno-strict-aliasing
EMLDFLAGS=-O3 -s NO_EXIT_RUNTIME=1 -s NO_FILESYSTEM=1 \
    -s "EXPORTED_FUNCTIONS=['_console_queue_char','_vm_start','_fs_import_file','_fs_import_text','_display_key_event','_display_mouse_event','_display_wheel_event','_net_write_packet','_net_set_carrier']" \
    -s 'EXPORTED_RUNTIME_METHODS=["ccall", "cwrap"]' \
    -s INITIAL_MEMORY=67108864 -s ALLOW_MEMORY_GROWTH=1 \
    --js-library js/lib.js

WASM_DIR=build/wasm
PROGS=js/riscbox-wasm.js

all: $(PROGS)

JS_OBJS=jsemu.o softfp.o virtio.o fs.o p9_js.o fs_net.o fs_wget.o fs_utils.o \
    simplefb.o pci.o json.o block_net.o iomem.o cutils.o aes.o sha256.o \
    uart16550.o

RISCBOX_OBJS=$(addprefix $(WASM_DIR)/,$(JS_OBJS) riscv_cpu64.o \
    riscv_machine.o machine.o)

js/riscbox-wasm.js: $(RISCBOX_OBJS) js/lib.js
	$(EMCC) $(EMLDFLAGS) -o $@ $(RISCBOX_OBJS)

$(WASM_DIR)/riscv_cpu64.o: riscv_cpu.c
	mkdir -p $(@D)
	$(EMCC) $(EMCPPFLAGS) $(EMCFLAGS) -c -o $@ $<

$(WASM_DIR)/%.o: %.c
	mkdir -p $(@D)
	$(EMCC) $(EMCPPFLAGS) $(EMCFLAGS) -c -o $@ $<

clean:
	rm -rf $(WASM_DIR)
	rm -f js/riscbox-wasm.js js/riscbox-wasm.wasm

-include $(wildcard $(WASM_DIR)/*.d)

.PHONY: all clean
