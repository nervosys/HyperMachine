# HV2-Core Integration Tests

This directory contains integration tests for the hv2-core hypervisor library.

## Test Files

### `interrupt_integration.rs`
End-to-end tests for the interrupt delivery system.

**Tests** (6/7 passing):
1. ✅ `test_direct_pic_irq` - Basic IRQ 0 raising and acknowledgment
2. ✅ `test_direct_pic_irq4` - IRQ 4 (serial device) validation
3. ✅ `test_interrupt_priority` - Priority ordering (IRQ 0 > IRQ 4)
4. ✅ `test_lower_irq` - IRQ raising and lowering
5. ✅ `test_multiple_irqs` - Sequential IRQ handling (0,1,3-7)
6. ✅ `test_vm_with_backend_integration` - VM/backend/PIC integration
7. ⚠️ `test_slave_pic_cascade` - **Ignored** (hangs, needs investigation)

**Known Issues**:
- IRQ 2 cannot be tested directly (it's the cascade line between master and slave PICs)
- Slave cascade test hangs in integration environment (unit test works fine)

## Running Tests

### All integration tests:
```bash
cargo test --package hv2-core --test interrupt_integration
```

### Specific test:
```bash
cargo test --package hv2-core --test interrupt_integration test_direct_pic_irq -- --exact
```

### Include ignored tests:
```bash
cargo test --package hv2-core --test interrupt_integration -- --ignored
```

### With output:
```bash
cargo test --package hv2-core --test interrupt_integration -- --nocapture
```

## Test Pattern

Tests use direct PIC manipulation rather than device-based timing:

```rust
// Create test VM with unmasked PIC
let vm = create_test_vm("test-name").await;
let pic = vm.pic();

// Raise interrupt directly
pic.raise_irq(0)?;

// Verify pending
let vector = pic.get_pending_interrupt();
assert_eq!(vector, Some(0x20));

// Acknowledge
pic.acknowledge_interrupt(0x20)?;

// Verify cleared
assert!(pic.get_pending_interrupt().is_none());
```

**Why not device-based?** Timer `tick()` depends on actual elapsed time, making tests non-deterministic. Direct PIC manipulation is deterministic and reliable.

## Helper Functions

### `create_test_vm(name: &str) -> Arc<VM>`
Creates a VM with:
- 1 vCPU
- 64 MB memory
- All PIC interrupts unmasked (IMR = 0x00)

Used by most tests for consistent setup.

## Architecture Validated

```
┌─────────┐
│ Device  │
└────┬────┘
     │ raise_irq(n)
     ▼
┌─────────────┐
│ PIC 8259    │ IRR/ISR/IMR registers
└──────┬──────┘
       │ get_pending_interrupt()
       ▼
┌─────────────┐
│ VM          │ Orchestration
└──────┬──────┘
       │ inject_interrupt()
       ▼
┌─────────────┐
│ Backend     │ WHPX/KVM/HVF
└──────┬──────┘
       │
       ▼
┌─────────────┐
│ Guest vCPU  │
└─────────────┘
```

## Notes

### PIC Initialization
PICs start with all interrupts masked (IMR = 0xFF). Tests must unmask interrupts using `set_master_mask(0x00)` and `set_slave_mask(0x00)`.

### IRQ Priority
Lower IRQ numbers have higher priority:
- IRQ 0 (Timer) - Highest priority
- IRQ 1 (Keyboard)
- IRQ 2 (Cascade) - Not usable for devices
- IRQ 3 (COM2/4)
- IRQ 4 (COM1/3)
- ...
- IRQ 15 (Secondary IDE) - Lowest priority

### Interrupt Flow
1. Device raises IRQ → IRR bit set
2. PIC checks: IRR & ~IMR & ~ISR
3. VM gets pending interrupt → Vector returned
4. Backend injects interrupt → Guest receives
5. Guest acknowledges → IRR cleared, ISR set
6. Guest sends EOI → ISR cleared

## Future Tests

Potential additions:
- EOI handling validation
- Auto-EOI mode testing
- Rotate priority mode
- Stress tests (rapid interrupts)
- Latency measurements
- Error cases (invalid IRQs, double acknowledgment)
