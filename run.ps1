param(
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

$qemuArgs = @("-kernel", "cosmoskernel32.elf", "-m", "128M")

if ($Graphic) {
    Write-Host "Starting QEMU with graphical VGA console & PS/2 keyboard..." -ForegroundColor Green
    $qemuArgs += @("-serial", "stdio")
} else {
    Write-Host "Starting QEMU in headless terminal mode (COM1 serial stdio)..." -ForegroundColor Green
    Write-Host "Press Ctrl+A then X to exit QEMU." -ForegroundColor DarkGray
    $qemuArgs += @("-serial", "stdio", "-display", "none")
}

if ($Debug) {
    $qemuArgs += @("-d", "int,cpu_reset", "-no-reboot")
}

& $qemu @qemuArgs
