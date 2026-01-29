# Session 32: Real Device Integration Plan

**Status**: 🔄 Ready to Begin  
**Dependencies**: Session 29 (I/O Handlers), Session 30 (PIC), Session 31 (Interrupt Windows)  
**Estimated Duration**: 3-4 hours  
**Expected Test Count**: 330+ tests (from current 314)

## 🎯 Objective

Integrate real hardware devices (Timer, Keyboard, Serial Port) with the complete interrupt delivery pipeline, enabling them to generate interrupts that flow through the PIC and are properly delivered to the guest CPU.

## 🏗️ Architecture Overview

### Current State (After Session 31)

```
┌─────────────┐
│   Devices   │ Partially implemented (no interrupt integration)
│ (Timer, KB) │
└─────────────┘
      ↓ (missing connection)
┌─────────────┐
│  8259 PIC   │ ✅ Complete (Session 30)
│   IRQ Bus   │
└─────────────┘
      ↓
┌─────────────┐
│ Int Window  │ ✅ Complete (Session 31)
│ (RFLAGS.IF) │
└─────────────┘
      ↓
┌─────────────┐
│    vCPU     │ ✅ Complete
│  Injection  │
└─────────────┘
```

### Target State (After Session 32)

```
┌─────────────────────────────────────────────────────────┐
│                    Device Layer                         │
├─────────────┬─────────────┬─────────────┬───────────────┤
│   Timer     │  Keyboard   │   Serial    │     VGA       │
│  (8254 PIT) │  (8042 PS/2)│  (16550)    │  (Text Mode)  │
│             │             │             │               │
│  IRQ 0      │  IRQ 1      │  IRQ 3,4    │  (no IRQ)     │
└─────┬───────┴──────┬──────┴──────┬──────┴───────────────┘
      │              │             │
      └──────────────┼─────────────┘
                     ↓
          ┌──────────────────┐
          │    8259 PIC      │ Centralized IRQ management
          │  (Master/Slave)  │
          └────────┬─────────┘
                   ↓
          ┌──────────────────┐
          │ Interrupt Window │ RFLAGS.IF checking
          │   (Session 31)   │
          └────────┬─────────┘
                   ↓
          ┌──────────────────┐
          │  vCPU Injection  │ Hypervisor delivery
          │   (Session 28)   │
          └──────────────────┘
```

## 📋 Implementation Phases

### Phase 1: Timer Integration (IRQ 0) ⏱️

**Duration**: 45 minutes  
**Goal**: Make 8254 PIT generate interrupts at 18.2 Hz

#### Current Timer Implementation

```rust
// crates/hv2-core/src/devices/timer.rs
pub struct Pit8254 {
    channels: [PitChannel; 3],
    last_tick: Instant,
}
```

**Issues**:
- No PIC reference - can't raise IRQs
- No background task - timing is passive
- Channels configured but don't fire interrupts

#### Required Changes

1. **Add PIC reference to Timer**
   ```rust
   pub struct Pit8254 {
       channels: [PitChannel; 3],
       last_tick: Instant,
       pic: Arc<Pic8259>,  // ← NEW: Reference to interrupt controller
   }
   ```

2. **Create timer tick task**
   ```rust
   impl Pit8254 {
       pub fn start_timer_task(&self) {
           let pic = Arc::clone(&self.pic);
           let interval = Duration::from_micros(54925); // 18.2 Hz
           
           tokio::spawn(async move {
               let mut interval = tokio::time::interval(interval);
               loop {
                   interval.tick().await;
                   pic.raise_irq(0); // Timer IRQ
               }
           });
       }
   }
   ```

3. **Update constructor**
   ```rust
   pub fn new(pic: Arc<Pic8259>) -> Self {
       let timer = Self {
           channels: [
               PitChannel::new(0),
               PitChannel::new(1),
               PitChannel::new(2),
           ],
           last_tick: Instant::now(),
           pic,
       };
       timer.start_timer_task();
       timer
   }
   ```

#### Tests

