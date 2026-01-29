# Session 26 Plan: Full OS Boot Sequence

**Date**: November 5, 2025  
**Estimated Duration**: 3-4 hours (180-240 minutes)  
**Status**: 🔄 In Progress

---

## 🎯 Session Objectives

Implement complete operating system boot support by adding:
1. **Long Mode (64-bit) transitions** via IA32_EFER register
2. **GDT (Global Descriptor Table) management** for segment descriptors
3. **IDT (Interrupt Descriptor Table) setup** for interrupt handling
4. **Boot protocol helpers** for Linux, Windows, and multiboot standards
5. **End-to-end OS boot demonstration** with real kernel binaries

### Success Criteria

✅ IA32_EFER register accessible (LME, LMA, NXE bits)  
✅ GDT can be loaded and configured programmatically  
✅ IDT can be initialized with interrupt handlers  
✅ Real mode → Protected mode → Long mode transitions working  
✅ Boot a real Linux kernel or Windows bootloader  
✅ All tests passing (unit + integration + end-to-end)  
✅ Comprehensive documentation with boot examples

---

## 📋 Prerequisites

### Completed Sessions
- ✅ Session 22: Register State Management
- ✅ Session 23: vCPU State Management  
- ✅ Session 24: Guest Execution
- ✅ Session 25: Protected Mode & Control Registers

### Required Knowledge
- x86-64 boot protocols (Linux, Multiboot, UEFI)
- Long mode transition requirements
- Segment descriptor formats
- Interrupt descriptor table structure
- Paging structures for long mode

### Intel SDM References
- Volume 3A, Section 3.2: System Descriptor Tables (GDT/IDT)
- Volume 3A, Section 3.4.5: Segment Descriptors
- Volume 3A, Section 9.8.5: IA32_EFER MSR
- Volume 3A, Chapter 9: Advanced Programmable Interrupt Controller (APIC)

---

## 📊 Phase Breakdown

### Phase 1: IA32_EFER Register Access (40 minutes)

**Goal**: Enable long mode transitions via Extended Feature Enable Register.

**Tasks**:
1. Add IA32_EFER MSR constants to `whpx_ffi.rs`:
   - `IA32_EFER_SCE` (bit 0) - SYSCALL Enable
   - `IA32_EFER_LME` (bit 8) - Long Mode Enable
   - `IA32_EFER_LMA` (bit 10) - Long Mode Active (read-only)
   - `IA32_EFER_NXE` (bit 11) - No-Execute Enable

2. Add EFER field to `ControlRegisters` struct in `vcpu.rs`:
   ```rust
   pub struct ControlRegisters {
       pub cr0: u64,
       pub cr2: u64,
       pub cr3: u64,
       pub cr4: u64,
       pub cr8: u64,
       pub efer: u64,  // ← NEW
   }
   ```

3. Add EFER helper methods:
   ```rust
   impl ControlRegisters {
       pub fn is_long_mode_enabled(&self) -> bool;
       pub fn is_long_mode_active(&self) -> bool;
       pub fn is_nxe_enabled(&self) -> bool;
   }
   ```

4. Update `WhpxVcpu::get_control_registers()` to read EFER
5. Update `WhpxVcpu::set_control_registers()` to write EFER
6. Update validation to check EFER constraints:
   - LMA is read-only (set by CPU)
   - LME requires CR0.PG to activate
   - Long mode requires PAE (CR4.PAE)

7. Add unit tests for EFER operations

**Deliverables**:
- IA32_EFER bit flags defined
- EFER accessible via control registers API
- Validation prevents invalid long mode transitions
- Tests verify EFER read/write/validation

---

### Phase 2: Long Mode Transition Helpers (35 minutes)

**Goal**: High-level API for transitioning to 64-bit long mode.

**Tasks**:
1. Add `WhpxVcpu::enable_long_mode(page_directory_base)`:
   - Validates prerequisites (protected mode, PAE, paging)
   - Sets CR4.PAE if needed
   - Sets IA32_EFER.LME
   - Enables paging (CR0.PG)
   - Verifies IA32_EFER.LMA set by CPU

2. Add `WhpxVcpu::disable_long_mode()`:
   - Disables paging (CR0.PG)
   - Clears IA32_EFER.LME
   - Verifies IA32_EFER.LMA cleared

