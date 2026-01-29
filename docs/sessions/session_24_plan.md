# Session 24 Plan: Guest Execution with WHPX State Management

## 🎯 Goal
Integrate Session 23's vCPU state management helpers with guest binary execution, demonstrating real-mode boot setup and execution flow.

## 📋 Prerequisites
- ✅ Session 22 complete (WHPX register management, exit handling, interrupt injection)
- ✅ Session 23 Phases 1-3 complete (real-mode boot, entry points, reset)
- ✅ Existing guest_execution.rs test framework
- ✅ Guest binaries available (hello.bin, interrupt_demo, etc.)

## 🎨 Design Overview

### Current State
- WHPX backend has low-level execution (`WhpxVcpu::run()`)
- State management helpers exist but not integrated
- MockHypervisorBackend used for guest execution tests
- Guest binaries tested with mock backend only

### Target Architecture
```
┌─────────────────────────────────────┐
│   Guest Execution Tests             │
│  - Use WHPX state management        │
│  - Load binaries with proper setup  │
│  - Track execution telemetry        │
└──────────────┬──────────────────────┘
               │
┌──────────────▼──────────────────────┐
│   WHPX State Management (Session 23)│
│  - setup_real_mode_boot()           │
│  - set_entry_point()                │
│  - reset()                          │
└──────────────┬──────────────────────┘
               │
┌──────────────▼──────────────────────┐
│   WHPX Execution (Session 22)       │
│  - run()                            │
│  - inject_interrupt()               │
│  - get/set_register_set()           │
└─────────────────────────────────────┘
```

## 📝 Implementation Plan

### Phase 1: Add WHPX Boot Helper Integration (30 min)

**Goal**: Add convenience method to WhpxVcpu for loading and booting guest binaries

**Files to modify**:
- `crates/hv2-core/src/backends/whpx.rs`

**Tasks**:
1. Add `WhpxVcpu::load_and_boot_binary()` method:
   - Takes binary path and entry point
   - Reads binary file
   - Copies to guest memory
   - Calls `setup_real_mode_boot()`
   - Returns ready-to-execute state

2. Add helper for guest memory access:
   - `WhpxVm::write_guest_memory(addr, data)`
   - `WhpxVm::read_guest_memory(addr, len)`

**API**:
```rust
impl WhpxVcpu {
    /// Load binary and setup for boot
    /// 
    /// Convenience method that:
    /// 1. Loads binary from file
    /// 2. Writes to guest memory at load_addr
    /// 3. Sets up real-mode boot at CS:IP
    ///
    /// # Arguments
    /// * `vm` - VM containing guest memory
    /// * `binary_path` - Path to binary file
    /// * `load_addr` - Physical address to load binary
    /// * `cs` - Code segment for boot
    /// * `ip` - Instruction pointer for boot
    pub fn load_and_boot_binary(
        &self,
        vm: &WhpxVm,
        binary_path: &Path,
        load_addr: u64,
        cs: u16,
        ip: u16,
    ) -> Result<()>;
}

impl WhpxVm {
    /// Write data to guest physical memory
    pub fn write_guest_memory(&self, addr: u64, data: &[u8]) -> Result<()>;
    
    /// Read data from guest physical memory
    pub fn read_guest_memory(&self, addr: u64, len: usize) -> Result<Vec<u8>>;
}
```

**Success Criteria**:
- [x] Can load binary files
- [x] Writes to correct guest memory location
- [x] Properly configures vCPU for boot
- [x] Compiles and basic tests pass

---

### Phase 2: Enhance MockBackend with State Management (30 min)

**Goal**: Update MockHypervisorBackend to demonstrate state management pattern

**Files to modify**:
- `crates/hv2-core/tests/guest_execution.rs`

**Tasks**:
1. Add state tracking to MockHypervisorBackend:
   - Track boot configuration
   - Log state changes
   - Verify setup sequence

2. Add state management to mock vCPU execution:
   - Simulate real-mode boot setup
   - Track register changes
   - Validate entry points

3. Update telemetry:
   - Add boot_setups counter
   - Add reset_count counter
   - Add entry_point_changes counter

**Enhanced Telemetry**:
```rust
pub struct ExecutionTelemetry {
    // Existing fields...
    pub boot_setups: usize,
    pub resets: usize,
    pub entry_point_changes: usize,
    pub initial_cs_ip: Option<(u16, u16)>,
}
```

