# GPUs in HyperMachine

What exists, and what does not. The short version: there is host-side GPU
code in `hv2-gpu`, and **no VM created by this project has a GPU in it**.

## What does not exist

- **`enable_gpu` on a VM is refused.** `VMConfig::enable_gpu`, the
  `enable_gpu` field of `POST /api/v1/vms`, the `gpu_enabled` argument of the
  MCP `vm.create` tool, and `AgentVMBuilder::enable_gpu(true)` all reach
  `VM::new_with_backend`, which returns a configuration error when the flag is
  set. Nothing in VM creation attaches a GPU device to a guest, so the flag
  used to be accepted and ignored; it is now refused instead.
- **No CUDA, OpenCL, ROCm or DirectX runtime.** Strings such as `cuda-12.4`
  that appear in fleet metadata are scheduling labels.
- **No hardware-virtualized GPU** (SR-IOV or mediated devices).

## What exists

### Passthrough (`crates/hv2-gpu/src/passthrough.rs`), Linux only

Host-side VFIO code: it opens `/dev/vfio`, binds a device and maps its BARs
with real ioctls. It refuses on other platforms.

NOT VERIFIED: it has never run against a physical GPU and IOMMU (CI has no
such hardware; its tests exercise parsed structures). It is not connected to
a guest: there is no IOMMU DMA mapping of guest memory and no interrupt
routing into a VM, so "attach" here means the host has opened the device, not
that a guest can use it.

Host preparation, when someone does run it on hardware, is the usual VFIO
setup: IOMMU enabled in firmware and on the kernel command line
(`intel_iommu=on iommu=pt`, or AMD-Vi), the GPU alone in its IOMMU group, and
the device unbound from its host driver and bound to `vfio-pci`.

### Virtual GPU (`crates/hv2-gpu/src/vgpu.rs`)

A software GPU built on `wgpu` (Vulkan/WebGPU) that runs in the host process.
It is not presented to a guest as a device.

### Fleet metadata and placement (`hv2-runtime`)

GPU classes, interconnect topology and capacity reservations are a data model
used for placement decisions. They describe GPUs; they do not drive one.
