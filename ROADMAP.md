# HyperMachine Development Roadmap

## Overview

HyperMachine is a high-performance hypervisor framework written in Rust supporting both Type 2 (hosted) and Type 1 (bare-metal) modes. Currently targeting x86-64 architecture with comprehensive device emulation and interrupt handling.

---

## Completed Phases

### ✅ Phase 1: Timer Integration (IRQ 0)
**Status:** Complete  
**Tests:** 4 tests passing

- Implemented PIT (Programmable Interval Timer) device emulation
- Timer tick generation with configurable frequency
- IRQ 0 interrupt injection through PIC 8259
- Integration with DeviceManager

### ✅ Phase 2: Keyboard Integration (IRQ 1)
**Status:** Complete  
**Tests:** 4 tests passing

- PS/2 keyboard controller emulation
- Scancode generation for key press/release events
- IRQ 1 interrupt delivery
- Keyboard buffer management

### ✅ Phase 3: Serial Port Integration (IRQ 3/4)
**Status:** Complete  
**Tests:** 6 tests passing

- COM1 (IRQ 4) and COM2 (IRQ 3) serial port emulation
- UART 16550 register interface (THR, RBR, IER, IIR, LCR, MCR, LSR, MSR)
- Transmit and receive interrupt support
- Proper IER (Interrupt Enable Register) management

### ✅ Phase 4: VGA Text Mode Improvements
**Status:** Complete  
**Tests:** 10 tests passing

- 80x25 text mode buffer
- 16-color foreground/background support
- Cursor positioning and visibility
- Screen scrolling and clearing
- Character attribute handling

### ✅ Phase 5: Unified Device Manager
**Status:** Complete  
**Tests:** 7 tests passing

- Centralized device coordination through `DeviceManager`
- Integrated PIC 8259 interrupt controller (master + slave cascade)
- Unified I/O port routing to devices
- Device initialization and lifecycle management
- Serial interrupt enable helper methods

### ✅ Phase 6: End-to-End Integration Tests
**Status:** Complete  
**Tests:** 12 tests passing

- Complete interrupt flow verification: Device → PIC → Acknowledge → EOI
- Timer tick integration tests
- Keyboard scancode interrupt tests
- Serial I/O interrupt tests (COM1/COM2)
- Multi-device interrupt sequencing
- VGA display integration

**Total Tests:** 202 lib tests in hv2-core

---

## Current Architecture

```
┌─────────────────────────────────────────────────────┐
│                   DeviceManager                      │
├─────────────────────────────────────────────────────┤
│  ┌─────────┐  ┌──────────┐  ┌────────┐  ┌────────┐ │
│  │  Timer  │  │ Keyboard │  │ Serial │  │  VGA   │ │
│  │  IRQ 0  │  │  IRQ 1   │  │ IRQ3/4 │  │        │ │
│  └────┬────┘  └────┬─────┘  └───┬────┘  └────────┘ │
│       │            │            │                   │
│       └────────────┼────────────┘                   │
│                    ▼                                │
│            ┌──────────────┐                         │
│            │   PIC 8259   │                         │
│            │ Master+Slave │                         │
│            └──────────────┘                         │
└─────────────────────────────────────────────────────┘
```

### Interrupt Flow
1. Device raises IRQ → IRR (Interrupt Request Register) bit set
2. `get_pending_interrupt()` returns highest priority unmasked interrupt
3. `acknowledge_interrupt()` moves IRR → ISR (In-Service Register)
4. Guest handles interrupt
5. `send_eoi()` clears ISR bit, enabling next interrupt

### IRQ Assignments

| IRQ  | Vector    | Device              |
| ---- | --------- | ------------------- |
| 0    | 0x20      | Timer (PIT)         |
| 1    | 0x21      | Keyboard            |
| 2    | 0x22      | Cascade (Slave PIC) |
| 3    | 0x23      | COM2 Serial         |
| 4    | 0x24      | COM1 Serial         |
| 8-15 | 0x28-0x2F | Slave PIC IRQs      |

---

## Completed Phases (continued)

### ✅ Phase 7: Memory Management
**Status:** Complete  
**Tests:** 15 tests passing

- **GuestAddressSpace** - Comprehensive guest physical address space management
- **MemoryFlags** - Read/write/execute permission bits with dirty/accessed tracking
- **AddressRegion** - RAM, ROM, Video, and MMIO region types
- **AddressSpaceBuilder** - Standard PC memory layout (low memory, VGA, ROM, extended)
- **Address Translation** - GPA (Guest Physical Address) to HVA (Host Virtual Address)
- **Page Tracking** - Dirty page logging for live migration support
- **MMIO Detection** - Automatic MMIO vs RAM classification
- **Memory Protection** - ROM regions non-writable, MMIO requires device handlers

**Memory Layout Constants:**
| Region     | Address Range     | Size     |
| ---------- | ----------------- | -------- |
| Low Memory | 0x00000 - 0x9FFFF | 640 KB   |
| VGA Memory | 0xA0000 - 0xBFFFF | 128 KB   |
| ROM/BIOS   | 0xC0000 - 0xFFFFF | 256 KB   |
| Extended   | 0x100000+         | Variable |
| Local APIC | 0xFEE00000        | 4 KB     |
| I/O APIC   | 0xFEC00000        | 4 KB     |

### ✅ Phase 8: vCPU Management
**Status:** Complete  
**Tests:** 25 tests passing

- **CpuidEmulator** - Full CPUID instruction emulation with standard and extended leaves
- **CpuidConfig** - Configurable processor features, vendor ID, and brand string
- **VmExitHandler trait** - Extensible exit handling framework
- **StandardExitHandler** - Default handlers for I/O, MMIO, HLT, exceptions, CPUID
- **InterruptState** - Interrupt window tracking and injection state machine
- **ExitHandlerResult** - Rich return types for exit processing decisions

**CPUID Leaves Supported:**

| Leaf         | Description                                               |
| ------------ | --------------------------------------------------------- |
| 0x00         | Maximum leaf + Vendor ID                                  |
| 0x01         | Processor info + Features (SSE, SSE2, SSE3, POPCNT, etc.) |
| 0x04         | Cache parameters (L1D, L1I, L2)                           |
| 0x07         | Extended features                                         |
| 0x0D         | XSAVE features                                            |
| 0x40000000-1 | Hypervisor identification ("HyperMachne")                 |
| 0x80000000-8 | Extended info, brand string, address sizes                |

---

### ✅ Phase 9: Advanced Device Emulation
**Status:** Complete  
**Tests:** 46 tests passing

- **ACPI Tables** - Complete ACPI table generation for modern OS support
  - RSDP (Root System Description Pointer)
  - RSDT/XSDT (Root/Extended System Description Tables)
  - FADT (Fixed ACPI Description Table) - Power management
  - MADT (Multiple APIC Description Table) - Interrupt controllers
  - DSDT (Differentiated System Description Table) - AML bytecode
- **IDE Controller** - ATA disk controller with dual channel support
  - Primary/Secondary channels (0x1F0-0x1F7, 0x170-0x177)
  - PIO mode read/write operations
  - IDENTIFY DEVICE command
  - 28-bit and 48-bit LBA addressing
  - Software reset support
- **VirtIO Network** - Paravirtualized network interface
  - VirtQueue implementation with descriptor rings
  - RX/TX queue handling
  - MAC address configuration
  - Link status management
  - Feature negotiation (VIRTIO_NET_F_MAC, VIRTIO_NET_F_STATUS)
- **RTC** - MC146818 Real-Time Clock (Phase 6 enhancement)
  - Time/date registers with BCD/binary modes
  - CMOS RAM (128 bytes)
  - Periodic interrupt support (IRQ 8)
  - NMI disable control

**I/O Port Mapping:**
| Device             | Ports       | IRQ |
| ------------------ | ----------- | --- |
| IDE Primary        | 0x1F0-0x1F7 | 14  |
| IDE Primary Ctrl   | 0x3F6       | -   |
| IDE Secondary      | 0x170-0x177 | 15  |
| IDE Secondary Ctrl | 0x376       | -   |
| RTC Index          | 0x70        | -   |
| RTC Data           | 0x71        | 8   |

---

### ✅ Phase 10: Guest OS Boot
**Status:** Complete  
**Tests:** 42 tests passing

- **CPU Mode Transitions** - Complete mode transition infrastructure
  - CpuMode enum (RealMode, ProtectedMode, ProtectedModePaging, CompatibilityMode, LongMode64)
  - InitialCpuState configuration for each mode
  - CR0/CR4/EFER register flag constants
  - GdtBuilder for programmatic GDT construction
  - Mode validation to ensure register consistency
- **Boot Sector Support** - Traditional BIOS boot infrastructure
  - Boot sector loading at 0x7C00
  - MBR partition table parsing (4 partitions, bootable flag)
  - Partition type identification (Linux, FAT, NTFS, GPT)
  - BIOS Data Area (BDA) initialization
  - Extended BIOS Data Area (EBDA) setup
  - Interrupt Vector Table (IVT) creation
  - BootMemoryMap for coordinated memory setup
- **Descriptor Tables** - GDT/IDT/TSS management
  - GdtEntry64 for 32-bit and 64-bit segment descriptors
  - TssDescriptor64 (16-byte 64-bit TSS descriptor)
  - Tss64 with RSP0 and IST1-7 interrupt stacks
  - IdtEntry64 for interrupt/trap gates
  - IdtBuilder for creating full 256-entry IDTs
  - DescriptorTableRegister (GDTR/IDTR values)
  - StandardGdt64 helper for common layout

**Memory Layout (BIOS Boot):**
| Region      | Address | Size  |
| ----------- | ------- | ----- |
| IVT         | 0x0000  | 1 KB  |
| BDA         | 0x0400  | 256 B |
| Boot Sector | 0x7C00  | 512 B |
| EBDA        | 0x9FC00 | 1 KB  |
| ACPI Tables | 0xE0000 | 64 KB |
| RSDP        | 0xF0000 | 36 B  |

---

### ✅ Phase 11: Performance Optimization
**Status:** Complete  
**Tests:** 16 tests passing

- **Interrupt Coalescing** - Reduce VM exits by batching interrupts
  - `InterruptCoalescer` with configurable time windows
  - Per-IRQ coalescing enable/disable
  - Automatic flush on window expiry or max count
  - Low-latency and high-throughput presets
  - Real-time statistics (coalesced/delivered ratio)
- **I/O Batching** - Group I/O operations to reduce exits
  - `IoBatcher` for port I/O and MMIO operations
  - Configurable batch size and latency limits
  - Automatic flush on size or time threshold
  - Operation type tracking (PortIn/Out, MmioRead/Write)
- **Exit Statistics** - Track and analyze VM exit patterns
  - `ExitStats` with per-exit-type counters
  - Timing information (total, average per exit)
  - Exits-per-second rate calculation
  - Sorted summary by frequency
- **Performance Counters** - General-purpose timing infrastructure
  - `PerfCounter` with min/max/avg tracking
  - `TimerGuard` for automatic scope timing
  - Lock-free atomic operations
  - Reset and summary functionality

**Exit Types Tracked:**
| Type              | Description                  |
| ----------------- | ---------------------------- |
| IoPort            | Port I/O access              |
| Mmio              | Memory-mapped I/O            |
| Hlt               | HLT instruction              |
| Cpuid             | CPUID instruction            |
| Msr               | MSR read/write               |
| InterruptWindow   | Interrupt window request     |
| ExternalInterrupt | External interrupt injection |
| Exception         | CPU exception                |
| EptViolation      | EPT/NPT page fault           |
| PreemptionTimer   | VMX preemption timer         |

---

### ✅ Phase 12: Platform Integration
**Status:** Complete  
**Tests:** 17 tests passing

- **Platform Abstraction** - Cross-platform hypervisor API
  - `PlatformFeatures` for hardware capability detection
  - `PlatformVmBuilder` with fluent API for VM creation
  - `PlatformVmConfig` with presets for Windows/Linux guests
  - `PlatformInfo` for runtime platform detection
  - `CpuVendor` detection (Intel/AMD/ARM)
- **Hyper-V Enlightenments** - Windows guest performance optimizations
  - `HyperVEnlightenments` with synthetic timers, vapic, TLB flush
  - `HyperVPrivileges` for MSR access permissions
  - CPUID feature flags generation (leaves 0x40000003+)
  - Hyper-V MSR definitions (guest OS ID, hypercall, TSC, etc.)
  - Minimal/Full enlightenment presets
- **Memory Region Abstraction** - Platform-independent memory mapping
  - `PlatformMemoryRegion` with guest/host address mapping
  - `PlatformMemoryFlags` for RAM/ROM/dirty-tracking
  - Slot-based memory management
- **Platform Statistics** - Cross-platform VM exit tracking
  - `PlatformStats` with atomic counters
  - Exit type breakdown (I/O, MMIO, interrupts, hypercalls)
  - Snapshot and reset functionality

**Hyper-V MSRs Supported:**
| MSR Address | Name           | Description             |
| ----------- | -------------- | ----------------------- |
| 0x40000000  | GUEST_OS_ID    | Guest OS identification |
| 0x40000001  | HYPERCALL      | Hypercall page address  |
| 0x40000020  | TIME_REF_COUNT | Time reference counter  |
| 0x40000021  | REFERENCE_TSC  | Reference TSC page      |
| 0x40000073  | VP_ASSIST_PAGE | VP assist page address  |
| 0x400000B0  | STIMER0_CONFIG | Synthetic timer config  |

---

## Future Phases

### ✅ Phase 13: Advanced Interrupt Controller *(47 tests)*
- **IOAPIC** - I/O APIC emulation (13 tests)
  - 24 redirection table entries
  - Level/edge triggered interrupts
  - Interrupt routing to LAPIC
  - EOI broadcast handling
- **Local APIC** - Per-CPU LAPIC emulation (16 tests)
  - Timer (one-shot, periodic, TSC-deadline modes)
  - IPI (Inter-Processor Interrupt) support
  - LVT entries (timer, thermal, PMC, LINT0/1, error)
  - TPR/PPR priority management
  - IRR/ISR/TMR 256-bit registers
- **MSI/MSI-X** - Message Signaled Interrupts (18 tests)
  - MSI capability for PCI devices
  - MSI-X table and PBA support
  - Per-vector masking
  - Up to 2048 MSI-X vectors

### ✅ Phase 14: Storage Stack *(46 tests)*
- **NVMe Controller** - NVM Express SSD emulation (18 tests)
  - Admin and I/O submission/completion queues
  - Identify Controller/Namespace commands
  - Read/Write/Flush I/O commands
  - Doorbell-based queue management
  - 64KB queue entry support
- **VirtIO-blk** - Paravirtualized block device (15 tests)
  - Request types: Read/Write/Flush/GetId/Discard/WriteZeroes
  - Feature negotiation (SIZE_MAX, SEG_MAX, GEOMETRY, RO, FLUSH)
  - Configuration space with capacity and geometry
  - Interrupt generation on completion
- **Disk Image Formats** - Multiple format support (13 tests)
  - Raw image format (direct sector I/O)
  - QCOW2 header parsing (sparse images, COW)
  - VHDX header parsing (Hyper-V format)
  - In-memory images for testing
  - Auto-format detection

