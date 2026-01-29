# GPU Virtualization in AetherVM

## Overview

AetherVM provides two GPU virtualization approaches:

1. **Virtual GPU (vGPU)**: Software-emulated GPU using WebGPU/Vulkan
2. **GPU Passthrough**: Direct hardware access via VFIO

## Virtual GPU

### Architecture

The virtual GPU uses WebGPU as its backend, providing:
- Cross-platform compatibility
- Shader compilation and execution
- Graphics pipeline emulation
- Compute shader support

### Usage

```rust
use aethervm_gpu::vgpu::VirtualGpu;

let mut vgpu = VirtualGpu::new("default-gpu");
vgpu.init().await?;
```

## GPU Passthrough

### Requirements

- IOMMU enabled in BIOS
- VT-d (Intel) or AMD-Vi support
- GPU in separate IOMMU group
- Host driver unbound from device

### Configuration

```toml
[gpu]
enabled = true
passthrough = true
device = "0000:01:00.0"
vendor_id = "10de"  # NVIDIA
```

### Setup on Linux

```bash
# Enable IOMMU in kernel parameters
# Add to /etc/default/grub:
GRUB_CMDLINE_LINUX="intel_iommu=on iommu=pt"

# Update grub
sudo update-grub

# Unbind GPU from host driver
echo "0000:01:00.0" > /sys/bus/pci/drivers/nvidia/unbind

# Bind to vfio-pci
echo "10de 1b80" > /sys/bus/pci/drivers/vfio-pci/new_id
```

## Supported Workloads

- **Graphics**: OpenGL, Vulkan, DirectX
- **Compute**: CUDA, OpenCL, ROCm
- **AI/ML**: TensorFlow, PyTorch GPU acceleration
- **Gaming**: Cloud gaming, game streaming

## Performance

| Mode        | Latency | Throughput | Compatibility |
| ----------- | ------- | ---------- | ------------- |
| vGPU        | Medium  | Medium     | High          |
| Passthrough | Low     | Native     | Medium        |

## Examples

### CUDA Workload

```rust
let vm = AgentVM::builder()
    .name("cuda-vm")
    .cpu_cores(8)
    .memory_gb(16)
    .enable_gpu(true)
    .build()
    .await?;

// GPU is available inside VM
vm.execute_agent_script(r#"
    // Run CUDA code
    cuda_available()
"#).await?;
```

### Gaming VM

```rust
let vm = AgentVM::builder()
    .name("gaming-vm")
    .cpu_cores(6)
    .memory_gb(16)
    .enable_gpu(true)
    .build()
    .await?;

// Optimized for low latency
```
