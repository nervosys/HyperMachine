//! Virtual Generic Interrupt Controller (vGIC)
//!
//! Emulates the ARM GICv2/GICv3 interrupt controller for guest VMs.
//!
//! # GIC Components
//!
//! | Component              | Role                                    |
//! |------------------------|-----------------------------------------|
//! | Distributor (GICD)     | Routes interrupts to redistributors     |
//! | Redistributor (GICR)  | Per-CPU interrupt config (GICv3 only)   |
//! | CPU Interface (GICC)   | Interrupt acknowledge / EOI             |
//! | Virtual CPU (GICH/ICH) | Hardware-assisted virtual interrupts    |
//!
//! # Interrupt Types
//!
//! - SGI (0-15): Software Generated Interrupts (inter-processor)
//! - PPI (16-31): Private Peripheral Interrupts (per-CPU timers etc.)
//! - SPI (32-1019): Shared Peripheral Interrupts (devices)

use crate::{Error, Result};
use bitflags::bitflags;
use core::sync::atomic::{AtomicBool, Ordering};

/// Maximum number of SPI interrupt IDs
pub const MAX_SPI: usize = 988; // IDs 32..1019
/// Total interrupt ID space
pub const MAX_INTID: usize = 1020;
/// SGI range end (exclusive)
pub const SGI_END: u32 = 16;
/// PPI range end (exclusive)
pub const PPI_END: u32 = 32;
/// SPI range end (exclusive)
pub const SPI_END: u32 = 1020;

/// Interrupt state for a single INTID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptState {
    /// Inactive
    Inactive,
    /// Pending (waiting for CPU to acknowledge)
    Pending,
    /// Active (CPU has acknowledged, not yet EOI'd)
    Active,
    /// Active and Pending (re-triggered while active)
    ActiveAndPending,
}

/// Interrupt trigger type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerType {
    /// Level-triggered
    Level,
    /// Edge-triggered
    Edge,
}

/// Per-interrupt configuration.
#[derive(Debug, Clone, Copy)]
pub struct InterruptConfig {
    /// Whether this interrupt is enabled
    pub enabled: bool,
    /// Current state
    pub state: InterruptState,
    /// Trigger type
    pub trigger: TriggerType,
    /// Priority (0 = highest, 255 = lowest)
    pub priority: u8,
    /// Target CPU mask (for GICv2 SPI routing)
    pub target_cpus: u8,
    /// Group (0 or 1)
    pub group: u8,
}

impl Default for InterruptConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            state: InterruptState::Inactive,
            trigger: TriggerType::Level,
            priority: 0xFF,
            target_cpus: 0x01,
            group: 0,
        }
    }
}

bitflags! {
    /// GICD_CTLR flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DistributorCtrl: u32 {
        /// Enable Group 0 interrupts
        const ENABLE_GRP0 = 1 << 0;
        /// Enable Group 1 interrupts
        const ENABLE_GRP1 = 1 << 1;
        /// Affinity routing enable (GICv3)
        const ARE_S = 1 << 4;
    }
}

/// Virtual GIC distributor state.
///
/// Manages the shared interrupt routing state across all vCPUs in a VM.
#[derive(Debug)]
pub struct VirtualDistributor {
    /// Distributor control register
    ctrl: DistributorCtrl,
    /// Per-interrupt configuration (SGI+PPI+SPI)
    irqs: [InterruptConfig; MAX_INTID],
    /// Whether the distributor has been initialized
    initialized: bool,
}

impl Default for VirtualDistributor {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualDistributor {
    /// Create a new virtual distributor with all interrupts disabled.
    pub fn new() -> Self {
        Self {
            ctrl: DistributorCtrl::empty(),
            irqs: [InterruptConfig::default(); MAX_INTID],
            initialized: false,
        }
    }

    /// Initialize the distributor.
    pub fn initialize(&mut self) -> Result<()> {
        if self.initialized {
            return Err(Error::AlreadyInitialized);
        }

        // SGIs are edge-triggered and always enabled by default
        for i in 0..(SGI_END as usize) {
            self.irqs[i].trigger = TriggerType::Edge;
            self.irqs[i].enabled = true;
        }

        // PPIs default: disabled, level-triggered
        for i in (SGI_END as usize)..(PPI_END as usize) {
            self.irqs[i].trigger = TriggerType::Level;
        }

        self.initialized = true;
        Ok(())
    }

    /// Enable or disable the distributor.
    pub fn set_ctrl(&mut self, ctrl: DistributorCtrl) {
        self.ctrl = ctrl;
    }

    /// Get distributor control flags.
    pub fn ctrl(&self) -> DistributorCtrl {
        self.ctrl
    }