- `test_timer_generates_irq0`: Verify IRQ 0 raised after interval
- `test_timer_frequency`: Verify 18.2 Hz tick rate
- `test_timer_pic_integration`: Verify timer + PIC + vCPU flow

---

### Phase 2: Keyboard Integration (IRQ 1) ⌨️

**Duration**: 45 minutes  
**Goal**: Make PS/2 keyboard controller generate interrupts on keypress

#### Current Keyboard Implementation

```rust
// crates/hv2-core/src/devices/keyboard.rs
pub struct Ps2Controller {
    output_buffer: VecDeque<u8>,
    input_buffer: VecDeque<u8>,
    status: u8,
    command_byte: u8,
}
```

**Issues**:
- No PIC reference
- Data queued but no interrupt signaling
- Guest would have to poll for keypresses

#### Required Changes

1. **Add PIC reference**
   ```rust
   pub struct Ps2Controller {
       output_buffer: VecDeque<u8>,
       input_buffer: VecDeque<u8>,
       status: u8,
       command_byte: u8,
       pic: Arc<Pic8259>,  // ← NEW
   }
   ```

2. **Raise IRQ when data available**
   ```rust
   impl Ps2Controller {
       pub fn send_scancode(&mut self, scancode: u8) {
           self.output_buffer.push_back(scancode);
           self.status |= STATUS_OBF; // Output Buffer Full
           
           // Raise interrupt if enabled
           if (self.command_byte & CCB_INT_KEYBOARD) != 0 {
               self.pic.raise_irq(1); // ← NEW
           }
       }
       
       pub fn read_data(&mut self) -> u8 {
           let data = self.output_buffer.pop_front().unwrap_or(0);
           
           if self.output_buffer.is_empty() {
               self.status &= !STATUS_OBF;
               self.pic.clear_irq(1); // ← NEW
           }
           
           data
       }
   }
   ```

3. **Keyboard event injection API**
   ```rust
   impl Ps2Controller {
       pub fn inject_key(&mut self, key: Key, pressed: bool) {
           let scancode = key_to_scancode(key, pressed);
           self.send_scancode(scancode);
       }
   }
   
   pub enum Key {
       A, B, C, /* ... */ Z,
       Num0, Num1, /* ... */ Num9,
       Enter, Escape, Space, Tab,
       LeftShift, RightShift, Ctrl, Alt,
       // etc.
   }
   ```

#### Tests

- `test_keyboard_irq_on_scancode`: Verify IRQ 1 raised when key pressed
- `test_keyboard_irq_clear_on_read`: Verify IRQ cleared after reading
- `test_keyboard_interrupt_enable`: Verify CCB interrupt enable flag
- `test_key_injection`: Verify API for sending key events

---

### Phase 3: Serial Port Integration (IRQ 3/4) 📡

**Duration**: 45 minutes  
**Goal**: Make UART generate interrupts for transmit/receive

#### Current Serial Implementation

```rust
// crates/hv2-core/src/devices/serial.rs
pub struct SerialPort {
    id: u8,
    base_port: u16,
    data_buffer: VecDeque<u8>,
    interrupt_enable: u8,
    line_status: u8,
}
```

**Issues**:
- No PIC reference
- Interrupt enable register exists but not used
- No actual interrupt generation

#### Required Changes

1. **Add PIC reference and IRQ mapping**
   ```rust
   pub struct SerialPort {
       id: u8,
       base_port: u16,
       irq: u8,  // ← NEW: COM1=IRQ4, COM2=IRQ3
       data_buffer: VecDeque<u8>,
       interrupt_enable: u8,
       line_status: u8,
       pic: Arc<Pic8259>,  // ← NEW
   }
   ```

