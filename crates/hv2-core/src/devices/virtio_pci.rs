//! Modern virtio over PCI, so a stock kernel finds the device by itself.
//!
//! [`VirtioMmioTransport`](super::virtio_mmio::VirtioMmioTransport) works, but
//! only for a guest that was told where to look. There is no way for a kernel
//! to discover an MMIO device: the address arrives on the command line as
//! `virtio_mmio.device=4K@0xd0000000:5`, and the kernel must have been built
//! with `CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES`. A stock cloud image has neither,
//! so it boots and reports no device rather than failing in a way anyone would
//! notice.
//!
//! PCI is how that becomes discoverable. The guest enumerates a bus it already
//! knows how to walk, reads a vendor and device id it already recognises, and
//! binds `virtio_pci` with no command line argument at all.
//!
//! # What the driver sees
//!
//! Configuration space identifies the device, and a vendor-specific capability
//! chain says where in a BAR each structure lives (virtio 1.2 §4.1.4):
//!
//! ```text
//! config space                    BAR 0 (memory)
//! ┌───────────────────┐           ┌──────────────────────────┐ 0x0000
//! │ 1AF4:1040+id      │      ┌───►│ common configuration     │
//! │ ...               │      │    ├──────────────────────────┤ 0x1000
//! │ cap: COMMON  ─────┼──────┘ ┌─►│ ISR status               │
//! │ cap: ISR     ─────┼────────┘ ┌┼──────────────────────────┤ 0x2000
//! │ cap: NOTIFY  ─────┼──────────┘│ queue notification       │
//! │ cap: DEVICE  ─────┼───────────┼──────────────────────────┤ 0x3000
//! └───────────────────┘           │ device-specific config   │
//!                                 └──────────────────────────┘ 0x4000
//! ```
//!
//! The structures are spaced a page apart so a guest that wants to map one
//! without the others can, which is what a driver using `vfio` expects.
//!
//! # What this shares with the MMIO transport
//!
//! The device behind it: both drive a [`VirtioMmioDevice`], whose name is
//! historical rather than descriptive -- it names queues, features, config
//! space and a notify callback, none of which are transport-specific. Nothing
//! in `virtio_vsock` or `virtio_blk` had to change to be reachable over PCI.
//!
//! # What this does not do yet
//!
//! The BAR is programmed to the address the window was registered at, and a
//! guest that rewrites it is refused rather than obeyed -- moving the window
//! means re-registering the MMIO region, which the device manager does not
//! expose. Linux does not reprogram BARs that firmware already assigned, so
//! this is a limitation rather than a bug in practice, and it says so in the
//! log rather than silently ignoring the write.
//!
//! Interrupts are legacy INTx through the ISR register. MSI-X is what a modern
//! guest prefers and is not implemented; the capability is absent, so a driver
//! falls back rather than finding a structure that does nothing.

use crate::devices::virtio_mmio::VirtioMmioDevice;
use crate::memory::GuestMemory;
use crate::pci::{BarConfig, ClassCode, ConfigSpace, DeviceId, VendorId};
use crate::{Device, DeviceType, Pic8259, Result};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::sync::Arc;

/// Red Hat's vendor id, which every virtio device uses.
pub const VIRTIO_PCI_VENDOR: u16 = 0x1AF4;

/// Modern virtio device ids start here: `0x1040 + virtio device id`.
///
/// The transitional range at `0x1000..=0x103F` is for drivers that predate
/// virtio 1.0. Offering only the modern id keeps the device from being bound
/// by a legacy driver that would then find none of the registers it expects.
pub const VIRTIO_PCI_DEVICE_ID_BASE: u16 = 0x1040;

/// Size of the BAR window: four structures, one page each.
pub const VIRTIO_PCI_BAR_SIZE: u64 = 0x4000;

/// Offsets of the four structures within the BAR.
mod window {
    /// Common configuration, virtio 1.2 §4.1.4.3.
    pub const COMMON: u64 = 0x0000;
    /// Length of the common configuration structure.
    pub const COMMON_LEN: u64 = 0x38;
    /// ISR status, virtio 1.2 §4.1.4.5. One byte, read to clear.
    pub const ISR: u64 = 0x1000;
    /// Length of the ISR structure.
    pub const ISR_LEN: u64 = 0x1;
    /// Queue notification, virtio 1.2 §4.1.4.4.
    pub const NOTIFY: u64 = 0x2000;
    /// Length of the notification structure.
    pub const NOTIFY_LEN: u64 = 0x1000;
    /// Device-specific configuration, virtio 1.2 §4.1.4.6.
    pub const DEVICE: u64 = 0x3000;
    /// Length of the device-specific configuration structure.
    pub const DEVICE_LEN: u64 = 0x1000;
}

/// Bytes between one queue's notification address and the next.
///
/// The driver computes `notify_base + queue_notify_off * multiplier`. Four
/// gives every queue its own dword, so which queue was kicked is the address
/// rather than the value -- what a real device does, and what lets a guest use
/// a single `iowrite16` per queue.
pub const NOTIFY_OFF_MULTIPLIER: u32 = 4;

