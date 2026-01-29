# Session 31 Plan: Interrupt Window Handling

**Date:** November 7, 2025  
**Status:** 📋 PLANNED  
**Prerequisites:** Session 30 Complete (PIC Integration)  
**Estimated Duration:** 2.5-3 hours (150-180 minutes)

---

## 🎯 Session Goals

Implement proper interrupt window handling to ensure interrupts are only delivered when the guest CPU is ready to receive them. This completes the interrupt delivery mechanism started in Session 30 by adding the missing RFLAGS.IF checking and interrupt window request handling.

### Primary Objectives
1. ✅ Implement RFLAGS.IF (Interrupt Flag) checking
2. ✅ Add interrupt window request mechanism
3. ✅ Enhance interrupt delivery with proper timing
4. ✅ Add NMI (Non-Maskable Interrupt) support
5. ✅ Create comprehensive tests for interrupt windows
6. ✅ Add performance monitoring for interrupt latency

### Success Criteria
- Interrupts only delivered when RFLAGS.IF is set
- Interrupt window mechanism works correctly
- NMIs bypass interrupt flag checking
- Tests validate proper interrupt blocking/delivery
- Documentation covers interrupt window concepts
- No performance regression from additional checks

---

## 📋 Implementation Phases

### Phase 1: RFLAGS Access and Checking (30 minutes)

**Goal:** Add methods to read and check RFLAGS.IF state

**Tasks:**
1. Add `get_rflags()` method to WhpxVcpu
   - Reads RFLAGS register from WHPX
   - Returns u64 value
   - Cached for performance

2. Add `is_interrupt_enabled()` helper
   - Checks RFLAGS.IF bit (bit 9)
   - Returns bool indicating if interrupts are enabled
   - Used before interrupt injection

3. Add RFLAGS constants
   - `RFLAGS_CF` (bit 0) - Carry Flag
   - `RFLAGS_IF` (bit 9) - Interrupt Flag
   - `RFLAGS_DF` (bit 10) - Direction Flag
   - Other common flags for completeness

**Code Location:** `crates/hv2-core/src/backends/whpx.rs`

**Expected Output:**
```rust
impl WhpxVcpu {
    pub fn get_rflags(&self) -> Result<u64> {
        // Read RFLAGS register
    }
    
    pub fn is_interrupt_enabled(&self) -> Result<bool> {
        Ok((self.get_rflags()? & RFLAGS_IF) != 0)
    }
}

// Constants
const RFLAGS_IF: u64 = 1 << 9;  // Interrupt Enable Flag
```

**Tests:**
- test_rflags_read
- test_interrupt_flag_detection
- test_interrupt_flag_toggle

---

### Phase 2: Interrupt Window Mechanism (40 minutes)

**Goal:** Implement interrupt window request and handling

**Tasks:**
1. Add interrupt window request support
   - Check if hypervisor supports interrupt windows
   - Request notification when interrupts become enabled
   - Handle interrupt window exit

2. Enhance `run_with_handlers_and_interrupts()`
   - Check RFLAGS.IF before injection
   - Request interrupt window if IF=0
   - Deliver interrupt when window opens

3. Add interrupt window state tracking
   - Track if window was requested
   - Avoid redundant window requests
   - Clear state after delivery

**Code Location:** `crates/hv2-core/src/backends/whpx.rs`, `crates/hv2-core/src/exit.rs`

**Expected Output:**
```rust
impl WhpxVcpu {
    pub fn run_with_handlers_and_interrupts(
        &self,
        vm: &WhpxVm,
        pic: &crate::interrupt::Pic8259,
    ) -> Result<VmExit> {
        let mut window_requested = false;
        
        loop {
            // Check for pending interrupts
            if let Some(vector) = pic.get_pending_interrupt() {
                // Check if we can deliver
                if self.is_interrupt_enabled()? {
                    self.inject_interrupt(vector)?;
                    let irq = /* calculate IRQ */;
                    pic.acknowledge_interrupt(irq)?;
                    window_requested = false;
                } else if !window_requested {
                    // Request interrupt window
                    self.request_interrupt_window()?;
                    window_requested = true;
                }
            }
            
            let exit = self.run()?;
            
            match &exit {
                VmExit::InterruptWindow => {
                    // Window opened, try injection again
                    window_requested = false;
                    continue;
                }
                // ... other exit handling
            }
        }
    }
}
```

**Tests:**
- test_interrupt_window_request
- test_interrupt_delivery_when_if_set
- test_interrupt_blocked_when_if_clear
- test_interrupt_window_exit

---

### Phase 3: NMI Support (30 minutes)

**Goal:** Add Non-Maskable Interrupt support that bypasses IF checking

**Tasks:**
1. Add NMI injection method
   - Separate from regular interrupt injection
   - Bypasses RFLAGS.IF checking
   - Uses vector 2 by convention

2. Add NMI blocking state tracking
   - NMIs are blocked while handling NMI
   - Cleared by IRET instruction
   - Track blocking state

