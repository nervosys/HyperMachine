//! Build script for creating bootable disk images

fn main() {
    // Tell cargo to re-run this if the main source changes
    println!("cargo:rerun-if-changed=src/main.rs");
}
