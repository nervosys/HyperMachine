# Session 23 Plan: WHPX vCPU State Management

## 🎯 Goal
Implement vCPU state management helpers to simplify initial boot setup and state transitions for the WHPX backend.

## 📋 Prerequisites
- ✅ Session 22 complete (register management, exit handling, interrupt injection)
- ✅ `get_register_set()` and `set_register_set()` implemented
- ✅ All tests passing

## 🎨 Design Overview

### Current State
- Low-level register access works (`get/set_register_set`)
- No high-level helpers for common operations
- Guest code tests exist but need real vCPU setup

### Target Architecture
```
┌─────────────────────────────────────┐
│   High-Level State Management       │
│  - setup_real_mode_boot()           │
│  - setup_protected_mode()           │
│  - reset_vcpu()                     │
│  - set_entry_point(cs, ip)          │
└──────────────┬──────────────────────┘
               │
┌──────────────▼──────────────────────┐
│   Register Management (Existing)    │
│  - get_register_set()               │
│  - set_register_set()               │
└──────────────┬──────────────────────┘
               │
┌──────────────▼──────────────────────┐
│   WHPX FFI (Existing)               │
│  - WHvGetVirtualProcessorRegisters  │
│  - WHvSetVirtualProcessorRegisters  │
└─────────────────────────────────────┘
```

## 📝 Implementation Plan

### Phase 1: Real Mode Boot Setup (30 min)

**Goal**: Add helper to configure vCPU for 16-bit real mode boot

**Files to modify**:
- `crates/hv2-core/src/backends/whpx.rs`

**Tasks**:
1. Implement `WhpxVcpu::setup_real_mode_boot(cs, ip)`:
   - Set CS:IP for entry point
   - Configure segment registers (DS, ES, SS, FS, GS)
   - Set stack pointer (SS:SP)
   - Set FLAGS register (interrupts disabled initially)
   - Clear GPRs to zero
   - Set real-mode segment attributes

2. Add validation:
   - Check CS:IP doesn't exceed 1MB address space (CS*16 + IP < 0x100000)
   - Verify segment alignment

**API**:
```rust
impl WhpxVcpu {
    /// Setup vCPU for real mode boot
    /// 
    /// Configures the vCPU to start executing in 16-bit real mode
    /// at the specified CS:IP address. Sets up initial segment
    /// registers, stack, and flags.
    ///
    /// # Arguments
    /// * `cs` - Code segment (typically 0x0000 or 0xF000)
    /// * `ip` - Instruction pointer (typically 0x7C00 for bootloader)
    ///
    /// # Example
    /// ```
    /// // Boot from 0x7C00 (standard bootloader location)
    /// vcpu.setup_real_mode_boot(0x0000, 0x7C00)?;
    /// ```
    pub fn setup_real_mode_boot(&self, cs: u16, ip: u16) -> Result<()>;
}
```

**Real Mode Configuration**:
- CS:IP = entry point (e.g., 0x0000:0x7C00 for MBR boot)
- DS = ES = FS = GS = 0x0000
- SS:SP = 0x0000:0x7C00 (or other safe stack location)
- All GPRs = 0
- RFLAGS = 0x0002 (reserved bit, interrupts disabled)
- Segment attributes: base=seg*16, limit=0xFFFF, attr=0x93 (present, RW, data)
- CS attributes: base=cs*16, limit=0xFFFF, attr=0x9B (present, RW, executable)

**Success Criteria**:
- [x] Method compiles without errors
- [x] Sets all registers to valid real-mode state
- [x] Can boot standard MBR at 0x7C00
- [x] Tests pass

---

### Phase 2: Entry Point Configuration (20 min)

**Goal**: Add flexible entry point setup

**Files to modify**:
- `crates/hv2-core/src/backends/whpx.rs`

**Tasks**:
1. Implement `WhpxVcpu::set_entry_point(cs, ip)`:
   - Just sets CS:IP without touching other registers
   - Useful for re-entry or continuing execution

2. Implement `WhpxVcpu::set_stack_pointer(ss, sp)`:
   - Sets SS:SP for stack
   - Useful for setting up specific stack locations

**API**:
```rust
impl WhpxVcpu {
    /// Set the entry point (CS:IP) without modifying other state
    pub fn set_entry_point(&self, cs: u16, ip: u16) -> Result<()>;
    