### ✅ Phase 15: Network Stack *(32 tests)*
- **E1000 Network Adapter** - Intel 82540EM gigabit Ethernet (22 tests)
  - PCI identity (Vendor: 0x8086, Device: 0x100E)
  - MMIO register space (128KB) with full register set
  - TX/RX descriptor rings with head/tail management
  - Legacy descriptor format (16 bytes each)
  - EEPROM emulation (64 words) with MAC address storage
  - Multi-cast table array (128 entries)
  - Interrupt cause/mask registers (ICR/IMS/IMC)
  - Link status detection and management
  - Receive address filtering (RAL/RAH)
- **Network Backend** - Host network connectivity (10 tests)
  - `NetworkBackend` trait for pluggable backends
  - `NullBackend` - Packet sink for testing
  - `LoopbackBackend` - Echo packets for testing
  - `TapBackend` - TAP device integration (platform placeholder)
  - `UserBackend` - SLIRP-style NAT with ARP handling
  - `NetworkStats` - TX/RX packet/byte counters
  - MAC address configuration
  - Connection state management

**E1000 Registers:**
| Register | Offset | Description            |
| -------- | ------ | ---------------------- |
| CTRL     | 0x0000 | Device Control         |
| STATUS   | 0x0008 | Device Status          |
| EERD     | 0x0014 | EEPROM Read            |
| ICR      | 0x00C0 | Interrupt Cause Read   |
| IMS      | 0x00D0 | Interrupt Mask Set     |
| IMC      | 0x00D8 | Interrupt Mask Clear   |
| RCTL     | 0x0100 | Receive Control        |
| TCTL     | 0x0400 | Transmit Control       |
| RDBAL    | 0x2800 | RX Descriptor Base Low |
| TDBAL    | 0x3800 | TX Descriptor Base Low |

### ✅ Phase 16: GPU & Display *(66 tests)*
- **Framebuffer** - Linear framebuffer device (26 tests)
  - Configurable resolution and pixel format
  - Pixel formats: ARGB32, XRGB32, RGBA32, BGRA32, RGB24, BGR24, RGB565, Indexed8
  - Color type with RGB/ARGB/RGB565 conversions
  - Rectangle-based dirty region tracking
  - Drawing primitives: set_pixel, fill_rect, draw_hline, draw_vline, draw_rect
  - Blit operations with overlap handling
  - 256-entry color palette for indexed modes
  - Raw buffer access for direct manipulation
- **VirtIO-GPU** - Paravirtualized graphics device (21 tests)
  - GPU commands: GetDisplayInfo, ResourceCreate2d, SetScanout, ResourceFlush, etc.
  - GPU formats: B8G8R8A8, X8R8G8B8, R8G8B8A8, etc.
  - 2D resource management with pixel data storage
  - Scanout configuration with framebuffer output
  - Cursor support: update, move, show/hide
  - Multi-scanout support (up to 16)
  - Backing storage attach/detach
  - Interrupt generation on flush
- **Display Backend** - Abstract display output (19 tests)
  - `DisplayBackend` trait for pluggable outputs
  - `NullDisplayBackend` - Discards output (for testing)
  - `MemoryDisplayBackend` - Stores in memory (for testing)
  - `CallbackDisplayBackend` - Custom callback on update
  - `DisplayManager` - Multi-display coordination
  - `DisplayStats` - Frame/byte/update counters
  - Partial region updates
  - Cursor position and visibility

**Pixel Formats:**
| Format   | BPP | Description           |
| -------- | --- | --------------------- |
| ARGB32   | 4   | 8-bit ARGB with alpha |
| XRGB32   | 4   | 8-bit RGB, alpha = FF |
| RGBA32   | 4   | 8-bit RGBA            |
| BGRA32   | 4   | 8-bit BGRA            |
| RGB24    | 3   | 8-bit RGB, no alpha   |
| BGR24    | 3   | 8-bit BGR, no alpha   |
| RGB565   | 2   | 5-6-5 bit RGB         |
| Indexed8 | 1   | 8-bit palette index   |

### ✅ Phase 17: Live Migration *(51 tests)*
- **Dirty Page Tracking** - Memory change detection (22 tests)
  - DirtyBitmap: Per-page dirty bit tracking (1 bit per 4KB page)
  - Atomic bitmap operations (mark, clear, collect)
  - Range-based dirty marking for bulk operations
  - Dirty page iterator for efficient enumeration
  - Generation counter for tracking scan cycles
  - Large region support (tested with 1GB+)
  - DirtyTracker: Multi-region coordinator
  - Enable/disable tracking control
  - Dirty rate statistics
- **State Serialization** - VM state capture/restore (18 tests)
  - Format version and magic number validation
  - Section types: Header, Cpu, Memory, Device, Timer, Interrupt, Custom
  - CpuState: 16 GPRs, RIP, RFLAGS, segments, control regs, MSRs, FPU
  - SegmentRegister: selector, base, limit, access rights
  - DescriptorTable: GDT/IDT base and limit
  - MemoryRegionState: GPA, size, flags, compressed data
  - DeviceState: name, type, serialized data
  - CRC32 checksum verification
  - StateSerializer/StateDeserializer for streaming
  - VmState container for complete VM snapshots
- **Migration Protocol** - Pre-copy algorithm (11 tests)
  - MigrationStage: Idle → Setup → PreCopy → StopAndCopy → Completed
  - MigrationRole: Source and Destination
  - MigrationConfig: thresholds, downtime limits, bandwidth
  - MigrationController: State machine with transitions
  - PreCopyMigration: Iterative dirty page transfer
  - MigrationStream: Message queue for coordination
  - MigrationMessage: Setup, Pages, CpuState, DeviceState, etc.
  - PageData: GPA with data, zero-page detection
  - Convergence detection (dirty rate vs transfer rate)
  - Statistics: bytes, pages, rate, expected downtime

### ✅ Phase 18: Security & Isolation *(66 tests)*
- **Memory Encryption** - AMD SEV / Intel TDX support (24 tests)
  - EncryptionTechnology: None, AmdSev, AmdSevEs, AmdSevSnp, IntelTdx, IntelMktme
  - KeyId/KeyState/KeyMetadata for encryption key management
  - PageEncryptionState: Shared, Encrypted(KeyId), Transitioning
  - CbitPosition for C-bit handling in page table entries (default bit 47)
  - EncryptionManager: page state tracking, key lifecycle, address translation
  - SevContext: AMD SEV launch state machine (Idle→Started→Measuring→Finished→Running)
  - TdxContext: Intel TDX trust domains with MR registers
  - EncryptionStats: statistics tracking
- **Virtual TPM** - TPM 2.0 device emulation (24 tests)
  - TpmCommandCode: Startup, Shutdown, SelfTest, GetCapability, GetRandom, PCR ops, etc.
  - TpmResponseCode: Success, Initialize, BadParam, AuthFail, etc.
  - HashAlgorithm: SHA-1, SHA-256, SHA-384, SHA-512, SM3 with output sizes
  - PcrBank: 24 PCRs per algorithm with extend/reset operations
  - NvEntry/NvIndex: Non-volatile storage with define/read/write
  - TpmKey/KeyHandle: Key management with RSA/ECC/Symmetric types
  - VirtualTpm: Full TPM 2.0 command dispatch (startup, PCR, NV, keys, hash)
  - Predefined handles: EK (0x81000001), SRK (0x81000002)
- **Secure Boot** - UEFI Secure Boot chain verification (18 tests)
  - SignatureAlgorithm: RSA-SHA256/384/512, ECDSA-SHA256/384
  - Certificate: X.509 with subject, issuer, validity, public key
  - CertificateType: PlatformKey, KeyExchangeKey, Database, ForbiddenDatabase
  - BootComponent: firmware, bootloader, kernel, driver verification
  - VerificationResult: Success, InvalidSignature, Revoked, etc.
  - SecureBootMode: Disabled, Setup, User, Deployed, Audit
  - SecureBootManager: PK/KEK/db/dbx management, hash allow/blocklists
  - SecureBootPolicy: unsigned driver/option ROM policy

**Key Security Types:**
| Type                 | Description                              |
| -------------------- | ---------------------------------------- |
| EncryptionTechnology | Memory encryption method (SEV/TDX/MKTME) |
| KeyId                | Encryption key identifier (16-bit)       |
| VirtualTpm           | TPM 2.0 device with PCR/NV/key support   |
| SecureBootManager    | UEFI Secure Boot verification            |

### ✅ Phase 19: Container & Isolation Extensions *(78 tests)*
- **Container Runtime** - OCI-compatible container lifecycle (28 tests)
  - ContainerState: Creating, Created, Running, Stopped, Paused
  - ContainerSpec: OCI 1.0.2 spec with process, mounts, linux config
  - ContainerProcess: command, cwd, env, uid/gid, terminal
  - ContainerRuntime: create, start, stop, kill, pause, resume, delete
  - MountType: Bind, Tmpfs, Proc, Sysfs, Devpts, Cgroup, Mqueue
  - SeccompConfig: syscall filtering with actions and argument matching
  - ResourceConfig: CPU, memory, block I/O, PIDs limits
  - RuntimeStats: container count by state
- **Namespace Isolation** - Linux namespace management (24 tests)
  - NsType: Pid, Net, Mnt, Uts, Ipc, User, Cgroup, Time
  - PidNamespace: PID allocation, virtual-to-host translation
  - NetNamespace: interfaces, routes, firewall rules
  - MntNamespace: mount points, bind mounts, tmpfs
  - UtsNamespace: hostname, domain name isolation
  - IpcNamespace: message queues, semaphores, shared memory
  - UserNamespace: UID/GID mappings with inside/outside translation
  - NamespaceManager: create and manage all namespace types
- **Cgroup Resource Control** - cgroup v1/v2 controllers (26 tests)
  - CgroupVersion: V1 (legacy), V2 (unified)
  - CpuController: shares, quota, period, weight conversion
  - CpusetController: CPU/memory pinning with range parsing
  - MemoryController: limit, soft limit, swap, OOM settings
  - IoController: weight, per-device throttling (BPS/IOPS)
  - PidsController: process count limits
  - DevicesController: device access rules (allow/deny)
  - FreezerState: Thawed, Freezing, Frozen
  - CgroupManager: hierarchical cgroup management

**Container Types:**
| Type             | Description                           |
| ---------------- | ------------------------------------- |
| ContainerRuntime | OCI runtime with lifecycle management |
| NamespaceManager | Multi-namespace coordination          |
| CgroupManager    | Resource controller hierarchy         |
| PidNamespace     | Process ID isolation                  |
| NetNamespace     | Network stack isolation               |

### ✅ Phase 20: Debugging & Introspection (52 tests)
- GDB stub for guest debugging with full RSP protocol
- VM introspection API for memory and CPU inspection
- Memory inspection tools with hex dump and pattern search
- CPU state examination with page table walking

**GDB Stub Implementation:**
- Complete GDB Remote Serial Protocol (RSP) support
- Software and hardware breakpoints (0-4 types)
- Watchpoints: write, read, and access modes
- Single-step execution control
- Register read/write (all x86-64 registers)
- Memory read/write operations
- Packet parsing with checksum validation
- Target description XML support

**Introspection Components:**
- CpuState: Full CPU register snapshot (GPRs, control regs, segments)
- CpuMode: Real, Protected, Long, Virtual8086, SMM detection
- PageTableWalker: 4-level (long mode) and 2-level (legacy) page walks
- MemoryInspector: Region tracking, hex dump, pattern search
- IdtInspector: 32-bit and 64-bit IDT entry parsing
- Event logging for breakpoints, syscalls, page faults, etc.

**Debug Types:**
| Type            | Description                     |
| --------------- | ------------------------------- |
| GdbStub         | GDB RSP protocol handler        |
| DebugManager    | Unified debugging coordinator   |
| MemoryInspector | Guest memory inspection         |
| CpuInspector    | CPU state tracking per vCPU     |
| PageTableWalker | Virtual-to-physical translation |
| IdtInspector    | Interrupt descriptor parsing    |

### ✅ Phase 21: Advanced Networking (64 tests)
- **Virtual Switch** - Software L2 switch (33 tests)
  - MacAddress: 6-byte MAC with broadcast/multicast/local detection
  - VlanId: 12-bit VLAN identifier (0-4094)
  - VlanMode: Access (single VLAN), Trunk (native + allowed), Hybrid (tagged/untagged)
  - VlanSet: 256-VLAN bitmap for membership testing
  - Port: Internal/External/Patch/Mirror types with STP state
  - PortState: Disabled, Blocking, Listening, Learning, Forwarding (STP)
  - MacTable: 8192-entry MAC learning table with 5-minute aging
  - MacEntry: MAC-to-port binding with VLAN and timestamp
  - EthernetFrame: Parse/serialize with VLAN tag support
  - StpState: Bridge/port priority, designated root/bridge
  - VirtualSwitch: Frame forwarding with unicast/broadcast/multicast handling
  - Port mirroring: Source-to-destination traffic duplication
  - SwitchStats: RX/TX frames, forwarded, flooded, dropped counters

- **Network Filtering** - Stateful packet filter (20 tests)
  - IpProtocol: ICMP, TCP, UDP, ICMPv6 with number() conversion
  - ConnTuple: 5-tuple (src_ip, dst_ip, src_port, dst_port, protocol)
  - ConnState: New, Established, Related, Invalid
  - TcpState: TCP connection state machine (SynSent→SynRecv→Established→...)
  - ConnTrackEntry: Connection tracking with bytes/packets, reply tracking
  - ConnTracker: Connection table with timeouts (TCP 120s, UDP 30s, ICMP 30s)
  - FilterAction: Accept, Drop, Reject, Log, Snat, Dnat
  - FilterRule: Match criteria with counters (src/dst IP, ports, protocol, state)
  - IpMatch: Any, Exact, Network (CIDR), Range, Not (negation)
  - PortMatch: Any, Exact, Range, Set (multiple), Not
  - ProtocolMatch: Any, Exact, Set
  - StateMatch: New, Established, Related, Invalid connection states
  - FilterChain: Ordered rule evaluation with default policy
  - NetworkFilter: Input/Output/Forward/Prerouting/Postrouting chains
  - NatTarget: SNAT/DNAT target addresses and port ranges

- **SR-IOV Passthrough** - PCI device passthrough (11 tests)
  - PciAddress: Domain:Bus:Device.Function with BDF string parsing
  - PciClass: Network, Storage, Display device classes
  - SriovCapability: VF count, offset, stride, page sizes, ARI support
  - VirtualFunction: VF index, state, MAC, VLAN, spoofcheck, trust, rate limits
  - VfState: Disabled, Enabled, Assigned, Error
  - VfLinkState: Auto (follow PF), Enable (always up), Disable (always down)
  - PhysicalFunction: PF with VF management (enable/disable/assign/release)
  - SriovManager: Multi-PF coordination with device assignment
  - DeviceAssignment: Device-to-VM mapping with guest address
  - IommuGroup: IOMMU group tracking for isolation verification

**Network Types:**
| Type             | Description                           |
| ---------------- | ------------------------------------- |
| VirtualSwitch    | L2 software switch with MAC learning  |
| NetworkFilter    | Stateful packet filter with conntrack |
| SriovManager     | SR-IOV VF assignment manager          |
| PhysicalFunction | SR-IOV capable PCI device             |

