# Build Script for Guest Code Examples
# 
# This script builds all assembly examples into bootable binary images.
# Requires NASM (Netwide Assembler) to be installed.

Write-Host "======================================"
Write-Host "Building AetherVM Guest Code Examples"
Write-Host "======================================"
Write-Host ""

# Check if NASM is installed
$nasmPath = Get-Command nasm -ErrorAction SilentlyContinue

# If not in PATH, check local tools directory
if (-not $nasmPath) {
    $localNasm = Join-Path $PSScriptRoot "..\..\tools\nasm-2.16.03\nasm.exe"
    if (Test-Path $localNasm) {
        $nasmPath = @{ Source = $localNasm }
        # Create alias for local NASM
        Set-Alias -Name nasm -Value $localNasm -Scope Script
    }
}

if (-not $nasmPath) {
    Write-Host "ERROR: NASM not found!" -ForegroundColor Red
    Write-Host "Please install NASM from https://www.nasm.us/" -ForegroundColor Yellow
    Write-Host "Or install via Chocolatey: choco install nasm" -ForegroundColor Yellow
    exit 1
}

Write-Host "NASM found: $($nasmPath.Source)" -ForegroundColor Green
Write-Host ""

# Get all .asm files
$asmFiles = Get-ChildItem -Path "$PSScriptRoot" -Filter "*.asm"

if ($asmFiles.Count -eq 0) {
    Write-Host "No .asm files found in $PSScriptRoot" -ForegroundColor Yellow
    exit 0
}

Write-Host "Found $($asmFiles.Count) assembly files to build:" -ForegroundColor Cyan
foreach ($file in $asmFiles) {
    Write-Host "  - $($file.Name)" -ForegroundColor Gray
}
Write-Host ""

# Build counters
$successCount = 0
$failCount = 0

# Build each file
foreach ($file in $asmFiles) {
    $inputFile = $file.FullName
    $outputFile = $file.FullName -replace '\.asm$', '.bin'
    $baseName = $file.BaseName
    
    Write-Host "Building $baseName..." -NoNewline
    
    # Run NASM
    $nasmArgs = @("-f", "bin", $inputFile, "-o", $outputFile)
    $result = & nasm @nasmArgs 2>&1
    
    if ($LASTEXITCODE -eq 0) {
        # Success - check file size
        $fileInfo = Get-Item $outputFile
        $fileSize = $fileInfo.Length
        
        Write-Host " OK " -ForegroundColor Green -NoNewline
        Write-Host "($fileSize bytes)" -ForegroundColor Gray
        
        $successCount++
    }
    else {
        # Failure
        Write-Host " FAILED" -ForegroundColor Red
        Write-Host "  Error: $result" -ForegroundColor Red
        $failCount++
    }
}

Write-Host ""
Write-Host "======================================"
Write-Host "Build Summary:"
Write-Host "  Success: $successCount" -ForegroundColor Green
Write-Host "  Failed:  $failCount" -ForegroundColor $(if ($failCount -eq 0) { "Green" } else { "Red" })
Write-Host "======================================"

# List generated files
if ($successCount -gt 0) {
    Write-Host ""
    Write-Host "Generated binary files:"
    $binFiles = Get-ChildItem -Path "$PSScriptRoot" -Filter "*.bin"
    foreach ($file in $binFiles) {
        $size = $file.Length
        Write-Host "  $($file.Name) - $size bytes" -ForegroundColor Cyan
    }
}

# Exit with error if any builds failed
if ($failCount -gt 0) {
    exit 1
}

exit 0
