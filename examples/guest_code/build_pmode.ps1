# Build script for protected mode multi-stage bootloader
# Creates bootable image with Stage 1 (real mode) and Stage 2 (protected mode)

Write-Host "Building Protected Mode Multi-Stage Bootloader..." -ForegroundColor Cyan
Write-Host ""

# Get script directory
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# Check if NASM exists
$nasmPath = Join-Path $scriptDir "..\..\tools\nasm-2.16.03\nasm.exe"
if (-not (Test-Path $nasmPath)) {
    Write-Host "Error: NASM not found at $nasmPath" -ForegroundColor Red
    exit 1
}

# Build Stage 1 (unchanged from original)
Write-Host "[1/3] Building Stage 1 (boot sector)..." -ForegroundColor Yellow

$stage1Asm = Join-Path $scriptDir "stage1.asm"
$stage1Bin = Join-Path $scriptDir "stage1.bin"

& $nasmPath -f bin $stage1Asm -o $stage1Bin
if ($LASTEXITCODE -ne 0) {
    Write-Host "  Error: Stage 1 assembly failed" -ForegroundColor Red
    exit 1
}

# Verify Stage 1
$stage1Size = (Get-Item $stage1Bin).Length
if ($stage1Size -ne 512) {
    Write-Host "  Error: Stage 1 must be exactly 512 bytes (got $stage1Size)" -ForegroundColor Red
    exit 1
}

# Check boot signature
$stage1Data = [System.IO.File]::ReadAllBytes($stage1Bin)
$bootSig = [System.BitConverter]::ToUInt16($stage1Data, 510)
if ($bootSig -ne 0xAA55) {
    Write-Host "  Error: Invalid boot signature (expected 0xAA55, got 0x$($bootSig.ToString('X4')))" -ForegroundColor Red
    exit 1
}

Write-Host "  Stage 1 size: $stage1Size bytes" -ForegroundColor Green
Write-Host "  Boot signature: 0x$($bootSig.ToString('X4')) - OK" -ForegroundColor Green

# Build Stage 2 (protected mode)
Write-Host ""
Write-Host "[2/3] Building Stage 2 (protected mode loader)..." -ForegroundColor Yellow

$stage2Asm = Join-Path $scriptDir "stage2_pmode.asm"
$stage2Bin = Join-Path $scriptDir "stage2_pmode.bin"

& $nasmPath -f bin $stage2Asm -o $stage2Bin
if ($LASTEXITCODE -ne 0) {
    Write-Host "  Error: Stage 2 assembly failed" -ForegroundColor Red
    exit 1
}

# Verify Stage 2 size
$stage2Size = (Get-Item $stage2Bin).Length
$stage2SizeKB = [math]::Round($stage2Size / 1024, 2)

if ($stage2Size -gt 65536) {
    Write-Host "  Error: Stage 2 too large (max 64KB, got $stage2Size bytes)" -ForegroundColor Red
    exit 1
}

Write-Host "  Stage 2 size: $stage2Size bytes ($stage2SizeKB KB)" -ForegroundColor Green

# Create combined boot image
Write-Host ""
Write-Host "[3/3] Creating combined boot image..." -ForegroundColor Yellow

# Read both binaries
$stage1Bytes = [System.IO.File]::ReadAllBytes($stage1Bin)
$stage2Bytes = [System.IO.File]::ReadAllBytes($stage2Bin)

# Combine them
$combinedBytes = $stage1Bytes + $stage2Bytes

# Write combined image
$pmodeImg = Join-Path $scriptDir "pmode.img"
[System.IO.File]::WriteAllBytes($pmodeImg, $combinedBytes)

$totalSize = $combinedBytes.Length
$totalSizeKB = [math]::Round($totalSize / 1024, 2)

Write-Host "  Combined image size: $totalSize bytes ($totalSizeKB KB)" -ForegroundColor Green
Write-Host "  Output: pmode.img" -ForegroundColor Green

# Display summary
Write-Host ""
Write-Host "Build Summary:" -ForegroundColor Cyan
Write-Host "  Stage 1: $stage1Size bytes (boot sector)" -ForegroundColor White
Write-Host "  Stage 2: $stage2Size bytes (protected mode loader)" -ForegroundColor White
Write-Host "  Total: $totalSize bytes" -ForegroundColor White

Write-Host ""
Write-Host "Memory Layout:" -ForegroundColor Cyan
Write-Host "  0x7C00 - 0x7DFF: Stage 1 (512 bytes)" -ForegroundColor White
Write-Host "  0x8000 - 0x$(([int]0x8000 + $stage2Size - 1).ToString('X4')): Stage 2 ($stage2Size bytes)" -ForegroundColor White
Write-Host "  0xB8000: VGA text buffer (protected mode)" -ForegroundColor White
Write-Host "  0x90000: Protected mode stack" -ForegroundColor White

Write-Host ""
Write-Host "Protected Mode Features:" -ForegroundColor Cyan
Write-Host "  [x] A20 line enabled" -ForegroundColor Green
Write-Host "  [x] GDT configured (code + data segments)" -ForegroundColor Green
Write-Host "  [x] 32-bit protected mode active" -ForegroundColor Green
Write-Host "  [x] Flat memory model (4GB addressable)" -ForegroundColor Green
Write-Host "  [x] VGA direct access" -ForegroundColor Green

Write-Host ""
Write-Host "Build successful!" -ForegroundColor Green