### ✅ Phase 22: USB & HID (71 tests)
- **xHCI Controller** - USB 3.0 host controller (34 tests)
  - UsbSpeed: Full (12Mbps), Low (1.5Mbps), High (480Mbps), Super (5Gbps), SuperPlus (10Gbps)
  - PortState: Disconnected, Powered, Enabled, Reset, Suspended, Error
  - PortRegister: Port status/control register emulation
  - XhciPort: USB2/USB3 port with speed detection, reset, state management
  - TrbType: 40+ TRB types (Normal, Setup, Data, Status, Link, Command, Event...)
  - TrbCompletionCode: 36 completion codes (Success, DataBuffer, Stall, TRB, Bandwidth...)
  - Trb: 16-byte Transfer Request Block with cycle bit management
  - RingSegment: Ring memory segment with producer/consumer indices
  - CommandRing: Command ring with TRB dequeue and doorbell
  - EventRing: Event ring with multi-segment support (ERST entries)
  - ErstEntry: Event Ring Segment Table entry (address, size)
  - Interrupter: Interrupt moderation with pending management
  - SlotState: Disabled, Enabled, Default, Addressed, Configured
  - DeviceSlot: 64 slots with port binding and endpoint context
  - XhciController: Full controller with opreg read/write, command processing

- **USB Device Framework** - Core USB device support (27 tests)
  - DeviceState: Detached, Attached, Powered, Default, Address, Configured
  - DeviceClass: HID, MassStorage, Hub, Audio, Video, Communications, etc.
  - DescriptorType: Device, Configuration, String, Interface, Endpoint, HID, HidReport
  - DeviceDescriptor: 18-byte USB device descriptor
  - ConfigDescriptor: Configuration descriptor (9 bytes + interfaces)
  - InterfaceDescriptor: Interface descriptor (9 bytes)
  - EndpointDescriptor: Endpoint descriptor (7 bytes)
  - StringDescriptor: Unicode string descriptor
  - SetupPacket: 8-byte USB control setup packet
  - EndpointDirection: In, Out
  - TransferType: Control, Bulk, Interrupt, Isochronous
  - Endpoint: Transfer endpoint with buffer management
  - UsbDevice trait: Standard device interface
  - ControlResult: Ok(data), Stall, Nak
  - TransferResult: Ok(data), Stall, Nak
  - BaseUsbDevice: Default USB device implementation

- **HID Class Devices** - Human interface devices (20 tests)
  - HidSubclass: None, Boot
  - HidProtocol: None, Keyboard, Mouse
  - HidDescriptor: HID class descriptor (9 bytes)
  - ReportType: Input, Output, Feature
  - KeyboardModifiers: Left/Right Ctrl, Shift, Alt, GUI
  - MouseButtons: Left, Right, Middle, Button4, Button5
  - UsbKeyboard: Boot protocol keyboard with LED support
  - UsbMouse: Relative movement mouse (X, Y, wheel)
  - UsbTablet: Absolute positioning tablet (32767x32767)
  - HidStats: Reports sent, key presses, releases

**USB Types:**
| Type           | Description                     |
| -------------- | ------------------------------- |
| XhciController | USB 3.0 xHCI host controller    |
| UsbDevice      | USB device trait and base impl  |
| UsbKeyboard    | HID boot protocol keyboard      |
| UsbMouse       | HID relative mouse              |
| UsbTablet      | HID absolute positioning device |

### ✅ Phase 23: GPU & Display (68 tests)
- **GPU Core** - Common GPU types and abstractions (25 tests)
  - PixelFormat: ARGB32, XRGB32, RGBA32, BGRA32, RGB565, RGB24, BGR24, Indexed8, Gray8
  - Color: RGBA with constants (BLACK, WHITE, RED, GREEN, BLUE, TRANSPARENT)
  - Color conversions: to/from ARGB32, XRGB32, RGBA32, BGRA32, RGB565
  - Color::blend() for alpha compositing
  - DisplayMode: Resolution, format, refresh rate, common presets (VGA, SVGA, XGA, HD, FULL_HD, UHD_4K)
  - Rect: Rectangle with intersection, union, contains operations
  - CursorShape: Hardware cursor with hotspot and indexed pixels
  - CursorState: Position, visibility, cursor index
  - DisplaySurface trait: Pixel operations interface
  - Scanout: Multi-monitor configuration with position offsets
  - GpuStats: Atomic counters for frames, pixels, blits, flushes

- **Framebuffer** - Software framebuffer implementation (25 tests)
  - DisplayMode-based initialization with stride calculation
  - DisplaySurface implementation for pixel operations
  - set_pixel/get_pixel with multi-format support
  - fill_rect with per-format optimization
  - draw_hline, draw_vline: Line primitives
  - draw_rect: Rectangle outline
  - copy_rect: Region copy with overlap handling
  - scroll_up/scroll_down: Content scrolling with fill
  - resize: Dynamic resolution change with content preservation
  - blit: Surface-to-surface copy
  - convert_format: Format conversion to byte array
  - Dirty region tracking for efficient updates
  - DoubleBuffer: Front/back buffer with swap, vsync support

- **VirtIO-GPU** - Paravirtualized graphics device (27 tests)
  - VirtioGpuCtrlType: 40+ command types (GetDisplayInfo, ResourceCreate2d, SetScanout, etc.)
  - VirtioGpuFormat: Pixel formats with BPP and conversions
  - GpuResource: 2D resource with pixel storage
  - Resource operations: set_pixel, fill_region, transfer_region
  - ScanoutState: Scanout configuration with resource binding
  - VirtioGpu: Full device with framebuffer output
  - Display info query for up to 16 scanouts
  - create_resource_2d: 2D resource allocation
  - unref_resource: Resource deallocation
  - attach_backing/detach_backing: Guest memory binding
  - transfer_to_host_2d: Guest-to-host pixel transfer
  - set_scanout: Configure display output
  - resource_flush: Flush resource to display
  - Enable/disable display control
  - Stats: Frames, flushes, resources, bytes tracking

**GPU Pixel Formats:**
| Format   | BPP | Description               |
| -------- | --- | ------------------------- |
| ARGB32   | 4   | 8-bit ARGB with alpha     |
| XRGB32   | 4   | 8-bit RGB, X byte ignored |
| RGBA32   | 4   | 8-bit RGBA                |
| BGRA32   | 4   | 8-bit BGRA                |
| RGB565   | 2   | 5-6-5 bit RGB             |
| RGB24    | 3   | 8-bit RGB, no alpha       |
| BGR24    | 3   | 8-bit BGR, no alpha       |
| Indexed8 | 1   | 8-bit palette index       |
| Gray8    | 1   | 8-bit grayscale           |

### ✅ Phase 24: Audio & Sound (90 tests)
- **Audio Core Types** - Common audio abstractions (31 tests)
  - SampleFormat: U8, S16Le, S16Be, S24Le, S32Le, F32Le with bytes/bits
  - SampleRate: 8kHz, 11.025kHz, 16kHz, 22.05kHz, 32kHz, 44.1kHz, 48kHz, 96kHz, 192kHz
  - ChannelLayout: Mono, Stereo, Surround21, Quad, Surround51, Surround71
  - AudioParams: Format, rate, channels with bytes_per_frame/second calculations
  - AudioParams presets: CD_QUALITY, DVD_QUALITY, HIGH_RES
  - AudioBuffer: Ring buffer with circular read/write/peek operations
  - AudioStream trait: Start/stop/read/write interface
  - PcmStream: AudioStream implementation with state machine
  - StreamDirection: Playback, Capture
  - StreamState: Stopped, Running, Paused, Draining
  - Volume: Gain level with mute, from_db, apply_s16
  - StereoVolume: Per-channel volume control
  - AudioMixer: Multi-input audio mixing with S16Le support
  - AudioStats: Atomic counters (bytes played/captured, underruns, interrupts)

- **AC97 Controller** - AC97 audio controller emulation (25 tests)
  - Ac97Register: 30+ register addresses (Reset, MasterVolume, PcmOutVolume, etc.)
  - BusMaster registers: PI/PO/MC channel base, CIV, LVI, SR, PICB, CR
  - StatusBits: DCH, CELV, LVBCI, BCIS, FIFOE
  - ControlBits: RPBM, RR, LVBIE, IOCE, FEIE
  - GlobalControl: GIE, COLD, WARM, SHUT, PCM modes
  - GlobalStatus: Codec ready, channel capabilities, AC97 version
  - BufferDescriptor: 8-byte DMA descriptor with IOC/BUP flags
  - DmaChannel: PI (input), PO (output), MC (microphone) channels
  - Ac97Mixer: Volume control, mute, sample rate (8-48kHz)
  - Ac97Controller: Full controller with register read/write, interrupt generation

- **Intel HDA Controller** - High Definition Audio emulation (25 tests)
  - Registers module: GCAP, GCTL, WAKEEN, INTCTL, CORB/RIRB, stream descriptors
  - GlobalCtl: CRST (controller reset), FCNTRL, UNSOL
  - StreamReg: Per-stream register offsets (CTL, STS, LPIB, CBL, LVI, FMT, BDPL/U)
  - StreamCtl: SRST, RUN, IOCE, FEIE, STRIPE bits
  - StreamSts: BCIS, FIFOE, DESE bits
  - WidgetType: AudioOutput, AudioInput, AudioMixer, AudioSelector, PinComplex, etc.
  - PinConfig: Jack configuration with factory methods (line_out, headphone, mic_in, line_in)
  - Widget: Codec node with amp caps, connections, format, power state
  - HdaCodec: Widget tree with verb processing and parameter queries
  - StreamDescriptor: Format parsing (rate/bits/channels), audio I/O
  - HdaController: 4 input + 4 output streams, CORB/RIRB command processing
  - Wall clock counter for synchronization

- **VirtIO-Sound** - Paravirtualized sound device (19 tests)
  - VirtioSndRequestType: JackInfo, JackRemap, PcmInfo, PcmSetParams, etc.
  - VirtioSndStatus: Ok, BadMsg, NotSupp, IoErr
  - VirtioSndDirection: Output (playback), Input (capture)
  - VirtioSndPcmFormat: 25 formats (U8, S16, S24, S32, Float, IMA-ADPCM, etc.)
  - VirtioSndPcmRate: 14 rates (5512Hz to 384kHz)
  - VirtioSndChmapPosition: Channel positions (FL, FR, RL, RR, FC, LFE, SL, SR, etc.)
  - JackType: LineOut, Speaker, Headphone, CD, SPDIF, Mic, LineIn, etc.
  - Jack: Jack state with type, connection, stream association
  - PcmStreamInfo: Stream capabilities (formats, rates, channels)
  - PcmParams: Buffer/period bytes, format, rate, channels
  - VirtioSndPcmStream: PCM stream with state machine
  - ChannelMap: Channel position mappings (stereo, 5.1, 7.1)
  - VirtioSoundDevice: Full device with jacks, streams, channel maps

**Audio Types:**
| Type              | Description                        |
| ----------------- | ---------------------------------- |
| AudioParams       | Sample format/rate/channels config |
| AudioBuffer       | Ring buffer for PCM data           |
| Ac97Controller    | AC97 audio controller              |
| HdaController     | Intel HDA controller               |
| VirtioSoundDevice | VirtIO sound device                |

### ✅ Phase 25: Input Devices (84 tests)
- **PS/2 Keyboard** - Full keyboard emulation with scan code sets (24 tests)
  - ScanCodeSet: Set1 (XT compatible), Set2 (AT default), Set3 (PS/2)
  - KeyCode: 80+ keys (A-Z, 0-9, F1-F12, modifiers, keypad, arrows, special)
  - Scan code generation: set1_make(), set2_make(), set3_make() per key
  - LedState: Scroll Lock, Num Lock, Caps Lock with byte conversion
  - TypematicConfig: Rate (2-30 chars/sec) and delay (250-1000ms)
  - Ps2Command: SetLeds (0xED), Echo (0xEE), ScanCodeSet (0xF0), Identify (0xF2), SetTypematic (0xF3), Enable/Disable, Reset
  - Response codes: ACK (0xFA), RESEND (0xFE), ECHO (0xEE), BAT_OK (0xAA), ID1/ID2
  - CommandState: State machine for multi-byte commands
  - Ps2Keyboard: Full device with command processing, key events, type_string()
  - KeyboardStats: Keys pressed/released, commands, bytes output

- **PS/2 Mouse** - Mouse protocol with scroll wheel (24 tests)
  - MouseProtocol: Standard (3-byte), Intellimouse (4-byte), Explorer (5-button)
  - Protocol upgrade via magic sample rate sequences (200-100-80, 200-200-80)
  - MouseButtons: Left, right, middle, button4, button5 with byte conversion
  - MouseResolution: 1, 2, 4, 8 counts per mm
  - SampleRate: 10, 20, 40, 60, 80, 100, 200 samples/second
  - ScalingMode: Linear (1:1), NonLinear (2:1) with apply()
  - MouseCommand: SetScaling, SetResolution, StatusRequest, SetSampleRate, Enable/Disable, Reset
  - Packet generation: Button state, X/Y movement, overflow, scroll wheel
  - Ps2Mouse: Full device with movement, buttons, scroll, protocol negotiation
  - MouseStats: Packets sent, button events, movement, scroll events

- **Touchscreen Device** - Multi-touch with gesture support (22 tests)
  - TouchState: Up, Down, Move
  - TouchPoint: ID, X/Y (0.0-1.0), pressure, major/minor axes, orientation
  - GestureType: Tap, DoubleTap, LongPress, SwipeLeft/Right/Up/Down, Pinch, Spread, Rotate
  - Gesture: Type with center X/Y, scale, rotation, velocity
  - TouchEvent: Point array with timestamp
  - TouchConfig: Resolution, max touches, pressure/multitouch/gesture support
  - TouchStats: Events, touch downs/ups/moves, gestures detected
  - Touchscreen: Full device with touch_down/move/up, tap, double_tap, swipe, pinch
  - Gesture detection: Single-touch (tap, long press, swipe), multi-touch (pinch, rotate)
  - Touch tracking: Per-touch state with start position and time

- **Game Controller** - Gamepad with force feedback (24 tests)
  - ControllerType: Generic, Xbox, PlayStation, Nintendo with button/axis counts
  - Button: 16 standard buttons (South/East/West/North, bumpers, D-pad, sticks, guide)
  - Axis: LeftStickX/Y, RightStickX/Y, LeftTrigger, RightTrigger
  - RumbleEffect: Strong/weak motor intensity with duration
  - ControllerState: Button bitfield, axis values, timestamp
  - ControllerEvent: ButtonPressed, ButtonReleased, AxisChanged, Connected, Disconnected
  - DeadzoneConfig: Inner/outer deadzone with value scaling
  - GameController: Full device with button press/release, axis values, rumble
  - ControllerStats: Button presses/releases, axis updates, rumble effects
  - Convenience methods: tap_button, set_dpad, set_left/right_stick, set_triggers

**Input Types:**
| Type           | Description                            |
| -------------- | -------------------------------------- |
| Ps2Keyboard    | PS/2 keyboard with scan code sets      |
| Ps2Mouse       | PS/2 mouse with scroll wheel protocols |
| Touchscreen    | Multi-touch with gesture recognition   |
| GameController | Gamepad with buttons, axes, rumble     |