3. Add mode detection helpers:
   ```rust
   pub enum CpuMode {
       RealMode,
       ProtectedMode,
       LongModeCompatibility,  // 32-bit in 64-bit mode
       LongMode64Bit,          // True 64-bit mode
   }
   
   impl WhpxVcpu {
       pub fn get_cpu_mode(&self) -> Result<CpuMode>;
   }
   ```

4. Add integration tests:
   - Real → Protected → Long mode sequence
   - Long → Protected → Real mode sequence
   - Invalid transitions (missing PAE, etc.)

**Deliverables**:
- `enable_long_mode()` / `disable_long_mode()` methods
- CPU mode detection API
- Tests for all transition sequences

---

### Phase 3: GDT Management (45 minutes)

**Goal**: Programmatic Global Descriptor Table setup.

**Tasks**:
1. Define descriptor structures in new `src/descriptors.rs`:
   ```rust
   /// GDT Entry (8 bytes)
   #[repr(C, packed)]
   pub struct SegmentDescriptor {
       limit_low: u16,
       base_low: u16,
       base_middle: u8,
       access: u8,
       granularity: u8,
       base_high: u8,
   }
   
   /// GDTR/IDTR (10 bytes: 2-byte limit + 8-byte base)
   #[repr(C, packed)]
   pub struct DescriptorTablePointer {
       limit: u16,
       base: u64,
   }
   ```

2. Add descriptor type constants:
   ```rust
   // Access byte bits
   pub const DESC_PRESENT: u8 = 1 << 7;
   pub const DESC_DPL_0: u8 = 0 << 5;
   pub const DESC_DPL_3: u8 = 3 << 5;
   pub const DESC_CODE_DATA: u8 = 1 << 4;
   pub const DESC_EXECUTABLE: u8 = 1 << 3;
   pub const DESC_WRITABLE: u8 = 1 << 1;
   pub const DESC_READABLE: u8 = 1 << 1;
   
   // Granularity byte bits
   pub const DESC_GRANULAR: u8 = 1 << 7;    // 4KB granularity
   pub const DESC_32BIT: u8 = 1 << 6;       // 32-bit mode
   pub const DESC_64BIT: u8 = 1 << 5;       // 64-bit mode (long)
   ```

3. Add helper functions for common descriptors:
   ```rust
   impl SegmentDescriptor {
       pub fn null() -> Self;
       pub fn code_64bit() -> Self;           // 64-bit code segment
       pub fn data_64bit() -> Self;           // 64-bit data segment
       pub fn code_32bit(base: u32, limit: u32) -> Self;
       pub fn data_32bit(base: u32, limit: u32) -> Self;
       pub fn code_16bit(base: u32, limit: u32) -> Self;
   }
   ```

4. Add GDT builder:
   ```rust
   pub struct GdtBuilder {
       entries: Vec<SegmentDescriptor>,
   }
   
   impl GdtBuilder {
       pub fn new() -> Self;
       pub fn add_null(&mut self) -> &mut Self;
       pub fn add_code_segment(&mut self, ...) -> &mut Self;
       pub fn add_data_segment(&mut self, ...) -> &mut Self;
       pub fn build(&self) -> Vec<u8>;
       pub fn build_pointer(&self, base: u64) -> DescriptorTablePointer;
   }
   ```

5. Add `WhpxVcpu::load_gdt()`:
   - Writes GDT to guest memory
   - Loads GDTR via segment registers
   - Returns selector values for CS/DS/SS/ES

6. Add unit tests for descriptor building
7. Add integration test for GDT loading

**Deliverables**:
- `descriptors.rs` module with GDT structures
- Descriptor builder API
- `load_gdt()` method
- Tests for descriptor creation and loading

---

### Phase 4: IDT Management (40 minutes)

**Goal**: Interrupt Descriptor Table setup for interrupt handling.

**Tasks**:
1. Define IDT structures in `descriptors.rs`:
   ```rust
   /// IDT Entry (16 bytes in 64-bit mode, 8 bytes in 32-bit)
   #[repr(C, packed)]
   pub struct InterruptDescriptor64 {
       offset_low: u16,
       selector: u16,
       ist: u8,           // Interrupt Stack Table
       attributes: u8,
       offset_middle: u16,
       offset_high: u32,
       reserved: u32,
   }
   
   #[repr(C, packed)]
   pub struct InterruptDescriptor32 {
       offset_low: u16,
       selector: u16,
       reserved: u8,
       attributes: u8,
       offset_high: u16,
   }
   ```

