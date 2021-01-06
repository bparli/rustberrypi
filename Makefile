# Default to the RPi3
BSP ?= rpi3

# Default to a serial device name that is common in Linux.
DEV_SERIAL ?= /dev/ttyUSB0

# Query the host system's kernel name
UNAME_S = $(shell uname -s)

# BSP-specific arguments
TARGET            = aarch64-unknown-none
KERNEL_BIN        = kernel8.img
QEMU_BINARY       = qemu-system-aarch64
QEMU_MACHINE_TYPE = raspi3
QEMU_RELEASE_ARGS = -serial stdio -display none
QEMU_TEST_ARGS    = $(QEMU_RELEASE_ARGS) -semihosting
LINKER_FILE       = src/link.ld
RUSTC_MISC_ARGS   = -C target-cpu=cortex-a53 -C link-arg=--no-dynamic-linker 

# Export for build.rs
export LINKER_FILE

# Testing-specific arguments
ifdef TEST
    ifeq ($(TEST),unit)
        TEST_ARG = --lib
    else
        TEST_ARG = --test $(TEST)
    endif
endif

QEMU_MISSING_STRING = "This board is not yet supported for QEMU."

RUSTFLAGS          = -C link-arg=-T$(LINKER_FILE) $(RUSTC_MISC_ARGS)
RUSTFLAGS_ETH      = $(RUSTFLAGS) -C link-arg=-L.cargo -C link-arg=-luspi -C link-arg=-luspienv
RUSTFLAGS_PEDANTIC = $(RUSTFLAGS) -D warnings

FEATURES      = bsp_$(BSP)
COMPILER_ARGS = --target=$(TARGET) --release

RUSTC_CMD   = cargo rustc $(COMPILER_ARGS)
#RUSTC_CMD	= cargo xbuild --release --verbose
CLIPPY_CMD  = cargo clippy $(COMPILER_ARGS)
CHECK_CMD   = cargo check $(COMPILER_ARGS)
TEST_CMD    = cargo test $(COMPILER_ARGS)
OBJCOPY_CMD = rust-objcopy \
    --strip-all            \
    -O binary

KERNEL_ELF = target/$(TARGET)/release/kernel

DOCKER_IMAGE         = rustembedded/osdev-utils
DOCKER_CMD_TEST      = docker run -i --rm -v $(shell pwd):/work/tutorial -w /work/tutorial
DOCKER_CMD_USER      = $(DOCKER_CMD_TEST) -t
DOCKER_ARG_DIR_UTILS = -v $(shell pwd)/utils:/work/utils
DOCKER_ARG_DEV       = --privileged -v /dev:/dev
DOCKER_ARG_NET       = --network host

DOCKER_QEMU = $(DOCKER_CMD_USER) $(DOCKER_IMAGE)
DOCKER_GDB  = $(DOCKER_CMD_USER) $(DOCKER_ARG_NET) $(DOCKER_IMAGE)
DOCKER_TEST = $(DOCKER_CMD_TEST) $(DOCKER_IMAGE)

# Dockerize commands that require USB device passthrough only on Linux
ifeq ($(UNAME_S),Linux)
    DOCKER_CMD_DEV = $(DOCKER_CMD_USER) $(DOCKER_ARG_DEV)

    DOCKER_CHAINBOOT = $(DOCKER_CMD_DEV) $(DOCKER_ARG_DIR_UTILS) $(DOCKER_IMAGE)
    DOCKER_JTAGBOOT  = $(DOCKER_CMD_DEV) $(DOCKER_ARG_DIR_UTILS) $(DOCKER_ARG_DIR_JTAG) $(DOCKER_IMAGE)
    DOCKER_OPENOCD   = $(DOCKER_CMD_DEV) $(DOCKER_ARG_NET) $(DOCKER_IMAGE)
else
    DOCKER_OPENOCD   = echo "Not yet supported on non-Linux systems."; \#
endif

EXEC_QEMU     = $(QEMU_BINARY) -M $(QEMU_MACHINE_TYPE)
EXEC_MINIPUSH = ruby ./utils/minipush.rb

.PHONY: all $(KERNEL_ELF) $(KERNEL_BIN) qemu test chainboot gdb gdb-opt0 \
    clippy clean readelf objdump nm check uspi

all: $(KERNEL_BIN)

uspi:
	@(cd ext/uspi/lib; make)
	cp -f ext/uspi/lib/libuspi.a ./.cargo
	@(cd ext/uspi/env/lib; make)
	cp -f ext/uspi/env/lib/libuspienv.a ./.cargo

$(KERNEL_ELF):
	RUSTFLAGS="$(RUSTFLAGS_ETH)" $(RUSTC_CMD)

$(KERNEL_BIN): $(KERNEL_ELF)
	@$(OBJCOPY_CMD) $(KERNEL_ELF) $(KERNEL_BIN)

ifeq ($(QEMU_MACHINE_TYPE),)
qemu test:
	@echo $(QEMU_MISSING_STRING)
else
qemu: $(KERNEL_BIN)
	@$(DOCKER_QEMU) $(EXEC_QEMU) $(QEMU_RELEASE_ARGS) -kernel $(KERNEL_BIN)

define KERNEL_TEST_RUNNER
    #!/usr/bin/env bash

    $(OBJCOPY_CMD) $$1 $$1.img
    TEST_BINARY=$$(echo $$1.img | sed -e 's/.*target/target/g')
    $(DOCKER_TEST) ruby tests/runner.rb $(EXEC_QEMU) $(QEMU_TEST_ARGS) -kernel $$TEST_BINARY
endef

export KERNEL_TEST_RUNNER
test:
	@mkdir -p target
	@echo "$$KERNEL_TEST_RUNNER" > target/kernel_test_runner.sh
	@chmod +x target/kernel_test_runner.sh
	RUSTFLAGS="$(RUSTFLAGS_PEDANTIC)" $(TEST_CMD) $(TEST_ARG)
endif

chainboot: $(KERNEL_BIN)
	@$(DOCKER_CHAINBOOT) $(EXEC_MINIPUSH) $(DEV_SERIAL) $(KERNEL_BIN)

define gen_gdb
    RUSTFLAGS="$(RUSTFLAGS_PEDANTIC) $1" $(RUSTC_CMD)
    @$(DOCKER_GDB) gdb-multiarch -q $(KERNEL_ELF)
endef

gdb:
	$(call gen_gdb,-C debuginfo=2)

gdb-opt0:
	$(call gen_gdb,-C debuginfo=2 -C opt-level=0)

clippy:
	RUSTFLAGS="$(RUSTFLAGS_PEDANTIC)" $(CLIPPY_CMD)

clean:
	rm -rf target $(KERNEL_BIN)

readelf: $(KERNEL_ELF)
	readelf -a $(KERNEL_ELF)

objdump: $(KERNEL_ELF)
	rust-objdump --arch-name aarch64 --disassemble --demangle --no-show-raw-insn \
	    --print-imm-hex $(KERNEL_ELF)

nm: $(KERNEL_ELF)
	rust-nm --demangle --print-size $(KERNEL_ELF) | sort

# For rust-analyzer
check:
	@RUSTFLAGS="$(RUSTFLAGS)" $(CHECK_CMD) --message-format=json