### ✅ Phase 26: UEFI Boot Support (76 tests)
- **UEFI firmware interface** - types.rs (16 tests)
  - Guid: 16-byte identifier with from_bytes/to_bytes
  - 14 well-known GUIDs (EFI_GLOBAL_VARIABLE, EFI_ACPI_TABLE, GOP, etc.)
  - Status: SUCCESS, 6 warnings, 35+ error codes with is_success/is_error/is_warning
  - Handle: u64 wrapper with NULL constant
  - MemoryType enum: 15 memory types with is_runtime/is_conventional
  - MemoryAttribute: UC, WC, WT, WB, WP, RP, XP, NV, RUNTIME flags
  - MemoryDescriptor: 48-byte structure for memory map entries
  - Time/TimeCapabilities: UEFI time structures
  - TableHeader: System/Boot/Runtime table headers with signatures
  - AllocateType enum: AllocateAnyPages, AllocateMaxAddress, AllocateAddress

- **EFI system table emulation** - system_table.rs (28 tests)
  - BootServiceFunction enum: 44 function indices
  - Tpl enum: Application, Callback, Notify, HighLevel
  - Event management: create_event, set_timer, signal_event, check_event, close_event
  - Memory services: allocate_pages, free_pages, get_memory_map, allocate_pool, free_pool
  - Protocol services: install_protocol, handle_protocol, locate_handle, locate_protocol
  - Handle database with protocol interface tracking
  - SystemTable: firmware info, console handles, configuration tables
  - BootServicesStats: allocation/event/protocol install counters

- **GOP (Graphics Output Protocol)** - gop.rs (22 tests)
  - GopPixelFormat: RGBX/BGRX 8-bit per color, BitMask, BltOnly
  - GopPixelBitmask: Red/green/blue/reserved masks
  - GopModeInfo: Resolution, pixel format, stride, framebuffer size
  - GopBltPixel: BGRX pixel with color constants (BLACK, WHITE, RED, etc.)
  - GopBltOperation: VideoFill, VideoToBltBuffer, BltBufferToVideo, VideoToVideo
  - GraphicsOutputProtocol: query_mode, set_mode, blt operations
  - Framebuffer emulation with get_pixel/set_pixel/clear
  - GopStats: Mode queries, mode sets, BLT operations, pixels transferred

- **EFI runtime services** - runtime_services.rs (20 tests)
  - Time services: get_time, set_time, get_wakeup_time, set_wakeup_time
  - Variable services: get_variable, set_variable, get_next_variable_name, query_variable_info
  - VariableAttributes: NV, BS_ACCESS, RT_ACCESS, authenticated flags
  - Virtual memory: set_virtual_address_map, convert_pointer
  - Miscellaneous: get_next_high_monotonic_count, reset_system
  - Capsule services: update_capsule, query_capsule_capabilities
  - Standard variables: SecureBoot, SetupMode, BootOrder initialization
  - RuntimeServicesStats: Time/variable/reset call counters

**UEFI Types:**
| Type                   | Description                          |
| ---------------------- | ------------------------------------ |
| Guid                   | 16-byte unique identifier            |
| Status                 | EFI return code with success/error   |
| MemoryDescriptor       | Memory map entry (type, addr, pages) |
| GraphicsOutputProtocol | GOP framebuffer graphics             |
| RuntimeServices        | Time, variables, reset services      |
| BootServices           | Memory, events, protocol services    |

### ✅ Phase 27: PCI Express Support (COMPLETED)
- PCIe configuration space emulation
- PCI capability structures (MSI, MSI-X, PM, PCIe)
- PCI bus topology and device enumeration
- PCI Express link training

**Implementation Details:**
- **PCI types** - types.rs (17 tests)
  - PciAddress: Segment/bus/device/function with BDF conversion and ECAM offset
  - VendorId: INVALID (0xFFFF), INTEL (0x8086), AMD (0x1022), NVIDIA, RED_HAT, QEMU
  - DeviceId: INVALID, VIRTIO_NET/BLK/CONSOLE/GPU device IDs
  - ClassCode: Base/sub/prog_if with 15+ device class constants
  - HeaderType: Standard (Type 0), PciBridge (Type 1), CardBusBridge (Type 2)
  - BarType: Memory32, Memory64, Io, Unused with prefetchable flag
  - CommandRegister: IO_SPACE, MEMORY_SPACE, BUS_MASTER, INTERRUPT_DISABLE
  - StatusRegister: CAPABILITIES_LIST, error status bits
  - InterruptPin: None, IntA-IntD with bridge swizzle routing

- **PCI config space** - config.rs (17 tests)
  - ConfigSpace: 4KB Type 0 device config with register offsets
  - BridgeConfigSpace: Type 1 PCI-to-PCI bridge config
  - BarConfig: Memory32/64/IO BAR configuration with size detection
  - Write masks for read-only/write-1-to-clear registers
  - BAR size detection via 0xFFFFFFFF write pattern
  - Bridge memory window and bus number management
  - ConfigStats: Atomic read/write/bar_access counters

- **PCI capabilities** - capabilities.rs (18 tests)
  - CapabilityId: 21 standard PCI capabilities
  - ExtendedCapabilityId: 44 PCIe extended capabilities (AER, SRIOV, etc.)
  - MsiCapability: 32/64-bit address, per-vector masking, pending bits
  - MsixCapability: Table and PBA with BIR/offset, function masking
  - PmCapability: D0-D3Hot power states, PME support
  - PcieCapability: Device types, link speed (Gen1-Gen6), width (x1-x32)
  - PcieLinkSpeed: 2.5 GT/s to 64 GT/s with bandwidth calculation
  - PcieLinkStatus: Training, DL active, bandwidth management

- **PCI bus topology** - bus.rs (14 tests)
  - PciBus: Bus number, parent bridge, device map
  - PciDeviceSlot: Standard device or bridge config space holder
  - PciDeviceInfo: Address, vendor/device ID, class code, header type
  - PciRootComplex: Root bus, device enumeration, ECAM access
  - BridgeForwarder: Memory/IO/config forwarding decisions
  - BusStats: Config reads/writes, devices/bridges found

**PCI Types:**
| Type              | Description                            |
| ----------------- | -------------------------------------- |
| PciAddress        | Segment:Bus:Device.Function addressing |
| ConfigSpace       | Type 0 standard device config          |
| BridgeConfigSpace | Type 1 PCI bridge config               |
| MsiCapability     | MSI interrupt capability               |
| MsixCapability    | MSI-X interrupt capability             |
| PcieCapability    | PCIe link and device capability        |
| PciRootComplex    | Root bus and device enumeration        |

### ✅ Phase 28: IOMMU Support
**Status:** Complete  
**Tests:** 64 tests (1384 total)

- Intel VT-d DMAR table generation
- AMD IOMMU (AMD-Vi) support
- DMA remapping for device passthrough
- Interrupt remapping tables

**Implementation Details:**
- **IOMMU types** - types.rs (18 tests)
  - AddressWidth: Bits30/39/48/57 with levels() and from_agaw()
  - DeviceId: Segment:Bus:DevFn with source_id(), requester_id()
  - DeviceScope: PCI endpoint, bridge, IOAPIC, HPET, ACPI namespace
  - PageTableEntry: PRESENT, READ, WRITE, EXECUTE, PAGE_SIZE, SNOOP flags
  - FaultReason: 13 fault types (root/context/page not present, invalid, etc.)
  - FaultRecord: Device, address, reason, write/read tracking
  - DomainId, TranslationType, IommuStats with hit rate calculation

- **Intel VT-d** - vtd.rs (15 tests)
  - VtdUnit: Full hardware emulation with MMIO registers
  - RootEntry: 256-entry root table for bus mapping
  - ContextEntry: Device-to-domain translation context
  - Iotlb: IOVA→HPA cache with device/domain/page invalidation
  - Register access: VER, CAP, ECAP, GCMD, GSTS, RTADDR, FSTS, etc.
  - Translation: Identity and remapped with fault recording

- **AMD IOMMU** - amd.rs (14 tests)
  - AmdIommu: Full AMD-Vi unit with command buffer
  - DeviceTableEntry: 256-bit device table entries
  - CommandEntry: Completion wait, invalidation commands
  - EventLogEntry: IO page fault, illegal DTE events
  - Event types: PageFault, IllegalDevice, HardwareErrors

- **Interrupt remapping** - interrupt_remap.rs (16 tests)
  - IntelIrte: 128-bit interrupt remapping table entries
  - AmdIrte: 64-bit AMD interrupt remapping entries
  - IntelInterruptRemapTable: Vector/destination remapping
  - AmdInterruptRemapTable: Per-device interrupt tables
  - MsiMessage: MSI address/data format with delivery modes
  - PostedInterruptDescriptor: 256-bit PIR with notification
  - SourceValidation: Source ID and bus range validation

**IOMMU Types:**
| Type                      | Description                       |
| ------------------------- | --------------------------------- |
| VtdUnit                   | Intel VT-d IOMMU hardware unit    |
| AmdIommu                  | AMD-Vi IOMMU hardware unit        |
| DeviceId                  | Segment:Bus:Device.Function ID    |
| PageTableEntry            | IOMMU page table entry            |
| IntelIrte                 | Intel interrupt remap table entry |
| AmdIrte                   | AMD interrupt remap table entry   |
| PostedInterruptDescriptor | Posted interrupt descriptor       |

### ✅ Phase 29: NUMA Support
**Status:** Complete  
**Tests:** 79 tests (1463 total)

- NUMA topology discovery and management
- Memory node and CPU affinity tracking
- SRAT/SLIT ACPI table generation
- Guest NUMA-aware memory allocation
- Inter-node latency modeling

**Implementation Details:**
- **NUMA types** - types.rs (32 tests)
  - NodeId: NUMA node identifier with MAX_NODES (256)
  - MemoryRange: Physical memory ranges with hotplug/NVDIMM flags
  - DistanceMatrix: Inter-node latency matrix (SLIT format)
  - CpuAffinity: CPU-to-node mapping with proximity domains
  - MemoryAffinity: Memory-to-node mapping for SRAT
  - NodeStats: Per-node statistics (hits, misses, allocations)
  - AllocationPolicy: Local, Preferred, NearestAvailable, Interleaved, Bind
  - InterleavingMode: None, RoundRobin, Weighted, NodeSet

- **NUMA topology** - topology.rs (17 tests)
  - NumaNode: Single node with CPUs and memory ranges
  - NumaTopology: Complete topology with distance matrix
  - NumaTopologyBuilder: Fluent API for topology construction
  - two_node/four_node: Preset topology generators
  - CPU/memory affinity generation for ACPI
  - Node selection based on allocation policy

- **ACPI tables** - acpi.rs (14 tests)
  - SratBuilder: Generate SRAT (System Resource Affinity Table)
  - SlitBuilder: Generate SLIT (System Locality Information Table)
  - ProcessorLocalApicAffinity: CPU affinity structures
  - ProcessorX2ApicAffinity: x2APIC support for large APIC IDs
  - MemoryAffinityStructure: Memory affinity with hotplug flags
  - AcpiHeader: Standard ACPI table header generation
  - Automatic checksum calculation

- **NUMA allocator** - allocator.rs (16 tests)
  - NumaAllocator: NUMA-aware memory allocation
  - NodeMemoryPool: Per-node free list allocator
  - FreeRegion: Free region tracking with coalescing
  - Allocation policies: Local, preferred, bound, interleaved
  - Round-robin interleaving across nodes
  - AllocError: Detailed error types (OutOfMemory, InvalidNode)

**NUMA Types:**
| Type             | Description                 |
| ---------------- | --------------------------- |
| NodeId           | NUMA node identifier        |
| NumaTopology     | Complete NUMA topology      |
| NumaAllocator    | NUMA-aware memory allocator |
| DistanceMatrix   | Inter-node latency matrix   |
| SratBuilder      | ACPI SRAT table generator   |
| SlitBuilder      | ACPI SLIT table generator   |
| AllocationPolicy | Memory allocation policy    |

### ✅ Phase 30: Power Management
- ✅ ACPI S-states (S0-S5) power state management
- ✅ CPU C-states (C0-C10) idle power management
- ✅ CPU P-states frequency scaling
- ✅ Device power state transitions (D0-D3Cold)
- ✅ Wake event handling and sources
- ✅ Power governors (Menu, Ladder, Ondemand, Performance)

**Implementation Details:**

- **Power types** - types.rs (30 tests)
  - SState: System power states S0-S5 (Working to Soft-Off)
  - CState: CPU idle states C0-C10 with MWAIT hints
  - PState: Performance states with frequency/voltage/power
  - DState: Device power states D0-D3Cold
  - WakeSource: 12 wake source types (PowerButton, RTC, etc.)
  - WakeEvent: Wake event tracking with timestamps
  - PowerEvent: Event types for state changes
  - ThermalEventType/BatteryEventType: System events
  - PowerStats: Comprehensive power statistics

- **S-state management** - sstate.rs (21 tests)
  - SStateManager: System power state controller
  - TransitionPhase: Multi-phase sleep/wake transitions
  - WakeSourceConfig: Configurable wake sources
  - Suspend/hibernate/shutdown support
  - Wake event history and statistics

- **C-state management** - cstate.rs (28 tests)
  - CStateManager: Per-CPU idle state management
  - CpuCState: Per-CPU idle tracking and history
  - CStateGovernor: Menu, Ladder, Teo, HaltPoll, Fixed
  - Idle duration prediction with exponential average
  - Latency constraints and residency tracking

- **P-state management** - pstate.rs (28 tests)
  - PStateManager: CPU frequency scaling controller
  - CpuPState: Per-CPU performance state tracking
  - PStateGovernor: Ondemand, Conservative, Schedutil, Performance, Powersave
  - Utilization tracking with history
  - Turbo boost control and energy preferences

**Power State Summary:**
| State Type | States          | Description                |
| ---------- | --------------- | -------------------------- |
| S-State    | S0-S5           | System power (Work to Off) |
| C-State    | C0, C1, C1E-C10 | CPU idle depth levels      |
| P-State    | P0-Pn           | CPU frequency/voltage      |
| D-State    | D0-D3Cold       | Device power states        |

**Governor Types:**
| Governor     | Behavior                         |
| ------------ | -------------------------------- |
| Menu         | Optimal state by idle prediction |
| Ladder       | Gradual state transitions        |
| Ondemand     | Jump to max on high load         |
| Conservative | Step through states gradually    |
| Performance  | Always maximum frequency         |
| Powersave    | Always minimum frequency         |

### ✅ Phase 31: Snapshot/Restore (74 tests)
- VM state snapshots
- Memory snapshot management
- Device state serialization
- Incremental snapshots
- External snapshot API

**Snapshot Components:**
| Component            | Description                            |
| -------------------- | -------------------------------------- |
| SnapshotId           | Unique snapshot identifier             |
| SnapshotState        | Creating/Valid/Restoring/Invalid       |
| SnapshotType         | Full/Incremental/MemoryOnly/Checkpoint |
| SnapshotInfo         | Metadata with tags, parent, timestamps |
| CpuSnapshot          | Full vCPU state (GPRs, segments, MSRs) |
| MemoryRegionSnapshot | Memory region with compression support |
| DeviceSnapshot       | Serialized device state                |
| VmSnapshot           | Complete VM state container            |

**Memory Snapshot Features:**
| Feature          | Description                          |
| ---------------- | ------------------------------------ |
| DirtyPageTracker | Bitmap-based dirty page tracking     |
| Compression      | Lz4/Zstd/Deflate compression support |
| Deduplication    | Zero page deduplication              |
| Incremental      | Dirty-page-only captures             |

