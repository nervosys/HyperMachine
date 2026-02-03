# Getting Started with HyperMachine

This guide walks you through building and running your first virtual machine with HyperMachine.

## Installation

### Option 1: Pre-built Binaries

Download from [GitHub Releases](https://github.com/nervosys/hypermachine/releases):

```bash
# Linux
curl -LO https://github.com/nervosys/hypermachine/releases/latest/download/hm-x86_64-unknown-linux-gnu.tar.gz
tar xzf hm-x86_64-unknown-linux-gnu.tar.gz
sudo mv hm /usr/local/bin/

# macOS
curl -LO https://github.com/nervosys/hypermachine/releases/latest/download/hm-aarch64-apple-darwin.tar.gz
tar xzf hm-aarch64-apple-darwin.tar.gz
sudo mv hm /usr/local/bin/
```

**Windows (PowerShell):**
```powershell
Invoke-WebRequest -Uri "https://github.com/nervosys/hypermachine/releases/latest/download/hm-x86_64-pc-windows-msvc.zip" -OutFile "hm.zip"
Expand-Archive hm.zip -DestinationPath .
Move-Item hm.exe C:\Windows\System32\
```

### Option 2: Build from Source

```bash
git clone https://github.com/nervosys/hypermachine.git
cd hypermachine
cargo build --release
sudo cp target/release/hm /usr/local/bin/
```

## Platform Setup

### Linux (KVM)

```bash
# Check KVM support
lsmod | grep kvm

# Add user to kvm group
sudo usermod -aG kvm $USER

# Verify access
ls -la /dev/kvm
```

### Windows (WHPX)

1. Enable Windows Hypervisor Platform:
   ```powershell
   Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform
   ```
2. Restart your computer

### macOS (HVF)

macOS Hypervisor.framework is available automatically on Apple Silicon and Intel Macs with macOS 10.10+.

## Quick Start

### 1. Check System Status

```bash
hm status
```

Output:
```
HyperMachine v0.1.0
Backend: KVM (Linux)
VMs: 0 running, 0 stopped
Memory Available: 16.0 GB
CPUs Available: 8
```

### 2. Create a VM

```bash
# Interactive creation
hm create my-first-vm

# Or with options
hm create my-vm --cpus 2 --memory 2048 --disk 20
```

### 3. List VMs

```bash
hm list
```

Output:
```
NAME          STATE     CPUS    MEMORY    CREATED
my-first-vm   stopped   2       2048 MB   2024-01-15 10:30:00
```

### 4. Start the VM

```bash
hm start my-first-vm
```

### 5. Connect to Console

```bash
hm console my-first-vm
```

Press `Ctrl+]` to detach.

### 6. Execute Commands

```bash
# Run a command in the guest
hm exec my-first-vm "uname -a"

# Run a script
hm exec my-first-vm --script setup.sh
```

### 7. Stop and Delete

```bash
hm stop my-first-vm
hm delete my-first-vm
```

## Using the API

Start the API server:

```bash
hm serve --port 8080
```

### REST API Examples

```bash
# List VMs
curl http://localhost:8080/api/v1/vms

# Create VM
curl -X POST http://localhost:8080/api/v1/vms \
  -H "Content-Type: application/json" \
  -d '{"name": "api-vm", "cpus": 2, "memory_mb": 2048}'

# Start VM
curl -X POST http://localhost:8080/api/v1/vms/api-vm/start

# Get VM status
curl http://localhost:8080/api/v1/vms/api-vm
```

### gRPC API

```bash
# Using grpcurl
grpcurl -plaintext localhost:50051 hypermachine.v1.VmService/List
```

## Configuration

### VM Configuration File

Create `myvm.yaml`:

```yaml
name: production-vm
cpus: 4
memory_mb: 8192
disk:
  size_gb: 100
  format: qcow2
network:
  type: bridge
  bridge: br0
boot:
  kernel: /path/to/vmlinuz
  initrd: /path/to/initrd
  cmdline: "console=ttyS0"
```

Create from config:

```bash
hm create -f myvm.yaml
```

### Global Configuration

Edit `~/.config/hypermachine/config.yaml`:

```yaml
default_cpus: 2
default_memory_mb: 2048
api:
  port: 8080
  auth: false
storage:
  path: /var/lib/hypermachine
logging:
  level: info
  format: json
```

## AI Agent Integration

HyperMachine is designed for AI automation. Get OpenAI-compatible tool definitions:

```bash
curl http://localhost:8080/agentic/tools/openai > tools.json
```

Use with your AI framework:

```python
from openai import OpenAI
import requests

client = OpenAI()
tools = requests.get("http://localhost:8080/agentic/tools/openai").json()["tools"]

response = client.chat.completions.create(
    model="gpt-4",
    messages=[
        {"role": "user", "content": "Create a VM with 4 CPUs for running tests"}
    ],
    tools=tools
)
```

## Next Steps

- [API Reference](./API_QUICKSTART.md) - Full API documentation
- [Architecture](./architecture.md) - System design and internals
- [Security](./security/FIPS_COMPLIANCE.md) - FIPS 140-3 compliance
- [GPU Passthrough](./gpu.md) - GPU virtualization guide

## Troubleshooting

### "Permission denied" on /dev/kvm

```bash
sudo usermod -aG kvm $USER
# Log out and log back in
```

### WHPX not available on Windows

1. Check virtualization is enabled in BIOS
2. Ensure Hyper-V is not conflicting:
   ```powershell
   bcdedit /set hypervisorlaunchtype off
   # Reboot, then:
   Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform
   ```

### VM fails to start

Check logs:
```bash
hm logs my-vm --tail 100
```

## Getting Help

- [GitHub Issues](https://github.com/nervosys/hypermachine/issues)
- [Discord Community](https://discord.gg/nervosys)
- [Documentation](https://docs.nervosys.com/hypermachine)
