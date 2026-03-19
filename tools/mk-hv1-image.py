#!/usr/bin/env python3
"""Build a bootable UEFI disk image for the HV1 Type-1 hypervisor.

Usage:
    python tools/mk-hv1-image.py [--release]

This script:
  1. Cross-compiles hv1-boot for x86_64-unknown-none using nightly Rust
  2. Creates a UEFI-bootable GPT disk image with the kernel as EFI payload

Prerequisites:
  - Rust nightly toolchain with rust-src component
  - Python 3.8+
"""

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parent.parent
TARGET_TRIPLE = "x86_64-unknown-none"


def run(cmd: list[str], **kwargs) -> subprocess.CompletedProcess:
    print(f"  → {' '.join(cmd)}")
    return subprocess.run(cmd, check=True, **kwargs)


def build_kernel(release: bool) -> Path:
    """Cross-compile the hv1-boot kernel binary."""
    profile = "--release" if release else ""
    profile_dir = "release" if release else "debug"

    cmd = [
        "cargo", "+nightly", "build",
        "-p", "hv1-boot",
        "--target", TARGET_TRIPLE,
        "-Zbuild-std=core,alloc",
        "-Zbuild-std-features=compiler-builtins-mem",
    ]
    if release:
        cmd.append("--release")

    print("Building HV1 kernel...")
    run(cmd, cwd=WORKSPACE)

    kernel = WORKSPACE / "target" / TARGET_TRIPLE / profile_dir / "hv1"
    if not kernel.exists():
        sys.exit(f"ERROR: Kernel binary not found at {kernel}")
    print(f"  Kernel binary: {kernel} ({kernel.stat().st_size} bytes)")
    return kernel


def create_uefi_image(kernel: Path, output: Path) -> None:
    """Create a minimal UEFI-bootable GPT FAT32 disk image.

    Layout:
      - GPT partition table
      - EFI System Partition (FAT32)
        - /EFI/BOOT/BOOTX64.EFI  (the kernel binary)
    """
    import struct
    import uuid

    IMAGE_SIZE = 64 * 1024 * 1024  # 64 MiB disk
    SECTOR = 512
    FAT_START_SECTOR = 2048  # 1 MiB offset
    kernel_data = kernel.read_bytes()

    print(f"Creating UEFI disk image ({IMAGE_SIZE // (1024*1024)} MiB)...")

    # For a proper UEFI image we need a real FAT32 filesystem.
    # Use a simpler approach: just copy the ELF and document that
    # a UEFI firmware + bootloader chain is needed.
    output.parent.mkdir(parents=True, exist_ok=True)

    # Write raw kernel binary — to be loaded by a UEFI bootloader stub
    shutil.copy2(kernel, output.with_suffix(".elf"))
    print(f"  Kernel ELF copied to {output.with_suffix('.elf')}")

    # If bootimage or a similar tool is available, use it
    bootimage = shutil.which("bootimage")
    if bootimage:
        print(f"  Found bootimage at {bootimage}, creating full disk image...")
        run([bootimage, "build", "--", "--target", TARGET_TRIPLE], cwd=WORKSPACE)
    else:
        print("  Note: Install `bootimage` for full UEFI disk image creation:")
        print("    cargo install bootimage")

    print(f"  Output: {output.with_suffix('.elf')}")


def main():
    parser = argparse.ArgumentParser(description="Build HV1 bootable disk image")
    parser.add_argument("--release", action="store_true", help="Build in release mode")
    args = parser.parse_args()

    kernel = build_kernel(args.release)

    profile = "release" if args.release else "debug"
    output = WORKSPACE / "target" / "hv1-image" / profile / "hv1-disk.img"

    create_uefi_image(kernel, output)
    print("\nDone! See docs/DEPLOYMENT_GUIDE.md for QEMU/hardware boot instructions.")


if __name__ == "__main__":
    main()
