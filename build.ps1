# SublimeOS (KiteKernel) Build Script
Write-Host "==============================================" -ForegroundColor Cyan
Write-Host "        Building SublimeOS (KiteKernel)       " -ForegroundColor Cyan
Write-Host "==============================================" -ForegroundColor Cyan

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

# 1. Compile Userland standalone executables (/bin/*)
Write-Host "[1/3] Compiling SublimeOS Userland standalone binaries..." -ForegroundColor Yellow
Push-Location userland
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Error: Userland build failed!" -ForegroundColor Red
        exit 1
    }
} finally {
    Pop-Location
}

# 2. Cargo build targeting bare-metal x86_64
Write-Host "[2/3] Compiling Rust kernel binary (KiteKernel)..." -ForegroundColor Yellow
cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Host "Error: Cargo build failed!" -ForegroundColor Red
    exit 1
}

# 3. Convert to Multiboot-compatible 32-bit ELF container
Write-Host "[3/3] Generating multiboot-compatible binary..." -ForegroundColor Yellow
$objcopy = "$env:USERPROFILE\.rustup\toolchains\nightly-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin\rust-objcopy.exe"
if (!(Test-Path $objcopy)) {
    $objcopy = "rust-objcopy"
}

& $objcopy -O elf32-i386 target\x86_64-unknown-none\release\kitekernel sublimekernel32.elf
if ($LASTEXITCODE -ne 0) {
    Write-Host "Error: rust-objcopy conversion failed!" -ForegroundColor Red
    exit 1
}

# Backward compatibility copy
Copy-Item "sublimekernel32.elf" -Destination "cosmoskernel32.elf" -Force

Write-Host "Building SublimeOS UEFI Multiboot2 Bootloader (BOOTX64.EFI)..." -ForegroundColor Cyan
Push-Location uefi-boot
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Error: uefi-boot Cargo build failed!" -ForegroundColor Red
        exit 1
    }
} finally {
    Pop-Location
}
Copy-Item "uefi-boot\target\x86_64-unknown-uefi\release\uefi-boot.efi" -Destination "BOOTX64.EFI" -Force

Write-Host "Build Succeeded!" -ForegroundColor Green
Write-Host "Output files:"
Write-Host "  - 64-bit Kernel ELF: target\x86_64-unknown-none\release\kitekernel"
Write-Host "  - Multiboot ELF:     sublimekernel32.elf (and cosmoskernel32.elf)"
Write-Host "  - UEFI Bootloader:   BOOTX64.EFI"
