VERSION := $(strip $(shell cat version))
BASE_URL := https://ftp.denx.de/pub/u-boot
ARCHIVE := downloads/u-boot-$(VERSION).tar.bz2
ARCHIVE_SHA256 := cdef7d507c93f1bbd9f015ea9bc21fa074268481405501945abc6f854d5b686f
VERIFIED := downloads/.u-boot-$(VERSION).verified
SOURCE_DIR := sources/u-boot-$(VERSION)
SOURCE_READY := sources/.u-boot-$(VERSION).ready
BUILD_DIR := build/u-boot-$(VERSION)
CONFIGURED := $(BUILD_DIR)/.config
IMAGE := $(BUILD_DIR)/u-boot.bin
BUILT := $(BUILD_DIR)/.image-built

CROSS_COMPILE ?= riscv64-linux-gnu-
JOBS ?= $(shell getconf _NPROCESSORS_ONLN)

all: u-boot.bin

u-boot.bin: $(BUILT)
	install -m 644 $(IMAGE) $@

$(BUILT): $(CONFIGURED)
	$(MAKE) -C $(SOURCE_DIR) O=$(abspath $(BUILD_DIR)) \
		CROSS_COMPILE=$(CROSS_COMPILE) -j $(JOBS) u-boot.bin
	test -s $(IMAGE)
	touch $@

$(CONFIGURED): config default.env $(SOURCE_READY)
	mkdir -p $(BUILD_DIR)
	install -m 644 default.env $(SOURCE_DIR)/board/emulation/qemu-riscv/riscbox.env
	install -m 644 config $@
	$(MAKE) -C $(SOURCE_DIR) O=$(abspath $(BUILD_DIR)) \
		CROSS_COMPILE=$(CROSS_COMPILE) olddefconfig

$(SOURCE_READY): $(VERIFIED)
	mkdir -p sources
	if [ ! -f $(SOURCE_DIR)/Makefile ]; then \
		tmp=$$(mktemp -d sources/.u-boot-$(VERSION).XXXXXX); \
		trap 'rm -rf "$$tmp"' EXIT HUP INT TERM; \
		tar -xjf $(ARCHIVE) -C "$$tmp" --strip-components=1; \
		mv "$$tmp" $(SOURCE_DIR); \
		trap - EXIT HUP INT TERM; \
	fi
	actual=$$(sed -n -e 's/^VERSION = //p' -e 's/^PATCHLEVEL = //p' \
		$(SOURCE_DIR)/Makefile | paste -sd . -); \
	test "$$actual" = $(VERSION) || \
		{ echo "expected U-Boot $(VERSION), found $$actual" >&2; exit 1; }
	touch $@

$(VERIFIED): $(ARCHIVE) Makefile
	printf '%s  %s\n' $(ARCHIVE_SHA256) $(ARCHIVE) | \
		sha256sum --check --status || \
		{ echo "U-Boot archive checksum failed: $(ARCHIVE)" >&2; exit 1; }
	touch $@

$(ARCHIVE): version
	mkdir -p downloads
	curl --fail --location --show-error --output $@.part \
		$(BASE_URL)/$(notdir $@)
	mv $@.part $@

clean:
	rm -rf build downloads sources
	rm -f u-boot.bin

.PHONY: all clean
