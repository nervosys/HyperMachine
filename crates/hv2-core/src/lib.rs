//! HyperMachine Core - Foundation for high-performance Type 2 hypervisor
//!
//! This crate provides the core abstractions and engine for HyperMachine,
//! a Type 2 hypervisor designed for AI agent scriptability and remote control.

#![allow(dead_code)]
// unused_imports and unused_mut allows removed in Phase 52; fix at source.
#![allow(unused_variables)]
// FFI interop requires non-Rust naming conventions
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(clashing_extern_declarations)]
#![allow(private_interfaces)]
#![allow(unused_parens)]
// Clippy: hypervisor core has complex patterns that are intentional
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::transmute_undefined_repr)]
#![allow(clippy::missing_transmute_annotations)]
#![allow(clippy::identity_op)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::single_match)]
#![allow(clippy::let_and_return)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::useless_format)]
#![allow(clippy::map_entry)]
#![allow(clippy::manual_clamp)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::vec_init_then_push)]
#![allow(clippy::wildcard_in_or_patterns)]
#![allow(clippy::nonminimal_bool)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::readonly_write_lock)]
#![allow(clippy::should_implement_trait)]
#![allow(clippy::inherent_to_string)]
#![allow(clippy::await_holding_lock)]
#![allow(clippy::implicit_saturating_sub)]
#![allow(clippy::manual_map)]
#![allow(clippy::unnecessary_min_or_max)]
#![allow(clippy::manual_is_multiple_of)]

pub mod acpi;
pub mod address_space;
pub mod audio;
pub mod backends;
pub mod boot;
pub mod config;
pub mod container;
pub mod cpuid;
pub mod crypto;
pub mod debug;
pub mod descriptors;
pub mod device;
pub mod device_manager;
pub mod devices;
pub mod error;
pub mod events;
pub mod exit;
pub mod exit_handler;
pub mod gpu;
pub mod hypervisor;
pub mod input;
pub mod interrupt;
pub mod iommu;
pub mod memory;
pub mod migration;
pub mod mmio;
pub mod nested;
pub mod networking;
pub mod numa;
pub mod pci;
pub mod perf;
pub mod platform;
pub mod power;
pub mod security;
pub mod snapshot;
pub mod telemetry;
pub mod uefi;
pub mod usb;
pub mod vcpu;
pub mod vm;