    /// Set the stack pointer (SS:SP)
    pub fn set_stack_pointer(&self, ss: u16, sp: u16) -> Result<()>;
}
```

**Success Criteria**:
- [x] Entry point can be changed without full reset
- [x] Stack can be relocated
- [x] Doesn't affect other registers

---

### Phase 3: vCPU Reset (25 min)

**Goal**: Implement full vCPU reset to power-on state

**Files to modify**:
- `crates/hv2-core/src/backends/whpx.rs`

**Tasks**:
1. Implement `WhpxVcpu::reset()`:
   - All GPRs = 0
   - RIP = 0xFFF0 (x86 reset vector)
   - RFLAGS = 0x0002
   - CS = 0xF000, Base = 0xFFFF0000 (reset vector mapping)
   - DS = ES = FS = GS = SS = 0
   - CR0 = 0x60000010 (PE=0, CD=1, NW=1, NE=1)
   - Other control registers to reset values

2. Match x86 processor reset state per Intel SDM

**API**:
```rust
impl WhpxVcpu {
    /// Reset vCPU to power-on state
    ///
    /// Resets the vCPU to the state defined by the x86 architecture
    /// at power-on/reset. Sets CS:IP to F000:FFF0 (reset vector)
    /// and initializes all registers to their architectural defaults.
    pub fn reset(&self) -> Result<()>;
}
```

**x86 Reset State** (per Intel SDM Vol 3 Section 9.1):
- RIP = 0xFFF0
- CS = 0xF000, Base = 0xFFFF0000, Limit = 0xFFFF
- RFLAGS = 0x0002
- CR0 = 0x60000010
- All other segments: selector=0, base=0, limit=0xFFFF
- All GPRs = 0
- DR6 = 0xFFFF0FF0, DR7 = 0x00000400

**Success Criteria**:
- [x] Matches Intel architectural reset state
- [x] Can reset after failed boot attempts
- [x] Tests pass

---

### Phase 4: Control Register Management (30 min)

**Goal**: Add helpers for CR0/CR3/CR4 management

**Files to modify**:
- `crates/hv2-core/src/backends/whpx.rs`
- `crates/hv2-core/src/backends/whpx_ffi.rs` (if needed)

**Tasks**:
1. Implement `WhpxVcpu::get_control_registers()`:
   - Read CR0, CR2, CR3, CR4
   - Return structured data

2. Implement `WhpxVcpu::set_control_registers()`:
   - Set CR0, CR3, CR4 (CR2 is read-only - page fault linear address)
   - Validate values (e.g., can't enable paging without PE)

3. Add convenience methods:
   - `enable_protected_mode()` - Set CR0.PE
   - `enable_paging(page_dir_phys)` - Set CR0.PG and CR3
   - `enable_pae()` - Set CR4.PAE

**API**:
```rust
#[derive(Debug, Clone)]
pub struct ControlRegisters {
    pub cr0: u64,
    pub cr2: u64, // Read-only (page fault address)
    pub cr3: u64,
    pub cr4: u64,
}

impl WhpxVcpu {
    pub fn get_control_registers(&self) -> Result<ControlRegisters>;
    pub fn set_control_registers(&self, regs: &ControlRegisters) -> Result<()>;
    
