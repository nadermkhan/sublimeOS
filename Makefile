# CosmosKernel Makefile

TARGET ?= x86_64-unknown-none
CARGO ?= cargo
OBJCOPY ?= $(USERPROFILE)/.rustup/toolchains/nightly-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/bin/rust-objcopy.exe
QEMU ?= C:/msys64/mingw64/bin/qemu-system-x86_64.exe

KERNEL_ELF = target/$(TARGET)/release/cosmoskernel
MULTIBOOT_ELF = cosmoskernel32.elf

.PHONY: all build run run-graphic clean test

all: build

build: $(MULTIBOOT_ELF)

$(KERNEL_ELF):
	$(CARGO) build --release

$(MULTIBOOT_ELF): $(KERNEL_ELF)
	$(OBJCOPY) -O elf32-i386 $< $@

run: $(MULTIBOOT_ELF)
	$(QEMU) -kernel $< -m 128M -serial stdio -display none

run-graphic: $(MULTIBOOT_ELF)
	$(QEMU) -kernel $< -m 128M -serial stdio

test: build
	powershell -ExecutionPolicy Bypass -File ./run.ps1

clean:
	$(CARGO) clean
	rm -f $(MULTIBOOT_ELF) serial.log qemu.log