pub use acpi::{
    AcpiTableBuilder, AcpiTables, Dsdt, Fadt, Madt, MadtEntryType, MadtInterruptOverride,
    MadtIoApic, MadtLocalApic, MadtLocalApicNmi, Rsdp, Rsdt, Xsdt, ACPI_TABLE_BASE, RSDP_ADDRESS,
};
pub use address_space::{
    AddressRegion, AddressSpaceBuilder, GuestAddressSpace, GuestPhysAddr, HostVirtAddr,
    MemoryFlags, PageInfo, RegionType, EXTENDED_MEMORY_START, HUGE_PAGE_SIZE, IO_APIC_BASE,
    LARGE_PAGE_SIZE, LOCAL_APIC_BASE, LOW_MEMORY_END, PAGE_SIZE, ROM_END, ROM_START,
    VGA_MEMORY_END, VGA_MEMORY_START,
};
pub use audio::{
    Ac97Controller, Ac97Mixer, Ac97Register, AudioBuffer, AudioMixer, AudioParams, AudioStats,
    AudioStatsSnapshot, AudioStream, ChannelLayout, ChannelMap, HdaCodec, HdaController, Jack,
    JackType, PcmParams, PcmState, PcmStream, PcmStreamInfo, PinConfig, SampleFormat, SampleRate,
    StereoVolume, StreamDescriptor, StreamDirection, StreamState, VirtioSndDirection,
    VirtioSndPcmFormat, VirtioSndPcmRate, VirtioSndStatus, VirtioSoundDevice, Volume, Widget,
    WidgetType,
};
pub use config::Config;
pub use container::{
    Cgroup, CgroupManager, CgroupManagerStats, CgroupVersion, Container, ContainerCpuConfig,
    ContainerMemoryConfig, ContainerProcess, ContainerRuntime, ContainerSpec, ContainerState,
    Controller, CpuController, CpusetController, DeviceRule, DevicesController, FirewallAction,
    FirewallChain, FirewallRule, FreezerState, IdMap, IdMapping, InterfaceType, IoController,
    IpAddress, IpcNamespace, LinuxConfig, MemoryController, MntNamespace, Mount, MountFlags,
    MountOption, MountPoint, MountType, NamespaceConfig, NamespaceManager, NamespaceStats,
    NamespaceType, NetInterface, NetNamespace, NsHandle, NsId, NsType, PidNamespace,
    PidsController, Protocol, ResourceConfig, RootFs, Route, RuntimeError, RuntimeResult,
    RuntimeStats, SeccompAction, SeccompArg, SeccompConfig, SeccompOp, SeccompSyscall,
    UserNamespace, UtsNamespace,
};
pub use cpuid::{CpuidConfig, CpuidEmulator, CpuidResult};
pub use debug::{
    Breakpoint, BreakpointType, CpuInspector, CpuMode, CpuState as DebugCpuState, DebugManager,
    DebugStats, GateType, GdbError, GdbRegister, GdbRegisters, GdbResult, GdbStats, GdbStub,
    GdbTarget, IdtInspector, InterruptDescriptor, IntrospectionEvent, MemoryAttributes,
    MemoryInspector, MemoryRegion as DebugMemoryRegion, MemoryRegionType, PacketParser,
    PacketState, PageTableEntry, PageTableWalker, PageWalkError, PageWalkResult, SegmentState,
    StopReason, TableState,
};
pub use descriptors::{
    DescriptorTablePointer, GdtBuilder, SegmentDescriptor, DESC_64BIT, DESC_DPL_0, DESC_DPL_3,
    DESC_PRESENT,
};
pub use device::{Device, DeviceManager, DeviceType};
pub use device_manager::DeviceManager as StandardDeviceManager;
pub use device_manager::SerialPort;
pub use devices::{SerialDevice, TimerDevice};
pub use error::{Error, Result};
pub use events::{EventBus, VmEvent, VmEventType};
pub use exit::{IoDirection, VmExit};
pub use exit_handler::{
    ExitContext, ExitHandlerResult, InterruptState, StandardExitHandler, VmExitHandler,
};
pub use gpu::{
    Color, CursorShape, CursorState, DisplayInfo, DisplayMode, DisplaySurface, DoubleBuffer,
    Framebuffer, GpuResource, GpuStats, PixelFormat, Rect, Scanout, ScanoutState, VirtioGpu,
    VirtioGpuCtrlType, VirtioGpuError, VirtioGpuFormat, VirtioGpuStats,
};
pub use hypervisor::{HypervisorBackend, HypervisorCapabilities, HypervisorPlatform, HypervisorVm};
pub use input::{
    Axis, Button, ControllerEvent, ControllerState, ControllerStats, ControllerType,
    DeadzoneConfig, GameController, Gesture, GestureType, KeyCode, KeyboardStats, LedState,
    MouseButtons as Ps2MouseButtons, MouseCommand, MouseProtocol, MouseResolution, MouseStats,
    Ps2Command, Ps2Keyboard, Ps2Mouse, RumbleEffect, SampleRate as MouseSampleRate, ScalingMode,
    ScanCodeSet, TouchConfig, TouchEvent, TouchPoint, TouchState, TouchStats, Touchscreen,
    TypematicConfig,
};
pub use interrupt::Pic8259;
pub use iommu::{
    amd_control, amd_registers, amd_status, cap, ecap, gcmd, gsts, vtd_registers, AddressWidth,
    AmdInterruptRemapTable, AmdIommu, AmdIotlbEntry, AmdIrte, CommandEntry, CommandType,
    ContextEntry, DeliveryMode, DeviceId as IommuDeviceId, DeviceScope, DeviceScopeType,
    DeviceTableEntry, DomainId, EventLogEntry, EventType, FaultReason, FaultRecord,
    IntelInterruptRemapTable, IntelIrte, InterruptRemapStats, InterruptRemapStatsSnapshot,
    InterruptType, IommuStats, IommuStatsSnapshot, Iotlb, IotlbEntry, MsiMessage,
    PageTableEntry as IommuPageTableEntry, PageTableFlags as IommuPageTableFlags,
    PostedInterruptDescriptor, RootEntry, SourceValidation, TranslationType, VtdUnit, PAGE_SIZE_1G,
    PAGE_SIZE_2M, PAGE_SIZE_4K,
};
pub use memory::{GuestMemory, MemoryMapping, MemoryRegion};
pub use migration::{
    crc32, shared_dirty_tracker, CpuState as MigrationCpuState, DescriptorTable, DeviceState,
    DirtyBitmap, DirtyStats, DirtyTracker, MemoryRegionState, Migratable, MigrationConfig,
    MigrationController, MigrationMessage, MigrationRole, MigrationStage, MigrationStats,
    MigrationStream, PageData, PreCopyMigration, SectionHeader, SectionType, SegmentRegister,
    SerializeError, SerializeResult, SharedDirtyTracker, StateDeserializer, StateSerializer,
    VmState as MigrationVmState, FORMAT_VERSION, PAGE_SIZE as MIGRATION_PAGE_SIZE, STATE_MAGIC,
};
pub use mmio::{MmioManager, MmioRegion};
pub use networking::{
    ConnState, ConnTrackEntry, ConnTracker, DeviceAssignment, EthernetFrame, FilterAction,
    FilterChain, FilterRule, IommuGroup, IpMatch, IpProtocol, MacAddress, MacEntry, MacTable,
    NetworkFilter, PciAddress as NetworkPciAddress, PciClass, PhysicalFunction, Port, PortMatch,
    PortState, PortStats, PortType, ProtocolMatch, SriovCapability, SriovError, SriovManager,
    StateMatch, StpState, SwitchStats, VfLinkState, VfState, VirtualFunction, VirtualSwitch,
    VlanId, VlanMode, VlanSet,
};
pub use numa::{
    AcpiHeader, AllocError, AllocResult, Allocation, AllocationPolicy, CpuAffinity, DistanceMatrix,
    FreeRegion, InterleavingMode, MemoryAffinity, MemoryAffinityFlags, MemoryAffinityStructure,
    MemoryRange, NodeId, NodeMemoryPool, NodePoolStats, NodeStats, NumaAcpiTables, NumaAllocator,
    NumaDistance, NumaNode, NumaNodeConfig, NumaTopology, NumaTopologyBuilder,
    ProcessorAffinityFlags, ProcessorLocalApicAffinity, ProcessorX2ApicAffinity, SlitBuilder,
    SratBuilder, SratSubtableType, NUMA_DISTANCE_FAR, NUMA_DISTANCE_LOCAL, NUMA_DISTANCE_REMOTE,
    NUMA_DISTANCE_UNREACHABLE, NUMA_DISTANCE_VERY_FAR, SLIT_REVISION, SLIT_SIGNATURE,
    SRAT_REVISION, SRAT_SIGNATURE,
};
pub use pci::{
    bridge_control, registers, BarConfig, BarType, BridgeConfigSpace, BridgeForwarder, BusStats,
    BusStatsSnapshot, CapabilityHeader, CapabilityId, CapabilityStats, ClassCode, CommandRegister,
    ConfigSpace, ConfigStats, DeviceId, ExtendedCapabilityId, HeaderType, InterruptPin,
    MsiCapability, MsiControl, MsixCapability, MsixControl, MsixTableEntry, PciAddress, PciBus,
    PciDeviceInfo, PciDeviceSlot, PciRootComplex, PcieCapability, PcieDeviceType, PcieLinkSpeed,
    PcieLinkStatus, PcieLinkWidth, PmCapability, PmControl, PowerState, StatusRegister, VendorId,
    MAX_BUSES, MAX_DEVICES, MAX_FUNCTIONS, PCIE_CONFIG_SIZE, PCI_CONFIG_SIZE,
};
pub use perf::{
    CoalesceConfig, CoalesceStats, CoalescedInterrupt, ExitStats, ExitStatsSummary, ExitType,
    ExitTypeStat, InterruptCoalescer, IoBatchConfig, IoBatchStats, IoBatcher, PerfCounter,
    PerfCounterSummary, TimerGuard,
};
pub use platform::{
    CpuVendor, HyperVEnlightenments, HyperVPrivileges, PlatformFeatures, PlatformInfo,
    PlatformMemoryFlags, PlatformMemoryRegion, PlatformStats, PlatformStatsSnapshot,
    PlatformVmBuilder, PlatformVmConfig,
};
pub use power::{
    BatteryEventType, CState, CStateError, CStateGovernor, CStateManager, CStateResult, CpuCState,
    CpuPState, DState, PState, PStateError, PStateGovernor, PStateManager, PStateResult,
    PowerEvent, PowerStats, SState, SStateError, SStateManager, SStateResult, ThermalEventType,
    ThermalTripType, TransitionPhase, WakeEvent, WakeSource, WakeSourceConfig,
};
pub use security::{
    BootComponent, BootComponentType, CbitPosition, Certificate, CertificateStatus,
    CertificateType, EncryptionConfig, EncryptionError, EncryptionManager, EncryptionResult,
    EncryptionStats, EncryptionTechnology, HashAlgorithm, KeyHandle, KeyId, KeyMetadata, KeyState,
    KeyType, NvEntry, NvIndex, PageEncryptionState, PcrBank, SecureBootError, SecureBootManager,
    SecureBootMode, SecureBootPolicy, SecureBootResult, SecureBootStats, SevContext,
    SevLaunchState, Signature, SignatureAlgorithm, TdxContext, TpmCommandCode, TpmKey,
    TpmResponseCode, VerificationResult, VirtualTpm,
};
pub use snapshot::{
    CompressionType, CpuSnapshot, CreateSnapshotOptions, DescriptorTableSnapshot, DeviceResult,
    DeviceSnapshot, DeviceStateDeserializer, DeviceStateError, DeviceStateManager,
    DeviceStateSerializer, DeviceStateStats, DirtyPageIterator, DirtyPageTracker,
    MemoryRegionSnapshot, MemoryResult, MemorySnapshotConfig, MemorySnapshotError,
    MemorySnapshotManager, MemorySnapshotStats, PageState, RestoreSnapshotOptions, SegmentSnapshot,
    SnapshotConfig, SnapshotError, SnapshotId, SnapshotInfo, SnapshotManager, SnapshotManagerStats,
    SnapshotResult, SnapshotState, SnapshotStats, SnapshotType, Snapshottable, TestDevice,
    VmSnapshot,
};
pub use telemetry::{
    CarbonExporter,
    // Collector types
    CollectorError,
    CollectorResult,
    Counter,
    DiskMetrics,
    Ewma,
    // Exporter types
    ExporterError,
    ExporterResult,
    Gauge,
    Histogram,
    HistogramData,
    HypervisorMetrics,
    JsonExporter,
    JsonMetric,
    MemoryMetrics,
    MetricCollector,
    MetricDescriptor,
    MetricExporter,
    MetricFamily,
    MetricLabel,
    MetricLabels,
    MetricRegistry,
    MetricSample,
    // Core types
    MetricType,
    MetricValue,
    MovingAverage,
    NetworkMetrics,
    OpenTelemetryExporter,
    PrometheusExporter,
    RateCalculator,
    StatsDExporter,
    StatsDFormat,
    SummaryData,
    Timer,
    TimerObservation,
    Timestamp,
    // VM metrics
    VcpuMetrics,
    VmMetrics,
    // Constants
    DEFAULT_HISTOGRAM_BUCKETS,
    DEFAULT_QUANTILES,
    LATENCY_BUCKETS,
    SIZE_BUCKETS,
};
pub use uefi::{
    AllocateType, BootServices, BootServicesStats, CapsuleCapabilities, GopBltOperation,
    GopBltPixel, GopMode, GopModeInfo, GopPixelBitmask, GopPixelFormat, GopStats,
    GraphicsOutputProtocol, Guid, Handle, MemoryAttribute, MemoryDescriptor, MemoryType, ResetType,
    RuntimeServices, RuntimeServicesStats, Status, SystemTable, TableHeader, Time,
    TimeCapabilities, Variable, VariableAttributes, VariableInfo,
};
pub use usb::{
    BaseUsbDevice, CommandRing, ConfigDescriptor, ControlResult, DescriptorType, DeviceClass,
    DeviceDescriptor, DeviceSlot, DeviceState as UsbDeviceState, Endpoint, EndpointDescriptor,
    EndpointDirection, ErstEntry, EventRing, HidDescriptor, HidProtocol, HidStats, HidSubclass,
    InterfaceDescriptor, Interrupter, KeyboardModifiers, MouseButtons, PortRegister,
    PortState as UsbPortState, ReportType, RingSegment, SetupPacket, SlotState, StringDescriptor,
    TransferResult, TransferType, Trb, TrbCompletionCode, TrbType, UsbDevice, UsbKeyboard,
    UsbMouse, UsbSpeed, UsbTablet, XhciController, XhciPort,
};
pub use vcpu::{ControlRegisters, RegisterSet, VCpu, VCpuState};
pub use vm::{VMConfig, VMState, VM};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMMIT_HASH: Option<&str> = option_env!("GIT_COMMIT_HASH");

/// Architecture support
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    X86_64,
    AArch64,
    RiscV64,
}

/// VM capabilities
#[derive(Debug, Clone)]
pub struct Capabilities {
    pub max_vcpus: usize,
    pub max_memory: u64,
    pub gpu_support: bool,
    pub networking: bool,
    pub nested_virtualization: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            max_vcpus: 256,
            max_memory: 1024 * 1024 * 1024 * 1024, // 1TB
            gpu_support: true,
            networking: true,
            nested_virtualization: false,
        }
    }
}
