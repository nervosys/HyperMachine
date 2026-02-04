# Installation

This guide covers installing HyperMachine on various platforms.

## Requirements

### System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| CPU | x86_64 with VT-x/AMD-V | Modern CPU with VT-d/AMD-Vi |
| RAM | 8 GB | 32 GB+ |
| Storage | 20 GB | 100 GB+ SSD |
| OS | Linux 5.10+, Windows 10+, macOS 12+ | Latest stable |

### Platform-Specific Requirements

**Linux:**
- KVM kernel module (`kvm`, `kvm_intel` or `kvm_amd`)
- `/dev/kvm` access (add user to `kvm` group)

**Windows:**
- Windows Hypervisor Platform (WHPX) enabled
- Hyper-V capabilities

**macOS:**
- Hypervisor.framework support (Apple Silicon or Intel)

## Installation Methods

### From Source (Recommended)

```bash
# Clone the repository
git clone https://github.com/nervosys/HyperMachine
cd HyperMachine

# Build release binaries
cargo build --release

# Install to system path
cargo install --path crates/hm-cli
```

### Pre-built Binaries

Download from [GitHub Releases](https://github.com/nervosys/HyperMachine/releases):

```bash
# Linux
curl -LO https://github.com/nervosys/HyperMachine/releases/latest/download/hm-linux-x86_64.tar.gz
tar xzf hm-linux-x86_64.tar.gz
sudo mv hm /usr/local/bin/

# macOS
curl -LO https://github.com/nervosys/HyperMachine/releases/latest/download/hm-darwin-arm64.tar.gz
tar xzf hm-darwin-arm64.tar.gz
sudo mv hm /usr/local/bin/

# Windows (PowerShell)
Invoke-WebRequest -Uri "https://github.com/nervosys/HyperMachine/releases/latest/download/hm-windows-x86_64.zip" -OutFile hm.zip
Expand-Archive hm.zip -DestinationPath C:\Program Files\HyperMachine
```

### Cargo Install

```bash
cargo install hypermachine
```

### Docker

```bash
docker pull ghcr.io/nervosys/hypermachine:latest
docker run -it --privileged ghcr.io/nervosys/hypermachine:latest
```

## Verify Installation

```bash
# Check version
hm --version

# Verify hypervisor support
hm doctor

# Expected output:
# ✓ Hypervisor support: KVM
# ✓ CPU virtualization: Intel VT-x
# ✓ IOMMU support: Intel VT-d
# ✓ GPU passthrough: Available
```

## Post-Installation Setup

### Linux: KVM Permissions

```bash
# Add user to kvm group
sudo usermod -aG kvm $USER

# Verify access
ls -la /dev/kvm
```

### Windows: Enable WHPX

```powershell
# Enable Windows Hypervisor Platform
Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform

# Restart required
Restart-Computer
```

### macOS: Hypervisor Entitlements

For development builds, you may need to sign with hypervisor entitlements:

```bash
codesign --entitlements entitlements.plist --force -s - ./target/release/hm
```

## Next Steps

- [Quick Start](./quick-start.md) - Create your first VM
- [Configuration](./configuration.md) - Configure HyperMachine settings
