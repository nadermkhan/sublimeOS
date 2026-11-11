param(
    [switch]$Headless,
    [switch]$Graphic,
    [switch]$Debug
)

$qemu = "C:\msys64\mingw64\bin\qemu-system-x86_64.exe"
if (!(Test-Path $qemu)) {
    $qemu = "qemu-system-x86_64"
}

# Ensure binary is built
if (!(Test-Path "cosmoskernel32.elf")) {
    Write-Host "Kernel binary not found. Building first..." -ForegroundColor Yellow
    & .\build.ps1
    if ($LASTEXITCODE -ne 0) { exit 1 }
}

Write-Host "==============================================" -ForegroundColor Cyan
Write-Host "        Launching CosmosKernel in QEMU        " -ForegroundColor Cyan
Write-Host "==============================================" -ForegroundColor Cyan

$qemuArgs = @("-kernel", "cosmoskernel32.elf", "-m", "128M", "-audiodev", "none,id=snd0", "-device", "intel-hda", "-device", "hda-duplex,audiodev=snd0")

if ($Headless) {
    Write-Host "Starting QEMU in headless terminal mode (COM1 serial stdio)..." -ForegroundColor Green
    Write-Host "Press Ctrl+A then X to exit QEMU." -ForegroundColor DarkGray
    $qemuArgs += @("-serial", "stdio", "-display", "none")
} else {
    Write-Host "Starting QEMU in graphical desktop mode (1024x768 VBE & PS/2 mouse)..." -ForegroundColor Green
    $qemuArgs += @("-serial", "stdio")
}

if ($Debug) {
    $qemuArgs += @("-d", "int,cpu_reset", "-no-reboot")
}

& $qemu @qemuArgs
