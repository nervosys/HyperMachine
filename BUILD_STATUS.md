# HyperMachine — Build Status

## Build Summary

| Metric           | Value                                          |
| ---------------- | ---------------------------------------------- |
| **Status**       | ✅ Clean                                        |
| **Platform**     | Windows (x86_64), Linux, macOS                 |
| **Rust Version** | 1.87+ (stable); nightly for hv1-core, hv1-boot |
| **Profile**      | Debug + Release                                |
| **Tests**        | 3,923 passed, 0 failed, 28 ignored             |
| **Clippy**       | 0 warnings (`-D warnings`)                     |
| **Crates**       | 12 total (10 stable, 2 nightly)                |

## Compiled Crates

| #   | Crate           | Lines   | Tests | Status    |
| --- | --------------- | ------- | ----- | --------- |
| 1   | **hv2-core**    | 107,748 | 2,207 | ✅         |
| 2   | **hv2-api**     | 29,190  | 956   | ✅         |
| 3   | **hv2-agent**   | 19,423  | 396   | ✅         |
| 4   | **hv2-runtime** | 6,592   | 145   | ✅         |
| 5   | **hm-cli**      | 5,341   | 55    | ✅         |
| 6   | **hv2-cpu**     | 4,340   | 66    | ✅         |
| 7   | **hm-gui**      | 3,227   | 34    | ✅         |
| 8   | **hv2-gpu**     | 1,716   | 20    | ✅         |
| 9   | **hv2-net**     | 1,696   | 13    | ✅         |
| 10  | **hv2-cli**     | 1,026   | 22    | ✅         |
| 11  | **hv1-core**    | 5,924   | —     | ✅ nightly |
| 12  | **hv1-boot**    | 253     | —     | ✅ nightly |

**Total: 186,481 lines of Rust across 263 source files.**

## Build Commands

```bash
# Standard build (excludes nightly-only Type-1 crates)
cargo build --workspace --exclude hv1-core --exclude hv1-boot

# Release build
cargo build --release --workspace --exclude hv1-core --exclude hv1-boot

# Run all tests
cargo test --workspace --exclude hv1-core --exclude hv1-boot

# Lint check
cargo clippy --workspace --exclude hv1-core --exclude hv1-boot -- -D warnings

# Type-1 bare-metal build (requires nightly)
cargo +nightly check -p hv1-core --target x86_64-unknown-none "-Zbuild-std=core,alloc"
cargo +nightly check -p hv1-boot --target x86_64-unknown-none "-Zbuild-std=core,alloc"

# Benchmarks
cargo bench -p hv2-core --bench crypto_bench
```

## Key Features

- ✅ Async/await with Tokio runtime
- ✅ AI agent scripting with Rhai (sync-enabled)
- ✅ Dual gRPC (Tonic) and REST (Axum) APIs
- ✅ GPU virtualization (WGPU + VFIO passthrough)
- ✅ Zero-copy guest memory management
- ✅ FIPS 140-3 cryptography (AES-GCM, SHA-2, RSA, ECDSA, post-quantum)
- ✅ Capability-based security model
- ✅ Cross-platform: Windows (WHPX), Linux (KVM), macOS (HVF)
- ✅ Full interrupt subsystem (PIC, LAPIC, I/O APIC, MSI/MSI-X)
- ✅ 20+ device emulations
- ✅ Live migration, snapshot/restore
- ✅ Nested virtualization
- ✅ Container support (cgroups, namespaces, seccomp)

## Dependencies

All major dependencies resolved:
- ✅ Protocol Buffers compiler (`protoc`) for gRPC
- ✅ Rhai (sync feature) for thread-safe script execution
- ✅ parking_lot for fast Mutexes and RwLocks
- ✅ Tokio for async runtime
- ✅ Tonic for gRPC
- ✅ Axum for REST API
- ✅ WGPU for GPU virtualization
- ✅ ring / sha2 / aes-gcm for cryptography

## Platform Notes

### Windows
- ✅ WHPX backend support
- ✅ Protobuf compilation
- ✅ Full test suite passes

### Linux
- ✅ KVM backend support
- ✅ seccompiler for sandboxing
- ✅ TAP/TUN networking

### macOS
- ✅ Hypervisor.framework (HVF) backend

## Troubleshooting

| Problem                         | Solution                                                     |
| ------------------------------- | ------------------------------------------------------------ |
| `KVM not available`             | Enable VT-x/AMD-V in BIOS; `modprobe kvm_intel` or `kvm_amd` |
| `WHPX not found` (Windows)      | Enable "Windows Hypervisor Platform" in Windows Features     |
| `protoc not found`              | `apt install protobuf-compiler` or `brew install protobuf`   |
| Build fails on nightly crates   | Use `--exclude hv1-core --exclude hv1-boot`                  |
| Permission denied on `/dev/kvm` | `sudo usermod -aG kvm $USER`                                 |

---

*Last Updated: March 18, 2026*

**Last Updated**: 2025-01-28  
**Version**: 0.1.0  
**Status**: Development - Core infrastructure complete
