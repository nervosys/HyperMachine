# Session 22: WHPX vCPU Execution Implementation

**Date**: November 4, 2025  
**Objective**: Implement actual vCPU execution in the WHPX backend  
**Status**: 🚧 In Progress

---

## 📋 Background

Session 21 completed the guest code execution test framework using MockHypervisorBackend. Now we need to implement the real WHPX backend so tests can run with actual hardware virtualization on Windows.

### Current State

**WHPX Backend Status**:
- ✅ Backend creation and initialization
- ✅ VM/partition creation  
- ✅ Guest memory allocation and mapping
- ✅ vCPU creation
- ⏸️ vCPU execution (`run_vcpu` - stub)
- ⏸️ Interrupt injection (`inject_interrupt` - stub)
- ⏸️ Register management (get/set)
- ⏸️ VM exit handling

**Current Implementation**:
```rust
async fn run_vcpu(&self, vcpu: &VCpu) -> Result<VmExit> {
    tracing::debug!("WHPX: Running vCPU {}", vcpu.id());
    Ok(VmExit::Hlt)  // ← Just returns HLT
}
```

---

## 🎯 Goals

### Primary Goals
1. **Implement `run_vcpu`**: Execute vCPU with WHvRunVirtualProcessor()
2. **Handle VM Exits**: Translate WHPX exit context to VmExit enum
3. **Register Management**: Get/set vCPU registers via WHPX
4. **Exit Translation**: Map WHPX_RUN_VP_EXIT_REASON to VmExit

### Secondary Goals
5. **Interrupt Injection**: Implement WHvRequestInterrupt()
6. **Testing**: Enable real execution tests with WHPX
7. **Error Handling**: Proper HRESULT to Error mapping

---

## 📝 Implementation Plan

### Phase 1: Register Management (30 min)

**Files to modify**:
- `crates/hv2-core/src/backends/whpx.rs`

**Tasks**:
1. Add `get_registers()` method to WhpxVcpu
   - Use WHvGetVirtualProcessorRegisters()
   - Map WHV_REGISTER_NAME to RegisterSet
   - Handle all GPRs + RIP + RFLAGS + segments

2. Add `set_registers()` method to WhpxVcpu
   - Use WHvSetVirtualProcessorRegisters()
   - Convert RegisterSet to WHV_REGISTER_VALUE[]

3. Add register helper methods
   - `read_register(name)` - read single register
   - `write_register(name, value)` - write single register

**API**:
```rust
impl WhpxVcpu {
    fn get_registers(&self) -> Result<RegisterSet>;
    fn set_registers(&self, regs: &RegisterSet) -> Result<()>;
    fn read_register(&self, name: WHV_REGISTER_NAME) -> Result<u64>;
    fn write_register(&self, name: WHV_REGISTER_NAME, value: u64) -> Result<()>;
}
```

### Phase 2: VM Exit Translation (45 min)

**Files to modify**:
- `crates/hv2-core/src/backends/whpx.rs`
- `crates/hv2-core/src/backends/whpx_ffi.rs` (if needed)

**Tasks**:
1. Create `translate_exit()` function
   - Input: WHV_RUN_VP_EXIT_CONTEXT
   - Output: VmExit
   - Map all exit reasons

2. Handle exit reasons:
   - WHvRunVpExitReasonX64IoPortAccess → VmExit::Io
   - WHvRunVpExitReasonMemoryAccess → VmExit::Mmio
   - WHvRunVpExitReasonX64Halt → VmExit::Hlt
   - WHvRunVpExitReasonX64InterruptWindow → VmExit::InterruptWindow
   - WHvRunVpExitReasonException → VmExit::Exception
   - WHvRunVpExitReasonCanceled → VmExit::Shutdown
   - Others → VmExit::Unknown

3. Extract exit-specific data
   - I/O: port, direction, size, data
   - MMIO: address, size, is_write, data
   - Exception: vector, error_code

**API**:
```rust
fn translate_exit(exit_context: &WHV_RUN_VP_EXIT_CONTEXT) -> VmExit;
```

### Phase 3: vCPU Execution (45 min)

**Files to modify**:
- `crates/hv2-core/src/backends/whpx.rs`

**Tasks**:
1. Implement `WhpxVcpu::run()` method
   - Call WHvRunVirtualProcessor()
   - Get exit context
   - Translate to VmExit
   - Return to caller

2. Update `WhpxBackend::run_vcpu()`
   - Find WhpxVcpu for given VCpu
   - Call vcpu.run()
   - Handle errors

3. Add execution context management
   - Track vCPU state (running, stopped, etc.)
   - Handle cancellation
   - Proper cleanup

**Implementation**:
```rust
impl WhpxVcpu {
    fn run(&self) -> Result<VmExit> {
        unsafe {
            let mut exit_context = std::mem::zeroed();
            
            let hr = WHvRunVirtualProcessor(
                self.partition,
                self.vp_index,
                &mut exit_context,
                std::mem::size_of::<WHV_RUN_VP_EXIT_CONTEXT>() as UINT32,
            );
            
            if hr != S_OK {
                return Err(Error::VM(format!(
                    "WHvRunVirtualProcessor failed: 0x{:08X}", hr
                )));
            }
            
            Ok(translate_exit(&exit_context))
        }
    }
}
```