2. Add IDT type constants:
   ```rust
   pub const IDT_INTERRUPT_GATE: u8 = 0x0E;
   pub const IDT_TRAP_GATE: u8 = 0x0F;
   pub const IDT_PRESENT: u8 = 1 << 7;
   pub const IDT_DPL_0: u8 = 0 << 5;
   pub const IDT_DPL_3: u8 = 3 << 5;
   ```

3. Add IDT builder:
   ```rust
   pub struct IdtBuilder {
       entries: Vec<InterruptDescriptor64>,
       mode: CpuMode,
   }
   
   impl IdtBuilder {
       pub fn new(mode: CpuMode) -> Self;
       pub fn add_handler(&mut self, vector: u8, handler: u64, 
                          selector: u16) -> &mut Self;
       pub fn add_trap_gate(&mut self, ...) -> &mut Self;
       pub fn add_interrupt_gate(&mut self, ...) -> &mut Self;
       pub fn build(&self) -> Vec<u8>;
       pub fn build_pointer(&self, base: u64) -> DescriptorTablePointer;
   }
   ```

4. Add `WhpxVcpu::load_idt()`:
   - Writes IDT to guest memory
   - Loads IDTR via special instruction
   - Supports both 32-bit and 64-bit IDT formats

5. Add convenience method for default IDT:
   ```rust
   impl WhpxVcpu {
       pub fn setup_default_idt(&self, mode: CpuMode) -> Result<()>;
   }
   ```

6. Add unit tests for IDT building
7. Add integration test for IDT loading

**Deliverables**:
- IDT structures and builder
- `load_idt()` method
- Default IDT setup
- Tests for IDT creation and loading

---

### Phase 5: Boot Protocol Helpers (50 minutes)

**Goal**: High-level helpers for booting real operating systems.

**Tasks**:
1. Create new `src/boot.rs` module with boot protocols:
   ```rust
   pub mod linux;
   pub mod multiboot;
   pub mod uefi;
   ```

2. Implement Linux boot protocol (`boot/linux.rs`):
   ```rust
   /// Linux Boot Protocol (Documentation/x86/boot.rst)
   pub struct LinuxBootParams {
       pub kernel_image: Vec<u8>,
       pub initrd_image: Option<Vec<u8>>,
       pub cmdline: String,
       pub boot_params: BootParams,  // setup header
   }
   
   impl WhpxVcpu {
       pub fn boot_linux(&self, vm: &dyn HypervisorVm, 
                         params: &LinuxBootParams) -> Result<()>;
   }
   ```
   
   Steps:
   - Parse bzImage header
   - Load protected mode kernel at 1MB
   - Load real mode setup code at 0x90000
   - Setup boot_params structure
   - Load initrd if provided
   - Setup command line
   - Configure initial GDT/IDT
   - Jump to kernel entry point

3. Implement Multiboot protocol (`boot/multiboot.rs`):
   ```rust
   /// Multiboot Specification 1.0/2.0
   pub struct MultibootInfo {
       pub kernel_image: Vec<u8>,
       pub modules: Vec<MultibootModule>,
       pub cmdline: String,
       pub memory_map: Vec<MemoryRegion>,
   }
   
   impl WhpxVcpu {
       pub fn boot_multiboot(&self, vm: &dyn HypervisorVm,
                             info: &MultibootInfo) -> Result<()>;
   }
   ```

4. Add boot helper utilities:
   ```rust
   pub struct BootSetup {
       gdt_base: u64,
       idt_base: u64,
       page_table_base: u64,
   }
   
   impl BootSetup {
       pub fn allocate_tables(vm: &dyn HypervisorVm) -> Result<Self>;
       pub fn setup_minimal_environment(&self, vcpu: &WhpxVcpu) -> Result<()>;
   }
   ```

5. Add integration tests:
   - Boot minimal Linux kernel (if available)
   - Boot multiboot-compliant binary
   - Boot with various configurations

**Deliverables**:
- `boot.rs` module with protocol implementations
- Linux boot protocol support
- Multiboot protocol support
- Boot helper utilities
- Integration tests

---

### Phase 6: End-to-End Boot Testing (30 minutes)