    /// Enable an interrupt by INTID.
    pub fn enable_irq(&mut self, intid: u32) -> Result<()> {
        let idx = intid as usize;
        if idx >= MAX_INTID {
            return Err(Error::InvalidInterruptId);
        }
        self.irqs[idx].enabled = true;
        Ok(())
    }

    /// Disable an interrupt by INTID.
    pub fn disable_irq(&mut self, intid: u32) -> Result<()> {
        let idx = intid as usize;
        if idx >= MAX_INTID {
            return Err(Error::InvalidInterruptId);
        }
        self.irqs[idx].enabled = false;
        Ok(())
    }

    /// Set the priority of an interrupt.
    pub fn set_priority(&mut self, intid: u32, priority: u8) -> Result<()> {
        let idx = intid as usize;
        if idx >= MAX_INTID {
            return Err(Error::InvalidInterruptId);
        }
        self.irqs[idx].priority = priority;
        Ok(())
    }

    /// Set the target CPU mask for an SPI.
    pub fn set_target(&mut self, intid: u32, target_cpus: u8) -> Result<()> {
        let idx = intid as usize;
        if intid < PPI_END || idx >= MAX_INTID {
            return Err(Error::InvalidInterruptId);
        }
        self.irqs[idx].target_cpus = target_cpus;
        Ok(())
    }

    /// Set interrupt to pending.
    pub fn set_pending(&mut self, intid: u32) -> Result<()> {
        let idx = intid as usize;
        if idx >= MAX_INTID {
            return Err(Error::InvalidInterruptId);
        }
        if !self.irqs[idx].enabled {
            return Ok(()); // silently ignored when disabled
        }
        self.irqs[idx].state = match self.irqs[idx].state {
            InterruptState::Inactive => InterruptState::Pending,
            InterruptState::Active => InterruptState::ActiveAndPending,
            other => other,
        };
        Ok(())
    }

    /// Acknowledge the highest-priority pending interrupt for a given CPU.
    ///
    /// Returns the INTID, or `None` if no interrupt is pending.
    pub fn acknowledge(&mut self, cpu_id: u8) -> Option<u32> {
        let mut best_id: Option<u32> = None;
        let mut best_prio: u8 = 0xFF;

        for i in 0..MAX_INTID {
            let irq = &self.irqs[i];
            if !irq.enabled {
                continue;
            }
            if irq.state != InterruptState::Pending && irq.state != InterruptState::ActiveAndPending
            {
                continue;
            }
            // Check CPU targeting (SGI/PPI always route to the owning CPU)
            if i >= PPI_END as usize && (irq.target_cpus & (1 << cpu_id)) == 0 {
                continue;
            }
            if irq.priority < best_prio {
                best_prio = irq.priority;
                best_id = Some(i as u32);
            }
        }

        if let Some(id) = best_id {
            let idx = id as usize;
            self.irqs[idx].state = match self.irqs[idx].state {
                InterruptState::Pending => InterruptState::Active,
                InterruptState::ActiveAndPending => InterruptState::Active,
                other => other,
            };
        }

        best_id
    }

    /// End-of-interrupt: move an active interrupt back to inactive.
    pub fn eoi(&mut self, intid: u32) -> Result<()> {
        let idx = intid as usize;
        if idx >= MAX_INTID {
            return Err(Error::InvalidInterruptId);
        }
        self.irqs[idx].state = match self.irqs[idx].state {
            InterruptState::Active => InterruptState::Inactive,
            InterruptState::ActiveAndPending => InterruptState::Pending,
            other => other,
        };
        Ok(())
    }

    /// Get the configuration for an interrupt.
    pub fn irq_config(&self, intid: u32) -> Result<&InterruptConfig> {
        let idx = intid as usize;
        if idx >= MAX_INTID {
            return Err(Error::InvalidInterruptId);
        }
        Ok(&self.irqs[idx])
    }
}

/// Virtual GIC redistributor state (per-vCPU, GICv3).
#[derive(Debug)]
pub struct VirtualRedistributor {
    /// CPU ID this redistributor belongs to
    cpu_id: u8,
    /// Wake request pending
    wake_pending: bool,
}

impl VirtualRedistributor {
    /// Create a new redistributor for the given CPU.
    pub fn new(cpu_id: u8) -> Self {
        Self {
            cpu_id,
            wake_pending: false,
        }
    }

    /// Get the CPU ID.
    pub fn cpu_id(&self) -> u8 {
        self.cpu_id
    }

    /// Signal a wake request.
    pub fn wake(&mut self) {
        self.wake_pending = true;
    }