### Phase 4: Interrupt Injection (30 min)

**Files to modify**:
- `crates/hv2-core/src/backends/whpx.rs`

**Tasks**:
1. Implement `WhpxVcpu::inject_interrupt()`
   - Build WHV_INTERRUPT_CONTROL structure
   - Call WHvRequestInterrupt()
   - Handle errors

2. Update `WhpxBackend::inject_interrupt()`
   - Find WhpxVcpu
   - Call vcpu.inject_interrupt()

**Implementation**:
```rust
impl WhpxVcpu {
    fn inject_interrupt(&self, vector: u8) -> Result<()> {
        unsafe {
            let mut control = std::mem::zeroed::<WHV_INTERRUPT_CONTROL>();
            control.Type = WHV_INTERRUPT_TYPE_FIXED;
            control.DestinationMode = WHV_INTERRUPT_DESTINATION_MODE_PHYSICAL;
            control.TriggerMode = WHV_INTERRUPT_TRIGGER_MODE_EDGE;
            control.Vector = vector as UINT64;
            control.Destination = self.vp_index as UINT64;
            
            let hr = WHvRequestInterrupt(
                self.partition,
                &control,
                std::mem::size_of::<WHV_INTERRUPT_CONTROL>() as UINT32,
            );
            
            if hr != S_OK {
                return Err(Error::VM(format!(
                    "WHvRequestInterrupt failed: 0x{:08X}", hr
                )));
            }
            
            Ok(())
        }
    }
}
```

### Phase 5: Testing (30 min)

**Files to create/modify**:
- `crates/hv2-core/tests/whpx_execution.rs` (new)

**Tasks**:
1. Create WHPX-specific execution tests
   - Test with actual hardware virtualization
   - Verify exit handling
   - Test interrupt injection

2. Conditional compilation
   - #[cfg(all(target_os = "windows", feature = "whpx"))]
   - Skip on non-Windows platforms

3. Test cases:
   - test_whpx_vcpu_execution - Basic execution
   - test_whpx_io_exit - I/O port access
   - test_whpx_halt - HLT instruction
   - test_whpx_interrupt - Interrupt injection

---

## 🔧 Technical Details

### WHPX API Calls Required

1. **WHvRunVirtualProcessor**
   - Runs vCPU until exit
   - Returns exit context with reason + data

2. **WHvGetVirtualProcessorRegisters**
   - Reads register values
   - Takes array of register names
   - Returns array of register values

3. **WHvSetVirtualProcessorRegisters**
   - Writes register values
   - Takes array of name/value pairs

4. **WHvRequestInterrupt**
   - Injects interrupt into vCPU
   - Takes interrupt control structure

### Exit Context Structure

```c
typedef struct WHV_RUN_VP_EXIT_CONTEXT {
    WHV_RUN_VP_EXIT_REASON ExitReason;
    UINT32 Reserved;
    WHV_VP_EXIT_CONTEXT VpContext;
    union {
        WHV_X64_IO_PORT_ACCESS_CONTEXT IoPortAccess;
        WHV_MEMORY_ACCESS_CONTEXT MemoryAccess;
        WHV_X64_CPUID_ACCESS_CONTEXT CpuidAccess;
        WHV_VP_EXCEPTION_CONTEXT VpException;
        WHV_X64_INTERRUPTION_DELIVERABLE_CONTEXT InterruptWindow;
        WHV_X64_UNSUPPORTED_FEATURE_CONTEXT UnsupportedFeature;
        WHV_RUN_VP_CANCELED_CONTEXT CanceledContext;
    };
} WHV_RUN_VP_EXIT_CONTEXT;
```

### Register Mapping

Map WHPX register names to our RegisterSet:
- WHvX64RegisterRax → rax
- WHvX64RegisterRip → rip
- WHvX64RegisterRflags → rflags
- WHvX64RegisterCs → cs
- (and all other GPRs + segments)

---

## 📊 Success Criteria

### Minimum Viable
- ✅ `run_vcpu()` executes actual guest code
- ✅ HLT exit is handled correctly
- ✅ Registers can be read/written

### Full Success
- ✅ All VM exit types translated
- ✅ I/O and MMIO exits work
- ✅ Interrupt injection functional
- ✅ Tests pass on Windows with WHPX

### Bonus
- ✅ Performance profiling
- ✅ Execution telemetry integration
- ✅ Debug tracing for all exits

---

## 📈 Expected Impact

**Enables**:
- Real hardware virtualization on Windows
- Guest OS booting and execution
- Device I/O with actual VMs
- Performance testing with real workloads

**Unlocks Next Steps**:
- Full VM lifecycle management
- Guest OS support
- Device passthrough
- Production readiness

---

## 🚧 Current Status

- [x] Session plan created
- [ ] Phase 1: Register management
- [ ] Phase 2: Exit translation
- [ ] Phase 3: vCPU execution
- [ ] Phase 4: Interrupt injection
- [ ] Phase 5: Testing
- [ ] Documentation updated

---

**Last Updated**: November 4, 2025  
**Estimated Time**: 3-4 hours  
**Priority**: High (critical path for real execution)