2. **Interrupt generation on events**
   ```rust
   impl SerialPort {
       pub fn receive_byte(&mut self, byte: u8) {
           self.data_buffer.push_back(byte);
           self.line_status |= LSR_DATA_READY;
           
           // Raise interrupt if enabled
           if (self.interrupt_enable & IER_RECEIVED_DATA) != 0 {
               self.pic.raise_irq(self.irq); // ← NEW
           }
       }
       
       pub fn transmit_complete(&mut self) {
           self.line_status |= LSR_THR_EMPTY;
           
           if (self.interrupt_enable & IER_THR_EMPTY) != 0 {
               self.pic.raise_irq(self.irq); // ← NEW
           }
       }
       
       pub fn read_data(&mut self) -> u8 {
           let data = self.data_buffer.pop_front().unwrap_or(0);
           
           if self.data_buffer.is_empty() {
               self.line_status &= !LSR_DATA_READY;
               self.pic.clear_irq(self.irq); // ← NEW
           }
           
           data
       }
   }
   ```

3. **Factory methods for COM1/COM2**
   ```rust
   impl SerialPort {
       pub fn com1(pic: Arc<Pic8259>) -> Self {
           Self::new(0x3F8, 4, pic) // COM1: port 0x3F8, IRQ 4
       }
       
       pub fn com2(pic: Arc<Pic8259>) -> Self {
           Self::new(0x2F8, 3, pic) // COM2: port 0x2F8, IRQ 3
       }
   }
   ```

#### Tests

- `test_serial_rx_interrupt`: Verify IRQ on byte received
- `test_serial_tx_interrupt`: Verify IRQ on transmit complete
- `test_serial_interrupt_enable`: Verify IER register control
- `test_serial_com1_irq4`: Verify COM1 uses IRQ 4
- `test_serial_com2_irq3`: Verify COM2 uses IRQ 3

---

### Phase 4: VGA Text Mode Output (No IRQ) 🖥️

**Duration**: 30 minutes  
**Goal**: Polish VGA text mode for debugging output (non-interrupt device)

#### Current VGA Implementation

```rust
// crates/hv2-core/src/devices/vga.rs
pub struct VgaController {
    framebuffer: Vec<u8>,
    cursor_x: u8,
    cursor_y: u8,
    attribute: u8,
}
```

**Status**: Already functional, no interrupt support needed

#### Required Changes

1. **Add scroll support**
   ```rust
   impl VgaController {
       fn scroll_up(&mut self) {
           // Move all lines up by one
           self.framebuffer.copy_within(160..4000, 0);
           // Clear last line
           for i in 0..80 {
               self.framebuffer[3840 + i * 2] = b' ';
               self.framebuffer[3840 + i * 2 + 1] = self.attribute;
           }
       }
   }
   ```

2. **Character output improvements**
   ```rust
   impl VgaController {
       pub fn put_char(&mut self, c: char) {
           match c {
               '\n' => {
                   self.cursor_x = 0;
                   self.cursor_y += 1;
                   if self.cursor_y >= 25 {
                       self.scroll_up();
                       self.cursor_y = 24;
                   }
               }
               '\r' => self.cursor_x = 0,
               '\t' => self.cursor_x = (self.cursor_x + 8) & !7,
               c => {
                   let offset = ((self.cursor_y as usize * 80) + self.cursor_x as usize) * 2;
                   self.framebuffer[offset] = c as u8;
                   self.framebuffer[offset + 1] = self.attribute;
                   self.cursor_x += 1;
                   if self.cursor_x >= 80 {
                       self.cursor_x = 0;
                       self.cursor_y += 1;
                       if self.cursor_y >= 25 {
                           self.scroll_up();
                           self.cursor_y = 24;
                       }
                   }
               }
           }
       }
   }
   ```

#### Tests

- `test_vga_scroll`: Verify scrolling when line 25 reached
- `test_vga_newline`: Verify newline handling
- `test_vga_tab`: Verify tab expansion

---

### Phase 5: Unified Device Manager 🎛️

**Duration**: 45 minutes  
**Goal**: Create centralized device management system

#### Architecture

