//! virtio over MMIO, version 2 — the transport a guest driver actually finds.
//!
//! # Why a transport was missing
//!
//! Every virtio device in this crate before this one modelled a device but had
//! no way for a guest to reach it: nothing mapped a virtio register file into
//! guest physical address space, so no driver could probe, negotiate features,
//! or publish a queue. The devices were reachable from tests and from nowhere
//! else.
//!
//! [`VirtioMmioTransport`] is that missing half. It implements [`Device`], so
//! registering it with [`DeviceManager::register_mmio_region`] puts it on the
//! path MMIO exits already take, and it speaks the register protocol in
//! section 4.2 of the virtio 1.1 spec. A Linux guest given
//! `virtio_mmio.device=4K@0xd0000000:5` on its command line will probe it.
//!
//! # Register map
//!
//! ```text
//! 0x000 MagicValue      R   "virt"
//! 0x004 Version         R   2
//! 0x008 DeviceID        R   device-specific; 0 means "no device here"
//! 0x00c VendorID        R
//! 0x010 DeviceFeatures  R   bank selected by DeviceFeaturesSel
//! 0x014 DeviceFeaturesSel W
//! 0x020 DriverFeatures  W   bank selected by DriverFeaturesSel
//! 0x024 DriverFeaturesSel W
//! 0x030 QueueSel        W
//! 0x034 QueueNumMax     R
//! 0x038 QueueNum        W
//! 0x044 QueueReady      RW
//! 0x050 QueueNotify     W
//! 0x060 InterruptStatus R
//! 0x064 InterruptACK    W
//! 0x070 Status          RW
//! 0x080 QueueDescLow    W   0x084 QueueDescHigh
//! 0x090 QueueDriverLow  W   0x094 QueueDriverHigh   (available ring)
//! 0x0a0 QueueDeviceLow  W   0x0a4 QueueDeviceHigh   (used ring)
//! 0x0fc ConfigGeneration R
//! 0x100 Config space    RW  device-specific
//! ```
//!
//! [`DeviceManager::register_mmio_region`]: crate::DeviceManager::register_mmio_region

use crate::devices::virtio_queue::GuestQueue;
use crate::{Device, DeviceType, Error, GuestMemory, Pic8259, Result};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::sync::Arc;

/// "virt" in little-endian ASCII, the value at offset 0.
pub const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976;

/// Transport version. 2 is the non-legacy layout this module implements.
pub const VIRTIO_MMIO_VERSION: u32 = 2;

/// Virtio vendor ID reported to the driver.
pub const VIRTIO_MMIO_VENDOR_ID: u32 = 0x554D_4551;

/// Size of the register window, in bytes. One page, which is what the Linux
/// `virtio_mmio.device=` parameter conventionally names.
pub const VIRTIO_MMIO_REGION_SIZE: u64 = 0x1000;

/// Offset at which device-specific configuration begins.
pub const CONFIG_OFFSET: u64 = 0x100;

/// Feature bit every non-legacy device must offer and every non-legacy driver
/// must accept.
pub const VIRTIO_F_VERSION_1: u64 = 1 << 32;

/// Device status bits, as the driver sets them.
pub mod status {
    pub const ACKNOWLEDGE: u32 = 1;
    pub const DRIVER: u32 = 2;
    pub const DRIVER_OK: u32 = 4;
    pub const FEATURES_OK: u32 = 8;
    pub const DEVICE_NEEDS_RESET: u32 = 64;
    pub const FAILED: u32 = 128;
}

/// InterruptStatus bits.
pub mod interrupt {
    /// A virtqueue has been used.
    pub const VRING: u32 = 1;
    /// The device configuration changed.
    pub const CONFIG: u32 = 2;
}

