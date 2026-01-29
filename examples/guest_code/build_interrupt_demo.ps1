# Build script for interrupt_demo (multi-stage)
# Combines Stage 1 with interrupt_demo_extended as Stage 2

Write-Host "Building Interrupt Demo (Multi-Stage)..." -ForegroundColor Cyan
Write-Host ""

# Get script directory
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# Check if NASM exists
$nasmPath = Join-Path $scriptDir "..\..\tools\nasm-2.16.03\nasm.exe"
if (-not (Test-Path $nasmPath)) {
    Write-Host "Error: NASM not found at $nasmPath" -ForegroundColor Red
    exit 1
}

# Build Stage 1 (boot sector)
Write-Host "[1/3] Building Stage 1 (boot sector)..." -ForegroundColor Yellow

$stage1Asm = Join-Path $scriptDir "stage1.asm"
$stage1Bin = Join-Path $scriptDir "stage1.bin"

if (-not (Test-Path $stage1Bin)) {
    & $nasmPath -f bin $stage1Asm -o $stage1Bin
    if ($LASTEXITCODE -ne 0) {
        Write-Host "  Error: Stage 1 assembly failed" -ForegroundColor Red
        exit 1
    }
}

$stage1Size = (Get-Item $stage1Bin).Length
Write-Host "  Stage 1 size: $stage1Size bytes" -ForegroundColor Green

# Build Stage 2 (interrupt demo extended)
Write-Host ""
Write-Host "[2/3] Building Stage 2 (interrupt demo)..." -ForegroundColor Yellow

$stage2Asm = Join-Path $scriptDir "interrupt_demo_extended.asm"
$stage2Bin = Join-Path $scriptDir "interrupt_demo_extended.bin"

& $nasmPath -f bin $stage2Asm -o $stage2Bin
if ($LASTEXITCODE -ne 0) {
    Write-Host "  Error: Stage 2 assembly failed" -ForegroundColor Red
    exit 1
}

$stage2Size = (Get-Item $stage2Bin).Length
$stage2SizeKB = [math]::Round($stage2Size / 1024, 2)

Write-Host "  Stage 2 size: $stage2Size bytes ($stage2SizeKB KB)" -ForegroundColor Green

# Create combined image
Write-Host ""
Write-Host "[3/3] Creating combined boot image..." -ForegroundColor Yellow

$stage1Bytes = [System.IO.File]::ReadAllBytes($stage1Bin)
$stage2Bytes = [System.IO.File]::ReadAllBytes($stage2Bin)

$combinedBytes = $stage1Bytes + $stage2Bytes

$outputImg = Join-Path $scriptDir "interrupt_demo.img"
[System.IO.File]::WriteAllBytes($outputImg, $combinedBytes)

$totalSize = $combinedBytes.Length
$totalSizeKB = [math]::Round($totalSize / 1024, 2)

Write-Host "  Combined image size: $totalSize bytes ($totalSizeKB KB)" -ForegroundColor Green
Write-Host "  Output: interrupt_demo.img" -ForegroundColor Green

# Display summary
Write-Host ""
Write-Host "Build Summary:" -ForegroundColor Cyan
Write-Host "  Stage 1: $stage1Size bytes (boot sector)" -ForegroundColor White
Write-Host "  Stage 2: $stage2Size bytes (interrupt demo)" -ForegroundColor White
Write-Host "  Total: $totalSize bytes" -ForegroundColor White

Write-Host ""
Write-Host "Memory Layout:" -ForegroundColor Cyan
Write-Host "  0x7C00 - 0x7DFF: Stage 1 (512 bytes)" -ForegroundColor White
Write-Host "  0x8000 - 0x$(([int]0x8000 + $stage2Size - 1).ToString('X4')): Stage 2 ($stage2Size bytes)" -ForegroundColor White

Write-Host ""
Write-Host "Features:" -ForegroundColor Cyan
Write-Host "  [x] Interrupt Vector Table setup" -ForegroundColor Green
Write-Host "  [x] PIC initialization" -ForegroundColor Green
Write-Host "  [x] Timer interrupts (IRQ 0)" -ForegroundColor Green
Write-Host "  [x] Software interrupts (INT 0x80-0x82)" -ForegroundColor Green
Write-Host "  [x] Exception handling (INT 0x00)" -ForegroundColor Green

Write-Host ""
Write-Host "Build successful!" -ForegroundColor Green