/// Offsets within the common configuration structure.
mod common {
    pub const DEVICE_FEATURE_SELECT: u64 = 0x00;
    pub const DEVICE_FEATURE: u64 = 0x04;
    pub const DRIVER_FEATURE_SELECT: u64 = 0x08;
    pub const DRIVER_FEATURE: u64 = 0x0C;
    pub const CONFIG_MSIX_VECTOR: u64 = 0x10;
    pub const NUM_QUEUES: u64 = 0x12;
    pub const DEVICE_STATUS: u64 = 0x14;
    pub const CONFIG_GENERATION: u64 = 0x15;
    pub const QUEUE_SELECT: u64 = 0x16;
    pub const QUEUE_SIZE: u64 = 0x18;
    pub const QUEUE_MSIX_VECTOR: u64 = 0x1A;
    pub const QUEUE_ENABLE: u64 = 0x1C;
    pub const QUEUE_NOTIFY_OFF: u64 = 0x1E;
    pub const QUEUE_DESC: u64 = 0x20;
    pub const QUEUE_DRIVER: u64 = 0x28;
    pub const QUEUE_DEVICE: u64 = 0x30;
}

/// Device status bits, virtio 1.2 §2.1.
pub mod status {
    /// The guest has noticed the device.
    pub const ACKNOWLEDGE: u8 = 1;
    /// The guest has a driver for it.
    pub const DRIVER: u8 = 2;
    /// The driver is ready to drive it.
    pub const DRIVER_OK: u8 = 4;
    /// The driver accepted the feature set.
    pub const FEATURES_OK: u8 = 8;
    /// Something went wrong; the device must be reset.
    pub const NEEDS_RESET: u8 = 0x40;
    /// The driver gave up.
    pub const FAILED: u8 = 0x80;
}

/// ISR status bits, virtio 1.2 §4.1.4.5.
mod isr {
    /// A virtqueue was used.
    pub const QUEUE: u8 = 0x1;
}

/// PCI capability ids and virtio configuration types.
mod cap {
    /// Vendor-specific capability, which is what virtio uses.
    pub const VENDOR_SPECIFIC: u8 = 0x09;
    pub const COMMON_CFG: u8 = 1;
    pub const NOTIFY_CFG: u8 = 2;
    pub const ISR_CFG: u8 = 3;
    pub const DEVICE_CFG: u8 = 4;
}

/// Transport-owned registers, as opposed to the device behind it.
#[derive(Debug, Default)]
struct CommonRegs {
    device_feature_select: u32,
    driver_feature_select: u32,
    driver_features: u64,
    queue_select: u16,
    device_status: u8,
    config_generation: u8,
    isr: u8,
}

/// A modern virtio-pci register window in front of one device.
pub struct VirtioPciTransport {
    name: String,
    /// Guest physical address the BAR window is mapped at.
    bar_base: u64,
    memory: Arc<GuestMemory>,
    device: Arc<Mutex<dyn VirtioMmioDevice>>,
    regs: Mutex<CommonRegs>,
    /// Userspace interrupt controller and line, when a VM wired one up.
    interrupt: Option<(Arc<Pic8259>, u8)>,
    /// Where an interrupt actually reaches a guest whose interrupt controller
    /// lives in the hypervisor. Installed at registration.
    interrupt_sink: Mutex<Option<Arc<dyn crate::device::InterruptSink>>>,
}

impl VirtioPciTransport {
    /// Place `device` behind a virtio-pci window at `bar_base`.
    pub fn new(
        name: impl Into<String>,
        bar_base: u64,
        memory: Arc<GuestMemory>,
        device: Arc<Mutex<dyn VirtioMmioDevice>>,
    ) -> Self {
        Self {
            name: name.into(),
            bar_base,
            memory,
            device,
            regs: Mutex::new(CommonRegs::default()),
            interrupt: None,
            interrupt_sink: Mutex::new(None),
        }
    }

    /// Route this device's interrupts to `irq` on `pic`.
    pub fn with_interrupt(mut self, pic: Arc<Pic8259>, irq: u8) -> Self {
        self.interrupt = Some((pic, irq));
        self
    }

    /// Guest physical address of the BAR window.
    pub fn bar_base(&self) -> u64 {
        self.bar_base
    }

    /// Size of the BAR window.
    pub fn bar_size(&self) -> u64 {
        VIRTIO_PCI_BAR_SIZE
    }

    /// The device behind this transport, for a host-side caller.
    ///
    /// Do not hold this lock across a call back into the transport: register
    /// access takes the same lock, and it is not reentrant.
    pub fn device(&self) -> Arc<Mutex<dyn VirtioMmioDevice>> {
        self.device.clone()
    }

    /// Bits the driver has set in the device status register.
    pub fn status(&self) -> u8 {
        self.regs.lock().device_status
    }

    /// Whether the driver has finished bringing the device up.
    ///
    /// A host-side caller that queues something before this is queueing into a
    /// device the guest is not yet listening to.
    pub fn is_driver_ok(&self) -> bool {
        self.regs.lock().device_status & status::DRIVER_OK != 0
    }