**Goal**: Demonstrate complete OS boot capability with real binaries.

**Tasks**:
1. Create test kernels:
   - Minimal 16-bit boot sector (real mode)
   - 32-bit protected mode kernel
   - 64-bit long mode kernel
   - Linux bzImage (if available)

2. Create comprehensive boot test (`tests/integration/full_boot.rs`):
   ```rust
   #[tokio::test]
   async fn test_real_mode_boot() -> Result<()> {
       // Boot 16-bit boot sector
   }
   
   #[tokio::test]
   async fn test_protected_mode_boot() -> Result<()> {
       // Boot 32-bit kernel with GDT
   }
   
   #[tokio::test]
   async fn test_long_mode_boot() -> Result<()> {
       // Boot 64-bit kernel with paging
   }
   
   #[tokio::test]
   async fn test_linux_boot() -> Result<()> {
       // Boot real Linux kernel
   }
   
   #[tokio::test]
   async fn test_full_boot_sequence() -> Result<()> {
       // Real → Protected → Long mode with interrupts
   }
   ```

3. Add boot examples to `examples/`:
   - `examples/boot_16bit.rs` - Real mode boot
   - `examples/boot_32bit.rs` - Protected mode boot
   - `examples/boot_64bit.rs` - Long mode boot
   - `examples/boot_linux.rs` - Linux kernel boot

4. Update documentation with boot examples

5. Performance testing:
   - Measure boot time for each stage
   - Profile mode transitions
   - Benchmark descriptor table loading

**Deliverables**:
- Test kernel binaries
- Comprehensive integration tests
- Boot examples
- Performance benchmarks
- Updated documentation

---

## 🔄 Integration Points

### With Session 25 (Control Registers)
- Extend `ControlRegisters` with EFER field
- Use `enable_protected_mode()` in boot sequence
- Use `enable_paging()` for long mode
- Build on mode transition infrastructure

### With Session 24 (Guest Execution)
- Use `load_and_boot_binary()` for kernel loading
- Use `write_guest_memory()` for descriptor tables
- Use `read_guest_memory()` for verification
- Integrate with VM exit handling

### With Session 23 (vCPU State)
- Use segment register management for descriptor loading
- Combine with control register transitions
- Build on register state management

### With Session 22 (Register State)
- Use general-purpose register setup for boot parameters
- Configure RSI for kernel boot info structures

---

## 📚 Technical Reference

### Long Mode Transition Requirements

**Prerequisites**:
1. ✅ Start in protected mode (CR0.PE = 1)
2. ✅ Enable PAE (CR4.PAE = 1)
3. ✅ Setup valid page tables (4-level or 5-level)
4. ✅ Enable long mode (IA32_EFER.LME = 1)
5. ✅ Enable paging (CR0.PG = 1)
6. ✅ CPU activates long mode (IA32_EFER.LMA = 1, read-only)

**Result**: CPU in IA-32e mode (long mode compatibility)

**To enter 64-bit mode**:
7. ✅ Load 64-bit code segment (CS with L=1 in descriptor)
8. ✅ Far jump to 64-bit code

### Standard GDT Layout

Common GDT layout for boot:
```
Offset  Selector  Description
------  --------  -----------
0x00    0x0000    Null descriptor (required)
0x08    0x0008    64-bit code segment (ring 0)
0x10    0x0010    64-bit data segment (ring 0)
0x18    0x0018    32-bit code segment (ring 0)
0x20    0x0020    32-bit data segment (ring 0)
0x28    0x0028    64-bit code segment (ring 3)
0x30    0x0030    64-bit data segment (ring 3)
```

### Standard IDT Layout

Standard x86-64 interrupt vectors:
```
Vector  Description
------  -----------
0-19    CPU exceptions (divide, debug, NMI, breakpoint, etc.)
20-31   Reserved
32-255  User-defined interrupts (IRQs mapped here)
```

Common vectors:
- 0x00: Divide by zero
- 0x01: Debug
- 0x02: NMI
- 0x03: Breakpoint
- 0x0D: General protection fault
- 0x0E: Page fault
- 0x20-0x2F: Hardware IRQs (legacy PIC)

### Linux Boot Protocol

Linux kernel expects:
- **RSI**: Physical address of boot_params structure
- **GDT**: Flat segments (base=0, limit=4GB)
- **IDT**: Can be empty initially
- **CR0**: PE=1, PG=0 (protected mode, no paging)
- **Kernel**: Loaded at physical 0x100000 (1MB)
- **Entry**: Jump to 32-bit kernel entry point

