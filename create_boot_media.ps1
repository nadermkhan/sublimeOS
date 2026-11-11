# Cosmos macOS Boot Media Preparation Script
param(
    [string]$DriveLetter = "",
    [switch]$CreateRawImage
)

Write-Host "=======================================================" -ForegroundColor Cyan
Write-Host "    Cosmos macOS Sequoia (Darwin) Boot Media Creator    " -ForegroundColor Cyan
Write-Host "=======================================================" -ForegroundColor Cyan

# 1. Check and build kernel if necessary
if (!(Test-Path "cosmoskernel32.elf")) {
    Write-Host "[1/3] Building kernel binary first..." -ForegroundColor Yellow
    & .\build.ps1
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Build failed! Exiting." -ForegroundColor Red
        exit 1
    }
}

# 2. Assemble Boot Directory Payload
Write-Host "[2/3] Assembling boot files structure in .\usb_payload..." -ForegroundColor Yellow
$payloadDir = ".\usb_payload"
if (Test-Path $payloadDir) {
    Remove-Item -Recurse -Force $payloadDir
}

New-Item -ItemType Directory -Path "$payloadDir\boot\grub" -Force | Out-Null
New-Item -ItemType Directory -Path "$payloadDir\EFI\BOOT" -Force | Out-Null

# Copy kernel
Copy-Item "cosmoskernel32.elf" -Destination "$payloadDir\cosmoskernel32.elf" -Force
Copy-Item "cosmoskernel32.elf" -Destination "$payloadDir\boot\cosmoskernel32.elf" -Force

# Create standard GRUB configuration
$grubCfg = @"
set default="0"
set timeout=5

insmod vbe
insmod vga
insmod video_bochs
insmod video_cirrus
insmod all_video

menuentry "Cosmos macOS Sequoia 15.1 (Darwin 24.1.0 x86_64)" {
    multiboot /cosmoskernel32.elf
    boot
}

menuentry "Cosmos macOS (Verbose / Safe Boot -v)" {
    multiboot /cosmoskernel32.elf -v
    boot
}

menuentry "Cosmos macOS (Darwin Text Console Mode -s)" {
    multiboot /cosmoskernel32.elf -s
    boot
}
"@

[System.IO.File]::WriteAllText("$payloadDir\boot\grub\grub.cfg", $grubCfg)

Write-Host "  [OK] Payload ready at $payloadDir" -ForegroundColor Green

# 3. Handle Target Output
if ($DriveLetter -ne "") {
    $targetDrive = $DriveLetter.TrimEnd('\')
    if (Test-Path "$targetDrive\") {
        Write-Host "[3/3] Copying boot files to drive $targetDrive\ ..." -ForegroundColor Yellow
        Copy-Item -Path "$payloadDir\*" -Destination "$targetDrive\" -Recurse -Force
        Write-Host "Boot files successfully written to $targetDrive\ !" -ForegroundColor Green
    } else {
        Write-Host "Target drive $targetDrive does not exist." -ForegroundColor Red
    }
} else {
    Write-Host "[3/3] How to boot on real hardware (USB or HDD):" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "METHOD 1 (Recommended - Easiest & Works on ANY PC):" -ForegroundColor Green
    Write-Host "  1. Format your USB drive once with Ventoy (https://www.ventoy.net)."
    Write-Host "  2. Drag and drop 'cosmoskernel32.elf' into the Ventoy USB drive."
    Write-Host "  3. Plug into any real Intel/AMD PC, boot from USB, and select it."
    Write-Host ""
    Write-Host "METHOD 2 (Rufus / Standard GRUB2 USB):" -ForegroundColor Green
    Write-Host "  1. Format USB drive as FAT32 (MBR or GPT)."
    Write-Host "  2. Run: .\create_boot_media.ps1 -DriveLetter E: (replace E: with your USB drive)."
    Write-Host "  3. Use GRUB24DOS or Rufus to install standard Multiboot MBR."
    Write-Host ""
    Write-Host "METHOD 3 (Raw Disk Image dd):" -ForegroundColor Green
    Write-Host "  The kernel supports raw ELF loading via standard Multiboot loaders."
    Write-Host ""
}

Write-Host "Done!" -ForegroundColor Green