**Device State Serialization:**
| Component               | Description                         |
| ----------------------- | ----------------------------------- |
| DeviceStateSerializer   | Binary serialization primitives     |
| DeviceStateDeserializer | Binary deserialization primitives   |
| Snapshottable trait     | Device state save/restore interface |
| DeviceStateManager      | Device state coordination           |

**Snapshot Manager:**
| Feature               | Description                       |
| --------------------- | --------------------------------- |
| SnapshotConfig        | Storage path, limits, compression |
| CreateSnapshotOptions | Snapshot type, parent, tags       |
| Snapshot lifecycle    | Begin/complete/abort operations   |
| Snapshot chains       | Incremental snapshot hierarchy    |

### ✅ Phase 32: Nested Virtualization (90 tests)
**Status:** Complete
**Tests:** 90 tests passing

Implemented comprehensive nested virtualization support:
- VMX types and capabilities detection
- Shadow VMCS management and caching
- L1/L2 guest transitions
- Nested EPT/VPID management
- VM exit classification and reflection

**Components:**
| Component        | Description                         |
| ---------------- | ----------------------------------- |
| VmcsField        | VMCS field encodings (tuple struct) |
| VmExitReason     | VM exit reasons with constants      |
| VmxCapabilities  | VMX feature detection               |
| NestedGuestState | L1/L2 guest state tracking          |
| ShadowVmcs       | Shadow VMCS with field caching      |
| ShadowVmcsCache  | Multiple shadow VMCS management     |
| EptPointer       | Extended Page Table pointer         |
| EptEntry         | EPT page table entries              |
| NestedEptManager | Nested EPT translation management   |
| NestedManager    | Main nested virtualization manager  |
| L2EntryInfo      | Information for L2 VM entry         |
| L2ExitInfo       | Information for L2 VM exit          |
| ExitDisposition  | Reflect to L1 or handle in L0       |

**VMX Instructions Emulated:**
- VMXON/VMXOFF - VMX operation enable/disable
- VMPTRLD/VMPTRST - VMCS pointer load/store
- VMCLEAR - VMCS clearing
- VMREAD/VMWRITE - VMCS field access
- VMLAUNCH/VMRESUME - L2 guest entry
- INVEPT/INVVPID - TLB invalidation

---

### ✅ Phase 33: Agent Telemetry (29 tests)
**Status:** Complete
**Tests:** 29 tests passing
**Location:** `crates/hv2-agent/src/telemetry.rs`

Implemented comprehensive observability for AI agent operations:
- Metric types (Counter, Gauge, Histogram)
- Metric collection and time-series storage
- Distributed tracing with spans
- Event logging with severity levels
- JSON export for telemetry data

**Components:**
| Component          | Description                      |
| ------------------ | -------------------------------- |
| MetricType         | Counter/Gauge/Histogram/Rate     |
| MetricUnit         | Bytes/Ms/Percent/OpsPerSec       |
| Counter            | Atomic monotonic counter         |
| Gauge              | Point-in-time value              |
| Histogram          | Value distribution with buckets  |
| Span               | Distributed trace span           |
| AgentEvent         | Structured event with attributes |
| TelemetryCollector | Central collection and storage   |

---

### ✅ Phase 34: Agent Resource Limits (27 tests)
**Status:** Complete
**Tests:** 27 tests passing
**Location:** `crates/hv2-agent/src/limits.rs`

Implemented resource quotas and enforcement for agent safety:
- Memory allocation limits and tracking
- CPU time limits and monitoring
- Operation count limits
- Rate limiting with sliding windows
- Concurrency limiting with guards
- Token bucket bandwidth control
- Resource usage summaries

**Components:**
| Component          | Description                      |
| ------------------ | -------------------------------- |
| ResourceLimits     | Configuration for all limits     |
| ResourceUsage      | Atomic usage tracking            |
| RateLimiter        | Sliding window rate control      |
| ConcurrencyLimiter | Max concurrent operation control |
| TokenBucket        | Bandwidth rate limiting          |
| ResourceEnforcer   | Combined limit enforcement       |
| ResourceSummary    | Usage report with utilization    |

---

### ✅ Phase 35: Agent Events (27 tests)
**Status:** Complete
**Tests:** 27 tests passing
**Location:** `crates/hv2-agent/src/events.rs`

Implemented event streaming and subscription system:
- Event categories (Lifecycle, Security, Resource, etc.)
- Event severity levels (Debug to Emergency)
- Event filtering and subscriptions
- Event bus with broadcast channels
- Event history and correlation
- Event aggregation and statistics

**Components:**
| Component       | Description                     |
| --------------- | ------------------------------- |
| EventCategory   | Event classification            |
| EventSeverity   | Syslog-style severity levels    |
| VmEvent         | Structured VM event             |
| EventFilter     | Subscription filtering criteria |
| EventBus        | Publish/subscribe event system  |
| EventReceiver   | Async event reception           |
| EventAggregator | Statistics collection           |

---

### ✅ Phase 36: Agent Actions (24 tests)
**Status:** Complete
**Tests:** 24 tests passing
**Location:** `crates/hv2-agent/src/actions.rs`

Implemented comprehensive action system for VM control:
- Power actions (Start/Stop/Pause/Resume/Reboot)
- Snapshot actions (Create/Restore/Delete/Export)
- Resource actions (CPU/Memory hot-add/remove)
- Network actions (Interface/Firewall management)
- Storage actions (Disk attach/resize/throttle)
- Debug actions (Memory read/breakpoints/trace)
- Action validation and permission checking
- Action queuing and execution

**Components:**
| Component       | Description                        |
| --------------- | ---------------------------------- |
| ActionCategory  | Power/Snapshot/Resource/Network... |
| PowerAction     | VM power state control             |
| SnapshotAction  | Checkpoint management              |
| ResourceAction  | CPU/Memory hotplug                 |
| NetworkAction   | NIC and firewall operations        |
| StorageAction   | Disk management                    |
| DebugAction     | Introspection operations           |
| ActionValidator | Permission and limit checking      |
| ActionQueue     | Request batching and ordering      |

---

### ✅ Phase 37: Agent Policies (30 tests)
**Status:** Complete
**Tests:** 30 tests passing
**Location:** `crates/hv2-agent/src/policies.rs`

Implemented comprehensive policy framework for agent control:
- Permission policies with allow/deny rules
- Time-based policies (scheduling windows)
- Resource quotas (VM, CPU, memory, storage)
- Rate limiting specifications
- Policy conditions with combinators (AND/OR/NOT)
- Policy engine for evaluation

**Components:**
| Component       | Description                       |
| --------------- | --------------------------------- |
| PolicyEffect    | Allow/Deny decision               |
| PolicyAction    | VM/Resource/Snapshot/Network ops  |
| ResourceId      | Resource targeting with wildcards |
| TimeWindow      | Business hours/off-hours windows  |
| PolicyCondition | Conditional policy evaluation     |
| PolicyRule      | Individual allow/deny rule        |
| PolicySet       | Collection of rules               |
| QuotaSpec       | Resource quota limits             |
| AgentPolicy     | Complete policy configuration     |
| PolicyEngine    | Multi-policy evaluation engine    |

---

### ✅ Phase 38: Agent Communication (28 tests)
**Status:** Complete
**Tests:** 28 tests passing
**Location:** `crates/hv2-agent/src/communication.rs`

Implemented inter-agent messaging and coordination:
- Message passing with priority queuing
- Request/response patterns with correlation
- Publish/subscribe channels
- Agent discovery and registration
- Message payloads (JSON, text, binary)
- Message TTL and expiration
- Thread-safe shared router

**Components:**
| Component       | Description                     |
| --------------- | ------------------------------- |
| Message         | Agent-to-agent message          |
| MessagePayload  | JSON/text/binary payload        |
| MessagePriority | Low/Normal/High/Critical        |
| MessageType     | Info/Request/Response/Broadcast |
| AgentInfo       | Agent registration data         |
| MessageQueue    | Priority-sorted message queue   |
| Channel         | Pub/sub channel with history    |
| MessageRouter   | Central message routing         |
| SharedRouter    | Thread-safe router wrapper      |

---

### ✅ Phase 39: Agent State (25 tests)
**Status:** Complete
**Tests:** 25 tests passing
**Location:** `crates/hv2-agent/src/state.rs`

Implemented persistent state management for AI agents:
- Key-value state storage with versioning
- State history tracking
- Optimistic locking (version checking)
- State checkpoints and restore
- State synchronization between stores
- TTL and expiration support
- Thread-safe shared store

**Components:**
| Component         | Description                      |
| ----------------- | -------------------------------- |
| StateValue        | Versioned value with metadata    |
| StateStore        | Key-value store with history     |
| StateCheckpoint   | Point-in-time state snapshot     |
| CheckpointManager | Checkpoint create/restore/delete |
| SharedStateStore  | Thread-safe store wrapper        |
| StateSynchronizer | Sync between stores              |
| ConflictStrategy  | LocalWins/RemoteWins/NewerWins   |
| SyncResult        | Sync operation statistics        |

---

### ✅ Phase 40: Agent Tasks (24 tests)
**Status:** Complete
**Tests:** 24 tests passing
**Location:** `crates/hv2-agent/src/tasks.rs`

Implemented task scheduling and workflow orchestration for AI agents:
- Task lifecycle management (pending, ready, running, completed, failed)
- Task priority scheduling (low, normal, high, critical)
- Task dependencies with automatic blocking/unblocking
- Retry policies with exponential backoff and jitter
- Timeout support for task execution limits
- Workflow DAG execution with topological ordering
- Cycle detection in task dependency graphs
- Task cancellation and status tracking
- Execution statistics (completed, failed, retried counts)
- Thread-safe task scheduler

**Components:**
| Component        | Description                                                       |
| ---------------- | ----------------------------------------------------------------- |
| Task             | Task with dependencies & priority                                 |
| TaskStatus       | Pending/Ready/Running/Completed/Failed/Cancelled/Retrying/Blocked |
| TaskPriority     | Low/Normal/High/Critical                                          |
| TaskOutput       | Success data or error message                                     |
| RetryPolicy      | Max retries, backoff, jitter                                      |
| TaskQueue        | Priority-sorted queue with deps                                   |
| Workflow         | DAG of tasks with validation                                      |
| WorkflowStatus   | Pending/Running/Completed/Failed/Cancelled                        |
| WorkflowExecutor | Execute workflow in order                                         |
| TaskScheduler    | Thread-safe task scheduler                                        |
| ExecutionStats   | Task execution statistics                                         |

---

### ✅ Phase 41: Agent Reasoning (27 tests)
**Status:** Complete
**Tests:** 27 tests passing
**Location:** `crates/hv2-agent/src/reasoning.rs`

Implemented reasoning and decision-making capabilities for AI agents:
- Knowledge base with triple facts (subject-predicate-object)
- Truth values (True/False/Unknown/Probable) with confidence
- Fact indexing by subject/predicate/object for fast queries
- Rule-based inference engine with forward chaining
- Pattern matching with variable bindings
- Goal-driven planning with backward chaining
- Decision trees for conditional decision making
- BDI (Belief-Desire-Intention) architecture
- Thread-safe shared reasoner

**Components:**
| Component       | Description                        |
| --------------- | ---------------------------------- |
| TruthValue      | True/False/Unknown/Probable        |
| Fact            | Subject-predicate-object triple    |
| FactSource      | Asserted/Derived/Observed/External |
| FactPattern     | Pattern with wildcards & variables |
| KnowledgeBase   | Indexed fact storage               |
| Rule            | Conditions → Conclusions           |
| InferenceEngine | Forward chaining inference         |
| Goal            | Desired postconditions             |
| Action          | Preconditions + Effects            |
| Planner         | Goal-to-action planning            |
| DecisionTree    | Condition-based decisions          |
| Belief          | BDI belief with confidence         |
| Desire          | BDI desire with priority           |
| Intention       | BDI intention with plan            |
| BdiAgent        | Complete BDI agent                 |
| SharedReasoner  | Thread-safe reasoner wrapper       |

---

### ✅ Phase 42: Agent Memory (29 tests)
**Status:** Complete
**Tests:** 29 tests passing
**Location:** `crates/hv2-agent/src/memory.rs`

Implemented memory system for AI agents with multiple memory types:
- Episodic memory for experiences and events
- Semantic memory for facts and knowledge (triple storage)
- Working memory for current context (token-limited)
- Memory retrieval with relevance scoring
- Forgetting curve with strength decay
- Memory consolidation (automatic pruning)
- Context window management with eviction
- Pinned items (never evicted from working memory)
- Thread-safe shared memory system

**Components:**
| Component       | Description                          |
| --------------- | ------------------------------------ |
| MemoryType      | Episodic/Semantic/Procedural/Working |
| Importance      | Low/Normal/High/Critical             |
| Episode         | Episodic memory with strength decay  |
| SemanticFact    | Triple-based semantic memory         |
| WorkingItem     | Context window item with tokens      |
| EpisodicMemory  | Episode storage with search          |
| SemanticMemory  | Fact storage with triple indexes     |
| WorkingMemory   | Token-limited context window         |
| RetrievalResult | Query result with relevance score    |
| RetrievalConfig | Query configuration                  |
| MemorySystem    | Unified memory coordinator           |
| SharedMemory    | Thread-safe memory wrapper           |

---

### ✅ Phase 43: Agent Tools (23 tests)
**Status:** Complete
**Tests:** 23 tests passing
**Location:** `crates/hv2-agent/src/tools.rs`

Implemented tool-use framework for AI agents:
- Tool registration and discovery by name/category
- Tool definitions with parameter schemas
- Parameter validation (type, required, enum constraints)
- Tool execution with result handling
- Tool call history and auditing
- Tool enable/disable for safety
- Tool chaining with argument mapping
- JSON-based argument passing
- Thread-safe shared tool registry

**Components:**
| Component          | Description                        |
| ------------------ | ---------------------------------- |
| ToolCategory       | FileSystem/Network/Code/API/Custom |
| ParameterType      | String/Integer/Float/Boolean/Array |
| ToolParameter      | Parameter schema with validation   |
| ToolDefinition     | Tool metadata and parameters       |
| ToolCall           | Tool invocation request            |
| ToolCallResult     | Execution result with duration     |
| RegisteredTool     | Tool definition + handler function |
| ToolRegistry       | Tool storage and execution         |
| ToolChain          | Sequential tool execution          |
| ToolChainStep      | Step with argument mapping         |
| ArgSource          | Literal/Input/StepOutput/StepField |
| SharedToolRegistry | Thread-safe registry wrapper       |

---

### ✅ Phase 44: Agent Perception (27 tests)
**Status:** Complete
**Tests:** 27 tests passing
**Location:** `crates/hv2-agent/src/perception.rs`

Implemented perception system for AI agents to observe environments:
- Sensor abstraction for different input types
- Observable definitions with type/unit metadata
- Multi-value observation types (Boolean/Integer/Float/String/Binary/Structured)
- Observation quality with confidence and accuracy
- Perception filtering by sensor type, prefix, tags
- World model for current state and history
- Change detection between time points
- Sensor enable/disable for control
- Thread-safe shared perception system