```rust
// crates/hv2-core/src/devices/mod.rs

pub struct DeviceManager {
    timer: Arc<Mutex<Pit8254>>,
    keyboard: Arc<Mutex<Ps2Controller>>,
    serial_ports: Vec<Arc<Mutex<SerialPort>>>,
    vga: Arc<Mutex<VgaController>>,
    pic: Arc<Pic8259>,
}

impl DeviceManager {
    pub fn new(pic: Arc<Pic8259>) -> Self {
        let timer = Arc::new(Mutex::new(Pit8254::new(Arc::clone(&pic))));
        let keyboard = Arc::new(Mutex::new(Ps2Controller::new(Arc::clone(&pic))));
        let serial_ports = vec![
            Arc::new(Mutex::new(SerialPort::com1(Arc::clone(&pic)))),
            Arc::new(Mutex::new(SerialPort::com2(Arc::clone(&pic)))),
        ];
        let vga = Arc::new(Mutex::new(VgaController::new()));
        
        Self {
            timer,
            keyboard,
            serial_ports,
            vga,
            pic,
        }
    }
    
    pub fn register_all_io_handlers(&self, vm: &mut WhpxVm) -> Result<()> {
        // Timer (ports 0x40-0x43)
        vm.register_io_handler(0x40, 4, self.timer.create_io_handler())?;
        
        // Keyboard (ports 0x60, 0x64)
        vm.register_io_handler(0x60, 1, self.keyboard.create_io_handler())?;
        vm.register_io_handler(0x64, 1, self.keyboard.create_io_handler())?;
        
        // COM1 (ports 0x3F8-0x3FF)
        vm.register_io_handler(0x3F8, 8, self.serial_ports[0].create_io_handler())?;
        
        // COM2 (ports 0x2F8-0x2FF)
        vm.register_io_handler(0x2F8, 8, self.serial_ports[1].create_io_handler())?;
        
        // VGA text mode (ports 0x3D4-0x3D5, MMIO at 0xB8000)
        vm.register_io_handler(0x3D4, 2, self.vga.create_io_handler())?;
        vm.register_mmio_handler(0xB8000, 0x1000, self.vga.create_mmio_handler())?;
        
        // PIC (ports 0x20-0x21, 0xA0-0xA1)
        vm.register_io_handler(0x20, 2, self.pic.create_io_handler())?;
        vm.register_io_handler(0xA0, 2, self.pic.create_io_handler())?;
        
        Ok(())
    }
    
    pub fn keyboard(&self) -> Arc<Mutex<Ps2Controller>> {
        Arc::clone(&self.keyboard)
    }
    
    pub fn serial(&self, port: usize) -> Option<Arc<Mutex<SerialPort>>> {
        self.serial_ports.get(port).map(Arc::clone)
    }
    
    pub fn vga(&self) -> Arc<Mutex<VgaController>> {
        Arc::clone(&self.vga)
    }
}
```

#### Tests

- `test_device_manager_creation`: Verify all devices initialized
- `test_register_all_handlers`: Verify I/O handler registration
- `test_device_manager_accessors`: Verify getter methods

---

### Phase 6: End-to-End Integration Test 🧪

**Duration**: 30 minutes  
**Goal**: Test complete interrupt flow from device to guest

#### Comprehensive Test