mod reg {
    pub const MAGIC: u64 = 0x000;
    pub const VERSION: u64 = 0x004;
    pub const DEVICE_ID: u64 = 0x008;
    pub const VENDOR_ID: u64 = 0x00c;
    pub const DEVICE_FEATURES: u64 = 0x010;
    pub const DEVICE_FEATURES_SEL: u64 = 0x014;
    pub const DRIVER_FEATURES: u64 = 0x020;
    pub const DRIVER_FEATURES_SEL: u64 = 0x024;
    pub const QUEUE_SEL: u64 = 0x030;
    pub const QUEUE_NUM_MAX: u64 = 0x034;
    pub const QUEUE_NUM: u64 = 0x038;
    pub const QUEUE_READY: u64 = 0x044;
    pub const QUEUE_NOTIFY: u64 = 0x050;
    pub const INTERRUPT_STATUS: u64 = 0x060;
    pub const INTERRUPT_ACK: u64 = 0x064;
    pub const STATUS: u64 = 0x070;
    pub const QUEUE_DESC_LOW: u64 = 0x080;
    pub const QUEUE_DESC_HIGH: u64 = 0x084;
    pub const QUEUE_DRIVER_LOW: u64 = 0x090;
    pub const QUEUE_DRIVER_HIGH: u64 = 0x094;
    pub const QUEUE_DEVICE_LOW: u64 = 0x0a0;
    pub const QUEUE_DEVICE_HIGH: u64 = 0x0a4;
    pub const CONFIG_GENERATION: u64 = 0x0fc;
}

/// A virtio device that a [`VirtioMmioTransport`] can carry.
///
/// This is deliberately not [`crate::devices::virtio::VirtioDevice`]: that
/// trait hands out the host-side [`crate::devices::virtio::VirtQueue`], which
/// has no guest memory behind it. A device reached by a real driver needs
/// [`GuestQueue`], so it needs its own trait.
pub trait VirtioMmioDevice: Send + Sync {
    /// Virtio device ID (for example 19 for vsock). Zero tells a probing
    /// driver there is no device at this address.
    fn device_id(&self) -> u32;

    /// Features this device offers, including [`VIRTIO_F_VERSION_1`].
    fn device_features(&self) -> u64;

    /// Record what the driver accepted. Everything the device does afterwards
    /// must respect it.
    fn ack_features(&mut self, features: u64);

    /// The device's virtqueues, in index order.
    fn queues(&mut self) -> &mut [GuestQueue];

    /// Read from device-specific configuration space.
    fn read_config(&self, offset: u64, data: &mut [u8]);

    /// Write to device-specific configuration space.
    fn write_config(&mut self, offset: u64, data: &[u8]);

    /// The driver kicked `queue`. Returns whether the device consumed anything
    /// and so owes the driver an interrupt.
    fn notify(&mut self, queue: u16, mem: &GuestMemory) -> Result<bool>;

    /// Return to the post-reset state.
    fn reset(&mut self);
}

/// Registers the transport owns, as opposed to the device behind it.
#[derive(Debug, Default)]
struct TransportRegs {
    device_features_sel: u32,
    driver_features_sel: u32,
    driver_features: u64,
    queue_sel: u32,
    status: u32,
    interrupt_status: u32,
    config_generation: u32,
}

/// A virtio-MMIO v2 register window in front of one device.
pub struct VirtioMmioTransport {
    name: String,
    base_address: u64,
    memory: Arc<GuestMemory>,
    device: Arc<Mutex<dyn VirtioMmioDevice>>,
    regs: Mutex<TransportRegs>,
    /// Interrupt controller and line, when the VM wired one up. `None` means
    /// the driver must poll — which a test does, and a real guest does not.
    interrupt: Option<(Arc<Pic8259>, u8)>,
    /// Where an interrupt actually reaches a guest.
    ///
    /// The `Pic8259` above is a userspace model, and a guest whose interrupt
    /// controller lives inside the hypervisor never reads it -- so raising
    /// only there is indistinguishable from raising nothing. Installed by the
    /// device manager at registration.
    interrupt_sink: Mutex<Option<Arc<dyn crate::device::InterruptSink>>>,
}

impl VirtioMmioTransport {
    /// Place `device` behind a register window at `base_address`.
    pub fn new(
        name: impl Into<String>,
        base_address: u64,
        memory: Arc<GuestMemory>,
        device: Arc<Mutex<dyn VirtioMmioDevice>>,
    ) -> Self {
        Self {
            name: name.into(),
            base_address,
            memory,
            device,
            regs: Mutex::new(TransportRegs::default()),
            interrupt: None,
            interrupt_sink: Mutex::new(None),
        }
    }

    /// Route this device's interrupts to `irq` on `pic`.
    pub fn with_interrupt(mut self, pic: Arc<Pic8259>, irq: u8) -> Self {
        self.interrupt = Some((pic, irq));
        self
    }