3. Enhance interrupt delivery for NMI priority
   - Check for pending NMIs first
   - NMIs take priority over maskable interrupts
   - Proper sequencing

**Code Location:** `crates/hv2-core/src/backends/whpx.rs`

**Expected Output:**
```rust
impl WhpxVcpu {
    pub fn inject_nmi(&self) -> Result<()> {
        // Inject NMI (vector 2) - bypasses IF check
    }
    
    pub fn is_nmi_blocked(&self) -> Result<bool> {
        // Check NMI blocking state
    }
}
```

**Tests:**
- test_nmi_injection
- test_nmi_bypasses_if
- test_nmi_blocking

---

### Phase 4: Enhanced Interrupt Delivery (30 minutes)

**Goal:** Improve interrupt delivery with proper prioritization and timing

**Tasks:**
1. Add interrupt priority handling
   - NMIs have highest priority
   - Check NMI state before maskable interrupts
   - Proper ordering

2. Add interrupt latency tracking
   - Measure time from IRQ raise to injection
   - Track interrupt delivery statistics
   - Report in execution stats

3. Optimize interrupt checking
   - Avoid redundant RFLAGS reads
   - Cache interrupt enable state per exit
   - Batch window requests

**Code Location:** `crates/hv2-core/src/backends/whpx.rs`

**Expected Output:**
```rust
pub struct InterruptStats {
    pub pending_interrupts: u64,
    pub delivered_interrupts: u64,
    pub blocked_interrupts: u64,
    pub window_requests: u64,
    pub nmis_delivered: u64,
    pub avg_latency_cycles: u64,
}
```

**Tests:**
- test_interrupt_priority
- test_interrupt_stats
- test_interrupt_latency_tracking

---

### Phase 5: Integration Tests (25 minutes)

**Goal:** Comprehensive tests for interrupt window scenarios

**Tasks:**
1. Create test scenarios
   - Interrupt with IF=1 (immediate delivery)
   - Interrupt with IF=0 (window request)
   - Multiple interrupts with window
   - NMI during masked interrupt

2. Add stress tests
   - Rapid interrupt generation
   - Window request under load
   - Priority verification

3. Add edge case tests
   - Window request cancellation
   - Interrupt during window transition
   - Multiple pending interrupts

**Code Location:** `crates/hv2-core/tests/interrupt_window.rs` (new file)

**Expected Tests:**
- test_interrupt_window_immediate_delivery
- test_interrupt_window_delayed_delivery
- test_interrupt_window_cancellation
- test_nmi_priority_over_maskable
- test_multiple_interrupts_with_window
- test_interrupt_window_stress

---

### Phase 6: Documentation and Examples (25 minutes)

**Goal:** Document interrupt window concepts and provide examples

**Tasks:**
1. Create documentation
   - Explain RFLAGS.IF
   - Describe interrupt window mechanism
   - Document NMI behavior
   - Include timing diagrams

2. Add code examples
   - Show interrupt window usage
   - Demonstrate NMI injection
   - Show statistics collection

3. Update existing documentation
   - Add interrupt window to Session 30 doc
   - Update API documentation
   - Add performance notes

**Code Location:** `docs/sessions/session_31_interrupt_windows.md`

**Expected Deliverables:**
- Complete session documentation
- API usage examples
- Timing diagrams
- Performance guidelines

---

## 📊 Expected Metrics

### Code Changes
- **Lines Added:** ~400
  - whpx.rs: ~250 lines (window handling, NMI)
  - exit.rs: ~30 lines (interrupt window exit type)
  - interrupt_window.rs: ~120 lines (tests)
- **New Files:** 1 (test file)
- **Modified Files:** 3

### Test Coverage
- **New Tests:** ~8-10
- **Test Categories:**
  - RFLAGS checking: 3 tests
  - Interrupt window: 3 tests
  - NMI support: 2 tests
  - Integration: 2-4 tests
- **Expected Pass Rate:** 100%
- **Total Tests After:** ~310

### Performance
- **Interrupt Check Overhead:** <100 cycles per exit
- **Window Request Overhead:** ~500 cycles (only when needed)
- **Overall Impact:** <1% on typical workloads

---

## 🔧 Technical Details

### Interrupt Window Flow

```
┌─────────────────┐
│ Device raises   │
│ IRQ 0           │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ PIC: pending    │
│ interrupt 0x20  │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Check RFLAGS.IF │◄──┐
└────────┬────────┘   │
         │            │
    ┌────┴────┐       │
    │ IF=1?   │       │
    └────┬────┘       │
         │            │
    Yes  │   No       │
    ┌────┴────┐       │
    │         │       │
    ▼         ▼       │
┌────────┐ ┌──────────────┐
│ Inject │ │ Request      │
│ INT    │ │ Window       │
└───┬────┘ └──────┬───────┘
    │             │
    │             ▼
    │        ┌─────────────┐
    │        │ Run vCPU    │
    │        └──────┬──────┘
    │               │
    │               ▼
    │        ┌─────────────┐
    │        │ Window Exit?│
    │        └──────┬──────┘
    │               │
    │          Yes  │
    │               │
    └───────────────┘
         │
         ▼
┌─────────────────┐
│ Acknowledge IRQ │
└─────────────────┘
```