**Components:**
| Component          | Description                            |
| ------------------ | -------------------------------------- |
| SensorType         | Resource/Network/Event/Security/Custom |
| ObservationValue   | Boolean/Integer/Float/String/Binary... |
| ObservationQuality | Confidence, accuracy, freshness        |
| Observation        | Named value with quality and tags      |
| Observable         | Definition with type/unit/interval     |
| SensorConfig       | Poll interval, buffer size, settings   |
| SensorDefinition   | Sensor metadata and observables        |
| Sensor             | Definition + handler with buffer       |
| PerceptionFilter   | Filter by type/prefix/tags/quality     |
| WorldModel         | Current state + history                |
| PerceptionSystem   | Sensors, subscriptions, world model    |
| SharedPerception   | Thread-safe perception wrapper         |

---

### ✅ Phase 45: Agent Learning (30 tests)
**Status:** Complete
**Tests:** 30 tests passing
**Location:** `crates/hv2-agent/src/learning.rs`

Implemented learning and adaptation system for AI agents:
- Reward signals for reinforcement learning
- State/action representations (discrete, continuous, categorical)
- Experience tuples (s, a, r, s') with metadata
- Experience replay buffer with circular storage
- Skill acquisition with proficiency tracking
- Skill levels (Novice to Expert) with decay
- Learning rate schedules (constant, linear, exponential, step)
- Learning statistics and episode tracking
- Thread-safe shared learning system

**Components:**
| Component            | Description                        |
| -------------------- | ---------------------------------- |
| Reward               | Reward signal with discount factor |
| LearningState        | Discrete/Continuous/Categorical    |
| ActionValue          | Discrete/Continuous/Named/Param    |
| Experience           | (s, a, r, s') tuple with metadata  |
| ExperienceBuffer     | Circular replay buffer             |
| SkillLevel           | Novice/Beginner/.../Expert         |
| Skill                | Learnable skill with proficiency   |
| LearningRateSchedule | Constant/Linear/Exponential/Step   |
| LearningConfig       | Learning hyperparameters           |
| LearningStats        | Episode/reward statistics          |
| LearningSystem       | Unified learning coordinator       |
| SharedLearning       | Thread-safe learning wrapper       |

---

### ✅ Phase 46: Agent Planning (28 tests)
**Status:** Complete
**Tests:** 28 tests passing
**Location:** `crates/hv2-agent/src/planning.rs`

Implemented hierarchical task planning and goal management for AI agents:
- Priority levels (Critical/High/Normal/Low/Background)
- Conditions with comparison operators (Equals, GreaterThan, etc.)
- Effects for modifying world state (Set/Remove/Increment/Decrement)
- Goals with success conditions and deadlines
- Plan actions with preconditions and effects
- Plan steps with execution tracking
- Multiple planning algorithms (Forward/Backward/GreedyBestFirst/A*)
- Goal manager with hierarchy support
- Unified planning system with plan execution
- Thread-safe shared planning wrapper

**Components:**
| Component         | Description                         |
| ----------------- | ----------------------------------- |
| Priority          | Goal/action priority levels         |
| Condition         | State condition with operator       |
| ConditionOperator | Comparison operators for conditions |
| Effect            | World state modification            |
| EffectOperation   | Set/Remove/Increment/Decrement      |
| Goal              | Goal with conditions and deadline   |
| GoalStatus        | Pending/Active/Achieved/Failed      |
| PlanAction        | Action with preconditions/effects   |
| PlanStep          | Single step in a plan               |
| Plan              | Sequence of steps for a goal        |
| PlanStatus        | Building/Ready/Executing/Completed  |
| PlanningAlgorithm | Forward/Backward/Greedy/A*          |
| PlanningConfig    | Planning parameters                 |
| Planner           | Plan generation engine              |
| GoalManager       | Goal tracking with hierarchy        |
| PlanningSystem    | Unified planning coordinator        |
| SharedPlanning    | Thread-safe planning wrapper        |

---

### ✅ Phase 47: KVM FFI Expansion & Exit Handling

**Status:** Complete
**Tests:** 2,568 passing (0 failures)
**Location:** crates/hv2-core/src/backends/kvm_ffi.rs, crates/hv2-core/src/backends/kvm.rs, crates/hv2-core/src/exit.rs

Massively expanded KVM FFI bindings based on Linux 6.18 reference source, added new VM exit types, and improved KVM backend robustness:

**kvm_ffi.rs** (646 → 1,518 lines):
- System ioctls: +KVM_GET_MSR_INDEX_LIST, KVM_GET_SUPPORTED_CPUID
- VM ioctls: +KVM_GET_DIRTY_LOG, KVM_SET_IDENTITY_MAP_ADDR, KVM_IRQ_LINE, KVM_GET/SET_IRQCHIP, KVM_SET_GSI_ROUTING, KVM_SIGNAL_MSI
- vCPU ioctls: +KVM_TRANSLATE, KVM_GET/SET_MSRS, KVM_SET_SIGNAL_MASK, KVM_GET/SET_FPU, KVM_GET/SET_MP_STATE, KVM_NMI, KVM_SET_GUEST_DEBUG, KVM_GET/SET_VCPU_EVENTS, KVM_GET/SET_DEBUGREGS, KVM_GET/SET_XSAVE, KVM_GET/SET_XCRS
- Capabilities: 8 → 24 constants (NR_VCPUS, MP_STATE, SYNC_MMU, VCPU_EVENTS, DEBUGREGS, XSAVE, XCRS, TSC_CONTROL, MAX_VCPUS, READONLY_MEM, SPLIT_IRQCHIP, MAX_VCPU_ID, X2APIC_API, MSI_DEVID, IMMEDIATE_EXIT, NESTED_STATE)
- Exit reasons: 13 → 24 constants (SET_TPR, TPR_ACCESS, NMI, IOAPIC_EOI, HYPERV, X86_RDMSR, X86_WRMSR, DIRTY_RING_FULL, X86_BUS_LOCK, NOTIFY, MEMORY_FAULT)
- New constant sections: MP states (8), IRQ routing types (3), chip IDs (3), guest debug flags (6), MSI flags (1)
- 20+ new struct definitions (kvm_msr_entry, kvm_msrs, kvm_fpu, kvm_mp_state, kvm_irq_level, kvm_dirty_log, kvm_lapic_state, kvm_irqchip, kvm_msi, kvm_translation, kvm_vcpu_events, kvm_guest_debug, kvm_xsave, kvm_xcrs, kvm_debugregs, kvm_irq_routing, kvm_signal_mask, etc.)
- 25+ new unsafe wrapper functions with SAFETY documentation

**kvm.rs** improvements:
- EINTR retry loop in 
un() for signal-safe vCPU execution
- Runtime capability detection via KVM_CAP_MAX_VCPUS (replaces hardcoded 288)
- Magic numbers replaced: 85 → KVM_CAP_NESTED_STATE, 117 → KVM_CAP_X2APIC_API
- 6 new convert_exit() match arms: NMI, IOAPIC_EOI, RDMSR, WRMSR, SystemEvent, Hypercall

**exit.rs** new VmExit variants:
- Hypercall { nr, args } — VMCALL/VMMCALL instruction
- SystemEvent { type_, flags } — reset, shutdown, crash events
- Nmi — non-maskable interrupt
- Rdmsr { index } — MSR read interception
- Wrmsr { index, data } — MSR write interception
- IoapicEoi { vector } — IOAPIC end-of-interrupt

**Downstream match updates:** vm.rs (2 match sites), whpx.rs, exit_handler.rs, exit_handling.rs example, guest_execution.rs test

### Phase 48: KVM Backend Methods

**Status:** Complete
**Tests:** 2,568+ passing (0 failures)
**Location:** crates/hv2-core/src/backends/kvm.rs, crates/hv2-core/src/backends/kvm_ffi.rs

Added high-level safe wrapper methods to KvmVcpu and KvmVm, consuming the 28 FFI bindings added in Phase 47. The KVM backend now exposes full vCPU state management, interrupt control, and memory operations through safe Rust APIs.

**kvm.rs** (745 -> 1,260 lines):

*KvmVcpu new methods (24):*
- Register state: get_regs/set_regs, get_sregs/set_sregs
- FPU state: get_fpu/set_fpu
- Extended state: get_xsave/set_xsave, get_xcrs/set_xcrs
- Debug registers: get_debugregs/set_debugregs
- LAPIC: get_lapic/set_lapic
- MSRs: get_msrs/set_msrs (model-specific registers)
- Multiprocessor state: get_mp_state/set_mp_state
- vCPU events: get_vcpu_events/set_vcpu_events
- Guest debugging: set_guest_debug (single-step, breakpoints)
- NMI injection: inject_nmi
- Address translation: translate (GVA -> GPA)
- CPUID: set_cpuid (CPUID leaf configuration)

*KvmVm new methods (10):*
- IRQ management: irq_line, get_irqchip, set_irqchip
- MSI: signal_msi
- GSI routing: set_gsi_routing
- Identity map: set_identity_map_addr
- Dirty page tracking: get_dirty_log (for live migration)
- Memory mapping: map_memory (additional memory slots)
- Accessor: vm_fd

**kvm_ffi.rs** Default derives added:
- kvm_fpu, kvm_xsave, kvm_xcrs, kvm_lapic_state

### Phase 49: Backend Architecture Consolidation

**Status:** Complete
**Tests:** 2,568+ passing (0 failures)
**Location:** crates/hv2-core/src/hypervisor.rs, crates/hv2-core/src/backends/

Extracted inline WHPX and HVF backend modules from hypervisor.rs into dedicated files under backends/, completing the backend separation started with KVM.

**hypervisor.rs** (1,687 -> 309 lines):
- Retained: HypervisorPlatform enum, detect(), availability checks, HypervisorCapabilities, HypervisorBackend trait, HypervisorVm, TcgBackend, create_backend()
- Removed: Inline whpx and hvf modules (moved to backends/)
- create_backend() now references backends::kvm::KvmBackend, backends::whpx::WhpxBackend, backends::hvf::HvfBackend

**backends/hvf_ffi.rs** (new):
- 12 extern "C" FFI functions for macOS Hypervisor.framework
- HV_SUCCESS and error constants, HvX86Reg enum (22 variants), VmcsField enum (12 fields)
- Memory flags (HV_MEMORY_READ/WRITE/EXEC), HvVcpuId type

**backends/hvf.rs** (574 lines, new):
- HvfBackend struct with full HypervisorBackend trait impl
- VM lifecycle, vCPU management, memory mapping, interrupt/exception injection
- VMX exit parsing (exception, I/O, EPT, HLT), set_io_result, shutdown with cleanup

**backends/mod.rs** updated:
- Registers hvf_ffi + hvf + HvfBackend (macOS cfg)

### Phase 51: Backend VM Exit Handling Completeness

**Status:** Complete
**Tests:** All passing (0 failures)
**Location:** crates/hv2-core/src/backends/{whpx,kvm,hvf}.rs, crates/hv2-core/src/vcpu.rs

Filled in missing VM exit reason translations across all three hardware backends and added supporting infrastructure.

**WHPX (whpx.rs, 7027 -> 7060 lines)** -- 4 new exit arms in convert_exit():
- WHvRunVpExitReasonX64MsrAccess -> VmExit::Rdmsr / VmExit::Wrmsr (reads IsWrite, MsrNumber, Rax, Rdx)
- WHvRunVpExitReasonX64ApicEoi -> VmExit::IoapicEoi (reads InterruptVector)
- WHvRunVpExitReasonHypercall -> VmExit::Hypercall (placeholder nr=0, no exit data available)

**HVF (hvf.rs, 574 -> 628 lines)** -- named constants + 4 new exit handlers:
- Replaced 7 magic numbers with 11 VMX_EXIT_REASON_* named constants
- VMX_EXIT_REASON_TRIPLE_FAULT -> VmExit::Shutdown
- VMX_EXIT_REASON_VMCALL -> VmExit::Hypercall (reads RAX for nr)
- VMX_EXIT_REASON_RDMSR -> VmExit::Rdmsr (reads RCX for index)
- VMX_EXIT_REASON_WRMSR -> VmExit::Wrmsr (reads RCX, RAX, RDX via hv_vcpu_read_register)

**KVM (kvm.rs, 1260 -> 1294 lines)** -- debug exit + CPUID wrapper:
- KVM_EXIT_DEBUG -> VmExit::Debug with vCPU id in info string
- New KvmBackend::get_supported_cpuid() method: wraps kvm_get_supported_cpuid FFI, uses buffer-based layout to handle kvm_cpuid2 flexible array member, returns Vec<kvm_cpuid_entry2>

**VCpu::run() (vcpu.rs, 791 -> 793 lines)**:
- Added tracing::warn!() to placeholder run() method directing users to HypervisorBackend::run_vcpu()


### Phase 52: Memory Safety Hardening & Stub Elimination

**Status:** Complete
**Tests:** All passing (2,568 passed, 0 failed, 26 ignored)
**Location:** crates/hv2-core/src/{memory,vcpu,hypervisor,lib,descriptors}.rs + 19 files import cleanup

Hardened memory safety, replaced dangerous stubs with proper errors, tightened warning suppressions, and cleaned up all unused imports across the workspace.

**memory.rs (188 -> 258 lines)** -- bounds-checked guest memory access:
- Added `translate_range(guest_addr, len)` method validating full `[start, start+len)` range within a single memory region, with `checked_add` overflow protection
- Wired `write_bytes()`, `read_bytes()`, `read_bytes_into()` to use `translate_range()` instead of `translate()` (which only checked start address)
- Added 3 new tests: `test_write_bytes_bounds_check`, `test_read_bytes_bounds_check`, `test_translate_range_zero_length`

**vcpu.rs (794 -> 783 lines)** -- stub elimination:
- `VCpu::run()` now returns `Err(Error::Cpu(...))` instead of placeholder `Ok(VCpuExit::Hlt)`
- Added doc comment directing to `HypervisorBackend::run_vcpu()`

**hypervisor.rs (358 -> 359 lines)** -- stub hardening:
- `HypervisorVm::map_memory()` now returns `Err(Error::VM(...))` instead of silent `Ok(())`
- TCG `init()` changed from `info!` to `warn!` with "non-functional fallback" message

**lib.rs (340 -> 337 lines)** -- warning suppression cleanup:
- Removed `#![allow(unused_imports)]` and `#![allow(unused_mut)]`
- Kept `dead_code` (hardware register forward-declarations) and `unused_variables` (callback params)

**descriptors.rs** -- added SAFETY comment on `transmute` for `InterruptDescriptor32` -> `[u8; 8]`

**Workspace-wide import cleanup (19 files)**:
- `cargo fix` auto-removed ~72 unused imports across the workspace
- Restored 8 imports wrongly removed from test/cfg-gated code using `#[cfg(test)]` guards
- Removed 6 genuinely unused imports (debug/gdb.rs, migration/state.rs, snapshot/{device,manager,memory}.rs)
- Fixed 3 test-only unused imports (whpx.rs, device_manager.rs, mmio.rs)
- Removed 2 unnecessary `mut` bindings (display_backend.rs, allocator.rs)

### Phase 56: Lock Poisoning Resilience Phase 3 -- Container, Device, Network, Debug & Security Hardening

**Status:** Complete
**Tests:** All passing (2,576+ tests, 0 failures)
**Location:** 20 files across hv2-core, hv2-agent

Final sweep of production lock unwrap sites, replacing all remaining `.lock().unwrap()`, `.read().unwrap()`, and `.write().unwrap()` calls in container, device, networking, debug, and security subsystems with `.unwrap_or_else(|e| e.into_inner())` for lock-poisoning resilience.

**hv2-core container layer (2 files, 77 sites):**
- `cgroup.rs`: 26 lock unwraps hardened (cpu, cpuset, memory, io, pids, devices, freezer, procs)
- `namespace.rs`: 51 lock unwraps hardened (processes, interfaces, routes, rules, mounts, hostname, domainname, IPC, user/pid/net/mnt/uts/ipc namespaces)

**hv2-core device layer (7 files, 73 sites):**
- `ide.rs`: 18 lock unwraps hardened (primary, secondary channel Mutex)
- `nvme.rs`: 21 lock unwraps hardened (admin_sq, admin_cq, io_sq, io_cq, storage)
- `msi.rs`: 10 lock unwraps hardened (table, pending, callback)
- `keyboard.rs`: 8 lock unwraps hardened (state Mutex)
- `rtc.rs`: 8 lock unwraps hardened (state Mutex)
- `disk_image.rs`: 6 lock unwraps hardened (file, data)
- `vga.rs`: 2 remaining lock unwraps hardened

**hv2-core networking layer (2 files, 54 sites):**
- `vswitch.rs`: 32 lock unwraps hardened (ports, mac_table, stats, mirror_sources, mirror_dest, stp)
- `sriov.rs`: 22 lock unwraps hardened (vfs, pfs, assignments)

**hv2-core debug layer (3 files, 22 sites):**
- `gdb.rs`: 9 lock unwraps hardened (breakpoints, parser)
- `introspection.rs`: 9 lock unwraps hardened (regions, states, events)
- `debug/mod.rs`: 4 lock unwraps hardened (memory_inspector)

**hv2-core security layer (3 files, 15 sites):**
- `memory_encryption.rs`: 12 lock unwraps hardened (keys, page_states)
- `secure_boot.rs`: 2 lock unwraps hardened
- `vtpm.rs`: 1 lock unwrap hardened

**hv2-agent remaining (2 files, 2 sites):**
- `actions.rs`: 1 lock unwrap hardened
- `reasoning.rs`: 1 lock unwrap hardened

### Phase 55: Lock Poisoning Resilience Phase 2 -- Device & Agent Hardening

**Status:** Complete
**Tests:** All passing (2,576+ tests, 0 failures)
**Location:** 15 files across hv2-core, hv2-agent

Continued the lock poisoning resilience campaign from Phase 54, hardening all remaining production `.lock().unwrap()` / `.read().unwrap()` / `.write().unwrap()` calls with `.unwrap_or_else(|e| e.into_inner())`. Also fixed NaN-panic risk in floating-point comparisons.

**hv2-core device layer (4 files, 68 sites):**
- `interrupt.rs`: 32 lock unwraps hardened (master, slave PIC Mutex)
- `vga.rs`: 15 lock unwraps hardened (state Mutex)
- `e1000.rs`: 20 lock unwraps hardened (rx_queue, tx_queue, rx/tx_ring_base, mta)
- `virtio.rs`: 1 lock unwrap hardened (inner Mutex)

**hv2-core infrastructure (2 files, 37 sites):**
- `protocol.rs`: 28 lock unwraps hardened (stage, stats, start_time, precopy_start, pending_pages, dirty_tracker, outgoing, incoming)
- `kvm.rs`: 9 lock unwraps hardened (vms, vcpu_map, vcpus)

**hv2-agent shared wrappers (7 files, 67 sites):**
- `orchestration.rs`: 28 lock unwraps hardened (agents, vm_claims, channels, agent_messages)
- `mcp.rs`: 13 lock unwraps hardened (tools, sessions, audit_log, state, session fields)
- `memory.rs`: 8 lock unwraps hardened (inner RwLock)
- `perception.rs`: 5 lock unwraps hardened (inner RwLock)
- `reasoning.rs`: 4 lock unwraps hardened (inner RwLock)
- `tasks.rs`: 5 lock unwraps hardened (inner Mutex)
- `limits.rs`: 4 lock unwraps hardened (timestamps, last_refill)

**NaN-panic prevention (2 files, 4 sites):**
- `planning.rs`: 2x `partial_cmp().unwrap()` -> `partial_cmp().unwrap_or(Ordering::Equal)`
- `memory.rs`: 2x same pattern (relevance sort in episodic/memory retrieval, 1 in production memory.rs)

### Phase 54: Lock Poisoning Resilience & Unwrap Elimination

**Status:** Complete
**Tests:** All passing (2,576+ tests, 0 failures)
**Location:** 10 files across hv2-core, hv2-agent, hm-cli

Eliminated ~85+ potential panic sites from production code by replacing `.lock().unwrap()` / `.read().unwrap()` / `.write().unwrap()` with poison-resilient `.unwrap_or_else(|e| e.into_inner())`. This prevents cascading panics when a thread panics while holding a lock.

**Lock poisoning resilience (7 files, ~70+ sites):**
- `vtpm.rs`: 28 lock unwraps hardened (state, pcr_banks, nv_storage, keys)
- `secure_boot.rs`: 26 lock unwraps hardened (policy, platform_key, keks, db, dbx, hash lists)
- `net_backend.rs`: 27 lock unwraps hardened (stats, queue, rx_queue, connected)
- `virtio_blk.rs`: 8 lock unwraps hardened (config, storage)
- `actions.rs`: 6 lock unwraps hardened (pending, in_progress)
- `communication.rs`: 8 lock unwraps hardened (inner)
- `learning.rs`: 8 lock unwraps hardened (inner)

**serde_json unwrap elimination (2 files, 11 sites):**
- `schema.rs`: 7x `serde_json::to_value().unwrap()` -> `.expect("schema serialization failed")`
- `adapters.rs`: 4x same pattern

**Control flow hardening (1 file, 3 sites):**
- `t1_manager.rs`: Refactored `is_none()` + `.unwrap()` to `let Some(conn) = ... else { bail!() }` in `start_vm`, `stop_vm`, `execute_script`

**Unsafe code audit (1 file, 1 site):**
- `virtio_blk.rs`: Added `// SAFETY:` comment explaining `#[repr(C)]` struct read as bytes

**Panic elimination (1 file, 1 site):**
- `runtime_services.rs`: Replaced `panic!("System reset requested")` with `eprintln!` + `std::process::abort()` to avoid stack unwinding

### Phase 53: Cryptographic Implementation Hardening

**Status:** Complete
**Tests:** All passing (1,868+ lib tests, 13 integration tests, 0 failed)
**Location:** crates/hv2-core/src/crypto/{fips,asymmetric,pqc}.rs + tests/crypto_integration.rs

Replaced all placeholder/insecure crypto implementations with `Err(CryptoError::NotImplemented(...))` so callers cannot silently rely on broken security. Added `NotImplemented` variant to `CryptoError` enum. Updated all unit and integration tests to use `#[cfg(feature = "ring")]` gating.

**fips.rs -- symmetric crypto hardening (9 changes):**
- Added `NotImplemented(String)` variant to `CryptoError` enum
- Replaced 8 `#[cfg(not(feature = "ring"))]` XOR/placeholder fallbacks with `NotImplemented` errors: `aes_gcm_encrypt_internal`, `aes_gcm_decrypt_internal`, `sha256`, `sha384`, `sha512`, `hmac_sha256`, `hmac_sha512`, `hkdf_sha256`
- Updated 5 unit tests with `#[cfg(not(feature = "ring"))]` / `#[cfg(feature = "ring")]` gating

**asymmetric.rs -- RSA/ECDSA hardening (8 changes):**
- Replaced `generate_rsa_keypair` (was random bytes, no primes), `rsa_encrypt`/`rsa_decrypt` (was XOR masking), `rsa_sign` (was XOR with private key), `rsa_verify` (was length check only)
- Replaced `generate_ecdsa_keypair` (was hash-derived point), `ecdsa_sign` (was HMAC-based), `ecdsa_verify` (was unconditional `Ok(true)`)
- Updated 7 unit tests + changed `get_crypto()` to use `FipsMode::Disabled` (avoids self-test failures without `ring`)

**pqc.rs -- post-quantum crypto hardening (2 changes):**
- Replaced `ml_dsa_verify` and `slh_dsa_verify` (were unconditional `Ok(true)`) with `NotImplemented` errors
- ML-KEM keygen still works (uses only RNG), but encaps/decaps now fail without `ring` (use SHA-256 internally)
- ML-DSA/SLH-DSA keygen also fail without `ring` (use HKDF/SHA internally)
- Updated 6 unit tests with proper expectations per operation

**crypto_integration.rs -- full rewrite:**
- Rewrote all 13 integration tests with `get_crypto()` helper using `FipsMode::Disabled`
- All crypto operations properly gated with `#[cfg(feature = "ring")]`

### Phase 50: Wire WHPX Backend Trait to Real Implementations

**Status:** Complete
**Tests:** 2,568+ passing (0 failures)
**Location:** crates/hv2-core/src/backends/whpx.rs

Wired WhpxBackend HypervisorBackend trait methods from stubs to real WhpxVcpu implementations. Previously, run_vcpu() returned hardcoded VmExit::Hlt and inject_interrupt() was a no-op log. Now all trait methods delegate to the real WHPX virtual processor API.

**WhpxBackend struct**:
- Added vcpu_map: Arc<RwLock<HashMap<u32, Arc<WhpxVcpu>>>> field for trait-to-vcpu delegation

**create_vm()**: Now creates vCPUs via WhpxVm::create_vcpu() and populates vcpu_map

**Wired trait methods (4)**:
- run_vcpu() -> WhpxVcpu::run() (real WHvRunVirtualProcessor + exit conversion)
- inject_interrupt() -> WhpxVcpu::inject_interrupt() (WHV_X64_PENDING_INTERRUPTION_REGISTER)
- inject_exception() override -> WhpxVcpu::inject_exception() (with error code support)
- set_io_result() override -> WhpxVcpu::set_rax() (with access-size masking)

**shutdown()**: Now clears vcpu_map before clearing VMs

**whpx.rs** (6,302 -> 6,353 lines)

### Phase 57: Dependency Modernization & Tooling Fixes

**Status:** Complete

**Dependency upgrades (Rust 1.93 compatibility):**
- `tonic` 0.12 → 0.14, `tonic-build` 0.12 → 0.14
- Added `tonic-prost` 0.14 and `tonic-prost-build` 0.14 (tonic 0.14 split prost codegen)
- `prost` 0.13 → 0.14
- `tower` 0.4 → 0.5 (0.4.13 `ready-cache` broke with Rust 1.93 type inference changes)
- `tower-http` 0.5 → 0.6 (hm-cli)
- `winapi` 0.3 → `windows-sys` 0.59 (hv2-net TAP device code)

**Tooling & CI fixes:**
- `clippy.toml`: Added `msrv = "1.87"` to prevent suggesting post-MSRV APIs
- `deny.toml`: Migrated to cargo-deny 0.19 format (removed deprecated `vulnerability`, `unmaintained`, `unlicensed`, `copyleft` fields; fixed `sources.allow-registry`/`allow-git` to array format; added `OFL-1.1`, `Ubuntu-font-1.0`, `NCSA`, `CDLA-Permissive-2.0` licenses; updated advisory ignores)
- `bench.yml`: Fixed `tool: 'customSmallerIsBetter'` → `tool: 'cargo'` for Criterion output parsing
- `coverage.yml`: Updated `codecov/codecov-action@v4` → `@v5`
- `ci.yml`: Added `test-core-windows` job for WHPX backend tests on Windows
- `hv1-core/Cargo.toml`: Added missing `homepage.workspace`, `rust-version.workspace`, `[lints] workspace = true`
- `hv1-boot/Cargo.toml`: Added missing `homepage.workspace`, `[lints] workspace = true`
- `CONTRIBUTING.md`: Updated MSRV from 1.75 to 1.87
- `LICENSE-COMMERCIAL`: Fixed stale "OCL v1" reference → "AGPL-3.0"

---

### Phase 58: Security Hardening & Code Quality

**Status:** Complete

**Security:**
- **Deleted `_run_b64.py` backdoor** — suspicious Python script in `hv2-core/src/` that decoded and exec'd base64 content
- Removed unused `crossbeam` dependency from `hv2-core`

**Tracing module (feature-gated):**
- Wired orphaned `tracing/` directory (~2,400 lines) into `hv2-core` as `hv_tracing` module (via `#[path]` to avoid name collision with `tracing` crate)
- Fixed unclosed delimiter in `exporters.rs`
- Feature-gated behind `hv-tracing` — module has 24 pre-existing compilation errors to fix in Phase 59

**SAFETY comments (11 sites):**
- `acpi.rs`: 2 `slice::from_raw_parts` calls in `calculate_checksum`/`calculate_extended_checksum`
- `arch.rs` (hv1-core): 7 inline asm blocks (`read_rip`, `read_rsp`, `read_rflags`, `hlt`, `cli`, `sti`, `pause`)
- `main.rs` (hv1-boot): 2 unsafe blocks (`init_global_serial`, `ALLOCATOR.lock().init`)

**Tests (12 new):**
- `config.rs`: 4 tests — default values, TOML roundtrip, invalid TOML, nonexistent file
- `hypervisor.rs`: 8 tests — Display variants, detect(), TCG backend defaults, VM creation, capabilities, equality

**Project config:**
- Added `.github/FUNDING.yml` (GitHub Sponsors)
- `.editorconfig`: Added `*.proto` (indent_size = 2) and `Containerfile` (indent_size = 4) entries

---

### Phase 59: Tracing Module Compilation Fixes

**Status:** Complete

**Fixed 14 compilation errors + warnings in the orphaned `hv_tracing` module (~2,400 lines, 5 files):**

- **Unused imports** (3): removed `Instant` from types.rs, `HashMap` from tracer.rs, `Arc` from profiler.rs
- **Missing `TraceId::as_u128()`** (3 call sites): added method composing high/low u64 → u128
- **`as_bytes()` → `to_bytes()`** (4 call sites): renamed to match actual method signatures on `TraceId`/`SpanId`
- **Debug impls** (2): replaced `#[derive(Debug)]` with manual impls for `ParentBasedSampler` and `SimpleSpanProcessor` (fields contain `Box<dyn Sampler>` / `Arc<dyn SpanExporter>`)
- **`Span::end()` move-after-drop** (1): changed `self.data` → `self.data.clone()` in `end_with_timestamp()`, removed `mut` from `end(self)`
- **Unclosed delimiter** (1, from Phase 58): missing `}` closing `impl JaegerSpanExporter`
- **Clippy issues** (3): `#[derive(Default)]` + `#[default]` for `SpanKind`/`StatusCode`, `const {}` for `thread_local!` initializer
- **Test fixes** (5): `resource` type mismatch (`vec![]` → `HashMap::new()`), `instrumentation_version` (`Option<String>` → `String`), `TraceState::get` return type (`&String` → `&str`), `from_header` return type (not `Result`), `ConsoleSpanExporter` lifetime issue, approx PI constant

**Result:** Removed `hv-tracing` feature gate — module now compiles unconditionally.

### Phase 60: Safety, Hardening & CI Improvements

**Status:** Complete

**Safety documentation:**
- Added `// SAFETY:` comments to 4 KVM backend `unsafe` blocks: `KvmVm::new()`, `KvmVcpu::new()`, `KvmVcpu::run()`, `KvmVcpu::inject_interrupt()`

**Code hardening:**
- Replaced 6 production `try_into().unwrap()` calls with `.expect()` in `framebuffer.rs` (3), `virtio_gpu.rs` (1), `gdb.rs` (2)
- Added `panic = "abort"` to `[profile.release]` — eliminates unwinding tables, appropriate for hypervisor code

**Metadata & config fixes:**
- Added `documentation = "https://docs.rs/hypermachine"` to `[workspace.package]`
- Added missing `rust-version.workspace = true` to `hv1-boot/Cargo.toml`
- Relaxed `wasmtime`/`wasmtime-wasi` from exact `"24.0.5"` pin to semver `"24.0"`
- Removed stale `{ name = "quote" }` and `{ name = "proc-macro2" }` skips from `deny.toml`

**CI improvements:**
- Added `test-core-macos` job to `ci.yml` — runs `hv2-core` tests on `macos-latest` (HVF backend)

---

## Test Summary

| Crate     | Test Count | Status        |
| --------- | ---------- | ------------- |
| hv2-core  | 1836       | ✅ All passing |
| hv2-agent | 387        | ✅ All passing |
| hv2-cpu   | 3          | ✅ All passing |
| Total     | 2226       | ✅ All passing |

---

## Key Files

| Path                                                | Description                     |
| --------------------------------------------------- | ------------------------------- |
| `crates/hv2-core/src/platform.rs`                   | Platform integration            |
| `crates/hv2-core/src/perf.rs`                       | Performance optimization        |
| `crates/hv2-core/src/acpi.rs`                       | ACPI table generation           |
| `crates/hv2-core/src/address_space.rs`              | Guest address space management  |
| `crates/hv2-core/src/boot/descriptor.rs`            | GDT/IDT/TSS descriptor tables   |
| `crates/hv2-core/src/boot/mode.rs`                  | CPU mode transitions            |
| `crates/hv2-core/src/boot/sector.rs`                | BIOS boot sector support        |
| `crates/hv2-core/src/boot/linux.rs`                 | Linux boot protocol             |
| `crates/hv2-core/src/boot/multiboot.rs`             | Multiboot specification         |
| `crates/hv2-core/src/cpuid.rs`                      | CPUID instruction emulation     |
| `crates/hv2-core/src/exit_handler.rs`               | VM exit handling framework      |
| `crates/hv2-core/src/interrupt.rs`                  | PIC 8259 implementation         |
| `crates/hv2-core/src/device_manager.rs`             | Unified device coordination     |
| `crates/hv2-core/src/devices/ioapic.rs`             | I/O APIC emulation              |
| `crates/hv2-core/src/devices/lapic.rs`              | Local APIC emulation            |
| `crates/hv2-core/src/devices/msi.rs`                | MSI/MSI-X interrupt support     |
| `crates/hv2-core/src/devices/nvme.rs`               | NVMe SSD controller             |
| `crates/hv2-core/src/devices/virtio_blk.rs`         | VirtIO block device             |
| `crates/hv2-core/src/devices/disk_image.rs`         | Disk image format support       |
| `crates/hv2-core/src/devices/ide.rs`                | IDE/ATA disk controller         |
| `crates/hv2-core/src/devices/virtio.rs`             | VirtIO network device           |
| `crates/hv2-core/src/devices/timer.rs`              | PIT timer emulation             |
| `crates/hv2-core/src/devices/keyboard.rs`           | PS/2 keyboard emulation         |
| `crates/hv2-core/src/devices/serial.rs`             | UART 16550 serial ports         |
| `crates/hv2-core/src/devices/rtc.rs`                | RTC/CMOS real-time clock        |
| `crates/hv2-core/src/devices/vga.rs`                | VGA text mode display           |
| `crates/hv2-core/src/migration/`                    | Live migration support          |
| `crates/hv2-core/src/security/memory_encryption.rs` | SEV/TDX memory encryption       |
| `crates/hv2-core/src/security/vtpm.rs`              | Virtual TPM 2.0 device          |
| `crates/hv2-core/src/security/secure_boot.rs`       | UEFI Secure Boot verification   |
| `crates/hv2-core/src/container/runtime.rs`          | OCI container runtime           |
| `crates/hv2-core/src/container/namespace.rs`        | Linux namespace isolation       |
| `crates/hv2-core/src/container/cgroup.rs`           | Cgroup resource controllers     |
| `crates/hv2-core/src/debug/gdb.rs`                  | GDB remote serial protocol      |
| `crates/hv2-core/src/debug/introspection.rs`        | VM introspection API            |
| `crates/hv2-core/src/networking/vswitch.rs`         | Virtual switch implementation   |
| `crates/hv2-core/src/networking/filter.rs`          | Network filtering & conntrack   |
| `crates/hv2-core/src/networking/sriov.rs`           | SR-IOV passthrough support      |
| `crates/hv2-core/src/usb/xhci.rs`                   | xHCI USB 3.0 host controller    |
| `crates/hv2-core/src/usb/device.rs`                 | USB device framework            |
| `crates/hv2-core/src/usb/hid.rs`                    | HID class devices (KB/mouse)    |
| `crates/hv2-core/src/gpu/core.rs`                   | GPU types and abstractions      |
| `crates/hv2-core/src/gpu/framebuffer.rs`            | Software framebuffer            |
| `crates/hv2-core/src/gpu/virtio_gpu.rs`             | VirtIO-GPU 2D device            |
| `crates/hv2-core/src/audio/core.rs`                 | Audio types and abstractions    |
| `crates/hv2-core/src/audio/ac97.rs`                 | AC97 audio controller           |
| `crates/hv2-core/src/snapshot/types.rs`             | Snapshot types and metadata     |
| `crates/hv2-core/src/snapshot/memory.rs`            | Memory snapshot management      |
| `crates/hv2-core/src/snapshot/device.rs`            | Device state serialization      |
| `crates/hv2-core/src/snapshot/manager.rs`           | Snapshot lifecycle management   |
| `crates/hv2-core/src/audio/hda.rs`                  | Intel HDA controller            |
| `crates/hv2-core/src/audio/virtio_sound.rs`         | VirtIO sound device             |
| `crates/hv2-core/src/input/ps2_keyboard.rs`         | PS/2 keyboard with scan codes   |
| `crates/hv2-core/src/input/ps2_mouse.rs`            | PS/2 mouse with scroll wheel    |
| `crates/hv2-core/src/input/touchscreen.rs`          | Multi-touch with gestures       |
| `crates/hv2-core/src/input/gamepad.rs`              | Game controller with rumble     |
| `crates/hv2-core/src/uefi/types.rs`                 | UEFI core types and GUIDs       |
| `crates/hv2-core/src/uefi/system_table.rs`          | EFI System Table & Boot Svcs    |
| `crates/hv2-core/src/uefi/gop.rs`                   | Graphics Output Protocol        |
| `crates/hv2-core/src/uefi/runtime_services.rs`      | EFI Runtime Services            |
| `crates/hv2-core/src/pci/types.rs`                  | PCI addressing and types        |
| `crates/hv2-core/src/pci/config.rs`                 | PCI configuration space         |
| `crates/hv2-core/src/pci/capabilities.rs`           | PCI/PCIe capabilities           |
| `crates/hv2-core/src/pci/bus.rs`                    | PCI bus topology                |
| `crates/hv2-core/src/iommu/types.rs`                | IOMMU core types                |
| `crates/hv2-core/src/iommu/vtd.rs`                  | Intel VT-d support              |
| `crates/hv2-core/src/iommu/amd.rs`                  | AMD IOMMU (AMD-Vi)              |
| `crates/hv2-core/src/iommu/interrupt_remap.rs`      | Interrupt remapping             |
| `crates/hv2-core/src/numa/types.rs`                 | NUMA core types                 |
| `crates/hv2-core/src/numa/topology.rs`              | NUMA topology management        |
| `crates/hv2-core/src/numa/acpi.rs`                  | SRAT/SLIT ACPI tables           |
| `crates/hv2-core/src/numa/allocator.rs`             | NUMA-aware allocator            |
| `crates/hv2-core/src/nested/types.rs`               | VMX types and VMCS fields       |
| `crates/hv2-core/src/nested/shadow_vmcs.rs`         | Shadow VMCS management          |
| `crates/hv2-core/src/nested/ept.rs`                 | Nested EPT management           |
| `crates/hv2-core/src/nested/manager.rs`             | Nested virtualization manager   |
| `crates/hv2-core/src/devices/integration_tests.rs`  | End-to-end tests                |
| `crates/hv2-agent/src/telemetry.rs`                 | Agent observability & metrics   |
| `crates/hv2-agent/src/limits.rs`                    | Resource limits & enforcement   |
| `crates/hv2-agent/src/events.rs`                    | Event streaming & subscriptions |
| `crates/hv2-agent/src/actions.rs`                   | VM control actions & validation |
| `crates/hv2-agent/src/policies.rs`                  | Policy rules & enforcement      |
| `crates/hv2-agent/src/communication.rs`             | Inter-agent messaging & routing |
| `crates/hv2-agent/src/state.rs`                     | Persistent state & checkpoints  |
| `crates/hv2-agent/src/tasks.rs`                     | Task scheduling & workflows     |
| `crates/hv2-agent/src/reasoning.rs`                 | Reasoning & decision making     |
| `crates/hv2-agent/src/memory.rs`                    | Episodic & semantic memory      |
| `crates/hv2-agent/src/tools.rs`                     | Tool-use framework              |
| `crates/hv2-agent/src/perception.rs`                | Environment perception system   |

---

## HV1/HV2 Dual-Mode Architecture

### Overview

HyperMachine is designed to support two operational modes:

| Mode    | Type                | Host OS Required          | Target Use Case                |
| ------- | ------------------- | ------------------------- | ------------------------------ |
| **HV2** | Type 2 (Hosted)     | Yes (Linux/Windows/macOS) | Development, Testing, Edge     |
| **HV1** | Type 1 (Bare-metal) | No                        | Production, Cloud, Data Center |

### HV2 Mode (Type 2) - *Currently Implemented*

HV2 runs as a user-space application on an existing host operating system, leveraging platform-specific hypervisor APIs:

| Platform | Backend                    | Hardware Extension |
| -------- | -------------------------- | ------------------ |
| Linux    | KVM                        | Intel VT-x / AMD-V |
| Windows  | WHPX                       | Intel VT-x / AMD-V |
| macOS    | HVF (Hypervisor.framework) | Apple Hypervisor   |

**Architecture:**
```
┌─────────────────────────────────────────────────────────┐
│                    Guest VMs                             │
├─────────────────────────────────────────────────────────┤
│                 HyperMachine (HV2)                       │
│            (User-space hypervisor layer)                 │
├─────────────────────────────────────────────────────────┤
│              KVM / WHPX / HVF API                        │
├─────────────────────────────────────────────────────────┤
│                  Host OS Kernel                          │
├─────────────────────────────────────────────────────────┤
│                    Hardware                              │
└─────────────────────────────────────────────────────────┘
```

### HV1 Mode (Type 1) - *Planned*

HV1 will run directly on bare metal without a host OS, implementing its own VMX/SVM virtualization:

| Architecture   | Extension | Implementation                 |
| -------------- | --------- | ------------------------------ |
| x86-64 (Intel) | VMX       | Direct VMXON/VMCS manipulation |
| x86-64 (AMD)   | SVM       | Direct VMRUN/VMCB manipulation |
| ARM64          | EL2       | Direct EL2 hypervisor mode     |

**Architecture:**
```
┌─────────────────────────────────────────────────────────┐
│                    Guest VMs                             │
├─────────────────────────────────────────────────────────┤
│                 HyperMachine (HV1)                       │
│           (Bare-metal hypervisor layer)                  │
├─────────────────────────────────────────────────────────┤
│              VMX / SVM / EL2 Hardware                    │
├─────────────────────────────────────────────────────────┤
│                    Hardware                              │
└─────────────────────────────────────────────────────────┘
```

### Planned HV1 Implementation Phases

#### Phase HV1-1: Core VMX/SVM Support
- Direct hardware virtualization without host OS
- VMCS/VMCB setup and management
- VM entry/exit handling
- Minimal trusted computing base (TCB)

#### Phase HV1-2: Memory Virtualization
- Extended Page Tables (EPT) for Intel
- Nested Page Tables (NPT) for AMD
- Direct memory management without host OS assistance
- DMA remapping via VT-d/AMD-Vi

#### Phase HV1-3: Interrupt Virtualization
- Posted interrupts (Intel) / AVIC (AMD)
- Direct APIC virtualization
- Minimal interrupt latency
- MSI/MSI-X routing

#### Phase HV1-4: Device Passthrough
- VFIO-style PCI passthrough (without Linux VFIO)
- GPU passthrough for ML/AI workloads
- NVMe direct access
- IOMMU management

#### Phase HV1-5: Boot and Runtime
- UEFI boot loader
- Firmware integration
- Runtime services
- Management interface (serial, network)

### Code Sharing Strategy

The codebase is structured to maximize code sharing between HV1 and HV2:

| Component              | Shared | HV2-Specific      | HV1-Specific   |
| ---------------------- | ------ | ----------------- | -------------- |
| Device Emulation       | ✅      | -                 | -              |
| Guest State Management | ✅      | -                 | -              |
| Memory Abstractions    | ✅      | -                 | -              |
| Platform Backend       | -      | KVM/WHPX/HVF      | VMX/SVM direct |
| Memory Mapping         | -      | mmap/VirtualAlloc | EPT/NPT direct |
| Interrupt Delivery     | -      | Platform API      | Posted/AVIC    |

### Planned Crate Structure

```
crates/
├── hm-common/          # Shared abstractions (devices, memory, guest state)
├── hv2-core/           # Type 2 implementation (current)
├── hv2-cpu/            # Type 2 CPU backends
├── hv1-core/           # Type 1 implementation (planned)
├── hv1-vmx/            # Intel VMX backend (planned)
├── hv1-svm/            # AMD SVM backend (planned)
├── hv1-arm/            # ARM EL2 backend (planned)
├── hv2-gpu/            # GPU virtualization
├── hv2-net/            # Network virtualization
├── hv2-agent/          # AI agent interface
├── hv2-api/            # Remote APIs
└── hv2-cli/            # CLI tool
```

### Benefits of Dual-Mode Architecture

| Benefit             | HV2 Advantage                  | HV1 Advantage             |
| ------------------- | ------------------------------ | ------------------------- |
| **Development**     | Easy debugging, standard tools | -                         |
| **Performance**     | Good for most workloads        | Maximum, no host overhead |
| **Isolation**       | Good, OS-provided              | Maximum, minimal TCB      |
| **Deployment**      | Simple, runs anywhere          | Complex, bare metal only  |
| **Hardware Access** | Via host OS drivers            | Direct hardware control   |
| **Multi-tenant**    | Shared resources               | Dedicated hardware        |

### Timeline

| Phase             | Target     | Status        |
| ----------------- | ---------- | ------------- |
| HV2 Core          | Q1-Q2 2024 | ✅ Complete    |
| HV2 Full Features | Q3-Q4 2024 | 🚧 In Progress |
| HV1 Research      | Q1 2025    | 📋 Planned     |
| HV1 Alpha         | Q2-Q3 2025 | 📋 Planned     |
| HV1 Beta          | Q4 2025    | 📋 Planned     |
| HV1 Production    | 2026       | 📋 Planned     |