    /// Guest physical address of the register window.
    pub fn base_address(&self) -> u64 {
        self.base_address
    }

    /// Size of the register window.
    pub fn region_size(&self) -> u64 {
        VIRTIO_MMIO_REGION_SIZE
    }

    /// The device behind this transport, for a host-side caller.
    ///
    /// Do not hold this lock across a call back into the transport: register
    /// access takes the same lock, and it is not reentrant. Host-side use —
    /// opening a connection, reading what the guest sent — only ever locks the
    /// device, so the ordering holds as long as the two are not mixed.
    pub fn device(&self) -> Arc<Mutex<dyn VirtioMmioDevice>> {
        self.device.clone()
    }

    /// Bits the driver has set in the Status register.
    pub fn status(&self) -> u32 {
        self.regs.lock().status
    }

    /// Whether the driver finished bring-up: features accepted and DRIVER_OK
    /// set, with no failure recorded.
    ///
    /// A device must not touch a queue before this is true. Nothing about a
    /// published queue address says the driver is finished with it.
    pub fn driver_ok(&self) -> bool {
        let status = self.regs.lock().status;
        status & status::DRIVER_OK != 0
            && status & status::FEATURES_OK != 0
            && status & status::FAILED == 0
    }

    /// Pending interrupt bits the driver has not acknowledged.
    pub fn interrupt_status(&self) -> u32 {
        self.regs.lock().interrupt_status
    }

    /// Raise the vring interrupt: what a device calls after it has published
    /// used descriptors on its own initiative, rather than in response to a
    /// notify.
    pub fn signal_used_queue(&self) -> Result<()> {
        self.raise(interrupt::VRING)
    }

    /// Raise the configuration-changed interrupt.
    pub fn signal_config_change(&self) -> Result<()> {
        self.regs.lock().config_generation += 1;
        self.raise(interrupt::CONFIG)
    }

    fn raise(&self, bits: u32) -> Result<()> {
        // The status bits come first and matter either way: a driver's handler
        // reads InterruptStatus to find out why it was interrupted, and an
        // interrupt raised without them tells it there is nothing to do.
        self.regs.lock().interrupt_status |= bits;

        if let Some((pic, irq)) = &self.interrupt {
            pic.raise_irq(*irq)?;

            // Asserted and held, not pulsed. A virtio interrupt is
            // level-triggered: the driver clears it by writing `InterruptACK`,
            // and a line released before that acknowledgement can be asserted
            // and released between one delivery and the next, losing the
            // interrupt entirely. From the guest that is indistinguishable
            // from a device that simply stopped answering.
            let sink = self.interrupt_sink.lock().clone();
            if let Some(sink) = sink {
                sink.assert_line(*irq);
            }
        }
        Ok(())
    }

    /// Release the interrupt line once the driver has acknowledged everything.
    ///
    /// Called after a write to `InterruptACK`. While any status bit is still
    /// set there is an interrupt outstanding and the line stays asserted --
    /// dropping it early would lose the reason the driver has not yet handled.
    fn settle_interrupt(&self, remaining: u32) {
        if remaining != 0 {
            return;
        }
        if let Some((_, irq)) = &self.interrupt {
            let sink = self.interrupt_sink.lock().clone();
            if let Some(sink) = sink {
                sink.deassert_line(*irq);
            }
        }
    }

