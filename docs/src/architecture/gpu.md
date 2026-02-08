# GPU Virtualization

HyperMachine provides multiple GPU virtualization modes for different use cases.

## GPU Modes

| Mode                   | Performance | Isolation | Use Case            |
| ---------------------- | ----------- | --------- | ------------------- |
| **Passthrough**        | Native      | Full      | ML training, gaming |
| **Virtual GPU (vGPU)** | 80-95%      | Shared    | Multi-tenant        |
| **Software Rendering** | Low         | Full      | Testing             |

## GPU Passthrough

Direct assignment of a physical GPU to a VM using IOMMU:

```rust
pub struct GpuPassthrough {
    pci_device: PciDevice,
    iommu_group: u32,
    vfio_device: VfioDevice,
}

impl GpuPassthrough {
    pub fn new(pci_address: &str) -> Result<Self> {
        // Parse PCI address (e.g., "0000:01:00.0")
        let pci_device = PciDevice::from_address(pci_address)?;
        
        // Find IOMMU group
        let iommu_group = pci_device.iommu_group()?;
        
        // Unbind from host driver
        pci_device.unbind_driver()?;
        
        // Bind to vfio-pci
        pci_device.bind_driver("vfio-pci")?;
        
        // Open VFIO device
        let vfio_device = VfioDevice::open(iommu_group, &pci_device)?;
        
        Ok(Self {
            pci_device,
            iommu_group,
            vfio_device,
        })
    }
    
    pub fn attach_to_vm(&self, vm: &mut Vm) -> Result<()> {
        // Map device BARs to guest
        for bar in self.vfio_device.bars() {
            vm.map_mmio_region(bar.guest_addr, bar.host_addr, bar.size)?;
        }
        
        // Setup interrupt forwarding
        let irq = self.vfio_device.irq_info()?;
        vm.register_irq(irq)?;
        
        // Configure MSI-X if available
        if let Some(msix) = self.vfio_device.msix_info() {
            vm.configure_msix(msix)?;
        }
        
        Ok(())
    }
}
```

### IOMMU Configuration

```bash
# Enable IOMMU in GRUB
GRUB_CMDLINE_LINUX="intel_iommu=on iommu=pt"

# Or for AMD
GRUB_CMDLINE_LINUX="amd_iommu=on iommu=pt"
```

```rust
pub struct IommuManager {
    groups: HashMap<u32, IommuGroup>,
}

impl IommuManager {
    pub fn isolate_device(&mut self, device: &PciDevice) -> Result<()> {
        let group = self.groups.get_mut(&device.iommu_group())?;
        
        // Ensure all devices in group are bound to vfio
        for dev in group.devices() {
            if dev.driver() != "vfio-pci" {
                dev.unbind_driver()?;
                dev.bind_driver("vfio-pci")?;
            }
        }
        
        Ok(())
    }
}
```

## Virtual GPU (vGPU)

Share a single GPU across multiple VMs:

```rust
pub struct VirtualGpu {
    physical_gpu: Arc<PhysicalGpu>,
    vm_id: VmId,
    vram_slice: VramSlice,
    command_queue: CommandQueue,
}

impl VirtualGpu {
    pub fn new(physical: Arc<PhysicalGpu>, vram_mb: usize) -> Result<Self> {
        // Allocate VRAM slice
        let vram_slice = physical.allocate_vram(vram_mb * 1024 * 1024)?;
        
        // Create command queue
        let command_queue = physical.create_queue()?;
        
        Ok(Self {
            physical_gpu: physical,
            vm_id: VmId::new(),
            vram_slice,
            command_queue,
        })
    }
    
    pub fn submit_commands(&self, commands: &[GpuCommand]) -> Result<()> {
        // Translate guest addresses to host
        let translated: Vec<_> = commands.iter()
            .map(|cmd| self.translate_command(cmd))
            .collect::<Result<_>>()?;
        
        // Submit to physical GPU
        self.command_queue.submit(&translated)?;
        
        Ok(())
    }
}
```

## Vulkan/WebGPU Support

```rust
pub struct VulkanVirtualDevice {
    instance: ash::Instance,
    physical_device: vk::PhysicalDevice,
    device: ash::Device,
}

impl VulkanVirtualDevice {
    pub fn create_for_vm(vm: &Vm) -> Result<Self> {
        // Create Vulkan instance with required extensions
        let instance = Self::create_instance()?;
        
        // Select physical device
        let physical_device = Self::select_gpu(&instance)?;
        
        // Create logical device with compute queues
        let device = Self::create_device(&instance, physical_device)?;
        
        Ok(Self {
            instance,
            physical_device,
            device,
        })
    }
    
    pub fn allocate_buffer(&self, size: usize) -> Result<VulkanBuffer> {
        let buffer_info = vk::BufferCreateInfo::builder()
            .size(size as u64)
            .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        
        let buffer = unsafe {
            self.device.create_buffer(&buffer_info, None)?
        };
        
        // Allocate and bind memory
        let memory = self.allocate_device_memory(buffer)?;
        unsafe { self.device.bind_buffer_memory(buffer, memory, 0)? };
        
        Ok(VulkanBuffer { buffer, memory })
    }
}
```

## CUDA/OpenCL Support

For passthrough GPUs, CUDA and OpenCL work natively:

```rust
pub struct CudaSupport {
    passthrough: GpuPassthrough,
}

impl CudaSupport {
    /// Configure VM for CUDA support
    pub fn configure_vm(&self, vm: &mut Vm) -> Result<()> {
        // Attach GPU via passthrough
        self.passthrough.attach_to_vm(vm)?;
        
        // Map NVIDIA device files
        vm.add_device_node("/dev/nvidia0")?;
        vm.add_device_node("/dev/nvidiactl")?;
        vm.add_device_node("/dev/nvidia-uvm")?;
        
        Ok(())
    }
}
```

## GPU Scheduling

Fair scheduling across multiple VMs:

```rust
pub struct GpuScheduler {
    vgpus: Vec<Arc<VirtualGpu>>,
    time_slice_ms: u64,
    current_index: usize,
}

impl GpuScheduler {
    pub fn schedule(&mut self) {
        loop {
            let vgpu = &self.vgpus[self.current_index];
            
            // Run vGPU for time slice
            let start = Instant::now();
            while start.elapsed().as_millis() < self.time_slice_ms as u128 {
                if let Some(work) = vgpu.pending_work() {
                    work.execute();
                } else {
                    break;
                }
            }
            
            // Move to next vGPU
            self.current_index = (self.current_index + 1) % self.vgpus.len();
        }
    }
}
```

## Performance Considerations

### Passthrough Performance

- **Latency**: Native (~0% overhead)
- **Bandwidth**: Full PCIe bandwidth
- **Features**: All GPU features available

### vGPU Performance

- **Latency**: 5-20% overhead from scheduling
- **Bandwidth**: Shared across VMs
- **Features**: Most features, some limitations

## Configuration

```toml
# config.toml
[gpu]
mode = "passthrough"  # passthrough, vgpu, software

[gpu.passthrough]
device = "0000:01:00.0"
iommu_group = 1

[gpu.vgpu]
vram_mb = 4096
max_vms = 4
scheduling = "fair"  # fair, priority

[gpu.vulkan]
enabled = true
validation_layers = false
```

## Next Steps

- [Memory Management](./memory.md) - GPU memory in guest
- [Architecture Overview](./overview.md) - System architecture
- [Security](../security/overview.md) - GPU isolation