**Success Criteria**:
- [x] Mock backend tracks state management calls
- [x] Telemetry captures boot configuration
- [x] Tests demonstrate proper usage pattern
- [x] All existing tests still pass

---

### Phase 3: Create WHPX-Specific Test Example (45 min)

**Goal**: Add example test showing WHPX state management in action

**Files to create/modify**:
- `crates/hv2-core/tests/whpx_boot_example.rs` (new)
- `crates/hv2-core/examples/whpx_boot_demo.rs` (new, optional)

**Tasks**:
1. Create `test_whpx_boot_sequence()`:
   - Demonstrates complete boot flow
   - Uses state management helpers
   - Shows proper error handling
   - Conditional on Windows + WHPX

2. Add documentation example:
   - Shows typical usage pattern
   - Explains boot sequence
   - Demonstrates state transitions

3. Make test skip gracefully:
   - Check for WHPX availability
   - Skip with message on non-Windows
   - Skip with message if Hyper-V disabled

**Test Structure**:
```rust
#[test]
#[cfg(target_os = "windows")]
fn test_whpx_boot_sequence() {
    // Check WHPX availability
    let backend = match WhpxBackend::new() {
        Ok(b) => b,
        Err(_) => {
            println!("⏭️  Skipping: WHPX not available");
            return;
        }
    };
    
    // Create VM
    let vm = backend.create_vm(1, 16 * 1024 * 1024).await?;
    let vcpu = vm.create_vcpu(0)?;
    
    // Load binary
    vcpu.load_and_boot_binary(
        &vm,
        Path::new("guest/hello.bin"),
        0x7C00,
        0x0000,
        0x7C00,
    )?;
    
    // Execute
    loop {
        let exit = vcpu.run()?;
        match exit {
            VmExit::Hlt => break,
            VmExit::Io { .. } => { /* handle */ },
            _ => { /* handle */ }
        }
    }
    
    println!("✅ Boot sequence completed successfully");
}
```

**Success Criteria**:
- [x] Test demonstrates end-to-end flow
- [x] Skips gracefully when WHPX unavailable
- [x] Well-documented with comments
- [x] Can serve as template for users

---

### Phase 4: Update Existing Tests to Use State Management (30 min)

**Goal**: Refactor existing guest execution tests to use new helpers

**Files to modify**:
- `crates/hv2-core/tests/guest_execution.rs`

**Tasks**:
1. Update `test_execute_hello_binary()`:
   - Use `setup_real_mode_boot()` pattern
   - Show before/after state
   - Demonstrate benefits

2. Update `test_execute_interrupt_demo()`:
   - Show reset between test runs
   - Demonstrate entry point changes
   - Use state management helpers

3. Update `test_execute_mmio_test()`:
   - Configure boot state explicitly
   - Log state transitions
   - Verify register setup

4. Add state validation:
   - Check CS:IP after setup
   - Verify segment configuration
   - Validate stack pointer

**Example Refactor**:
```rust
// BEFORE (old pattern)
fn test_execute_hello_binary() {
    let vm = create_vm();
    load_binary(vm, "hello.bin", 0x7C00);
    // Manual register setup...
    let result = vm.run();
}

// AFTER (new pattern with state management)
fn test_execute_hello_binary() {
    let vm = create_vm();
    let vcpu = vm.get_vcpu(0);
    
    // Clear state management: load and boot
    vcpu.load_and_boot_binary(
        &vm,
        Path::new("guest/hello.bin"),
        0x7C00,
        0x0000,
        0x7C00,
    )?;
    
    // State is properly configured automatically
    let result = vm.run();
}
```

**Success Criteria**:
- [x] Tests are clearer and shorter
- [x] State management benefits demonstrated
- [x] All tests still pass
- [x] Better documentation through examples

---

### Phase 5: Documentation and Examples (25 min)

**Goal**: Document the integration and provide usage examples

**Files to modify**:
- `crates/hv2-core/src/backends/whpx.rs` (doc comments)
- `docs/sessions/session_24_completion.md` (new)
- README updates (if applicable)

**Tasks**:
1. Add module-level documentation:
   - Explain boot sequence
   - Show typical usage pattern
   - Link to examples

2. Enhance method documentation:
   - Add more examples
   - Explain when to use each method
   - Document common patterns

3. Create usage guide:
   - "How to boot a guest binary"
   - "State management best practices"
   - "Troubleshooting boot issues"

4. Update session completion:
   - Document what was accomplished
   - Show code examples
   - Provide recommendations