    /// Read one 32-bit register.
    fn read_register(&self, offset: u64) -> u32 {
        let regs = self.regs.lock();

        match offset {
            reg::MAGIC => VIRTIO_MMIO_MAGIC,
            reg::VERSION => VIRTIO_MMIO_VERSION,
            reg::DEVICE_ID => self.device.lock().device_id(),
            reg::VENDOR_ID => VIRTIO_MMIO_VENDOR_ID,
            reg::DEVICE_FEATURES => {
                let features = self.device.lock().device_features();
                match regs.device_features_sel {
                    0 => features as u32,
                    1 => (features >> 32) as u32,
                    // The spec defines two banks. A driver selecting a third
                    // is asking about features that cannot exist.
                    _ => 0,
                }
            }
            // Zero is how the transport says "there is no such queue", which
            // is what a driver probing past the last one needs to see.
            reg::QUEUE_NUM_MAX => self.selected_queue(&regs, |q| u32::from(q.max_size())),
            reg::QUEUE_READY => self.selected_queue(&regs, |q| u32::from(q.is_ready())),
            reg::INTERRUPT_STATUS => regs.interrupt_status,
            reg::STATUS => regs.status,
            reg::CONFIG_GENERATION => regs.config_generation,
            // Write-only registers read as zero rather than as stale state.
            reg::DEVICE_FEATURES_SEL
            | reg::DRIVER_FEATURES
            | reg::DRIVER_FEATURES_SEL
            | reg::QUEUE_SEL
            | reg::QUEUE_NUM
            | reg::QUEUE_NOTIFY
            | reg::INTERRUPT_ACK
            | reg::QUEUE_DESC_LOW
            | reg::QUEUE_DESC_HIGH
            | reg::QUEUE_DRIVER_LOW
            | reg::QUEUE_DRIVER_HIGH
            | reg::QUEUE_DEVICE_LOW
            | reg::QUEUE_DEVICE_HIGH => 0,
            other => {
                tracing::debug!(
                    "virtio-mmio '{}': read from unhandled register {:#x}",
                    self.name,
                    other
                );
                0
            }
        }
    }

    /// Write one 32-bit register.
    fn write_register(&self, offset: u64, value: u32) -> Result<()> {
        let mut regs = self.regs.lock();

        match offset {
            reg::DEVICE_FEATURES_SEL => regs.device_features_sel = value,
            reg::DRIVER_FEATURES_SEL => regs.driver_features_sel = value,
            reg::DRIVER_FEATURES => {
                let shift = if regs.driver_features_sel == 0 { 0 } else { 32 };
                regs.driver_features |= u64::from(value) << shift;
                self.device.lock().ack_features(regs.driver_features);
            }
            reg::QUEUE_SEL => regs.queue_sel = value,
            reg::QUEUE_NUM => self.with_selected_queue(&regs, |q| q.set_size(value as u16)),
            reg::QUEUE_READY => {
                tracing::debug!(
                    "virtio-mmio '{}': queue {} ready={value}",
                    self.name,
                    regs.queue_sel
                );
                self.with_selected_queue(&regs, |q| q.set_ready(value != 0));
            }
            reg::QUEUE_DESC_LOW => {
                self.with_selected_queue(&regs, |q| {
                    q.set_desc_addr(set_low(q.desc_addr(), value));
                });
            }
            reg::QUEUE_DESC_HIGH => {
                self.with_selected_queue(&regs, |q| {
                    q.set_desc_addr(set_high(q.desc_addr(), value));
                });
            }
            reg::QUEUE_DRIVER_LOW => {
                self.with_selected_queue(&regs, |q| {
                    q.set_avail_addr(set_low(q.avail_addr(), value));
                });
            }
            reg::QUEUE_DRIVER_HIGH => {
                self.with_selected_queue(&regs, |q| {
                    q.set_avail_addr(set_high(q.avail_addr(), value));
                });
            }
            reg::QUEUE_DEVICE_LOW => {
                self.with_selected_queue(&regs, |q| {
                    q.set_used_addr(set_low(q.used_addr(), value));
                });
            }
            reg::QUEUE_DEVICE_HIGH => {
                self.with_selected_queue(&regs, |q| {
                    q.set_used_addr(set_high(q.used_addr(), value));
                });
            }
            reg::QUEUE_NOTIFY => {
                // A notify before the driver finished bring-up is not a kick
                // the device should act on: the queue may be half-published.
                let ready = regs.status & status::DRIVER_OK != 0;
                drop(regs);
                if !ready {
                    tracing::debug!(
                        "virtio-mmio '{}': notify for queue {value} before DRIVER_OK, ignored",
                        self.name
                    );
                    return Ok(());
                }
                let used = self.device.lock().notify(value as u16, &self.memory)?;
                {
                    let mut device = self.device.lock();
                    let idx = device
                        .queues()
                        .get(value as usize)
                        .map(|q| q.avail_idx(&self.memory));
                    tracing::debug!(
                        "virtio-mmio '{}': notify queue {value}, used={used}, avail_idx={idx:?}",
                        self.name
                    );
                }
                if used {
                    self.raise(interrupt::VRING)?;
                }
                return Ok(());
            }
            reg::INTERRUPT_ACK => {
                regs.interrupt_status &= !value;
                let remaining = regs.interrupt_status;
                drop(regs);
                self.settle_interrupt(remaining);
                return Ok(());
            }
            reg::STATUS => {
                if value == 0 {
                    // The driver reset the device. Everything negotiated is
                    // void, including whatever the device buffered.
                    regs.status = 0;
                    regs.driver_features = 0;
                    regs.device_features_sel = 0;
                    regs.driver_features_sel = 0;
                    regs.queue_sel = 0;
                    regs.interrupt_status = 0;
                    drop(regs);
                    // A reset clears the status bits, so nothing is
                    // outstanding and the line must not stay held: the driver
                    // that reset the device is not going to acknowledge an
                    // interrupt it no longer knows about.
                    self.settle_interrupt(0);
                    self.device.lock().reset();
                    return Ok(());
                }
                regs.status = value;
            }
            other => {
                tracing::debug!(
                    "virtio-mmio '{}': write to unhandled register {:#x}",
                    self.name,
                    other
                );
            }
        }

        Ok(())
    }