### Multiboot Protocol

Multiboot kernel expects:
- **EAX**: Magic value 0x2BADB002
- **EBX**: Physical address of multiboot_info structure
- **GDT/IDT**: Undefined (kernel sets up)
- **CR0**: Protected mode enabled
- **A20**: Enabled
- **Interrupts**: Disabled

---

## ⚠️ Known Challenges

### Challenge 1: Page Table Setup

**Issue**: Long mode requires valid 4-level page tables (PML4)

**Solution Options**:
1. Guest sets up page tables (kernel handles)
2. Provide helper to create identity-mapped page tables
3. Pre-allocate and setup minimal page tables

**Recommended**: Option 2 - provide helper function

### Challenge 2: Real Kernel Testing

**Issue**: Testing with real kernel binaries is complex

**Mitigation**:
- Start with simple test kernels
- Use minimal Linux kernel build
- Mock kernel behavior in tests
- Provide clear documentation for kernel requirements

### Challenge 3: UEFI Complexity

**Issue**: UEFI boot protocol is significantly more complex than legacy BIOS

**Mitigation**:
- Defer full UEFI support to later session
- Focus on legacy BIOS and Linux boot protocol
- Document UEFI requirements for future work

### Challenge 4: Interrupt Handling

**Issue**: Full interrupt handling requires APIC setup

**Mitigation**:
- Setup minimal IDT (stub handlers)
- Defer full APIC configuration to Session 27
- Use legacy PIC for basic interrupt routing

---

## 📊 Estimated Timeline

| Phase     | Task                  | Duration    | Cumulative  |
| --------- | --------------------- | ----------- | ----------- |
| 1         | IA32_EFER Access      | 40 min      | 40 min      |
| 2         | Long Mode Helpers     | 35 min      | 75 min      |
| 3         | GDT Management        | 45 min      | 120 min     |
| 4         | IDT Management        | 40 min      | 160 min     |
| 5         | Boot Protocol Helpers | 50 min      | 210 min     |
| 6         | End-to-End Testing    | 30 min      | 240 min     |
| **Total** |                       | **240 min** | **4 hours** |

**Buffer**: 20% (48 minutes) for unexpected issues  
**Total with Buffer**: ~5 hours

---

## ✅ Success Metrics

### Functional Requirements
- [ ] EFER register accessible
- [ ] Long mode transitions working
- [ ] GDT can be programmatically created and loaded
- [ ] IDT can be programmatically created and loaded
- [ ] Boot protocol helpers implemented
- [ ] End-to-end boot tests passing

### Quality Requirements
- [ ] 100% test pass rate
- [ ] Zero regressions
- [ ] Code coverage ≥ 90% for new code
- [ ] All public APIs documented with examples
- [ ] Performance: boot sequence < 50ms

### Documentation Requirements
- [ ] API documentation complete
- [ ] Boot sequence diagrams
- [ ] Example code for each boot protocol
- [ ] Troubleshooting guide

---

## 📖 References

### Intel Software Developer's Manual
- Volume 3A, Chapter 3: Protected-Mode Memory Management
- Volume 3A, Chapter 4: Paging
- Volume 3A, Section 9.8.5: IA32_EFER MSR
- Volume 2A: Instruction Set (LGDT, LIDT, etc.)

### Boot Protocol Documentation
- Linux: `Documentation/x86/boot.rst` in kernel source
- Multiboot: https://www.gnu.org/software/grub/manual/multiboot/multiboot.html
- UEFI: https://uefi.org/specifications

### External Resources
- OSDev Wiki: https://wiki.osdev.org/
- Intel 64 and IA-32 Architectures SDM
- AMD64 Architecture Programmer's Manual

---

## 🚀 Next Session Preview

**Session 27**: Advanced Interrupt Handling
- Complete APIC setup (local APIC + I/O APIC)
- MSI/MSI-X interrupt delivery
- Interrupt injection and handling
- Timer interrupts
- IPI (Inter-Processor Interrupts)

---

**Session 26 Status**: 🔄 In Progress  
**Start Time**: [To be recorded]  
**Expected Completion**: [Start + 4 hours]

---

*End of Session 26 Plan*
