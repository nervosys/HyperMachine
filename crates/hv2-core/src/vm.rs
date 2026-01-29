//! Virtual Machine implementation

use crate::{
    DeviceManager, Error, EventBus, GuestMemory, HypervisorBackend, IoDirection, Pic8259, Result,
    VCpu, VmEvent, VmExit,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

/// VM state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VMState {
    Created,
    Running,
    Paused,
    Stopped,
    Error,
}

impl std::fmt::Display for VMState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created => write!(f, "Created"),
            Self::Running => write!(f, "Running"),
            Self::Paused => write!(f, "Paused"),
            Self::Stopped => write!(f, "Stopped"),
            Self::Error => write!(f, "Error"),
        }
    }
}

/// VM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VMConfig {
    pub name: String,
    pub vcpu_count: u32,
    pub memory_size: u64,
    pub enable_gpu: bool,
    pub enable_networking: bool,
    pub enable_tracing: bool,
}

impl Default for VMConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            vcpu_count: 1,
            memory_size: 1024 * 1024 * 1024, // 1GB
            enable_gpu: false,
            enable_networking: false,
            enable_tracing: false,
        }
    }
}

/// Virtual Machine
pub struct VM {
    config: VMConfig,
    state: Arc<RwLock<VMState>>,
    vcpus: Vec<Arc<VCpu>>,
    memory: Arc<GuestMemory>,
    devices: Arc<DeviceManager>,
    pic: Arc<Pic8259>,
    backend: Arc<dyn HypervisorBackend>,
    exit_notify: Arc<Notify>,
    event_bus: EventBus,
}

impl VM {
    /// Create a new VM with the given configuration
    pub fn new(config: VMConfig) -> Result<Self> {
        // Validate configuration
        if config.vcpu_count == 0 {
            return Err(Error::Config("vCPU count must be > 0".to_string()));
        }

        if config.memory_size == 0 {
            return Err(Error::Config("Memory size must be > 0".to_string()));
        }

        // Create hypervisor backend
        let backend = Arc::from(crate::hypervisor::create_backend()?);

        Self::new_with_backend(config, backend)
    }

    /// Create a new VM with a custom hypervisor backend
    ///
    /// This is primarily used for testing with mock backends.
    pub fn new_with_backend(config: VMConfig, backend: Arc<dyn HypervisorBackend>) -> Result<Self> {
        // Validate configuration
        if config.vcpu_count == 0 {
            return Err(Error::Config("vCPU count must be > 0".to_string()));
        }

        if config.memory_size == 0 {
            return Err(Error::Config("Memory size must be > 0".to_string()));
        }

        // Create vCPUs
        let vcpus: Vec<Arc<VCpu>> = (0..config.vcpu_count)
            .map(|id| Arc::new(VCpu::new(id)))
            .collect();

        // Create guest memory
        let memory = Arc::new(GuestMemory::new(config.memory_size)?);

        // Initialize main memory region
        memory.allocate_region(config.memory_size, false)?;

        // Create device manager
        let devices = Arc::new(DeviceManager::new());

        // Create PIC (Intel 8259)
        let pic = Arc::new(Pic8259::new());

        let event_bus = EventBus::default();

        Ok(Self {
            config,
            state: Arc::new(RwLock::new(VMState::Created)),
            vcpus,
            memory,
            devices,
            pic,
            backend,
            exit_notify: Arc::new(Notify::new()),
            event_bus,
        })
    }

    /// Get VM configuration
    pub fn config(&self) -> &VMConfig {
        &self.config
    }

    /// Get VM state
    pub fn state(&self) -> VMState {
        *self.state.read()
    }

    /// Start the VM
    pub async fn start(&self) -> Result<()> {
        let old_state = {
            let mut state = self.state.write();

            if *state != VMState::Created && *state != VMState::Stopped {
                return Err(Error::InvalidState(format!(
                    "Cannot start VM in state {:?}",
                    *state
                )));
            }

            let old = *state;
            *state = VMState::Running;
            old
        };

        tracing::info!(
            "Starting VM '{}' with {} vCPUs and {} GB memory",
            self.config.name,
            self.config.vcpu_count,
            self.config.memory_size / (1024 * 1024 * 1024)
        );

        // Emit state change event
        self.event_bus.publish(VmEvent::state_changed(
            self.config.name.clone(),
            old_state,
            VMState::Running,
        ));

        Ok(())
    }