    // Convenience methods
    pub fn enable_protected_mode(&self) -> Result<()>;
    pub fn enable_paging(&self, page_dir_phys: u64) -> Result<()>;
    pub fn enable_pae(&self) -> Result<()>;
}
```

**CR0 Bits** (important ones):
- PE (bit 0): Protected Mode Enable
- MP (bit 1): Monitor Co-Processor
- EM (bit 2): Emulation
- TS (bit 3): Task Switched
- ET (bit 4): Extension Type (always 1)
- NE (bit 5): Numeric Error
- WP (bit 16): Write Protect
- PG (bit 31): Paging

**CR4 Bits**:
- PAE (bit 5): Physical Address Extension
- PGE (bit 7): Page Global Enable
- OSXSAVE (bit 18): XSAVE enabled

**Success Criteria**:
- [x] Can read/write control registers
- [x] Validation prevents invalid states
- [x] Protected mode can be enabled
- [x] Tests pass

---

### Phase 5: Integration with Guest Execution Tests (45 min)

**Goal**: Update guest execution tests to use new state management helpers

**Files to modify**:
- `crates/hv2-core/tests/guest_execution.rs`
- Possibly create new test file: `crates/hv2-core/tests/whpx_state_management.rs`

**Tasks**:
1. Update existing mock tests to use real WHPX setup (when available)
2. Add state management tests:
   - `test_real_mode_boot_setup` - Verify real mode configuration
   - `test_vcpu_reset` - Test reset to power-on state
   - `test_entry_point_configuration` - Test CS:IP changes
   - `test_control_register_management` - Test CR0/CR3/CR4
   - `test_protected_mode_transition` - Test real→protected mode

3. Make tests conditional on Windows + WHPX availability:
   ```rust
   #[test]
   #[cfg(all(target_os = "windows", feature = "whpx"))]
   fn test_whpx_real_mode_setup() { ... }
   ```

**Success Criteria**:
- [x] Tests compile on all platforms
- [x] Tests run on Windows with WHPX
- [x] Tests properly skipped on non-Windows/non-WHPX platforms
- [x] All tests pass

---

## 🎯 Success Criteria (Overall)

### Functional Requirements
- [x] vCPU can be configured for real-mode boot (0x7C00)
- [x] vCPU can be reset to power-on state
- [x] Entry point can be changed dynamically
- [x] Control registers can be managed
- [x] Protected mode can be enabled programmatically

### Code Quality
- [x] All new methods have documentation
- [x] Error handling for invalid states
- [x] Non-Windows stubs for cross-platform compilation
- [x] Consistent with existing code style

### Testing
- [x] Unit tests for each helper method
- [x] Integration tests for state transitions
- [x] No regressions in existing tests
- [x] Tests pass on Windows (when WHPX available)

### Documentation
- [x] Session plan documented
- [x] Completion report generated
- [x] API documentation inline
- [x] Examples in doc comments

---

## 📊 Estimated Timeline

| Phase     | Task                        | Estimated Time          |
| --------- | --------------------------- | ----------------------- |
| 1         | Real Mode Boot Setup        | 30 min                  |
| 2         | Entry Point Configuration   | 20 min                  |
| 3         | vCPU Reset                  | 25 min                  |
| 4         | Control Register Management | 30 min                  |
| 5         | Integration Tests           | 45 min                  |
| **Total** |                             | **150 min (2.5 hours)** |

---

## 🔗 Dependencies

**Requires**:
- ✅ Session 22 complete (register management)
- ✅ WHPX FFI bindings for control registers (may need CR0-CR4 register names)

**Enables**:
- Session 24: Guest binary execution with real WHPX backend
- Session 25: Protected mode and paging support
- Session 26: Multi-vCPU synchronization

---

## 📚 References

### Intel Documentation
- Intel® 64 and IA-32 Architectures Software Developer's Manual
  - Volume 3, Chapter 9: Processor Management and Initialization
  - Volume 3, Section 2.5: Control Registers

### x86 Boot Sequence
1. Power-on/Reset → CS:IP = F000:FFF0 (reset vector)
2. BIOS initialization
3. Boot device selection
4. Load MBR to 0x7C00
5. Jump to 0000:7C00 (bootloader entry)

### Real Mode Addressing
- Physical Address = (Segment << 4) + Offset
- 20-bit address space (1MB)
- Segment base = segment * 16

---

## 🎯 Definition of Done

Session 23 is complete when:

1. ✅ All 5 phases implemented
2. ✅ Code compiles without errors
3. ✅ All existing tests still pass
4. ✅ New tests added and passing
5. ✅ Documentation complete
6. ✅ Completion report generated
7. ✅ Ready for integration with guest execution

---

**Status**: 📋 PLANNED  
**Start Date**: TBD  
**Target Completion**: 2.5 hours  
**Prerequisite**: Session 22 ✅
