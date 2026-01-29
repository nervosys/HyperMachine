# Multi-Stage Bootloader Build Script
# =====================================
# This script builds a multi-stage bootloader system:
#   1. Assembles Stage 1 (512-byte boot sector)
#   2. Assembles Stage 2 (larger, up to 64KB)
#   3. Combines them into a single boot image
#
# Usage: .\build_multistage.ps1

Write-Host "Building Multi-Stage Bootloader..." -ForegroundColor Cyan
Write-Host "===================================" -ForegroundColor Cyan
Write-Host ""

# Set paths
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$nasmPath = Join-Path $scriptDir "..\..\tools\nasm-2.16.03\nasm.exe"

# Check if NASM exists
if (-not (Test-Path $nasmPath)) {
    Write-Host "ERROR: NASM not found at: $nasmPath" -ForegroundColor Red
    Write-Host "Please ensure NASM is installed in tools/nasm-2.16.03/" -ForegroundColor Red
    exit 1
}

Write-Host "Using NASM: $nasmPath" -ForegroundColor Gray
Write-Host ""

# Build Stage 1
Write-Host "[1/3] Building Stage 1 (boot sector)..." -ForegroundColor Yellow
$stage1Asm = Join-Path $scriptDir "stage1.asm"
$stage1Bin = Join-Path $scriptDir "stage1.bin"

if (-not (Test-Path $stage1Asm)) {
    Write-Host "ERROR: stage1.asm not found" -ForegroundColor Red
    exit 1
}

& $nasmPath -f bin $stage1Asm -o $stage1Bin
if ($LASTEXITCODE -ne 0) {
    Write-Host "ERROR: Failed to build Stage 1" -ForegroundColor Red
    exit 1
}

# Verify Stage 1 size
$stage1Size = (Get-Item $stage1Bin).Length
Write-Host "  Stage 1 size: $stage1Size bytes" -ForegroundColor Green

if ($stage1Size -ne 512) {
    Write-Host "  WARNING: Stage 1 should be exactly 512 bytes!" -ForegroundColor Red
    exit 1
}

# Verify boot signature
$stage1Bytes = [System.IO.File]::ReadAllBytes($stage1Bin)
$signature = [BitConverter]::ToUInt16($stage1Bytes, 510)
if ($signature -ne 0xAA55) {
    Write-Host "  ERROR: Invalid boot signature!" -ForegroundColor Red
    exit 1
}
Write-Host "  Boot signature: 0x$($signature.ToString('X4')) - OK" -ForegroundColor Green
Write-Host ""

# Build Stage 2
Write-Host "[2/3] Building Stage 2 (extended loader)..." -ForegroundColor Yellow
$stage2Asm = Join-Path $scriptDir "stage2.asm"
$stage2Bin = Join-Path $scriptDir "stage2.bin"

if (-not (Test-Path $stage2Asm)) {
    Write-Host "ERROR: stage2.asm not found" -ForegroundColor Red
    exit 1
}

& $nasmPath -f bin $stage2Asm -o $stage2Bin
if ($LASTEXITCODE -ne 0) {
    Write-Host "ERROR: Failed to build Stage 2" -ForegroundColor Red
    exit 1
}

# Check Stage 2 size
$stage2Size = (Get-Item $stage2Bin).Length
Write-Host "  Stage 2 size: $stage2Size bytes ($([Math]::Round($stage2Size/1024, 2)) KB)" -ForegroundColor Green

if ($stage2Size -gt 65536) {
    Write-Host "  WARNING: Stage 2 is larger than 64KB!" -ForegroundColor Red
}
Write-Host ""

# Create combined boot image
Write-Host "[3/3] Creating combined boot image..." -ForegroundColor Yellow
$bootImage = Join-Path $scriptDir "multiboot.img"

# Read both stages
$stage1Data = [System.IO.File]::ReadAllBytes($stage1Bin)
$stage2Data = [System.IO.File]::ReadAllBytes($stage2Bin)

# Create combined image (Stage 1 + Stage 2)
$combinedData = $stage1Data + $stage2Data

# Write combined image
[System.IO.File]::WriteAllBytes($bootImage, $combinedData)

$totalSize = (Get-Item $bootImage).Length
Write-Host "  Combined image size: $totalSize bytes ($([Math]::Round($totalSize/1024, 2)) KB)" -ForegroundColor Green
Write-Host "  Output: multiboot.img" -ForegroundColor Green
Write-Host ""

# Summary
Write-Host "Build Summary:" -ForegroundColor Cyan
Write-Host "==============" -ForegroundColor Cyan
Write-Host "  Stage 1: $stage1Size bytes (boot sector)"
Write-Host "  Stage 2: $stage2Size bytes (extended loader)"
Write-Host "  Total:   $totalSize bytes"
Write-Host ""
Write-Host "Memory Layout:" -ForegroundColor Cyan
Write-Host "  0x7C00 - 0x7DFF: Stage 1 (512 bytes)"
Write-Host "  0x8000 - 0x$('{0:X4}' -f (0x8000 + $stage2Size - 1)): Stage 2 ($stage2Size bytes)"
Write-Host ""
Write-Host "Build successful!" -ForegroundColor Green
Write-Host ""
Write-Host "To test with AetherVM:" -ForegroundColor Yellow
Write-Host "  cargo run --example vm_runner -- multiboot.img"
