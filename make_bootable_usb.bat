@echo off
setlocal enabledelayedexpansion
title SublimeOS (KiteKernel) USB Boot Media Creator

:: Check for Administrative elevation
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo [INFO] Requesting Administrator privileges to partition Disk 1...
    powershell -Command "Start-Process cmd -ArgumentList '/c \"\"%~f0\"\"' -Verb RunAs"
    exit /b
)

cd /d "C:\Users\NiO\Desktop\cosmos"

echo ===================================================================
echo       SublimeOS 1.0 (KiteKernel x86_64) Boot Media Creator         
echo          Developer & Creator: Nader Mahbub Khan (c) 2026           
echo ===================================================================
echo.
echo [1/3] Target Drive: Disk 1 (Generic Flash Disk 3.72 GB)
echo WARNING: This will repartition and format Disk 1 as FAT32 (SUBLIMEOS).
echo.

:: Execute diskpart script to clean and create primary active FAT32 partition
echo [2/3] Repartitioning and formatting Disk 1...
diskpart /s "C:\Users\NiO\Desktop\cosmos\diskpart_usb.txt"
if %errorlevel% neq 0 (
    echo [ERROR] diskpart failed! Please check if the USB drive is connected and write-enabled.
    pause
    exit /b 1
)

timeout /t 3 /nobreak >nul

:: Discover assigned drive letter for SUBLIMEOS or SublimeOS
echo [3/3] Copying SublimeOS bootloader and KiteKernel payload...
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
    "$vol = Get-Volume -FileSystemLabel 'SUBLIMEOS' -ErrorAction SilentlyContinue; " ^
    "if (!$vol -or !$vol.DriveLetter) { $vol = Get-Volume -FileSystemLabel 'SublimeOS' -ErrorAction SilentlyContinue }; " ^
    "if (!$vol -or !$vol.DriveLetter) { Update-HostStorageCache; Start-Sleep -Seconds 2; $vol = Get-Volume -FileSystemLabel 'SUBLIMEOS' -ErrorAction SilentlyContinue }; " ^
    "if ($vol -and $vol.DriveLetter) { " ^
    "    $dl = $vol.DriveLetter + ':'; " ^
    "    Write-Host 'Found target volume at ' $dl -ForegroundColor Green; " ^
    "    & '.\create_boot_media.ps1' -DriveLetter $dl; " ^
    "} else { " ^
    "    Write-Host 'ERROR: Could not locate drive formatted with label SUBLIMEOS!' -ForegroundColor Red; " ^
    "    exit 1; " ^
    "}"

if %errorlevel% neq 0 (
    echo [ERROR] Failed to write boot media payload!
    pause
    exit /b 1
)

echo.
echo ===================================================================
echo [SUCCESS] SublimeOS (KiteKernel) Bootable USB Drive is Ready!
echo ===================================================================
echo.
echo How to boot SublimeOS on real hardware:
echo   - Apple Macs (MacBook Pro / iMac / Mac mini):
echo       Hold 'Option' (Alt) immediately upon turning on until boot chime,
echo       then choose 'EFI Boot' (orange external disk icon).
echo.
echo   - Standard Intel / AMD PCs (UEFI):
echo       Press F12, F11, F8, or Esc at power-on to open UEFI Boot Menu,
echo       then select your USB Flash Drive.
echo.
echo Press any key to finish...
pause >nul

