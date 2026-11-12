# Prepare USB flash drive (Disk 1) for SublimeOS boot
$ErrorActionPreference = "Stop"

Set-Location "C:\Users\NiO\Desktop\cosmos"

Write-Host "=======================================================" -ForegroundColor Cyan
Write-Host "       SublimeOS USB Flash Drive Bootloader Setup       " -ForegroundColor Cyan
Write-Host "=======================================================" -ForegroundColor Cyan
Write-Host "Targeting Disk 1 (Generic Flash Disk 3.72 GB)..." -ForegroundColor Yellow

# Execute diskpart script
Write-Host "[1/2] Repartitioning and formatting Disk 1 as FAT32 MBR (SublimeOS)..." -ForegroundColor Yellow
$dpOutput = diskpart /s "C:\Users\NiO\Desktop\cosmos\diskpart_usb.txt"
$dpOutput | Out-Host

Start-Sleep -Seconds 3

# Discover the newly formatted SublimeOS drive letter
$vol = Get-Volume -FileSystemLabel "SublimeOS" -ErrorAction SilentlyContinue
if (!$vol -or !$vol.DriveLetter) {
    Write-Host "Refreshing volumes..." -ForegroundColor Yellow
    Update-HostStorageCache
    Start-Sleep -Seconds 2
    $vol = Get-Volume -FileSystemLabel "SublimeOS" -ErrorAction SilentlyContinue
}

if (!$vol -or !$vol.DriveLetter) {
    Write-Host "Error: Could not locate formatted drive with label 'SublimeOS'!" -ForegroundColor Red
    exit 1
}

$driveLetter = "$($vol.DriveLetter):"
Write-Host "[2/2] Found USB volume at $driveLetter. Installing SublimeOS boot media..." -ForegroundColor Green
& .\create_boot_media.ps1 -DriveLetter $driveLetter

Write-Host ""
Write-Host "=======================================================" -ForegroundColor Green
Write-Host "   SublimeOS USB Drive Setup Complete on $driveLetter   " -ForegroundColor Green
Write-Host "=======================================================" -ForegroundColor Green
Start-Sleep -Seconds 3