    /// Apply `f` to the queue QueueSel names, ignoring a selector past the
    /// last queue rather than panicking on guest-chosen input.
    fn with_selected_queue(&self, regs: &TransportRegs, f: impl FnOnce(&mut GuestQueue)) {
        let mut device = self.device.lock();
        let sel = regs.queue_sel as usize;
        if let Some(queue) = device.queues().get_mut(sel) {
            f(queue);
        }
    }

    /// Read a value out of the queue QueueSel names, or 0 when the selector is
    /// past the last queue.
    fn selected_queue(&self, regs: &TransportRegs, f: impl FnOnce(&GuestQueue) -> u32) -> u32 {
        let mut device = self.device.lock();
        let sel = regs.queue_sel as usize;
        device.queues().get(sel).map(f).unwrap_or(0)
    }
}

fn set_low(addr: u64, value: u32) -> u64 {
    (addr & 0xffff_ffff_0000_0000) | u64::from(value)
}

fn set_high(addr: u64, value: u32) -> u64 {
    (addr & 0x0000_0000_ffff_ffff) | (u64::from(value) << 32)
}

#[async_trait]
impl Device for VirtioMmioTransport {
    fn set_interrupt_sink(&mut self, sink: Arc<dyn crate::device::InterruptSink>) {
        *self.interrupt_sink.lock() = Some(sink);
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Custom
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn init(&mut self) -> Result<()> {
        tracing::info!(
            "virtio-mmio '{}' at {:#x}, device id {}",
            self.name,
            self.base_address,
            self.device.lock().device_id()
        );
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        if offset >= CONFIG_OFFSET {
            self.device.lock().read_config(offset - CONFIG_OFFSET, data);
            return Ok(());
        }

        // Register reads are 32-bit; the exit path may still ask for fewer
        // bytes, so answer from the register the offset falls in.
        let aligned = offset & !0x3;
        let value = self.read_register(aligned);
        let bytes = value.to_le_bytes();
        let start = (offset - aligned) as usize;
        let available = 4 - start;
        if data.len() > available {
            return Err(Error::Device(format!(
                "virtio-mmio read of {} bytes at {offset:#x} crosses a register",
                data.len()
            )));
        }
        data.copy_from_slice(&bytes[start..start + data.len()]);
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        if offset >= CONFIG_OFFSET {
            self.device
                .lock()
                .write_config(offset - CONFIG_OFFSET, data);
            return Ok(());
        }

        if data.len() != 4 || !offset.is_multiple_of(4) {
            return Err(Error::Device(format!(
                "virtio-mmio register write must be an aligned 32-bit access, got {} bytes at {offset:#x}",
                data.len()
            )));
        }
        let value = u32::from_le_bytes(data.try_into().expect("4 bytes"));
        self.write_register(offset, value)
    }

    async fn reset(&mut self) -> Result<()> {
        *self.regs.lock() = TransportRegs::default();
        self.device.lock().reset();
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::virtio_vsock::{VsockDevice, VIRTIO_ID_VSOCK};

    const BASE: u64 = 0xd000_0000;

    /// A device that records what the transport did to it.
    struct StubDevice {
        queues: Vec<GuestQueue>,
        acked_features: u64,
        notified: Vec<u16>,
        resets: usize,
        config: Vec<u8>,
        /// What [`VirtioMmioDevice::notify`] reports back.
        consumes: bool,
    }

    impl StubDevice {
        fn new() -> Self {
            Self {
                queues: vec![GuestQueue::new(64), GuestQueue::new(64)],
                acked_features: 0,
                notified: Vec::new(),
                resets: 0,
                config: vec![0xaa, 0xbb, 0xcc, 0xdd],
                consumes: true,
            }
        }
    }

    impl VirtioMmioDevice for StubDevice {
        fn device_id(&self) -> u32 {
            42
        }

        fn device_features(&self) -> u64 {
            VIRTIO_F_VERSION_1 | 0b1010
        }

        fn ack_features(&mut self, features: u64) {
            self.acked_features = features;
        }

        fn queues(&mut self) -> &mut [GuestQueue] {
            &mut self.queues
        }

        fn read_config(&self, offset: u64, data: &mut [u8]) {
            for (i, byte) in data.iter_mut().enumerate() {
                *byte = self.config.get(offset as usize + i).copied().unwrap_or(0);
            }
        }

        fn write_config(&mut self, offset: u64, data: &[u8]) {
            for (i, byte) in data.iter().enumerate() {
                let idx = offset as usize + i;
                if idx < self.config.len() {
                    self.config[idx] = *byte;
                }
            }
        }

        fn notify(&mut self, queue: u16, _mem: &GuestMemory) -> Result<bool> {
            self.notified.push(queue);
            Ok(self.consumes)
        }

        fn reset(&mut self) {
            self.resets += 1;
            self.acked_features = 0;
            for queue in &mut self.queues {
                queue.reset();
            }
        }
    }

    fn transport() -> (VirtioMmioTransport, Arc<Mutex<StubDevice>>) {
        let device = Arc::new(Mutex::new(StubDevice::new()));
        let memory = Arc::new(GuestMemory::new(0x1000).expect("guest memory"));
        let transport = VirtioMmioTransport::new("virtio-test", BASE, memory, device.clone());
        (transport, device)
    }

    async fn read32(transport: &VirtioMmioTransport, offset: u64) -> u32 {
        let mut data = [0u8; 4];
        transport.read(offset, &mut data).await.expect("read");
        u32::from_le_bytes(data)
    }

    async fn write32(transport: &mut VirtioMmioTransport, offset: u64, value: u32) {
        transport
            .write(offset, &value.to_le_bytes())
            .await
            .expect("write");
    }

    /// Take the driver through feature negotiation, as a real one does before
    /// touching a queue.
    async fn bring_up(transport: &mut VirtioMmioTransport) {
        write32(transport, reg::STATUS, status::ACKNOWLEDGE).await;
        write32(transport, reg::STATUS, status::ACKNOWLEDGE | status::DRIVER).await;
        write32(transport, reg::DRIVER_FEATURES_SEL, 1).await;
        write32(transport, reg::DRIVER_FEATURES, 1).await;
        write32(
            transport,
            reg::STATUS,
            status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK,
        )
        .await;
        write32(
            transport,
            reg::STATUS,
            status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK | status::DRIVER_OK,
        )
        .await;
    }

    #[tokio::test]
    async fn a_probing_driver_finds_the_magic_version_and_device_id() {
        let (transport, _device) = transport();

        // This is the whole of device discovery: without these four values a
        // Linux driver decides there is nothing at this address.
        assert_eq!(read32(&transport, reg::MAGIC).await, VIRTIO_MMIO_MAGIC);
        assert_eq!(read32(&transport, reg::VERSION).await, VIRTIO_MMIO_VERSION);
        assert_eq!(read32(&transport, reg::DEVICE_ID).await, 42);
        assert_eq!(
            read32(&transport, reg::VENDOR_ID).await,
            VIRTIO_MMIO_VENDOR_ID
        );
    }

    #[tokio::test]
    async fn device_features_are_reported_in_two_banks() {
        let (mut transport, _device) = transport();

        write32(&mut transport, reg::DEVICE_FEATURES_SEL, 0).await;
        assert_eq!(read32(&transport, reg::DEVICE_FEATURES).await, 0b1010);

        // VIRTIO_F_VERSION_1 is bit 32, so it only appears in the high bank.
        // A driver that never reads bank 1 concludes this is a legacy device.
        write32(&mut transport, reg::DEVICE_FEATURES_SEL, 1).await;
        assert_eq!(read32(&transport, reg::DEVICE_FEATURES).await, 1);
    }

    #[tokio::test]
    async fn accepted_features_from_both_banks_reach_the_device() {
        let (mut transport, device) = transport();

        write32(&mut transport, reg::DRIVER_FEATURES_SEL, 0).await;
        write32(&mut transport, reg::DRIVER_FEATURES, 0b10).await;
        write32(&mut transport, reg::DRIVER_FEATURES_SEL, 1).await;
        write32(&mut transport, reg::DRIVER_FEATURES, 1).await;

        assert_eq!(
            device.lock().acked_features,
            VIRTIO_F_VERSION_1 | 0b10,
            "the two banks combine into one 64-bit set"
        );
    }

    #[tokio::test]
    async fn a_queue_address_published_in_halves_lands_whole() {
        let (mut transport, device) = transport();

        write32(&mut transport, reg::QUEUE_SEL, 1).await;
        write32(&mut transport, reg::QUEUE_NUM, 16).await;
        write32(&mut transport, reg::QUEUE_DESC_LOW, 0x8000_1000).await;
        write32(&mut transport, reg::QUEUE_DESC_HIGH, 0x7).await;
        write32(&mut transport, reg::QUEUE_DRIVER_LOW, 0x2000).await;
        write32(&mut transport, reg::QUEUE_DEVICE_LOW, 0x3000).await;
        write32(&mut transport, reg::QUEUE_READY, 1).await;

        {
            let mut device = device.lock();
            let queue = &device.queues()[1];
            assert_eq!(queue.size(), 16);
            assert_eq!(queue.desc_addr(), 0x7_8000_1000);
            assert_eq!(queue.avail_addr(), 0x2000);
            assert_eq!(queue.used_addr(), 0x3000);
            assert!(queue.is_ready());
        }
        assert_eq!(read32(&transport, reg::QUEUE_READY).await, 1);
    }

    #[tokio::test]
    async fn selecting_a_queue_that_does_not_exist_reports_no_queue() {
        let (mut transport, _device) = transport();

        write32(&mut transport, reg::QUEUE_SEL, 0).await;
        assert_eq!(read32(&transport, reg::QUEUE_NUM_MAX).await, 64);

        // A driver walks selectors until one reads zero. Anything else here
        // and it keeps configuring queues the device does not have.
        write32(&mut transport, reg::QUEUE_SEL, 7).await;
        assert_eq!(read32(&transport, reg::QUEUE_NUM_MAX).await, 0);
        write32(&mut transport, reg::QUEUE_NUM, 16).await;
        write32(&mut transport, reg::QUEUE_READY, 1).await;
        assert_eq!(read32(&transport, reg::QUEUE_READY).await, 0);
    }

    #[tokio::test]
    async fn a_notify_before_the_driver_is_ready_is_ignored() {
        let (mut transport, device) = transport();

        // Bring-up is not finished, so a queue may be half-published. Acting
        // on this kick means reading addresses the driver has not written.
        write32(&mut transport, reg::QUEUE_NOTIFY, 0).await;
        assert!(device.lock().notified.is_empty());

        bring_up(&mut transport).await;
        write32(&mut transport, reg::QUEUE_NOTIFY, 0).await;
        assert_eq!(device.lock().notified, vec![0]);
    }

    #[tokio::test]
    async fn a_notify_the_device_acted_on_raises_an_interrupt_the_driver_can_clear() {
        let (mut transport, _device) = transport();
        bring_up(&mut transport).await;

        write32(&mut transport, reg::QUEUE_NOTIFY, 1).await;
        assert_eq!(
            read32(&transport, reg::INTERRUPT_STATUS).await,
            interrupt::VRING
        );

        write32(&mut transport, reg::INTERRUPT_ACK, interrupt::VRING).await;
        assert_eq!(read32(&transport, reg::INTERRUPT_STATUS).await, 0);
    }

    #[tokio::test]
    async fn a_notify_the_device_ignored_raises_nothing() {
        let (mut transport, device) = transport();
        device.lock().consumes = false;
        bring_up(&mut transport).await;

        write32(&mut transport, reg::QUEUE_NOTIFY, 0).await;
        assert_eq!(
            read32(&transport, reg::INTERRUPT_STATUS).await,
            0,
            "an interrupt with nothing behind it is a wakeup the driver wasted"
        );
    }

    #[tokio::test]
    async fn driver_ok_requires_the_whole_handshake() {
        let (mut transport, _device) = transport();

        write32(&mut transport, reg::STATUS, status::DRIVER_OK).await;
        assert!(
            !transport.driver_ok(),
            "DRIVER_OK without FEATURES_OK is not a finished handshake"
        );

        bring_up(&mut transport).await;
        assert!(transport.driver_ok());

        let failed = transport.status() | status::FAILED;
        write32(&mut transport, reg::STATUS, failed).await;
        assert!(!transport.driver_ok(), "a failed driver is not ready");
    }

    #[tokio::test]
    async fn writing_zero_to_status_resets_the_device() {
        let (mut transport, device) = transport();
        bring_up(&mut transport).await;
        write32(&mut transport, reg::QUEUE_SEL, 0).await;
        write32(&mut transport, reg::QUEUE_NUM, 16).await;

        write32(&mut transport, reg::STATUS, 0).await;

        assert_eq!(device.lock().resets, 1);
        assert_eq!(read32(&transport, reg::STATUS).await, 0);
        assert_eq!(device.lock().acked_features, 0);
        assert_eq!(device.lock().queues()[0].size(), 0);
    }

    #[tokio::test]
    async fn config_space_reads_and_writes_reach_the_device() {
        let (mut transport, device) = transport();

        let mut data = [0u8; 2];
        transport
            .read(CONFIG_OFFSET + 1, &mut data)
            .await
            .expect("config read");
        assert_eq!(data, [0xbb, 0xcc], "offsets are relative to config space");

        transport
            .write(CONFIG_OFFSET, &[0x11])
            .await
            .expect("config write");
        assert_eq!(device.lock().config[0], 0x11);
    }

    #[tokio::test]
    async fn a_misaligned_register_write_is_refused() {
        let (mut transport, _device) = transport();

        // Registers are 32-bit and aligned. A byte write to the middle of one
        // is a driver bug, and guessing what it meant is worse than saying so.
        assert!(transport
            .write(reg::QUEUE_NUM + 1, &[1, 2, 3, 4])
            .await
            .is_err());
        assert!(transport.write(reg::QUEUE_NUM, &[1]).await.is_err());
    }

    #[tokio::test]
    async fn a_register_read_may_not_straddle_two_registers() {
        let (transport, _device) = transport();

        let mut data = [0u8; 4];
        assert!(transport.read(reg::MAGIC + 2, &mut data).await.is_err());
    }

    #[tokio::test]
    async fn a_vsock_device_comes_up_through_the_register_file() {
        // The two halves of this work meeting: a device that only knows how to
        // serve guest-memory queues, reached entirely through the registers a
        // driver writes.
        let device = Arc::new(Mutex::new(VsockDevice::new(3).expect("vsock")));
        let memory = Arc::new(GuestMemory::new(0x20000).expect("guest memory"));
        memory.allocate_region(0x20000, false).expect("region");
        let mut transport =
            VirtioMmioTransport::new("virtio-vsock", BASE, memory.clone(), device.clone());

        assert_eq!(read32(&transport, reg::DEVICE_ID).await, VIRTIO_ID_VSOCK);

        let mut cid = [0u8; 8];
        transport.read(CONFIG_OFFSET, &mut cid).await.expect("cid");
        assert_eq!(u64::from_le_bytes(cid), 3);

        bring_up(&mut transport).await;

        // Publish the rx queue the way a driver does, then confirm the device
        // sees a queue it is willing to use.
        write32(&mut transport, reg::QUEUE_SEL, 0).await;
        write32(&mut transport, reg::QUEUE_NUM, 8).await;
        write32(&mut transport, reg::QUEUE_DESC_LOW, 0x1000).await;
        write32(&mut transport, reg::QUEUE_DRIVER_LOW, 0x2000).await;
        write32(&mut transport, reg::QUEUE_DEVICE_LOW, 0x3000).await;
        write32(&mut transport, reg::QUEUE_READY, 1).await;

        assert!(device.lock().queues()[0].is_ready());
        assert!(transport.driver_ok());
    }
}
