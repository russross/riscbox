VERSION := $(strip $(shell cat version))
MAJOR := $(word 1,$(subst ., ,$(VERSION)))
SERIES := v$(MAJOR).x
BASE_URL := https://cdn.kernel.org/pub/linux/kernel/$(SERIES)

DOWNLOAD_DIR := downloads
SOURCE_DIR := sources/linux-$(VERSION)
SOURCE_READY := sources/.linux-$(VERSION).ready
BUILD_DIR := build/linux-$(VERSION)
ARCHIVE := $(DOWNLOAD_DIR)/linux-$(VERSION).tar.xz
CHECKSUMS := $(DOWNLOAD_DIR)/sha256sums-$(VERSION).asc
VERIFIED := $(DOWNLOAD_DIR)/.linux-$(VERSION).verified
CONFIGURED := $(BUILD_DIR)/.config
IMAGE := $(BUILD_DIR)/arch/riscv/boot/Image
BUILT := $(BUILD_DIR)/.image-built

CROSS_COMPILE ?= riscv64-linux-gnu-
JOBS ?= $(shell getconf _NPROCESSORS_ONLN)

all: linux

linux: $(BUILT)
	install -m 644 $(IMAGE) $@

$(BUILT): $(CONFIGURED)
	$(MAKE) -C $(SOURCE_DIR) O=$(abspath $(BUILD_DIR)) ARCH=riscv \
		CROSS_COMPILE=$(CROSS_COMPILE) -j $(JOBS) Image
	test -s $(IMAGE)
	touch $@

$(CONFIGURED): config $(SOURCE_READY)
	mkdir -p $(BUILD_DIR)
	install -m 644 config $@
	$(MAKE) -C $(SOURCE_DIR) O=$(abspath $(BUILD_DIR)) ARCH=riscv \
		CROSS_COMPILE=$(CROSS_COMPILE) olddefconfig

$(SOURCE_READY): $(VERIFIED)
	mkdir -p sources
	if [ ! -f $(SOURCE_DIR)/Makefile ]; then \
		tmp=$$(mktemp -d sources/.linux-$(VERSION).XXXXXX); \
		trap 'rm -rf "$$tmp"' EXIT HUP INT TERM; \
		tar -xJf $(ARCHIVE) -C "$$tmp" --strip-components=1; \
		mv "$$tmp" $(SOURCE_DIR); \
		trap - EXIT HUP INT TERM; \
	fi
	actual=$$(sed -n -e 's/^VERSION = //p' -e 's/^PATCHLEVEL = //p' \
		-e 's/^SUBLEVEL = //p' $(SOURCE_DIR)/Makefile | paste -sd . -); \
	test "$$actual" = $(VERSION) || \
		{ echo "expected Linux $(VERSION), found $$actual" >&2; exit 1; }
	touch $@

$(VERIFIED): $(ARCHIVE) $(CHECKSUMS)
	expected=$$(awk -v file=$(notdir $(ARCHIVE)) '$$2 == file { print $$1 }' $(CHECKSUMS)); \
	test -n "$$expected" || { echo "checksum not published for $(notdir $(ARCHIVE))" >&2; exit 1; }; \
	printf '%s  %s\n' "$$expected" $(ARCHIVE) | sha256sum --check --status || \
		{ echo "kernel archive checksum failed: $(ARCHIVE)" >&2; exit 1; }
	touch $@

$(ARCHIVE):
	mkdir -p $(DOWNLOAD_DIR)
	curl --fail --location --show-error --output $@.part \
		$(BASE_URL)/$(notdir $@)
	mv $@.part $@

$(CHECKSUMS): version
	mkdir -p $(DOWNLOAD_DIR)
	curl --fail --location --show-error --output $@.part \
		$(BASE_URL)/sha256sums.asc
	mv $@.part $@

clean:
	rm -rf build downloads sources
	rm -f linux

.PHONY: all clean