    /// Check and clear the wake-pending flag.
    pub fn take_wake(&mut self) -> bool {
        let w = self.wake_pending;
        self.wake_pending = false;
        w
    }
}

/// Combined virtual GIC state for a VM.
#[derive(Debug)]
pub struct VirtualGic {
    /// Shared distributor
    pub distributor: VirtualDistributor,
    /// Per-vCPU redistributors
    pub redistributors: alloc::vec::Vec<VirtualRedistributor>,
}

impl VirtualGic {
    /// Create a new virtual GIC for `num_cpus` vCPUs.
    pub fn new(num_cpus: u8) -> Result<Self> {
        if num_cpus == 0 {
            return Err(Error::InvalidParameter);
        }
        let mut redists = alloc::vec::Vec::new();
        for i in 0..num_cpus {
            redists.push(VirtualRedistributor::new(i));
        }
        Ok(Self {
            distributor: VirtualDistributor::new(),
            redistributors: redists,
        })
    }

    /// Initialize the virtual GIC (distributor + all redistributors).
    pub fn initialize(&mut self) -> Result<()> {
        self.distributor.initialize()
    }

    /// Inject a virtual SPI into the VM.
    pub fn inject_spi(&mut self, intid: u32) -> Result<()> {
        if !(PPI_END..SPI_END).contains(&intid) {
            return Err(Error::InvalidInterruptId);
        }
        self.distributor.set_pending(intid)
    }

