# Build script for mmio_test (multi-stage)
# Combines Stage 1 with mmio_test_extended as Stage 2

Write-Host "Building MMIO Test (Multi-Stage)..." -ForegroundColor Cyan
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

# Build Stage 2 (mmio test extended)
Write-Host ""
Write-Host "[2/3] Building Stage 2 (MMIO test)..." -ForegroundColor Yellow

$stage2Asm = Join-Path $scriptDir "mmio_test_extended.asm"
$stage2Bin = Join-Path $scriptDir "mmio_test_extended.bin"

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

$outputImg = Join-Path $scriptDir "mmio_test.img"
[System.IO.File]::WriteAllBytes($outputImg, $combinedBytes)

$totalSize = $combinedBytes.Length
$totalSizeKB = [math]::Round($totalSize / 1024, 2)

Write-Host "  Combined image size: $totalSize bytes ($totalSizeKB KB)" -ForegroundColor Green
Write-Host "  Output: mmio_test.img" -ForegroundColor Green

# Display summary
Write-Host ""
Write-Host "Build Summary:" -ForegroundColor Cyan
Write-Host "  Stage 1: $stage1Size bytes (boot sector)" -ForegroundColor White
Write-Host "  Stage 2: $stage2Size bytes (MMIO test)" -ForegroundColor White
Write-Host "  Total: $totalSize bytes" -ForegroundColor White

Write-Host ""
Write-Host "Memory Layout:" -ForegroundColor Cyan
Write-Host "  0x7C00 - 0x7DFF: Stage 1 (512 bytes)" -ForegroundColor White
Write-Host "  0x8000 - 0x$(([int]0x8000 + $stage2Size - 1).ToString('X4')): Stage 2 ($stage2Size bytes)" -ForegroundColor White

Write-Host ""
Write-Host "Features:" -ForegroundColor Cyan
Write-Host "  [x] I/O port demonstration (OUT)" -ForegroundColor Green
Write-Host "  [x] VGA MMIO demonstration" -ForegroundColor Green
Write-Host "  [x] Protected mode MMIO concepts" -ForegroundColor Green
Write-Host "  [x] Comparison table (I/O vs MMIO)" -ForegroundColor Green
Write-Host "  [x] Real-world device examples" -ForegroundColor Green

Write-Host ""
Write-Host "Build successful!" -ForegroundColor Green