    /// Pause the VM
    pub async fn pause(&self) -> Result<()> {
        let mut state = self.state.write();

        if *state != VMState::Running {
            return Err(Error::InvalidState(format!(
                "Cannot pause VM in state {:?}",
                *state
            )));
        }

        // Pause all vCPUs
        for vcpu in &self.vcpus {
            vcpu.pause()?;
        }

        *state = VMState::Paused;
        tracing::info!("VM '{}' paused", self.config.name);

        Ok(())
    }

    /// Resume the VM
    pub async fn resume(&self) -> Result<()> {
        let mut state = self.state.write();

        if *state != VMState::Paused {
            return Err(Error::InvalidState(format!(
                "Cannot resume VM in state {:?}",
                *state
            )));
        }

        *state = VMState::Running;
        tracing::info!("VM '{}' resumed", self.config.name);

        Ok(())
    }

    /// Stop the VM
    pub async fn stop(&self) -> Result<()> {
        let mut state = self.state.write();

        if *state == VMState::Stopped {
            return Ok(());
        }

        // Stop all vCPUs
        for vcpu in &self.vcpus {
            vcpu.stop()?;
        }

        *state = VMState::Stopped;
        self.exit_notify.notify_waiters();

        tracing::info!("VM '{}' stopped", self.config.name);

        Ok(())
    }

    /// Get vCPU by ID
    pub fn vcpu(&self, id: u32) -> Option<Arc<VCpu>> {
        self.vcpus.get(id as usize).cloned()
    }

    /// Get all vCPUs
    pub fn vcpus(&self) -> &[Arc<VCpu>] {
        &self.vcpus
    }

    /// Get guest memory
    pub fn memory(&self) -> Arc<GuestMemory> {
        Arc::clone(&self.memory)
    }

    /// Get device manager
    pub fn devices(&self) -> Arc<DeviceManager> {
        Arc::clone(&self.devices)
    }

    /// Get PIC (interrupt controller)
    pub fn pic(&self) -> Arc<Pic8259> {
        Arc::clone(&self.pic)
    }

    /// Wait for VM exit
    pub async fn wait_for_exit(&self) {
        self.exit_notify.notified().await;
    }

    /// Get the event bus
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Subscribe to VM events
    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<VmEvent> {
        self.event_bus.subscribe()
    }

