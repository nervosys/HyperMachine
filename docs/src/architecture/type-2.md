# Type-2 Hypervisor

The Type-2 (hosted) hypervisor runs on top of an existing operating system, leveraging the host's hypervisor APIs.

## Overview

```
┌─────────────────────────────────────────────────────────┐
│                     Guest VMs                            │
│  ┌───────────┐  ┌───────────┐  ┌───────────┐           │
│  │   VM 1    │  │   VM 2    │  │   VM N    │           │
│  │  (Linux)  │  │ (Windows) │  │  (Guest)  │           │
│  └───────────┘  └───────────┘  └───────────┘           │
├─────────────────────────────────────────────────────────┤
│              HyperMachine Type-2 (hv2-core)             │
├─────────────────────────────────────────────────────────┤
│                   Host Hypervisor API                    │
│            KVM (Linux) / WHPX (Windows) / HVF (macOS)   │
├─────────────────────────────────────────────────────────┤
│                      Host OS Kernel                      │
├─────────────────────────────────────────────────────────┤
│                       Hardware                           │
└─────────────────────────────────────────────────────────┘
```

## Platform Backends

### Linux - KVM

```rust
pub struct KvmBackend {
    kvm: File,           // /dev/kvm
    vm_fd: VmFd,         // VM file descriptor
    vcpus: Vec<VcpuFd>,  // vCPU file descriptors
}

impl KvmBackend {
    pub fn new() -> Result<Self> {
        let kvm = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")?;
        
        // Check KVM API version
        let api_version = unsafe { ioctl(&kvm, KVM_GET_API_VERSION) };
        assert_eq!(api_version, 12);
        
        // Create VM
        let vm_fd = unsafe { ioctl(&kvm, KVM_CREATE_VM, 0) };
        
        Ok(Self { kvm, vm_fd, vcpus: vec![] })
    }
    
    pub fn create_vcpu(&mut self, id: u32) -> Result<VcpuFd> {
        let vcpu_fd = unsafe { ioctl(&self.vm_fd, KVM_CREATE_VCPU, id) };
        self.vcpus.push(vcpu_fd);
        Ok(vcpu_fd)
    }
    
    pub fn run_vcpu(&self, vcpu: &VcpuFd) -> Result<KvmExit> {
        unsafe { ioctl(vcpu, KVM_RUN, 0) }
    }
}
```

### Windows - WHPX

```rust
pub struct WhpxBackend {
    partition: WHV_PARTITION_HANDLE,
    vcpus: Vec<WhpxVcpu>,
}

impl WhpxBackend {
    pub fn new() -> Result<Self> {
        // Check WHPX capability
        let capability = unsafe {
            WHvGetCapability(WHvCapabilityCodeHypervisorPresent)
        }?;
        
        if !capability.HypervisorPresent {
            return Err(Error::HypervisorNotAvailable);
        }
        
        // Create partition
        let partition = unsafe { WHvCreatePartition() }?;
        
        // Configure partition
        unsafe {
            WHvSetPartitionProperty(
                partition,
                WHvPartitionPropertyCodeProcessorCount,
                &vcpu_count,
            )?;
        }
        
        // Setup partition
        unsafe { WHvSetupPartition(partition) }?;
        
        Ok(Self { partition, vcpus: vec![] })
    }
    
    pub fn run_vcpu(&self, vcpu: &WhpxVcpu) -> Result<WhpxExitContext> {
        unsafe {
            WHvRunVirtualProcessor(
                self.partition,
                vcpu.index,
                &mut exit_context,
            )
        }
    }
}
```

### macOS - Hypervisor.framework (HVF)

```rust
pub struct HvfBackend {
    vcpus: Vec<hv_vcpuid_t>,
}

impl HvfBackend {
    pub fn new() -> Result<Self> {
        // Create VM
        let result = unsafe { hv_vm_create(HV_VM_DEFAULT) };
        if result != HV_SUCCESS {
            return Err(Error::HypervisorNotAvailable);
        }
        
        Ok(Self { vcpus: vec![] })
    }
    
    pub fn create_vcpu(&mut self) -> Result<hv_vcpuid_t> {
        let mut vcpu: hv_vcpuid_t = 0;
        unsafe { hv_vcpu_create(&mut vcpu, HV_VCPU_DEFAULT) }?;
        self.vcpus.push(vcpu);
        Ok(vcpu)
    }
    
    pub fn run_vcpu(&self, vcpu: hv_vcpuid_t) -> Result<HvfExit> {
        unsafe { hv_vcpu_run(vcpu) }
    }
}
```

