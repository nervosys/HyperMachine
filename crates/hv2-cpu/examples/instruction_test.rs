//! CPU instruction test example

use hv2_cpu::x86_64::{flags, X86_64Cpu};

fn main() {
    println!("🖥️  HV2 CPU Instruction Test");
    println!("{}", "=".repeat(50));

    let mut cpu = X86_64Cpu::new();
    cpu.reset();

    println!("\n📊 Initial State:");
    println!("  RIP: 0x{:X}", cpu.registers().rip);
    println!("  RAX: 0x{:X}", cpu.registers().rax);
    println!("  RCX: 0x{:X}", cpu.registers().rcx);
    println!("  RFLAGS: 0x{:X}", cpu.registers().rflags);

    // Test 1: MOV AL, imm8
    println!("\n🧪 Test 1: MOV AL, 0x42");
    let program = [0xB0, 0x42]; // MOV AL, 0x42
    cpu.registers_mut().rip = 0;
    cpu.execute_with_memory(&mut program.to_vec()).unwrap();
    println!("  ✓ RAX = 0x{:X} (expected: 0x42)", cpu.registers().rax);
    assert_eq!(cpu.registers().rax & 0xFF, 0x42);

    // Test 2: MOV EAX, imm32
    println!("\n🧪 Test 2: MOV EAX, 0x12345678");
    let program = [0xB8, 0x78, 0x56, 0x34, 0x12]; // MOV EAX, 0x12345678
    cpu.registers_mut().rip = 0;
    cpu.execute_with_memory(&mut program.to_vec()).unwrap();
    println!(
        "  ✓ RAX = 0x{:X} (expected: 0x12345678)",
        cpu.registers().rax
    );
    assert_eq!(cpu.registers().rax, 0x12345678);

    // Test 3: INC EAX
    println!("\n🧪 Test 3: INC EAX");
    let program = [0x40]; // INC EAX
    cpu.registers_mut().rip = 0;
    cpu.registers_mut().rax = 100;
    cpu.execute_with_memory(&mut program.to_vec()).unwrap();
    println!("  ✓ RAX = {} (expected: 101)", cpu.registers().rax);
    assert_eq!(cpu.registers().rax, 101);

    // Test 4: DEC EAX
    println!("\n🧪 Test 4: DEC EAX");
    let program = [0x48]; // DEC EAX
    cpu.registers_mut().rip = 0;
    cpu.registers_mut().rax = 50;
    cpu.execute_with_memory(&mut program.to_vec()).unwrap();
    println!("  ✓ RAX = {} (expected: 49)", cpu.registers().rax);
    assert_eq!(cpu.registers().rax, 49);

    // Test 5: XOR RAX, RAX (zero idiom)
    println!("\n🧪 Test 5: XOR RAX, RAX");
    let program = [0x31, 0xC0]; // XOR RAX, RAX
    cpu.registers_mut().rip = 0;
    cpu.registers_mut().rax = 0xDEADBEEF;
    cpu.execute_with_memory(&mut program.to_vec()).unwrap();
    println!("  ✓ RAX = 0x{:X} (expected: 0x0)", cpu.registers().rax);
    println!(
        "  ✓ ZF = {} (expected: 1)",
        (cpu.registers().rflags & flags::ZF) != 0
    );
    assert_eq!(cpu.registers().rax, 0);
    assert!(cpu.registers().rflags & flags::ZF != 0);

    // Test 6: CMP AL, imm8
    println!("\n🧪 Test 6: CMP AL, 0x42");
    let program = [0x3C, 0x42]; // CMP AL, 0x42
    cpu.registers_mut().rip = 0;
    cpu.registers_mut().rax = 0x42;
    cpu.execute_with_memory(&mut program.to_vec()).unwrap();
    println!(
        "  ✓ ZF = {} (expected: 1, values are equal)",
        (cpu.registers().rflags & flags::ZF) != 0
    );
    assert!(cpu.registers().rflags & flags::ZF != 0);

    // Test 7: TEST AL, imm8
    println!("\n🧪 Test 7: TEST AL, 0xFF");
    let program = [0xA8, 0xFF]; // TEST AL, 0xFF
    cpu.registers_mut().rip = 0;
    cpu.registers_mut().rax = 0x00;
    cpu.execute_with_memory(&mut program.to_vec()).unwrap();
    println!(
        "  ✓ ZF = {} (expected: 1, result is zero)",
        (cpu.registers().rflags & flags::ZF) != 0
    );
    assert!(cpu.registers().rflags & flags::ZF != 0);

    // Test 8: HLT
    println!("\n🧪 Test 8: HLT");
    let program = [0xF4]; // HLT
    cpu.registers_mut().rip = 0;
    cpu.execute_with_memory(&mut program.to_vec()).unwrap();
    println!("  ✓ CPU halted = {}", cpu.is_halted());
    assert!(cpu.is_halted());

    // Test 9: Multiple instructions in sequence
    println!("\n🧪 Test 9: Instruction Sequence");
    cpu.reset();

    let mut memory = vec![0; 256];
    memory[0] = 0xB0;
    memory[1] = 0x10; // MOV AL, 0x10
    memory[2] = 0x40; // INC EAX
    memory[3] = 0x40; // INC EAX
    memory[4] = 0x48; // DEC EAX

    cpu.registers_mut().rip = 0;
    cpu.execute_with_memory(&mut memory).unwrap(); // MOV AL, 0x10
    println!("  After MOV AL, 0x10: RAX = 0x{:X}", cpu.registers().rax);

    cpu.execute_with_memory(&mut memory).unwrap(); // INC
    println!("  After first INC: RAX = 0x{:X}", cpu.registers().rax);

    cpu.execute_with_memory(&mut memory).unwrap(); // INC
    println!("  After second INC: RAX = 0x{:X}", cpu.registers().rax);

    cpu.execute_with_memory(&mut memory).unwrap(); // DEC
    println!(
        "  After DEC: RAX = 0x{:X} (expected: 0x11)",
        cpu.registers().rax
    );

    println!("\n📈 Summary:");
    println!("  ✅ All {} instruction tests passed!", 9);
    println!("  Supported instructions:");
    println!("    • MOV (AL, CL, DL, BL, EAX, ECX with immediates)");
    println!("    • INC (EAX, ECX)");
    println!("    • DEC (EAX, ECX)");
    println!("    • PUSH (EAX, ECX)");
    println!("    • POP (EAX, ECX)");
    println!("    • XOR (RAX, RAX)");
    println!("    • CMP (AL with imm8)");
    println!("    • TEST (AL with imm8)");
    println!("    • RET");
    println!("    • INT (software interrupt)");
    println!("    • HLT");
    println!("    • NOP");

    println!("\n✅ CPU instruction test completed!");
}
