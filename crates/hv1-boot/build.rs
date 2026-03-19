//! Build script for creating bootable UEFI/BIOS disk images
//!
//! This build script uses the `bootloader` crate (v0.11) to package the compiled
//! kernel ELF binary into bootable disk images. The images are written to the
//! workspace `target/` directory.
//!
//! The actual disk image creation happens via `cargo run` on this crate after
//! the kernel binary has been built for `x86_64-unknown-none`.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/main.rs");

    // Emit the path where disk images should be placed
    let out_dir = std::env::var("OUT_DIR").unwrap_or_default();
    println!("cargo:rustc-env=HV1_OUT_DIR={out_dir}");

    // Check if the kernel binary exists (built via a prior nightly cross-compile step)
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_default();

    let kernel_binary = workspace
        .join("target")
        .join("x86_64-unknown-none")
        .join("release")
        .join("hv1");

    if kernel_binary.exists() {
        println!(
            "cargo:warning=Kernel binary found at {}",
            kernel_binary.display()
        );
    } else {
        println!(
            "cargo:warning=Kernel binary not yet built at {}. \
             Build with: cargo +nightly build -p hv1-boot --target x86_64-unknown-none \
             -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem --release",
            kernel_binary.display()
        );
    }
}