    /// The configuration space a guest reads for this device.
    ///
    /// The capability chain is built here rather than by the caller, because
    /// its offsets have to agree with the window this transport decodes, and a
    /// disagreement between the two is invisible until a driver reads the
    /// wrong structure.
    pub fn config_space(&self) -> ConfigSpace {
        let virtio_device_id = self.device.lock().device_id() as u16;
        let mut config = ConfigSpace::with_device(
            VendorId(VIRTIO_PCI_VENDOR),
            DeviceId(VIRTIO_PCI_DEVICE_ID_BASE + virtio_device_id),
            // 0x08/0x80: base system peripheral, other. Virtio devices carry a
            // class matching what they do, but the driver binds on vendor and
            // device id, and claiming a class we do not implement invites a
            // class-bound driver to try.
            ClassCode {
                base: 0x08,
                sub: 0x80,
                prog_if: 0x00,
            },
            0x01,
        );

        // Subsystem id repeats the virtio device id: virtio 1.2 §4.1.2.1 says
        // a transitional driver reads it there, and a modern one may check it.
        config.set_subsystem(VendorId(VIRTIO_PCI_VENDOR), DeviceId(virtio_device_id));

        // BAR 0 is where all four structures live, as a 64-bit memory BAR.
        write_bar0(&mut config, self.bar_base);

        write_capabilities(&mut config);

        config
    }

    /// Raise the queue interrupt: set the ISR bit and assert the line.
    fn signal_used(&self) -> Result<()> {
        // The bit comes first and matters either way: the driver's handler
        // reads the ISR to find out why it was interrupted, and an interrupt
        // raised without it says there is nothing to do.
        self.regs.lock().isr |= isr::QUEUE;

        if let Some((pic, irq)) = &self.interrupt {
            pic.raise_irq(*irq)?;

            // Asserted and held, not pulsed. INTx is level-triggered, and the
            // acknowledgement is the driver's read of the ISR -- see the read
            // path, which deasserts. A line released before that read can be
            // asserted and released between one delivery and the next, losing
            // the interrupt entirely.
            let sink = self.interrupt_sink.lock().clone();
            if let Some(sink) = sink {
                sink.assert_line(*irq);
            }
        }
        Ok(())
    }

    /// Release the line once the driver has read the ISR.
    ///
    /// Reading the ISR is the acknowledgement in virtio-pci -- there is no
    /// separate ACK register as there is in the MMIO transport -- so this is
    /// called from the read path rather than from a write.
    fn settle_interrupt(&self) {
        if let Some((_, irq)) = &self.interrupt {
            let sink = self.interrupt_sink.lock().clone();
            if let Some(sink) = sink {
                sink.deassert_line(*irq);
            }
        }
    }

    /// Read from the common configuration structure.
    fn read_common(&self, offset: u64, len: usize) -> u64 {
        let regs = self.regs.lock();
        match offset {
            common::DEVICE_FEATURE_SELECT => u64::from(regs.device_feature_select),
            common::DEVICE_FEATURE => {
                let features = self.device.lock().device_features();
                // The driver reads 64 bits of features 32 at a time, choosing
                // which half with the select register above.
                if regs.device_feature_select == 0 {
                    features & 0xFFFF_FFFF
                } else {
                    features >> 32
                }
            }
            common::DRIVER_FEATURE_SELECT => u64::from(regs.driver_feature_select),
            common::DRIVER_FEATURE => {
                if regs.driver_feature_select == 0 {
                    regs.driver_features & 0xFFFF_FFFF
                } else {
                    regs.driver_features >> 32
                }
            }
            // No MSI-X capability is offered, so the vector is always the
            // "no vector" value rather than whatever the driver last wrote.
            common::CONFIG_MSIX_VECTOR | common::QUEUE_MSIX_VECTOR => 0xFFFF,
            common::NUM_QUEUES => self.device.lock().queues().len() as u64,
            common::DEVICE_STATUS => u64::from(regs.device_status),
            common::CONFIG_GENERATION => u64::from(regs.config_generation),
            common::QUEUE_SELECT => u64::from(regs.queue_select),
            common::QUEUE_SIZE => self.with_selected_queue(&regs, |q| u64::from(q.size())),
            common::QUEUE_ENABLE => {
                self.with_selected_queue(&regs, |q| u64::from(u16::from(q.is_ready())))
            }
            // Every queue notifies at its own offset, so the driver can use a
            // single write per queue rather than writing the queue index.
            common::QUEUE_NOTIFY_OFF => u64::from(regs.queue_select),
            common::QUEUE_DESC => self.with_selected_queue(&regs, |q| q.desc_addr()),
            common::QUEUE_DRIVER => self.with_selected_queue(&regs, |q| q.avail_addr()),
            common::QUEUE_DEVICE => self.with_selected_queue(&regs, |q| q.used_addr()),
            _ => {
                // A read of the upper half of a 64-bit register lands here.
                if let Some(base) = sixty_four_bit_base(offset) {
                    let full = match base {
                        common::QUEUE_DESC => self.with_selected_queue(&regs, |q| q.desc_addr()),
                        common::QUEUE_DRIVER => self.with_selected_queue(&regs, |q| q.avail_addr()),
                        _ => self.with_selected_queue(&regs, |q| q.used_addr()),
                    };
                    return full >> 32;
                }
                tracing::debug!(
                    "virtio-pci '{}': read of unmapped common config offset {offset:#x} ({len} bytes)",
                    self.name
                );
                0
            }
        }
    }