**Documentation Structure**:
```rust
//! # WHPX Guest Execution
//!
//! This module provides Windows Hypervisor Platform (WHPX) backend support
//! for AetherVM, enabling hardware-accelerated virtualization on Windows.
//!
//! ## Boot Sequence
//!
//! Typical guest binary boot sequence:
//!
//! ```no_run
//! # use hv2_core::backends::whpx::*;
//! # async fn example() -> hv2_core::Result<()> {
//! // 1. Create backend and VM
//! let backend = WhpxBackend::new()?;
//! let vm = backend.create_vm(1, 16 * 1024 * 1024).await?;
//! let vcpu = vm.create_vcpu(0)?;
//!
//! // 2. Load and configure boot
//! vcpu.load_and_boot_binary(
//!     &vm,
//!     Path::new("bootloader.bin"),
//!     0x7C00,  // Load address
//!     0x0000,  // CS
//!     0x7C00,  // IP
//! )?;
//!
//! // 3. Execute until halt
//! loop {
//!     match vcpu.run()? {
//!         VmExit::Hlt => break,
//!         exit => handle_exit(exit)?,
//!     }
//! }
//! # Ok(())
//! # }
//! ```
```

**Success Criteria**:
- [x] Clear usage examples
- [x] Documented patterns
- [x] Completion report created
- [x] Ready for users to reference

---

## 🎯 Success Criteria (Overall)

### Functional Requirements
- [x] Can load guest binaries using state management
- [x] Boot sequence properly configured
- [x] Tests demonstrate usage patterns
- [x] Integration with existing execution framework
- [x] WHPX-specific tests skip gracefully on other platforms

### Code Quality
- [x] Well-documented methods
- [x] Clear usage examples
- [x] Error handling for file I/O and memory access
- [x] Consistent with existing patterns

### Testing
- [x] New tests demonstrate integration
- [x] Existing tests updated with new patterns
- [x] All tests pass
- [x] Graceful handling of unavailable WHPX

### Documentation
- [x] Usage guide complete
- [x] Examples in doc comments
- [x] Session completion report
- [x] Best practices documented

---

## 📊 Estimated Timeline

| Phase     | Task                         | Estimated Time          |
| --------- | ---------------------------- | ----------------------- |
| 1         | WHPX Boot Helper Integration | 30 min                  |
| 2         | Enhance MockBackend          | 30 min                  |
| 3         | WHPX-Specific Test Example   | 45 min                  |
| 4         | Update Existing Tests        | 30 min                  |
| 5         | Documentation and Examples   | 25 min                  |
| **Total** |                              | **160 min (2.7 hours)** |

---

## 🔗 Dependencies

**Requires**:
- ✅ Session 22: WHPX execution basics
- ✅ Session 23: vCPU state management helpers
- ✅ Guest binaries in guest/ directory
- ⚠️  Windows + WHPX enabled (for actual execution tests)

**Enables**:
- Real hardware-accelerated guest execution on Windows
- Complete boot sequence demonstration
- Foundation for full OS booting
- User-facing examples and documentation

---

## 📚 Boot Sequence Reference

### Standard MBR Boot
1. **Power-on**: vCPU at reset vector (F000:FFF0)
2. **BIOS**: Initializes hardware, finds boot device
3. **Load MBR**: Reads first sector (512 bytes) to 0x7C00
4. **Jump**: Sets CS:IP to 0000:7C00
5. **Execute**: Bootloader runs in real mode

### Our Simulation
1. **Reset**: `vcpu.reset()` → F000:FFF0 state
2. **Load**: Write binary to memory at 0x7C00
3. **Boot**: `setup_real_mode_boot(0x0000, 0x7C00)` → 0000:7C00
4. **Execute**: `vcpu.run()` → guest code runs
5. **Handle Exits**: Process I/O, MMIO, HLT, etc.

---

## 🎯 Definition of Done

Session 24 is complete when:

1. ✅ All 5 phases implemented
2. ✅ Code compiles without errors
3. ✅ All existing tests still pass
4. ✅ New examples demonstrate integration
5. ✅ Documentation complete with examples
6. ✅ Completion report generated
7. ✅ Users can follow examples to boot guest code

---

**Status**: 📋 PLANNED  
**Start Date**: TBD  
**Target Completion**: 2.7 hours  
**Prerequisites**: Sessions 22 ✅ + 23 ✅  
**Next Session**: 25 - Protected Mode & Control Registers