```rust
#[tokio::test]
async fn test_timer_interrupt_delivery_e2e() {
    // Create VM with all devices
    let backend = WhpxBackend::new().unwrap();
    let mut vm = backend.create_vm(1, 16 * 1024 * 1024).await.unwrap();
    let vcpu = vm.create_vcpu(0).unwrap();
    
    // Create PIC and device manager
    let pic = Arc::new(Pic8259::new());
    let devices = DeviceManager::new(Arc::clone(&pic));
    
    // Register all device I/O handlers
    devices.register_all_io_handlers(&mut vm).unwrap();
    
    // Set up guest code:
    // 1. Initialize PIC (ICW1-ICW4)
    // 2. Enable timer interrupt (unmask IRQ 0)
    // 3. Set up IDT entry for INT 0x20
    // 4. Execute STI (enable interrupts)
    // 5. HLT (wait for interrupt)
    
    let guest_code = assemble_timer_test_code();
    vm.write_guest_memory(0x1000, &guest_code).unwrap();
    
    // Set up interrupt handler at 0x2000
    let handler_code = vec![
        0x50,       // PUSH RAX
        0xB0, 0x20, // MOV AL, 0x20
        0xE6, 0x20, // OUT 0x20, AL  (send EOI)
        0x58,       // POP RAX
        0xCF,       // IRETQ
    ];
    vm.write_guest_memory(0x2000, &handler_code).unwrap();
    
    // Set up IDT entry
    setup_idt_entry(&vm, 0x20, 0x2000).unwrap();
    
    // Set entry point
    let mut regs = vcpu.get_register_set().unwrap();
    regs.rip = 0x1000;
    vcpu.set_register_set(&regs).unwrap();
    
    // Run until interrupt fires
    let mut interrupt_count = 0;
    let start = Instant::now();
    
    while start.elapsed() < Duration::from_secs(1) {
        match vcpu.run_with_handlers_and_interrupts(&pic) {
            Ok(VmExit::Halted) => {
                // HLT executed - wait for timer
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            Ok(VmExit::IoOut { port: 0x20, value: 0x20, .. }) => {
                // EOI received - interrupt was handled!
                interrupt_count += 1;
                if interrupt_count >= 5 {
                    break; // Success after 5 interrupts
                }
            }
            Ok(_) => continue,
            Err(e) => panic!("Execution error: {}", e),
        }
    }
    
    assert!(interrupt_count >= 5, "Should receive at least 5 timer interrupts");
    
    let stats = vcpu.get_interrupt_stats();
    println!("Interrupt stats: {:#?}", stats);
    assert!(stats.interrupts_injected >= 5);
}
```

#### Additional E2E Tests

- `test_keyboard_interrupt_e2e`: Full keyboard press → interrupt → handler flow
- `test_serial_interrupt_e2e`: Serial receive → interrupt → handler flow
- `test_multiple_devices_interrupts`: Multiple devices generating interrupts
- `test_interrupt_priority`: Verify PIC priority handling

---

## 📊 Expected Outcomes

### Functionality

1. **Timer interrupts at 18.2 Hz**
   - Background task generates periodic IRQ 0
   - Guest receives INT 0x20 every ~55ms
   - Verified via interrupt statistics

2. **Keyboard interrupts on keypresses**
   - API for injecting key events
   - IRQ 1 raised when scancode available
   - Guest can read from port 0x60

3. **Serial port interrupts**
   - IRQ 3 (COM2) and IRQ 4 (COM1)
   - Interrupts on receive/transmit
   - Proper IER register control

4. **VGA text output working**
   - Character display
   - Scrolling
   - Cursor positioning

5. **Unified device management**
   - Single initialization point
   - Automatic I/O handler registration
   - Easy device access

### Test Coverage

- **Before**: 314 tests
- **After**: 330+ tests
- **New tests**: ~16-20 integration tests

### Performance

- Timer accuracy: ±1ms (54.925ms target)
- Interrupt latency: <10μs (when IF=1)
- No measurable overhead on non-interrupt operations

## 🔍 Success Criteria

Session 32 complete when:

- [ ] Timer generates IRQ 0 at 18.2 Hz
- [ ] Keyboard generates IRQ 1 on key injection
- [ ] Serial ports generate IRQ 3/4 on events
- [ ] VGA text mode fully functional
- [ ] DeviceManager working
- [ ] All tests passing (330+)
- [ ] E2E test demonstrating timer interrupt delivery
- [ ] Documentation complete
- [ ] No performance regression

## 📚 References

### Hardware Documentation

- **8254 PIT**: Intel 8254 Programmable Interval Timer datasheet
- **8042 PS/2**: IBM PS/2 Keyboard Controller specification
- **16550 UART**: National Semiconductor 16550 UART datasheet
- **VGA**: IBM VGA Hardware Reference

### Previous Sessions

- **Session 29**: I/O Handler System
- **Session 30**: PIC Integration
- **Session 31**: Interrupt Window Handling

### Useful Resources

- OSDev Wiki: Timer, Keyboard, Serial Port pages
- QEMU source: Device emulation examples
- Linux kernel: Device driver implementations

---

**Status**: 🔄 Ready to Begin  
**Next Step**: Phase 1 - Timer Integration
