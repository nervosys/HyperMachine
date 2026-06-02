//! Device emulation

pub mod disk_image;
pub mod display_backend;
pub mod e1000;
pub mod framebuffer;
pub mod ide;
pub mod ioapic;
pub mod keyboard;
pub mod lapic;
pub mod msi;
pub mod net_backend;
pub mod nvme;
pub mod rtc;
pub mod serial;
pub mod timer;
pub mod vga;
pub mod virtio;
pub mod virtio_blk;

#[cfg(test)]
mod integration_tests;

pub use disk_image::{
    DiskImage, ImageFormat, InMemoryImage, Qcow2Header, RawImage, VhdxFileHeader, QCOW2_MAGIC,
    SECTOR_SIZE as DISK_SECTOR_SIZE, VHDX_SIGNATURE,
};
pub use display_backend::{
    CallbackDisplayBackend, DisplayBackend, DisplayError, DisplayManager, DisplayStats,
    MemoryDisplayBackend, NullDisplayBackend, SharedDisplayBackend,
};
pub use e1000::{
    Eeprom, RxDescriptor, SharedE1000, TxDescriptor, E1000, E1000_DEVICE_ID, E1000_REG_SIZE,
    E1000_VENDOR_ID,
};
pub use framebuffer::{
    Color, Framebuffer, FramebufferConfig, PixelFormat, Rect, SharedFramebuffer,
};
pub use ide::{
    IdeController, IDE_PRIMARY_BASE, IDE_PRIMARY_CTRL, IDE_SECONDARY_BASE, IDE_SECONDARY_CTRL,
    SECTOR_SIZE,
};
pub use ioapic::{
    DeliveryMode, DestinationMode, IoApic, Polarity, RedirectionEntry, TriggerMode, IOAPIC_BASE,
    IOAPIC_NUM_PINS, IOAPIC_SIZE,
};
pub use keyboard::KeyboardDevice;
pub use lapic::{
    IcrDeliveryMode, IcrDestShorthand, LocalApic, TimerMode as LapicTimerMode, LAPIC_BASE,
    LAPIC_SIZE,
};
pub use msi::{
    MsiCapability, MsiController, MsiDeliveryMode, MsiDestMode, MsiMessage, MsixCapability,
    MsixTableEntry, MSIX_MAX_VECTORS, MSI_ADDR_BASE, MSI_MAX_VECTORS,
};
pub use net_backend::{
    LoopbackBackend, NetworkBackend, NetworkStats, NullBackend, SharedNetworkBackend, TapBackend,
    TapConfig, UserBackend, MAX_FRAME_SIZE, MTU,
};
pub use nvme::{
    CompletionQueueEntry, NvmeController, NvmeQueue, SubmissionQueueEntry, NVME_BLOCK_SIZE,
    NVME_MAX_IO_QUEUES, NVME_MAX_QUEUE_ENTRIES, NVME_SECTOR_SIZE,
};
pub use rtc::RtcDevice;
pub use serial::SerialDevice;
pub use timer::TimerDevice;
pub use vga::{VgaAttribute, VgaColor, VgaDevice, VGA_HEIGHT, VGA_SIZE, VGA_TEXT_BASE, VGA_WIDTH};
pub use virtio::{VirtQueue, VirtioDevice, VirtioNet, VIRTIO_NET_F_MAC, VIRTIO_NET_F_STATUS};
pub use virtio_blk::{
    BlockConfig, BlockRequestHeader, RequestType, VirtioBlock, VIRTIO_BLK_DEVICE_ID,
    VIRTIO_BLK_SECTOR_SIZE,
};
// virtio_gpu lives in `crate::gpu` (the crate-root `VirtioGpu`); the former
// parallel `devices::virtio_gpu` implementation was removed as an unused
// duplicate.
