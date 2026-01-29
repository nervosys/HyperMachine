#!/bin/bash
# Build Script for Guest Code Examples (Linux/macOS)
# 
# This script builds all assembly examples into bootable binary images.
# Requires NASM (Netwide Assembler) to be installed.

echo "======================================"
echo "Building AetherVM Guest Code Examples"
echo "======================================"
echo ""

# Check if NASM is installed
if ! command -v nasm &> /dev/null; then
    echo "ERROR: NASM not found!"
    echo "Please install NASM:"
    echo "  Debian/Ubuntu: sudo apt install nasm"
    echo "  Fedora: sudo dnf install nasm"
    echo "  Arch: sudo pacman -S nasm"
    echo "  macOS: brew install nasm"
    exit 1
fi

echo "NASM found: $(which nasm)"
echo "Version: $(nasm -v)"
echo ""

# Get script directory
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR" || exit 1

# Count files
ASM_COUNT=$(ls -1 *.asm 2>/dev/null | wc -l)

if [ "$ASM_COUNT" -eq 0 ]; then
    echo "No .asm files found in $SCRIPT_DIR"
    exit 0
fi

echo "Found $ASM_COUNT assembly files to build:"
for f in *.asm; do
    echo "  - $f"
done
echo ""

# Build counters
SUCCESS_COUNT=0
FAIL_COUNT=0

# Build each file
for f in *.asm; do
    BASE_NAME="${f%.asm}"
    OUTPUT_FILE="${BASE_NAME}.bin"
    
    printf "Building %s... " "$BASE_NAME"
    
    # Run NASM
    if nasm -f bin "$f" -o "$OUTPUT_FILE" 2>&1; then
        # Success - check file size
        FILE_SIZE=$(stat -f%z "$OUTPUT_FILE" 2>/dev/null || stat -c%s "$OUTPUT_FILE" 2>/dev/null)
        
        echo -e "\033[32mOK\033[0m ($FILE_SIZE bytes)"
        SUCCESS_COUNT=$((SUCCESS_COUNT + 1))
    else
        # Failure
        echo -e "\033[31mFAILED\033[0m"
        FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
done

echo ""
echo "======================================"
echo "Build Summary:"
echo "  Success: $SUCCESS_COUNT"
echo "  Failed:  $FAIL_COUNT"
echo "======================================"

# List generated files
if [ "$SUCCESS_COUNT" -gt 0 ]; then
    echo ""
    echo "Generated binary files:"
    for f in *.bin; do
        if [ -f "$f" ]; then
            FILE_SIZE=$(stat -f%z "$f" 2>/dev/null || stat -c%s "$f" 2>/dev/null)
            echo "  $f - $FILE_SIZE bytes"
        fi
    done
fi

# Exit with error if any builds failed
if [ "$FAIL_COUNT" -gt 0 ]; then
    exit 1
fi

exit 0