## Unified Backend Interface

```rust
pub trait HypervisorBackend {
    fn create_vm(&mut self, config: &VmConfig) -> Result<VmHandle>;
    fn create_vcpu(&mut self, vm: &VmHandle, id: u32) -> Result<VcpuHandle>;
    fn run_vcpu(&self, vcpu: &VcpuHandle) -> Result<VmExit>;
    fn map_memory(&mut self, vm: &VmHandle, mapping: MemoryMapping) -> Result<()>;
    fn set_vcpu_registers(&mut self, vcpu: &VcpuHandle, regs: &Registers) -> Result<()>;
    fn get_vcpu_registers(&self, vcpu: &VcpuHandle) -> Result<Registers>;
}

// Platform-specific implementations
#[cfg(target_os = "linux")]
pub type PlatformBackend = KvmBackend;

#[cfg(target_os = "windows")]
pub type PlatformBackend = WhpxBackend;

#[cfg(target_os = "macos")]
pub type PlatformBackend = HvfBackend;
```

## VM Lifecycle

```rust
pub struct Vm {
    backend: Box<dyn HypervisorBackend>,
    config: VmConfig,
    state: VmState,
    vcpus: Vec<Vcpu>,
    memory: GuestMemory,
    devices: DeviceManager,
}

impl Vm {
    pub fn create(config: VmConfig) -> Result<Self> {
        let backend = PlatformBackend::new()?;
        let vm_handle = backend.create_vm(&config)?;
        
        // Setup memory
        let memory = GuestMemory::new(config.memory_mb * 1024 * 1024)?;
        backend.map_memory(&vm_handle, memory.as_mapping())?;
        
        // Create vCPUs
        let vcpus: Vec<Vcpu> = (0..config.cpu_cores)
            .map(|id| Vcpu::new(&backend, &vm_handle, id))
            .collect::<Result<_>>()?;
        
        // Initialize devices
        let devices = DeviceManager::new(&config)?;
        
        Ok(Self {
            backend,
            config,
            state: VmState::Created,
            vcpus,
            memory,
            devices,
        })
    }
    
    pub fn start(&mut self) -> Result<()> {
        self.state = VmState::Running;
        
        // Start vCPU threads
        for vcpu in &mut self.vcpus {
            vcpu.start()?;
        }
        
        Ok(())
    }
    
    pub fn stop(&mut self) -> Result<()> {
        self.state = VmState::Stopped;
        
        for vcpu in &mut self.vcpus {
            vcpu.stop()?;
        }
        
        Ok(())
    }
}
```

## Device Emulation

### virtio Devices

```rust
pub trait VirtioDevice {
    fn device_type(&self) -> u32;
    fn read_config(&self, offset: u64, data: &mut [u8]);
    fn write_config(&mut self, offset: u64, data: &[u8]);
    fn activate(&mut self, queues: Vec<VirtQueue>) -> Result<()>;
}

// Example: virtio-blk
pub struct VirtioBlock {
    disk: Box<dyn BlockDevice>,
    config: VirtioBlockConfig,
}

impl VirtioDevice for VirtioBlock {
    fn device_type(&self) -> u32 { VIRTIO_BLK_DEVICE_ID }
    
    fn activate(&mut self, queues: Vec<VirtQueue>) -> Result<()> {
        let request_queue = &queues[0];
        
        // Process block requests
        while let Some(desc_chain) = request_queue.pop() {
            let request = self.parse_request(&desc_chain)?;
            match request.request_type {
                VIRTIO_BLK_T_IN => self.handle_read(request)?,
                VIRTIO_BLK_T_OUT => self.handle_write(request)?,
                _ => {}
            }
        }
        
        Ok(())
    }
}
```

## Advantages

| Aspect | Type-2 Advantage |
|--------|------------------|
| **Installation** | No special boot requirements |
| **Development** | Easy debugging with host tools |
| **Hardware Support** | Leverages host OS drivers |
| **Integration** | Works with existing workflows |

## Performance Considerations

- **VM Exit Overhead** - Each VM exit requires context switch to userspace
- **Memory Mapping** - Use huge pages for better TLB efficiency
- **I/O Virtualization** - Use virtio for optimal performance

## Next Steps

- [Memory Management](./memory.md) - Memory architecture details
- [GPU Virtualization](./gpu.md) - GPU passthrough and virtual GPU
- [Type-1 Hypervisor](./type-1.md) - Bare-metal alternative