    /// Inject a virtual SGI from one CPU to another.
    pub fn inject_sgi(&mut self, from_cpu: u8, to_cpu: u8, intid: u32) -> Result<()> {
        if intid >= SGI_END {
            return Err(Error::InvalidInterruptId);
        }
        // SGIs are always targeted; we just mark pending
        self.distributor.set_pending(intid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distributor_initialize() {
        let mut dist = VirtualDistributor::new();
        assert!(dist.initialize().is_ok());
        // SGIs should be enabled
        for i in 0..SGI_END {
            assert!(dist.irq_config(i).unwrap().enabled);
        }
        // PPIs should be disabled
        for i in SGI_END..PPI_END {
            assert!(!dist.irq_config(i).unwrap().enabled);
        }
    }

    #[test]
    fn distributor_double_init_fails() {
        let mut dist = VirtualDistributor::new();
        dist.initialize().unwrap();
        assert_eq!(dist.initialize(), Err(Error::AlreadyInitialized));
    }

    #[test]
    fn distributor_enable_disable_irq() {
        let mut dist = VirtualDistributor::new();
        dist.initialize().unwrap();
        let spi = 64u32;
        assert!(!dist.irq_config(spi).unwrap().enabled);
        dist.enable_irq(spi).unwrap();
        assert!(dist.irq_config(spi).unwrap().enabled);
        dist.disable_irq(spi).unwrap();
        assert!(!dist.irq_config(spi).unwrap().enabled);
    }

    #[test]
    fn distributor_invalid_intid() {
        let mut dist = VirtualDistributor::new();
        assert_eq!(
            dist.enable_irq(MAX_INTID as u32),
            Err(Error::InvalidInterruptId)
        );
        assert_eq!(
            dist.disable_irq(MAX_INTID as u32),
            Err(Error::InvalidInterruptId)
        );
        assert_eq!(
            dist.set_priority(MAX_INTID as u32, 0),
            Err(Error::InvalidInterruptId)
        );
    }

    #[test]
    fn distributor_set_priority() {
        let mut dist = VirtualDistributor::new();
        dist.initialize().unwrap();
        dist.set_priority(64, 0x10).unwrap();
        assert_eq!(dist.irq_config(64).unwrap().priority, 0x10);
    }

    #[test]
    fn distributor_set_target_spi_only() {
        let mut dist = VirtualDistributor::new();
        dist.initialize().unwrap();
        // Setting target on a PPI should fail
        assert_eq!(dist.set_target(20, 0x01), Err(Error::InvalidInterruptId));
        // Setting target on an SPI should succeed
        assert!(dist.set_target(64, 0x03).is_ok());
        assert_eq!(dist.irq_config(64).unwrap().target_cpus, 0x03);
    }

    #[test]
    fn distributor_pending_acknowledge_eoi_cycle() {
        let mut dist = VirtualDistributor::new();
        dist.initialize().unwrap();
        let spi = 64u32;
        dist.enable_irq(spi).unwrap();
        dist.set_priority(spi, 0x10).unwrap();
        dist.set_target(spi, 0x01).unwrap();

        // No interrupt pending yet
        assert_eq!(dist.acknowledge(0), None);

        // Set pending
        dist.set_pending(spi).unwrap();
        assert_eq!(dist.irq_config(spi).unwrap().state, InterruptState::Pending);

        // Acknowledge
        assert_eq!(dist.acknowledge(0), Some(spi));
        assert_eq!(dist.irq_config(spi).unwrap().state, InterruptState::Active);

        // EOI
        dist.eoi(spi).unwrap();
        assert_eq!(
            dist.irq_config(spi).unwrap().state,
            InterruptState::Inactive
        );
    }

    #[test]
    fn distributor_acknowledge_respects_priority() {
        let mut dist = VirtualDistributor::new();
        dist.initialize().unwrap();

        let low = 64u32;
        let high = 65u32;

        dist.enable_irq(low).unwrap();
        dist.enable_irq(high).unwrap();
        dist.set_priority(low, 0x80).unwrap();
        dist.set_priority(high, 0x10).unwrap();
        dist.set_target(low, 0x01).unwrap();
        dist.set_target(high, 0x01).unwrap();

        dist.set_pending(low).unwrap();
        dist.set_pending(high).unwrap();

        // Higher priority (lower value) should be acknowledged first
        assert_eq!(dist.acknowledge(0), Some(high));
    }

    #[test]
    fn distributor_acknowledge_respects_cpu_target() {
        let mut dist = VirtualDistributor::new();
        dist.initialize().unwrap();

        let spi = 64u32;
        dist.enable_irq(spi).unwrap();
        dist.set_priority(spi, 0x10).unwrap();
        dist.set_target(spi, 0x02).unwrap(); // CPU 1 only

        dist.set_pending(spi).unwrap();

        // CPU 0 should not see it
        assert_eq!(dist.acknowledge(0), None);
        // CPU 1 should see it
        assert_eq!(dist.acknowledge(1), Some(spi));
    }

    #[test]
    fn distributor_active_and_pending() {
        let mut dist = VirtualDistributor::new();
        dist.initialize().unwrap();

        let spi = 64u32;
        dist.enable_irq(spi).unwrap();
        dist.set_priority(spi, 0x10).unwrap();
        dist.set_target(spi, 0x01).unwrap();

        dist.set_pending(spi).unwrap();
        dist.acknowledge(0).unwrap(); // now Active

        // Re-trigger while active
        dist.set_pending(spi).unwrap();
        assert_eq!(
            dist.irq_config(spi).unwrap().state,
            InterruptState::ActiveAndPending
        );

        // EOI should move to Pending
        dist.eoi(spi).unwrap();
        assert_eq!(dist.irq_config(spi).unwrap().state, InterruptState::Pending);
    }

    #[test]
    fn redistributor_wake_cycle() {
        let mut redist = VirtualRedistributor::new(3);
        assert_eq!(redist.cpu_id(), 3);
        assert!(!redist.take_wake());
        redist.wake();
        assert!(redist.take_wake());
        assert!(!redist.take_wake());
    }

    #[test]
    fn virtual_gic_creation() {
        let gic = VirtualGic::new(4).unwrap();
        assert_eq!(gic.redistributors.len(), 4);
    }

    #[test]
    fn virtual_gic_zero_cpus_fails() {
        assert!(matches!(VirtualGic::new(0), Err(Error::InvalidParameter)));
    }

    #[test]
    fn virtual_gic_inject_spi() {
        let mut gic = VirtualGic::new(2).unwrap();
        gic.initialize().unwrap();
        gic.distributor.enable_irq(64).unwrap();
        gic.distributor.set_priority(64, 0x10).unwrap();
        gic.distributor.set_target(64, 0x01).unwrap();

        assert!(gic.inject_spi(64).is_ok());
        assert_eq!(
            gic.distributor.irq_config(64).unwrap().state,
            InterruptState::Pending
        );
    }

    #[test]
    fn virtual_gic_inject_spi_invalid_range() {
        let mut gic = VirtualGic::new(1).unwrap();
        gic.initialize().unwrap();
        // PPI range should fail
        assert_eq!(gic.inject_spi(20), Err(Error::InvalidInterruptId));
        // Past SPI range should fail
        assert_eq!(gic.inject_spi(1020), Err(Error::InvalidInterruptId));
    }

    #[test]
    fn virtual_gic_inject_sgi() {
        let mut gic = VirtualGic::new(2).unwrap();
        gic.initialize().unwrap();
        assert!(gic.inject_sgi(0, 1, 5).is_ok());
    }

    #[test]
    fn virtual_gic_inject_sgi_invalid_range() {
        let mut gic = VirtualGic::new(1).unwrap();
        gic.initialize().unwrap();
        assert_eq!(gic.inject_sgi(0, 1, 16), Err(Error::InvalidInterruptId));
    }

    #[test]
    fn interrupt_config_default() {
        let cfg = InterruptConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.state, InterruptState::Inactive);
        assert_eq!(cfg.priority, 0xFF);
    }

    #[test]
    fn distributor_ctrl_flags() {
        let ctrl = DistributorCtrl::ENABLE_GRP0 | DistributorCtrl::ENABLE_GRP1;
        assert!(ctrl.contains(DistributorCtrl::ENABLE_GRP0));
        assert!(ctrl.contains(DistributorCtrl::ENABLE_GRP1));
        assert!(!ctrl.contains(DistributorCtrl::ARE_S));
    }
}