    /// Run the VM execution loop
    ///
    /// This is the main execution loop that runs vCPUs and handles VM exits.
    /// It will continue until the VM is stopped or encounters a shutdown exit.
    pub async fn run(&self) -> Result<()> {
        // Ensure VM is in running state
        if self.state() != VMState::Running {
            return Err(Error::InvalidState(format!(
                "Cannot run VM in state {:?}",
                self.state()
            )));
        }

        // For now, run single vCPU (vCPU 0)
        // TODO: Multi-vCPU support with parallel execution
        let vcpu = self.vcpus[0].clone();

        tracing::info!(
            "Starting VM execution loop for '{}' with vCPU {}",
            self.config.name,
            vcpu.id()
        );

        loop {
            // Check if VM should stop
            if self.state() == VMState::Stopped {
                tracing::info!("VM stopped, exiting execution loop");
                break;
            }

            // Run vCPU until exit
            let exit = self.backend.run_vcpu(&vcpu).await?;

            tracing::debug!("VM exit: {}", exit);

            // Handle the exit
            match self.handle_exit(&vcpu, exit).await {
                Ok(should_continue) => {
                    if !should_continue {
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("Error handling VM exit: {}", e);
                    *self.state.write() = VMState::Error;
                    return Err(e);
                }
            }
        }

        tracing::info!("VM execution loop exited for '{}'", self.config.name);
        Ok(())
    }

    /// Handle a VM exit
    ///
    /// Returns Ok(true) if execution should continue, Ok(false) if VM should stop.
    async fn handle_exit(&self, vcpu: &VCpu, exit: VmExit) -> Result<bool> {
        match exit {
            VmExit::Mmio {
                phys_addr,
                mut data,
                len,
                is_write,
            } => {
                self.handle_mmio_exit(vcpu, phys_addr, &mut data, len, is_write)
                    .await?;
                Ok(true)
            }

            VmExit::Io {
                port,
                direction,
                size,
                data,
            } => {
                self.handle_io_exit(vcpu, port, direction, size, data)
                    .await?;
                Ok(true)
            }

            VmExit::Hlt => {
                self.handle_hlt_exit(vcpu).await?;
                Ok(true)
            }

            VmExit::Shutdown => {
                tracing::info!("Guest initiated shutdown");
                self.stop().await?;
                Ok(false)
            }

            VmExit::InterruptWindow => {
                self.handle_interrupt_window(vcpu).await?;
                Ok(true)
            }

            VmExit::Exception { vector, error_code } => {
                tracing::warn!(
                    "Guest exception: vector={} error_code={:?}",
                    vector,
                    error_code
                );
                // For now, just log and continue
                // TODO: Proper exception injection into guest
                Ok(true)
            }

            VmExit::Debug { info } => {
                tracing::debug!("Debug exit: {}", info);
                Ok(true)
            }

            VmExit::Unknown { reason } => {
                tracing::warn!("Unknown VM exit reason: {}", reason);
                Ok(true)
            }
        }
    }

    /// Handle MMIO exit
    async fn handle_mmio_exit(
        &self,
        _vcpu: &VCpu,
        phys_addr: u64,
        data: &mut [u8; 8],
        len: u32,
        is_write: bool,
    ) -> Result<()> {
        if is_write {
            // MMIO write
            tracing::debug!(
                "MMIO write: addr={:#x} data={:?} len={}",
                phys_addr,
                &data[..len as usize],
                len
            );

            // Try to find device handler
            if let Some(device) = self.devices.find_mmio_device(phys_addr) {
                let offset = phys_addr - device.base_address();
                let value = match len {
                    1 => data[0] as u32,
                    2 => u16::from_le_bytes([data[0], data[1]]) as u32,
                    4 => u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
                    8 => {
                        // For 8-byte writes, we'll do two 4-byte writes
                        let low = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                        device.write_register(offset, low).await?;
                        let high = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                        device.write_register(offset + 4, high).await?;
                        return Ok(());
                    }
                    _ => return Err(Error::InvalidMemoryAccess { address: phys_addr }),
                };

                device.write_register(offset, value).await?;

                // Publish event
                self.event_bus.publish(VmEvent::memory_access(
                    self.config.name.clone(),
                    phys_addr,
                    len as u64,
                    true,
                ));
            } else {
                tracing::warn!("MMIO write to unmapped address: {:#x}", phys_addr);
            }
        } else {
            // MMIO read
            tracing::debug!("MMIO read: addr={:#x} len={}", phys_addr, len);

            if let Some(device) = self.devices.find_mmio_device(phys_addr) {
                let offset = phys_addr - device.base_address();
                let value = device.read_register(offset).await?;

                // Write value into data buffer
                match len {
                    1 => data[0] = value as u8,
                    2 => {
                        let bytes = (value as u16).to_le_bytes();
                        data[..2].copy_from_slice(&bytes);
                    }
                    4 => {
                        let bytes = value.to_le_bytes();
                        data[..4].copy_from_slice(&bytes);
                    }
                    8 => {
                        // For 8-byte reads, we'll do two 4-byte reads
                        let low = device.read_register(offset).await?;
                        let high = device.read_register(offset + 4).await?;
                        data[..4].copy_from_slice(&low.to_le_bytes());
                        data[4..8].copy_from_slice(&high.to_le_bytes());
                    }
                    _ => return Err(Error::InvalidMemoryAccess { address: phys_addr }),
                }

                // Publish event
                self.event_bus.publish(VmEvent::memory_access(
                    self.config.name.clone(),
                    phys_addr,
                    len as u64,
                    false,
                ));
            } else {
                tracing::warn!("MMIO read from unmapped address: {:#x}", phys_addr);
                // Return 0xFF for unmapped reads
                data[..len as usize].fill(0xFF);
            }
        }

        Ok(())
    }

    /// Handle I/O port exit
    async fn handle_io_exit(
        &self,
        _vcpu: &VCpu,
        port: u16,
        direction: IoDirection,
        size: u8,
        mut data: u32,
    ) -> Result<()> {
        match direction {
            IoDirection::Out => {
                tracing::debug!("IO OUT: port={:#x} data={:#x} size={}", port, data, size);

                // Check if this is a PIC port
                if self.pic.handles_port(port) {
                    self.pic.write_port(port, data as u8).await?;
                } else if let Some(device) = self.devices.find_io_device(port) {
                    // Device I/O write
                    let offset = (port - device.base_port()) as u64;
                    device.write_register(offset, data).await?;
                } else {
                    tracing::debug!("IO OUT to unhandled port: {:#x}", port);
                }

                // Publish event
                self.event_bus
                    .publish(VmEvent::io_operation(self.config.name.clone(), port, true));
            }

            IoDirection::In => {
                tracing::debug!("IO IN: port={:#x} size={}", port, size);

                // Check if this is a PIC port
                if self.pic.handles_port(port) {
                    data = self.pic.read_port(port).await? as u32;
                } else if let Some(device) = self.devices.find_io_device(port) {
                    // Device I/O read
                    let offset = (port - device.base_port()) as u64;
                    data = device.read_register(offset).await?;
                } else {
                    tracing::debug!("IO IN from unhandled port: {:#x}", port);
                    data = 0xFF; // Return 0xFF for unmapped ports
                }

                // Publish event
                self.event_bus.publish(VmEvent::io_operation(
                    self.config.name.clone(),
                    port,
                    false,
                ));

                // TODO: Return data to guest via backend interface
                tracing::debug!("IO IN result: {:#x}", data);
            }
        }

        Ok(())
    }

    /// Handle HLT exit
    async fn handle_hlt_exit(&self, vcpu: &VCpu) -> Result<()> {
        tracing::debug!("HLT: vCPU {} halted, checking for interrupts", vcpu.id());

        // Check if there are pending interrupts
        if let Some(vector) = self.pic.get_pending_interrupt() {
            tracing::debug!("Injecting pending interrupt: vector {:#x}", vector);

            // Inject the interrupt
            self.backend.inject_interrupt(vcpu, vector).await?;

            // Acknowledge the interrupt in PIC
            self.pic.acknowledge_interrupt(vector)?;

            // Publish interrupt event
            self.event_bus.publish(VmEvent::device_interrupt(
                self.config.name.clone(),
                vector as u32,
            ));
        } else {
            // No interrupts pending, sleep briefly
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        Ok(())
    }

    /// Handle interrupt window exit
    async fn handle_interrupt_window(&self, vcpu: &VCpu) -> Result<()> {
        tracing::debug!(
            "Interrupt window opened for vCPU {}, injecting pending interrupts",
            vcpu.id()
        );

        // Check if there are pending interrupts
        if let Some(vector) = self.pic.get_pending_interrupt() {
            tracing::debug!("Injecting interrupt: vector {:#x}", vector);

            // Inject the interrupt
            self.backend.inject_interrupt(vcpu, vector).await?;

            // Acknowledge the interrupt in PIC
            self.pic.acknowledge_interrupt(vector)?;

            // Publish interrupt event
            self.event_bus.publish(VmEvent::device_interrupt(
                self.config.name.clone(),
                vector as u32,
            ));
        }

        Ok(())
    }

    /// Get the hypervisor backend
    pub fn backend(&self) -> Arc<dyn HypervisorBackend> {
        Arc::clone(&self.backend)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vm_creation() {
        let config = VMConfig {
            name: "test-vm".to_string(),
            vcpu_count: 2,
            memory_size: 1024 * 1024 * 1024,
            ..Default::default()
        };

        let vm = VM::new(config).unwrap();
        assert_eq!(vm.state(), VMState::Created);
        assert_eq!(vm.vcpus().len(), 2);
    }

    #[tokio::test]
    async fn test_vm_lifecycle() {
        let config = VMConfig::default();
        let vm = VM::new(config).unwrap();

        vm.start().await.unwrap();
        assert_eq!(vm.state(), VMState::Running);

        // TODO: Enable pause/resume tests when vCPU state machine is complete
        // vm.pause().await.unwrap();
        // assert_eq!(vm.state(), VMState::Paused);

        // vm.resume().await.unwrap();
        // assert_eq!(vm.state(), VMState::Running);

        vm.stop().await.unwrap();
        assert_eq!(vm.state(), VMState::Stopped);
    }
}
