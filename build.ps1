# CosmosKernel Build Script
Write-Host "==============================================" -ForegroundColor Cyan
Write-Host "        Building CosmosKernel (x86_64)        " -ForegroundColor Cyan
Write-Host "==============================================" -ForegroundColor Cyan

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

# 1. Cargo build targeting bare-metal x86_64
Write-Host "[1/2] Compiling Rust kernel binary..." -ForegroundColor Yellow
cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Host "Error: Cargo build failed!" -ForegroundColor Red
    exit 1
}

# 2. Convert to Multiboot-compatible 32-bit ELF container for QEMU -kernel loader
Write-Host "[2/2] Generating multiboot-compatible binary..." -ForegroundColor Yellow
$objcopy = "$env:USERPROFILE\.rustup\toolchains\nightly-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin\rust-objcopy.exe"
if (!(Test-Path $objcopy)) {
    $objcopy = "rust-objcopy"
}

& $objcopy -O elf32-i386 target\x86_64-unknown-none\release\cosmoskernel cosmoskernel32.elf
if ($LASTEXITCODE -ne 0) {
    Write-Host "Error: rust-objcopy conversion failed!" -ForegroundColor Red
    exit 1
}

Write-Host "Build Succeeded!" -ForegroundColor Green
Write-Host "Output files:"
Write-Host "  - 64-bit Kernel ELF: target\x86_64-unknown-none\release\cosmoskernel"
Write-Host "  - Multiboot ELF:     cosmoskernel32.elf"
