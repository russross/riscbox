VERSION := $(strip $(shell cat version))
BASE_URL := https://github.com/riscv-software-src/opensbi/archive/refs/tags
ARCHIVE := downloads/opensbi-$(VERSION).tar.gz
ARCHIVE_SHA256 := d11702103f177a2914e94eec57ce5ed820296d874f6b6525c4482e55d71a3667
VERIFIED := downloads/.opensbi-$(VERSION).verified
SOURCE_DIR := sources/opensbi-$(VERSION)
SOURCE_READY := sources/.opensbi-$(VERSION).ready
BUILD_DIR := build/opensbi-$(VERSION)
CONFIGURED := $(BUILD_DIR)/.configured
FIRMWARE := $(BUILD_DIR)/platform/generic/firmware/fw_dynamic.bin
BUILT := $(BUILD_DIR)/.firmware-built
DEFCONFIG := defconfig

include config

CROSS_COMPILE ?= riscv64-linux-gnu-
JOBS ?= $(shell getconf _NPROCESSORS_ONLN)

all: fw_dynamic.bin

fw_dynamic.bin: $(BUILT)
	install -m 644 $(FIRMWARE) $@

$(BUILT): $(CONFIGURED)
	$(MAKE) -C $(SOURCE_DIR) O=$(abspath $(BUILD_DIR)) \
		PLATFORM=$(PLATFORM) PLATFORM_RISCV_XLEN=$(PLATFORM_RISCV_XLEN) \
		PLATFORM_DEFCONFIG=riscbox \
		FW_DYNAMIC=$(FW_DYNAMIC) FW_OPTIONS=$(FW_OPTIONS) \
		CROSS_COMPILE=$(CROSS_COMPILE) -j $(JOBS)
	test -s $(FIRMWARE)
	touch $@

$(CONFIGURED): config $(DEFCONFIG) $(SOURCE_READY)
	rm -rf $(BUILD_DIR)
	mkdir -p $(BUILD_DIR)
	install -m 644 $(DEFCONFIG) $(SOURCE_DIR)/platform/generic/configs/riscbox
	install -m 644 config $@

$(SOURCE_READY): $(VERIFIED)
	mkdir -p sources
	if [ ! -f $(SOURCE_DIR)/Makefile ]; then \
		tmp=$$(mktemp -d sources/.opensbi-$(VERSION).XXXXXX); \
		trap 'rm -rf "$$tmp"' EXIT HUP INT TERM; \
		tar -xzf $(ARCHIVE) -C "$$tmp" --strip-components=1; \
		mv "$$tmp" $(SOURCE_DIR); \
		trap - EXIT HUP INT TERM; \
	fi
	major=$$(sed -n 's/^#define OPENSBI_VERSION_MAJOR //p' \
		$(SOURCE_DIR)/include/sbi/sbi_version.h); \
	minor=$$(sed -n 's/^#define OPENSBI_VERSION_MINOR //p' \
		$(SOURCE_DIR)/include/sbi/sbi_version.h); \
	test "$$major.$$minor" = $(VERSION) || \
		{ echo "expected OpenSBI $(VERSION), found $$major.$$minor" >&2; exit 1; }
	touch $@

$(VERIFIED): $(ARCHIVE) Makefile
	printf '%s  %s\n' $(ARCHIVE_SHA256) $(ARCHIVE) | \
		sha256sum --check --status || \
		{ echo "OpenSBI archive checksum failed: $(ARCHIVE)" >&2; exit 1; }
	touch $@

$(ARCHIVE): version
	mkdir -p downloads
	curl --fail --location --show-error --output $@.part \
		$(BASE_URL)/v$(VERSION).tar.gz
	mv $@.part $@

clean:
	rm -rf build downloads sources
	rm -f fw_dynamic.bin

.PHONY: all clean