    /// Write to the common configuration structure.
    fn write_common(&self, offset: u64, value: u64) {
        let mut regs = self.regs.lock();
        match offset {
            common::DEVICE_FEATURE_SELECT => regs.device_feature_select = value as u32,
            common::DRIVER_FEATURE_SELECT => regs.driver_feature_select = value as u32,
            common::DRIVER_FEATURE => {
                if regs.driver_feature_select == 0 {
                    regs.driver_features =
                        (regs.driver_features & !0xFFFF_FFFF) | (value & 0xFFFF_FFFF);
                } else {
                    regs.driver_features =
                        (regs.driver_features & 0xFFFF_FFFF) | ((value & 0xFFFF_FFFF) << 32);
                }
                let accepted = regs.driver_features;
                drop(regs);
                self.device.lock().ack_features(accepted);
            }
            common::DEVICE_STATUS => {
                let new = value as u8;
                if new == 0 {
                    // Writing zero is a reset, and the device must come back in
                    // its post-reset state rather than keeping queues the old
                    // driver programmed.
                    regs.device_status = 0;
                    regs.driver_features = 0;
                    regs.isr = 0;
                    drop(regs);
                    self.device.lock().reset();
                } else {
                    regs.device_status = new;
                }
            }
            common::QUEUE_SELECT => regs.queue_select = value as u16,
            common::QUEUE_SIZE => self.with_selected_queue_mut(&regs, |q| q.set_size(value as u16)),
            common::QUEUE_ENABLE => {
                self.with_selected_queue_mut(&regs, |q| q.set_ready(value != 0));
            }
            common::QUEUE_DESC => {
                self.with_selected_queue_mut(&regs, |q| q.set_desc_addr(value));
            }
            common::QUEUE_DRIVER => {
                self.with_selected_queue_mut(&regs, |q| q.set_avail_addr(value));
            }
            common::QUEUE_DEVICE => {
                self.with_selected_queue_mut(&regs, |q| q.set_used_addr(value));
            }
            // MSI-X vectors are accepted and discarded: no MSI-X capability is
            // offered, so a driver should not be writing these, and refusing
            // would be a worse failure than ignoring.
            common::CONFIG_MSIX_VECTOR | common::QUEUE_MSIX_VECTOR => {}
            _ => {
                if let Some(base) = sixty_four_bit_base(offset) {
                    // Upper half of a 64-bit address register.
                    let high = (value & 0xFFFF_FFFF) << 32;
                    match base {
                        common::QUEUE_DESC => self.with_selected_queue_mut(&regs, |q| {
                            q.set_desc_addr((q.desc_addr() & 0xFFFF_FFFF) | high);
                        }),
                        common::QUEUE_DRIVER => self.with_selected_queue_mut(&regs, |q| {
                            q.set_avail_addr((q.avail_addr() & 0xFFFF_FFFF) | high);
                        }),
                        _ => self.with_selected_queue_mut(&regs, |q| {
                            q.set_used_addr((q.used_addr() & 0xFFFF_FFFF) | high);
                        }),
                    }
                    return;
                }
                tracing::debug!(
                    "virtio-pci '{}': write of unmapped common config offset {offset:#x}",
                    self.name
                );
            }
        }
    }

    fn with_selected_queue<T: Default>(
        &self,
        regs: &CommonRegs,
        f: impl FnOnce(&crate::devices::virtio_queue::GuestQueue) -> T,
    ) -> T {
        let mut device = self.device.lock();
        let index = regs.queue_select as usize;
        match device.queues().get(index) {
            Some(queue) => f(queue),
            None => T::default(),
        }
    }

    fn with_selected_queue_mut(
        &self,
        regs: &CommonRegs,
        f: impl FnOnce(&mut crate::devices::virtio_queue::GuestQueue),
    ) {
        let mut device = self.device.lock();
        let index = regs.queue_select as usize;
        if let Some(queue) = device.queues().get_mut(index) {
            f(queue);
        }
    }
}

/// The base offset of the 64-bit register an unaligned offset falls inside.
fn sixty_four_bit_base(offset: u64) -> Option<u64> {
    [
        common::QUEUE_DESC,
        common::QUEUE_DRIVER,
        common::QUEUE_DEVICE,
    ]
    .into_iter()
    .find(|base| offset > *base && offset < base + 8)
}

