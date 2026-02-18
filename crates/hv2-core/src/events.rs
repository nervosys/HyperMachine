//! Event system for VM state changes and notifications

use crate::VMState;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// VM event type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VmEventType {
    /// VM state changed
    StateChanged {
        old_state: VMState,
        new_state: VMState,
    },
    /// vCPU started
    VCpuStarted { vcpu_id: u32 },
    /// vCPU stopped
    VCpuStopped { vcpu_id: u32 },
    /// Memory allocated
    MemoryAllocated { size: u64, address: u64 },
    /// Device attached
    DeviceAttached { device_name: String },
    /// Device detached
    DeviceDetached { device_name: String },
    /// Error occurred
    Error { message: String },
    /// Memory access event (zero allocation variant for hot path)
    MemoryAccess {
        address: u64,
        size: u64,
        is_write: bool,
    },
    /// I/O port operation (zero allocation variant for hot path)
    IoOperation { port: u16, is_write: bool },
    /// Device interrupt (zero allocation variant for hot path)
    DeviceInterrupt { vector: u32 },
    /// Custom event
    Custom { event_type: String, data: String },
}

/// VM event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmEvent {
    /// VM name
    pub vm_name: String,
    /// Event timestamp (Unix timestamp in milliseconds)
    pub timestamp: i64,
    /// Event type
    pub event_type: VmEventType,
}

impl VmEvent {
    /// Create a new VM event
    #[must_use]
    pub fn new(vm_name: impl Into<String>, event_type: VmEventType) -> Self {
        Self {
            vm_name: vm_name.into(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            event_type,
        }
    }

    /// Create a state change event
    #[must_use]
    pub fn state_changed(
        vm_name: impl Into<String>,
        old_state: VMState,
        new_state: VMState,
    ) -> Self {
        Self::new(
            vm_name,
            VmEventType::StateChanged {
                old_state,
                new_state,
            },
        )
    }

    /// Create a vCPU started event
    #[must_use]
    pub fn vcpu_started(vm_name: impl Into<String>, vcpu_id: u32) -> Self {
        Self::new(vm_name, VmEventType::VCpuStarted { vcpu_id })
    }

    /// Create a vCPU stopped event
    #[must_use]
    pub fn vcpu_stopped(vm_name: impl Into<String>, vcpu_id: u32) -> Self {
        Self::new(vm_name, VmEventType::VCpuStopped { vcpu_id })
    }

    /// Create an error event
    #[must_use]
    pub fn error(vm_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(
            vm_name,
            VmEventType::Error {
                message: message.into(),
            },
        )
    }

    /// Create a memory access event
    #[must_use]
    pub fn memory_access(
        vm_name: impl Into<String>,
        address: u64,
        size: u64,
        is_write: bool,
    ) -> Self {
        Self::new(
            vm_name,
            VmEventType::Custom {
                event_type: if is_write {
                    "memory_write".to_string()
                } else {
                    "memory_read".to_string()
                },
                data: format!("address={:#x} size={}", address, size),
            },
        )
    }

    /// Create an I/O operation event
    #[must_use]
    pub fn io_operation(vm_name: impl Into<String>, port: u16, is_write: bool) -> Self {
        Self::new(
            vm_name,
            VmEventType::Custom {
                event_type: if is_write {
                    "io_write".to_string()
                } else {
                    "io_read".to_string()
                },
                data: format!("port={:#x}", port),
            },
        )
    }

    /// Create a device interrupt event
    #[must_use]
    pub fn device_interrupt(vm_name: impl Into<String>, vector: u32) -> Self {
        Self::new(
            vm_name,
            VmEventType::Custom {
                event_type: "device_interrupt".to_string(),
                data: format!("vector={:#x}", vector),
            },
        )
    }
}

/// Event bus for VM events
pub struct EventBus {
    sender: broadcast::Sender<VmEvent>,
}

impl EventBus {
    /// Create a new event bus
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publish an event
    pub fn publish(&self, event: VmEvent) {
        // Ignore send errors (no receivers is OK)
        let _ = self.sender.send(event);
    }

    /// Subscribe to events
    pub fn subscribe(&self) -> broadcast::Receiver<VmEvent> {
        self.sender.subscribe()
    }

    /// Get the number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1000) // Default capacity of 1000 events
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_bus() {
        let bus = EventBus::new(10);
        let mut receiver = bus.subscribe();

        let event =
            VmEvent::state_changed("test-vm".to_string(), VMState::Created, VMState::Running);

        bus.publish(event.clone());

        let received = receiver.recv().await.unwrap();
        assert_eq!(received.vm_name, "test-vm");
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new(10);
        let mut receiver1 = bus.subscribe();
        let mut receiver2 = bus.subscribe();

        let event = VmEvent::vcpu_started("test-vm".to_string(), 0);
        bus.publish(event);

        let r1 = receiver1.recv().await.unwrap();
        let r2 = receiver2.recv().await.unwrap();

        assert_eq!(r1.vm_name, r2.vm_name);
    }
}