### RFLAGS.IF (Interrupt Flag)

**Bit Position:** 9  
**Purpose:** Controls maskable interrupt delivery

**States:**
- **IF=1:** Interrupts enabled - CPU will accept maskable interrupts
- **IF=0:** Interrupts disabled - CPU blocks maskable interrupts (NMIs still delivered)

**Instructions that modify IF:**
- `STI` - Set Interrupt Flag (enables interrupts)
- `CLI` - Clear Interrupt Flag (disables interrupts)
- `IRET` - Return from interrupt (restores RFLAGS from stack)
- `POPF` - Pop flags (can modify IF if CPL allows)

### Interrupt Window vs. Interrupt Injection

**Interrupt Injection:**
- Immediate attempt to deliver interrupt
- Fails if IF=0 or other blocking conditions
- Returns error if cannot inject

**Interrupt Window:**
- Request notification when injection becomes possible
- Hypervisor monitors RFLAGS.IF
- Causes VM exit when window opens (IF transitions 0→1)
- Allows deferred interrupt delivery

**Benefits:**
- Reduces wasted injection attempts
- Guest continues executing while waiting
- Hypervisor efficiently tracks readiness

### NMI (Non-Maskable Interrupt)

**Vector:** 2  
**Purpose:** Critical system events that cannot be masked

**Characteristics:**
- Bypasses RFLAGS.IF checking
- Always delivered (unless already handling NMI)
- Used for critical errors, debugging, profiling

**Blocking:**
- NMIs are blocked while handling an NMI
- Cleared by IRET instruction
- Prevents NMI nesting

**Use Cases:**
- Hardware errors (memory parity, bus errors)
- Watchdog timers
- Profiling/performance monitoring
- Debugging (NMI button on servers)

---

## 🎓 Learning Objectives

By completing this session, you will understand:

1. **Interrupt Flag Mechanics**
   - How RFLAGS.IF controls interrupt delivery
   - When guests enable/disable interrupts
   - Impact on interrupt latency

2. **Interrupt Windows**
   - Why immediate injection isn't always possible
   - How hypervisors track interrupt readiness
   - Efficiency vs. latency tradeoffs

3. **NMI Behavior**
   - Difference between maskable and non-maskable interrupts
   - NMI blocking and nesting prevention
   - Critical interrupt handling

4. **Interrupt Prioritization**
   - NMI priority over maskable interrupts
   - Proper sequencing of multiple interrupts
   - Impact on real-time responsiveness

---

## 📝 Prerequisites Checklist

Before starting Session 31:

- [x] Session 30 complete (PIC integration)
- [x] All 301 tests passing
- [x] `run_with_handlers_and_interrupts()` implemented
- [x] PIC interrupt delivery working
- [ ] Review Intel SDM Volume 3A Section 6.8 (Interrupt and Exception Handling)
- [ ] Review RFLAGS register documentation
- [ ] Understand interrupt enable/disable mechanisms

---

## 🚀 Post-Session Goals

After completing Session 31:

1. **Full interrupt infrastructure complete**
   - Devices → PIC → Interrupt Window → vCPU injection
   - Proper timing and prioritization
   - NMI support for critical events

2. **Performance optimized**
   - Minimal overhead from IF checking
   - Efficient window requests
   - Low interrupt latency

3. **Ready for next sessions:**
   - **Session 32:** Real device integration (timer, keyboard, serial)
   - **Session 33:** Guest OS interrupt handler testing
   - **Session 34:** Advanced interrupt features (MSI, APIC)

---

## 🔍 Success Validation

Session 31 will be considered complete when:

- [x] RFLAGS.IF checking implemented
- [x] Interrupt window mechanism working
- [x] NMI support functional
- [x] All tests passing (~310 total)
- [x] Documentation complete
- [x] Example code provided
- [x] No performance regression
- [x] Code reviewed and documented

---

## 📚 References

- **Intel SDM Volume 3A**
  - Section 6.8: Interrupt and Exception Handling
  - Section 6.12: Priority Among Simultaneous Exceptions and Interrupts
  - Section 2.3: RFLAGS Register
  
- **Previous Sessions**
  - Session 28: VM Execution Loop (interrupt injection foundation)
  - Session 29: Device I/O Handling (I/O handler system)
  - Session 30: PIC Integration (interrupt controller)

- **WHPX Documentation**
  - Interrupt delivery mechanisms
  - Virtual processor state access
  - Exit reason handling

---

**Status:** Ready to begin  
**Next:** Start with Phase 1 (RFLAGS access)
