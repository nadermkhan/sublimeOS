# SublimeOS Boot Media Preparation Script
param(
    [string]$DriveLetter = "",
    [switch]$CreateRawImage
)

Write-Host "=======================================================" -ForegroundColor Cyan
Write-Host "      SublimeOS (KiteKernel) Boot Media Creator         " -ForegroundColor Cyan
Write-Host "         Copyright (c) 2026 Nader Mahbub Khan          " -ForegroundColor Cyan
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

# Copy kernel & pure-Rust UEFI Multiboot2 bootloader
Copy-Item "cosmoskernel32.elf" -Destination "$payloadDir\cosmoskernel32.elf" -Force
Copy-Item "cosmoskernel32.elf" -Destination "$payloadDir\boot\cosmoskernel32.elf" -Force
if (!(Test-Path "BOOTX64.EFI")) {
    Write-Host "Error: BOOTX64.EFI not found! Run .\build.ps1 first." -ForegroundColor Red
    exit 1
}
Copy-Item "BOOTX64.EFI" -Destination "$payloadDir\EFI\BOOT\BOOTX64.EFI" -Force

# Create standard GRUB configuration (Multiboot2 + UEFI GOP framebuffer support)
$grubCfg = @"
set default="0"
set timeout=5

insmod efi_gop
insmod efi_uga
insmod font
insmod all_video
set gfxmode=auto
set gfxpayload=keep

menuentry "SublimeOS 1.0 (KiteKernel x86_64) - by Nader Mahbub Khan" {
    multiboot2 /boot/cosmoskernel32.elf
    boot
}

menuentry "SublimeOS (Verbose / Safe Boot -v)" {
    multiboot2 /boot/cosmoskernel32.elf -v
    boot
}

menuentry "SublimeOS (KiteKernel Console Mode -s)" {
    multiboot2 /boot/cosmoskernel32.elf -s
    boot
}
"@

[System.IO.File]::WriteAllText("$payloadDir\boot\grub\grub.cfg", $grubCfg)

# Pre-allocate 64KB PANIC.LOG file for safe bare-metal polled crash dumps
$panicLogPath = "$payloadDir\PANIC.LOG"
$zeroBytes = New-Object byte[] (64 * 1024)
# Initialize with a helpful placeholder header
$initHeader = "SublimeOS (KiteKernel) Pre-Allocated Crash Dump Area`r`nCopyright (c) 2026 Nader Mahbub Khan`r`nNo kernel panic recorded yet.`r`n"
$headerBytes = [System.Text.Encoding]::ASCII.GetBytes($initHeader)
[System.Array]::Copy($headerBytes, $zeroBytes, [Math]::Min($headerBytes.Length, $zeroBytes.Length))
[System.IO.File]::WriteAllBytes($panicLogPath, $zeroBytes)

Write-Host "  [OK] Payload ready at $payloadDir" -ForegroundColor Green

# 3. Handle Target Output
if ($DriveLetter -ne "") {
    $cleanLetter = $DriveLetter.TrimEnd('\', ':', ' ')
    $targetDrive = "$($cleanLetter):"
    if (Test-Path "$targetDrive\") {
        Write-Host "[3/3] Copying boot files to drive $targetDrive\ ..." -ForegroundColor Yellow
        Copy-Item -Path "$payloadDir\*" -Destination "$targetDrive\" -Recurse -Force
        Write-Host "Boot files successfully written to $targetDrive\ !" -ForegroundColor Green
    } else {
        Write-Host "Target drive $targetDrive\ does not exist." -ForegroundColor Red
    }
} else {
    Write-Host "[3/3] How to boot on real hardware (USB or HDD):" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "METHOD 1 (Recommended - Native UEFI / MacBook Pro / Modern PC):" -ForegroundColor Green
    Write-Host "  1. Format your USB drive as FAT32 (GPT or MBR)."
    Write-Host "  2. Run: .\create_boot_media.ps1 -DriveLetter E: (replace E: with your USB drive)."
    Write-Host "  3. Insert USB into target machine and boot:"
    Write-Host "     - Late 2011 MacBook Pro (or Mac): Hold Option (Alt) at boot chime, choose 'EFI Boot'."
    Write-Host "     - Standard Intel/AMD PC: Press F12/F11/F8 for UEFI Boot Menu, select USB drive."
    Write-Host "  The pure-Rust BOOTX64.EFI loader initializes UEFI GOP graphics, exits BootServices, and starts CosmicOS."
    Write-Host ""
    Write-Host "METHOD 2 (Ventoy - Multiboot ELF):" -ForegroundColor Green
    Write-Host "  1. Format your USB drive once with Ventoy (https://www.ventoy.net)."
    Write-Host "  2. Drag and drop 'cosmoskernel32.elf' into the Ventoy USB drive."
    Write-Host "  3. Boot via Ventoy Multiboot menu on any PC."
    Write-Host ""
    Write-Host "METHOD 3 (Legacy BIOS / GRUB2):" -ForegroundColor Green
    Write-Host "  1. Format USB drive as FAT32 (MBR)."
    Write-Host "  2. Run: .\create_boot_media.ps1 -DriveLetter E:"
    Write-Host "  3. Install GRUB2 to the USB MBR partition using grub-install or Rufus."
    Write-Host ""
}

Write-Host "Done!" -ForegroundColor Green
