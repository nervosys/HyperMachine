//! IOMMU Support
//!
//! This module provides IOMMU (Input/Output Memory Management Unit) support
//! for both Intel VT-d and AMD-Vi platforms, enabling DMA remapping and
//! interrupt remapping for device passthrough.

mod amd;
mod interrupt_remap;
mod types;
mod vtd;

pub use amd::{
    control as amd_control, registers as amd_registers, status as amd_status, AmdIommu,
    AmdIotlbEntry, CommandEntry, CommandType, DeviceTableEntry, EventLogEntry, EventType,
};

pub use interrupt_remap::{
    AmdInterruptRemapTable, AmdIrte, DeliveryMode, IntelInterruptRemapTable, IntelIrte,
    InterruptRemapStats, InterruptRemapStatsSnapshot, InterruptType, MsiMessage,
    PostedInterruptDescriptor, SourceValidation,
};

pub use types::{
    AddressWidth, DeviceId, DeviceScope, DeviceScopeType, DomainId, FaultReason, FaultRecord,
    IommuStats, IommuStatsSnapshot, PageTableEntry, PageTableFlags, TranslationType,
    PAGE_SIZE_1G, PAGE_SIZE_2M, PAGE_SIZE_4K,
};

pub use vtd::{
    cap, ecap, gcmd, gsts, registers as vtd_registers, ContextEntry, Iotlb, IotlbEntry, RootEntry,
    VtdUnit,
};