/// Program BAR 0 as a 64-bit memory BAR at `base`.
///
/// Through `configure_bar` rather than by writing the config-space bytes:
/// BAR reads are served from the `BarConfig`, not from the raw array, so bytes
/// written directly would be visible to nothing. The size mask that
/// `new_memory64_lower` computes is also what makes BAR sizing work -- a
/// driver writes all ones and reads back the mask to learn the window is
/// 16 KiB.
///
/// Not prefetchable: the registers have read side effects -- the ISR clears
/// when read -- and a bridge that prefetched would clear it for nobody.
fn write_bar0(config: &mut ConfigSpace, base: u64) {
    let mut low = BarConfig::new_memory64_lower(VIRTIO_PCI_BAR_SIZE, false);
    low.write(base as u32);
    config.configure_bar(0, low);

    let mut high = BarConfig::new_memory64_upper(VIRTIO_PCI_BAR_SIZE);
    high.write((base >> 32) as u32);
    config.configure_bar(1, high);
}

/// Write the four virtio capabilities into the capability list.
fn write_capabilities(config: &mut ConfigSpace) {
    // Capabilities start after the standard header. 0x40 is the usual first
    // free offset and is where every other virtio implementation puts them.
    const FIRST: u16 = 0x40;
    /// A virtio_pci_cap is 16 bytes; the notify capability adds a 4-byte
    /// multiplier.
    const CAP_LEN: u16 = 16;
    const NOTIFY_CAP_LEN: u16 = 20;

    let common_at = FIRST;
    let notify_at = common_at + CAP_LEN;
    let isr_at = notify_at + NOTIFY_CAP_LEN;
    let device_at = isr_at + CAP_LEN;

    write_cap(
        config,
        common_at,
        notify_at as u8,
        cap::COMMON_CFG,
        CAP_LEN as u8,
        window::COMMON as u32,
        window::COMMON_LEN as u32,
    );
    write_cap(
        config,
        notify_at,
        isr_at as u8,
        cap::NOTIFY_CFG,
        NOTIFY_CAP_LEN as u8,
        window::NOTIFY as u32,
        window::NOTIFY_LEN as u32,
    );
    // The notify capability's extra field: how far apart consecutive queues'
    // notification addresses are.
    config.set_u32((notify_at + 16) as u8, NOTIFY_OFF_MULTIPLIER);

    write_cap(
        config,
        isr_at,
        device_at as u8,
        cap::ISR_CFG,
        CAP_LEN as u8,
        window::ISR as u32,
        window::ISR_LEN as u32,
    );
    write_cap(
        config,
        device_at,
        // End of the chain.
        0,
        cap::DEVICE_CFG,
        CAP_LEN as u8,
        window::DEVICE as u32,
        window::DEVICE_LEN as u32,
    );

    // Sets the pointer and the capabilities-list bit in the status register
    // together. Without that bit a driver does not walk the chain, however
    // correct the chain is.
    config.set_capabilities_ptr(common_at as u8);
}

/// One `virtio_pci_cap`, virtio 1.2 §4.1.4.
#[allow(clippy::too_many_arguments)]
fn write_cap(
    config: &mut ConfigSpace,
    at: u16,
    next: u8,
    cfg_type: u8,
    cap_len: u8,
    offset: u32,
    length: u32,
) {
    let at = at as u8;
    // Written a dword at a time because the unmasked setters work in 16- and
    // 32-bit units, and capability space is read-only to the guest, so the
    // masked byte writers would drop all of this.
    config.set_u32(
        at,
        u32::from(cap::VENDOR_SPECIFIC)
            | (u32::from(next) << 8)
            | (u32::from(cap_len) << 16)
            | (u32::from(cfg_type) << 24),
    );
    // BAR index in byte 0, structure id in byte 1, two padding bytes. Every
    // structure lives in BAR 0, and there is one of each, so this is zero.
    config.set_u32(at + 4, 0);
    config.set_u32(at + 8, offset);
    config.set_u32(at + 12, length);
}

#[async_trait]
impl Device for VirtioPciTransport {
    fn device_type(&self) -> DeviceType {
        DeviceType::Custom
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn set_interrupt_sink(&mut self, sink: Arc<dyn crate::device::InterruptSink>) {
        *self.interrupt_sink.lock() = Some(sink);
    }

    async fn init(&mut self) -> Result<()> {
        tracing::info!(
            "virtio-pci '{}': BAR 0 at {:#x}, {} queue(s)",
            self.name,
            self.bar_base,
            self.device.lock().queues().len()
        );
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        if offset < window::COMMON + window::COMMON_LEN {
            let value = self.read_common(offset - window::COMMON, data.len());
            let bytes = value.to_le_bytes();
            for (i, byte) in data.iter_mut().enumerate() {
                *byte = bytes.get(i).copied().unwrap_or(0);
            }
            return Ok(());
        }

        if (window::ISR..window::ISR + window::ISR_LEN).contains(&offset) {
            // Reading the ISR clears it, which is how the driver acknowledges
            // the interrupt. Doing this in a `&self` read is the whole reason
            // the registers sit behind a lock.
            let previous = {
                let mut regs = self.regs.lock();
                let previous = regs.isr;
                regs.isr = 0;
                previous
            };
            data[0] = previous;
            for byte in data.iter_mut().skip(1) {
                *byte = 0;
            }
            // The ISR is now clear, so nothing is outstanding and the line
            // drops. Holding it here would re-enter the handler forever.
            self.settle_interrupt();
            return Ok(());
        }

        if offset >= window::DEVICE {
            self.device
                .lock()
                .read_config(offset - window::DEVICE, data);
            return Ok(());
        }

        // The notification structure and the gaps between structures read as
        // zero rather than as stale bytes of a neighbour.
        for byte in data.iter_mut() {
            *byte = 0;
        }
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        let mut value = 0u64;
        for (i, byte) in data.iter().take(8).enumerate() {
            value |= u64::from(*byte) << (i * 8);
        }

        if offset < window::COMMON + window::COMMON_LEN {
            self.write_common(offset - window::COMMON, value);
            return Ok(());
        }

        if (window::NOTIFY..window::NOTIFY + window::NOTIFY_LEN).contains(&offset) {
            // Which queue was kicked is the address, not the value: the driver
            // computes notify_base + queue_notify_off * multiplier.
            let queue = ((offset - window::NOTIFY) / u64::from(NOTIFY_OFF_MULTIPLIER)) as u16;
            let used = self.device.lock().notify(queue, &self.memory)?;
            if used {
                self.signal_used()?;
            }
            return Ok(());
        }

        if offset >= window::DEVICE {
            self.device
                .lock()
                .write_config(offset - window::DEVICE, data);
            return Ok(());
        }

        // A write to the ISR or to a gap. The ISR is read-only to the driver.
        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        {
            let mut regs = self.regs.lock();
            *regs = CommonRegs::default();
        }
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
    use crate::devices::virtio_mmio::VIRTIO_F_VERSION_1;
    use crate::devices::virtio_queue::GuestQueue;

    const BAR: u64 = 0xd000_0000;

    /// A device that records what the transport did to it.
    struct StubDevice {
        queues: Vec<GuestQueue>,
        acked_features: u64,
        notified: Vec<u16>,
        resets: usize,
        config: Vec<u8>,
    }

    impl StubDevice {
        fn new() -> Self {
            Self {
                queues: vec![GuestQueue::new(64), GuestQueue::new(64)],
                acked_features: 0,
                notified: Vec::new(),
                resets: 0,
                config: vec![0xaa, 0xbb, 0xcc, 0xdd],
            }
        }
    }

    impl VirtioMmioDevice for StubDevice {
        fn device_id(&self) -> u32 {
            19 // vsock, so the PCI device id should come out as 0x1053
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
            Ok(true)
        }

        fn reset(&mut self) {
            self.resets += 1;
            self.acked_features = 0;
            for queue in &mut self.queues {
                queue.reset();
            }
        }
    }

    fn transport() -> (VirtioPciTransport, Arc<Mutex<StubDevice>>) {
        let device = Arc::new(Mutex::new(StubDevice::new()));
        let memory = Arc::new(GuestMemory::new(0x1000).expect("guest memory"));
        let transport = VirtioPciTransport::new("virtio-pci-test", BAR, memory, device.clone());
        (transport, device)
    }

    async fn read_u32(t: &VirtioPciTransport, offset: u64) -> u32 {
        let mut buf = [0u8; 4];
        t.read(offset, &mut buf).await.unwrap();
        u32::from_le_bytes(buf)
    }

    async fn write_u32(t: &mut VirtioPciTransport, offset: u64, value: u32) {
        t.write(offset, &value.to_le_bytes()).await.unwrap();
    }

    async fn write_u16(t: &mut VirtioPciTransport, offset: u64, value: u16) {
        t.write(offset, &value.to_le_bytes()).await.unwrap();
    }

    async fn write_u8(t: &mut VirtioPciTransport, offset: u64, value: u8) {
        t.write(offset, &[value]).await.unwrap();
    }

    #[tokio::test]
    async fn the_pci_identity_is_the_modern_one_for_the_device_behind_it() {
        let (t, _) = transport();
        let config = t.config_space();

        assert_eq!(config.vendor_id().0, 0x1AF4);
        // 0x1040 + 19. A transitional id here would let a pre-1.0 driver bind
        // and then find none of the registers it expects.
        assert_eq!(config.device_id().0, 0x1053);
        assert_eq!(config.subsystem_vendor_id().0, 0x1AF4);
    }

    #[tokio::test]
    async fn the_capability_chain_walks_to_all_four_structures() {
        let (t, _) = transport();
        let config = t.config_space();

        // A driver only walks the chain if this bit says there is one.
        assert_ne!(config.read_u16(0x06) & 0x0010, 0, "capabilities-list bit");

        let mut found = Vec::new();
        let mut at = u16::from(config.read_u8(0x34));
        // Bounded: a chain that loops would otherwise hang the test rather
        // than fail it, which is how a guest experiences the same bug.
        for _ in 0..8 {
            if at == 0 {
                break;
            }
            assert_eq!(
                config.read_u8(at),
                cap::VENDOR_SPECIFIC,
                "capability at {at:#x} is not vendor-specific"
            );
            let cfg_type = config.read_u8(at + 3);
            let bar = config.read_u8(at + 4);
            let offset = config.read_u32(at + 8);
            let length = config.read_u32(at + 12);
            found.push((cfg_type, bar, offset, length));
            at = u16::from(config.read_u8(at + 1));
        }

        assert_eq!(
            found,
            vec![
                (cap::COMMON_CFG, 0, 0x0000, 0x38),
                (cap::NOTIFY_CFG, 0, 0x2000, 0x1000),
                (cap::ISR_CFG, 0, 0x1000, 0x1),
                (cap::DEVICE_CFG, 0, 0x3000, 0x1000),
            ],
            "the chain must describe the window this transport actually decodes"
        );
    }

    #[tokio::test]
    async fn bar0_points_at_the_window_as_a_non_prefetchable_64_bit_memory_bar() {
        let (t, _) = transport();
        let config = t.config_space();

        let low = config.read_u32(0x10);
        let high = config.read_u32(0x14);
        assert_eq!(u64::from(low & 0xFFFF_FFF0) | (u64::from(high) << 32), BAR);
        assert_eq!(low & 1, 0, "memory space, not I/O");
        assert_eq!((low >> 1) & 0b11, 0b10, "64-bit");
        // Prefetchable would let a bridge read ahead, and reading the ISR
        // clears it -- a speculative read would clear it for nobody.
        assert_eq!((low >> 3) & 1, 0, "must not be prefetchable");
    }

    #[tokio::test]
    async fn the_notify_capability_gives_each_queue_its_own_address() {
        let (t, _) = transport();
        let config = t.config_space();

        // Walk to the notify capability and read the multiplier that follows
        // the common fields.
        let mut at = u16::from(config.read_u8(0x34));
        while at != 0 && config.read_u8(at + 3) != cap::NOTIFY_CFG {
            at = u16::from(config.read_u8(at + 1));
        }
        assert_ne!(at, 0, "no notify capability");
        assert_eq!(config.read_u32(at + 16), NOTIFY_OFF_MULTIPLIER);
        assert_ne!(
            NOTIFY_OFF_MULTIPLIER, 0,
            "a zero multiplier collapses every queue onto one address"
        );
    }

    /// The sequence virtio 1.2 3.1.1 prescribes, in order.
    #[tokio::test]
    async fn a_driver_can_bring_the_device_up_the_way_the_spec_says() {
        let (mut t, device) = transport();

        // 1. Reset, then acknowledge.
        write_u8(&mut t, common::DEVICE_STATUS, 0).await;
        write_u8(&mut t, common::DEVICE_STATUS, status::ACKNOWLEDGE).await;
        write_u8(
            &mut t,
            common::DEVICE_STATUS,
            status::ACKNOWLEDGE | status::DRIVER,
        )
        .await;

        // 2. Read the features, both halves.
        write_u32(&mut t, common::DEVICE_FEATURE_SELECT, 0).await;
        let low = read_u32(&t, common::DEVICE_FEATURE).await;
        write_u32(&mut t, common::DEVICE_FEATURE_SELECT, 1).await;
        let high = read_u32(&t, common::DEVICE_FEATURE).await;
        let offered = u64::from(low) | (u64::from(high) << 32);
        assert_eq!(offered, VIRTIO_F_VERSION_1 | 0b1010);
        assert_ne!(
            high, 0,
            "VIRTIO_F_VERSION_1 is bit 32; a device reporting only the low half \
             tells the driver it is a legacy device"
        );

        // 3. Accept them, both halves.
        write_u32(&mut t, common::DRIVER_FEATURE_SELECT, 0).await;
        write_u32(&mut t, common::DRIVER_FEATURE, low).await;
        write_u32(&mut t, common::DRIVER_FEATURE_SELECT, 1).await;
        write_u32(&mut t, common::DRIVER_FEATURE, high).await;
        assert_eq!(device.lock().acked_features, offered);

        // 4. FEATURES_OK, and read it back to confirm the device kept it.
        write_u8(
            &mut t,
            common::DEVICE_STATUS,
            status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK,
        )
        .await;
        let mut status_byte = [0u8; 1];
        t.read(common::DEVICE_STATUS, &mut status_byte)
            .await
            .unwrap();
        assert_ne!(status_byte[0] & status::FEATURES_OK, 0);

        // 5. Set up queue 1.
        write_u16(&mut t, common::QUEUE_SELECT, 1).await;
        write_u16(&mut t, common::QUEUE_SIZE, 32).await;
        t.write(common::QUEUE_DESC, &0x1000u64.to_le_bytes())
            .await
            .unwrap();
        t.write(common::QUEUE_DRIVER, &0x2000u64.to_le_bytes())
            .await
            .unwrap();
        t.write(common::QUEUE_DEVICE, &0x3000u64.to_le_bytes())
            .await
            .unwrap();
        write_u16(&mut t, common::QUEUE_ENABLE, 1).await;

        {
            let mut device = device.lock();
            let queue = &device.queues()[1];
            assert_eq!(queue.size(), 32);
            assert_eq!(queue.desc_addr(), 0x1000);
            assert_eq!(queue.avail_addr(), 0x2000);
            assert_eq!(queue.used_addr(), 0x3000);
            assert!(queue.is_ready());
        }

        // 6. DRIVER_OK.
        write_u8(
            &mut t,
            common::DEVICE_STATUS,
            status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK | status::DRIVER_OK,
        )
        .await;
        assert!(t.is_driver_ok());
    }

    #[tokio::test]
    async fn num_queues_reports_what_the_device_has() {
        let (t, _) = transport();
        let mut buf = [0u8; 2];
        t.read(common::NUM_QUEUES, &mut buf).await.unwrap();
        assert_eq!(u16::from_le_bytes(buf), 2);
    }

    /// A 64-bit queue address written as two dwords, which is what a 32-bit
    /// guest does and what a 64-bit one often does anyway.
    #[tokio::test]
    async fn a_queue_address_can_be_written_as_two_halves() {
        let (mut t, device) = transport();
        write_u16(&mut t, common::QUEUE_SELECT, 0).await;

        write_u32(&mut t, common::QUEUE_DESC, 0x8000_1000).await;
        write_u32(&mut t, common::QUEUE_DESC + 4, 0x7).await;

        assert_eq!(device.lock().queues()[0].desc_addr(), 0x7_8000_1000);
    }

    #[tokio::test]
    async fn the_queue_a_write_notifies_comes_from_its_address() {
        let (mut t, device) = transport();

        // Queue 1 notifies at notify_base + 1 * multiplier.
        t.write(
            window::NOTIFY + u64::from(NOTIFY_OFF_MULTIPLIER),
            &0u16.to_le_bytes(),
        )
        .await
        .unwrap();

        assert_eq!(
            device.lock().notified,
            vec![1],
            "the queue index is the address, not the value written"
        );
    }

    #[tokio::test]
    async fn the_isr_reports_a_used_queue_and_clears_when_read() {
        let (mut t, _) = transport();

        t.write(window::NOTIFY, &0u16.to_le_bytes()).await.unwrap();

        let mut isr = [0u8; 1];
        t.read(window::ISR, &mut isr).await.unwrap();
        assert_eq!(isr[0] & isr::QUEUE, isr::QUEUE, "queue interrupt not set");

        // Reading is the acknowledgement. A second read must find nothing, or
        // the handler runs forever.
        t.read(window::ISR, &mut isr).await.unwrap();
        assert_eq!(isr[0], 0, "the ISR did not clear on read");
    }

    #[tokio::test]
    async fn device_specific_config_reaches_the_device() {
        let (mut t, device) = transport();

        let mut buf = [0u8; 4];
        t.read(window::DEVICE, &mut buf).await.unwrap();
        assert_eq!(buf, [0xaa, 0xbb, 0xcc, 0xdd]);

        t.write(window::DEVICE + 1, &[0x11]).await.unwrap();
        assert_eq!(device.lock().config[1], 0x11);
    }

    #[tokio::test]
    async fn writing_zero_to_status_resets_the_device() {
        let (mut t, device) = transport();

        write_u32(&mut t, common::DRIVER_FEATURE, 0b10).await;
        write_u8(&mut t, common::DEVICE_STATUS, status::DRIVER_OK).await;
        write_u16(&mut t, common::QUEUE_SELECT, 0).await;
        write_u16(&mut t, common::QUEUE_ENABLE, 1).await;

        write_u8(&mut t, common::DEVICE_STATUS, 0).await;

        assert_eq!(t.status(), 0);
        assert!(!t.is_driver_ok());
        let mut device = device.lock();
        assert_eq!(device.resets, 1);
        assert_eq!(device.acked_features, 0);
        assert!(
            !device.queues()[0].is_ready(),
            "a reset must not leave the previous driver queues enabled"
        );
    }

    /// No MSI-X capability is offered, so the vector registers must read as
    /// "no vector" rather than echoing what a driver wrote. A driver that
    /// reads back its own vector concludes MSI-X works and then waits for
    /// interrupts that never come.
    #[tokio::test]
    async fn the_msix_vector_registers_report_no_vector() {
        let (mut t, _) = transport();

        write_u16(&mut t, common::QUEUE_MSIX_VECTOR, 0).await;
        let mut buf = [0u8; 2];
        t.read(common::QUEUE_MSIX_VECTOR, &mut buf).await.unwrap();
        assert_eq!(u16::from_le_bytes(buf), 0xFFFF);

        write_u16(&mut t, common::CONFIG_MSIX_VECTOR, 0).await;
        t.read(common::CONFIG_MSIX_VECTOR, &mut buf).await.unwrap();
        assert_eq!(u16::from_le_bytes(buf), 0xFFFF);
    }

    #[tokio::test]
    async fn each_queue_notifies_at_its_own_offset() {
        let (mut t, _) = transport();

        for queue in 0..2u16 {
            write_u16(&mut t, common::QUEUE_SELECT, queue).await;
            let mut buf = [0u8; 2];
            t.read(common::QUEUE_NOTIFY_OFF, &mut buf).await.unwrap();
            assert_eq!(
                u16::from_le_bytes(buf),
                queue,
                "queue {queue} shares a notification address with another"
            );
        }
    }
}
