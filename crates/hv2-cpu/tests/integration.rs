//! Integration tests for hv2-cpu: cross-architecture instruction execution,
//! register manipulation, memory I/O, interrupt handling, and CPU lifecycle.

// AArch64 instruction encodings below use bit-field grouped binary literals
// (e.g. sf|opc|imm|Rn|Rd) which trigger clippy::unusual_byte_groupings.
#![allow(clippy::unusual_byte_groupings)]

use hv2_cpu::aarch64::{
    self, AArch64Cpu, MemoryAccess as AArch64MemoryAccess, SliceMemory as AArch64SliceMemory,
};
use hv2_cpu::x86_64::{flags, MemoryAccess, SliceMemory, X86_64Cpu};

// ─── X86-64: Instruction Execution ─────────────────────────────────

#[test]
fn x86_mov_al_immediate() {
    let mut cpu = X86_64Cpu::new();
    cpu.registers_mut().rip = 0;
    let mut program = vec![0xB0, 0x42]; // MOV AL, 0x42
    cpu.execute_with_memory(&mut program).unwrap();
    assert_eq!(cpu.registers().rax & 0xFF, 0x42);
}

#[test]
fn x86_mov_eax_imm32() {
    let mut cpu = X86_64Cpu::new();
    cpu.registers_mut().rip = 0;
    // MOV EAX, 0x12345678
    let mut program = vec![0xB8, 0x78, 0x56, 0x34, 0x12];
    cpu.execute_with_memory(&mut program).unwrap();
    assert_eq!(cpu.registers().rax, 0x12345678);
}

#[test]
fn x86_inc_dec_eax() {
    let mut cpu = X86_64Cpu::new();

    // INC EAX from 100 → 101
    cpu.registers_mut().rip = 0;
    cpu.registers_mut().rax = 100;
    let mut prog = vec![0x40]; // INC EAX
    cpu.execute_with_memory(&mut prog).unwrap();
    assert_eq!(cpu.registers().rax, 101);

    // DEC EAX from 101 → 100
    cpu.registers_mut().rip = 0;
    let mut prog = vec![0x48]; // DEC EAX
    cpu.execute_with_memory(&mut prog).unwrap();
    assert_eq!(cpu.registers().rax, 100);
}

#[test]
fn x86_xor_zero_idiom_sets_zf() {
    let mut cpu = X86_64Cpu::new();
    cpu.registers_mut().rip = 0;
    cpu.registers_mut().rax = 0xDEADBEEF;
    let mut prog = vec![0x31, 0xC0]; // XOR EAX, EAX
    cpu.execute_with_memory(&mut prog).unwrap();
    assert_eq!(cpu.registers().rax, 0);
    assert_ne!(
        cpu.registers().rflags & flags::ZF,
        0,
        "ZF should be set after XOR zero idiom"
    );
}

#[test]
fn x86_cmp_equal_sets_zf() {
    let mut cpu = X86_64Cpu::new();
    cpu.registers_mut().rip = 0;
    cpu.registers_mut().rax = 0x42;
    let mut prog = vec![0x3C, 0x42]; // CMP AL, 0x42
    cpu.execute_with_memory(&mut prog).unwrap();
    assert_ne!(
        cpu.registers().rflags & flags::ZF,
        0,
        "ZF should be set when CMP operands equal"
    );
}

#[test]
fn x86_test_zero_sets_zf() {
    let mut cpu = X86_64Cpu::new();
    cpu.registers_mut().rip = 0;
    cpu.registers_mut().rax = 0x00;
    let mut prog = vec![0xA8, 0xFF]; // TEST AL, 0xFF
    cpu.execute_with_memory(&mut prog).unwrap();
    assert_ne!(
        cpu.registers().rflags & flags::ZF,
        0,
        "ZF should be set when TEST result is zero"
    );
}

#[test]
fn x86_hlt_halts_cpu() {
    let mut cpu = X86_64Cpu::new();
    cpu.registers_mut().rip = 0;
    let mut prog = vec![0xF4]; // HLT
    cpu.execute_with_memory(&mut prog).unwrap();
    assert!(cpu.is_halted());
}

#[test]
fn x86_nop_advances_rip() {
    let mut cpu = X86_64Cpu::new();
    cpu.registers_mut().rip = 0;
    let mut mem = vec![0x90; 16]; // NOP sled
    cpu.execute_with_memory(&mut mem).unwrap();
    assert_eq!(cpu.registers().rip, 1, "NOP should advance RIP by 1");
}

// ─── X86-64: Multi-instruction Sequences ───────────────────────────

#[test]
fn x86_instruction_sequence() {
    let mut cpu = X86_64Cpu::new();
    cpu.reset();

    let mut memory = vec![0u8; 256];
    memory[0] = 0xB0;
    memory[1] = 0x10; // MOV AL, 0x10
    memory[2] = 0x40; // INC EAX
    memory[3] = 0x40; // INC EAX
    memory[4] = 0x48; // DEC EAX

    cpu.registers_mut().rip = 0;
    cpu.execute_with_memory(&mut memory).unwrap(); // MOV AL, 0x10
    assert_eq!(cpu.registers().rax & 0xFF, 0x10);

    cpu.execute_with_memory(&mut memory).unwrap(); // INC
    assert_eq!(cpu.registers().rax, 0x11);

    cpu.execute_with_memory(&mut memory).unwrap(); // INC
    assert_eq!(cpu.registers().rax, 0x12);

    cpu.execute_with_memory(&mut memory).unwrap(); // DEC
    assert_eq!(cpu.registers().rax, 0x11);
}

// ─── X86-64: Reset & Lifecycle ─────────────────────────────────────

#[test]
fn x86_reset_restores_initial_state() {
    let mut cpu = X86_64Cpu::new();
    cpu.registers_mut().rax = 0xDEAD;
    cpu.registers_mut().rbx = 0xBEEF;
    cpu.registers_mut().rip = 0x9999;
    cpu.reset();

    // After reset, RIP should be at the standard real-mode entry point
    assert_eq!(cpu.registers().rip, 0xFFF0);
    assert_eq!(cpu.registers().rax, 0);
    assert!(!cpu.is_halted());
}

// ─── X86-64: Interrupts ────────────────────────────────────────────

#[test]
fn x86_queue_interrupt_marks_pending() {
    let mut cpu = X86_64Cpu::new();
    assert!(!cpu.has_pending_interrupt());

    // Enable interrupts (set IF) so regular interrupts are deliverable
    cpu.registers_mut().rflags |= flags::IF;
    cpu.queue_interrupt(0x20, None);
    assert!(cpu.has_pending_interrupt());
}

#[test]
fn x86_queue_nmi_marks_pending() {
    let mut cpu = X86_64Cpu::new();
    assert!(!cpu.has_pending_interrupt());

    cpu.queue_nmi();
    assert!(cpu.has_pending_interrupt());
}

// ─── X86-64: SliceMemory trait impl ────────────────────────────────

#[test]
fn x86_slice_memory_read_write_roundtrip() {
    let mut data = vec![0u8; 256];
    {
        let mut mem = SliceMemory::new(&mut data);
        mem.write_u8(0, 0x42).unwrap();
        mem.write_u16(2, 0xBEEF).unwrap();
        mem.write_u32(8, 0xCAFEBABE).unwrap();
        mem.write_u64(16, 0xDEAD_BEEF_1234_5678).unwrap();

        assert_eq!(mem.read_u8(0).unwrap(), 0x42);
        assert_eq!(mem.read_u16(2).unwrap(), 0xBEEF);
        assert_eq!(mem.read_u32(8).unwrap(), 0xCAFEBABE);
        assert_eq!(mem.read_u64(16).unwrap(), 0xDEAD_BEEF_1234_5678);
    }
}

#[test]
fn x86_slice_memory_out_of_bounds() {
    let mut data = vec![0u8; 4];
    let mem = SliceMemory::new(&mut data);
    // Reading past the end should fail
    assert!(mem.read_u64(0).is_err());
    assert!(mem.read_u32(2).is_err());
}

// ─── AArch64: Instruction Execution ────────────────────────────────

#[test]
fn aarch64_add_immediate() {
    let mut cpu = AArch64Cpu::new();
    cpu.set_xreg(1, 100);

    // ADD X0, X1, #42 (64-bit)
    let opcode: u32 = 0b1_00_100010_0_000000101010_00001_00000;
    let mut mem = vec![0u8; 4];
    let mut memory = AArch64SliceMemory::new(&mut mem);
    cpu.execute_instruction(opcode, &mut memory).unwrap();

    assert_eq!(cpu.get_xreg(0), 142);
}

#[test]
fn aarch64_sub_immediate() {
    let mut cpu = AArch64Cpu::new();
    cpu.set_xreg(1, 200);

    // SUB X0, X1, #50 (64-bit)
    let opcode: u32 = 0b1_10_100010_0_000000110010_00001_00000;
    let mut mem = vec![0u8; 4];
    let mut memory = AArch64SliceMemory::new(&mut mem);
    cpu.execute_instruction(opcode, &mut memory).unwrap();

    assert_eq!(cpu.get_xreg(0), 150);
}

#[test]
fn aarch64_movz() {
    let mut cpu = AArch64Cpu::new();

    // MOVZ X0, #0x1234
    let opcode: u32 = 0b1_10_100101_00_0001001000110100_00000;
    let mut mem = vec![0u8; 4];
    let mut memory = AArch64SliceMemory::new(&mut mem);
    cpu.execute_instruction(opcode, &mut memory).unwrap();

    assert_eq!(cpu.get_xreg(0), 0x1234);
}

#[test]
fn aarch64_store_and_load() {
    let mut cpu = AArch64Cpu::new();
    cpu.set_xreg(0, 0xCAFEBABE);
    cpu.set_xreg(1, 0x100);

    // STR W0, [X1] (32-bit store, unsigned imm offset 0)
    let str_opcode: u32 = 0b10_111001_00_000000000000_00001_00000;
    let mut mem = vec![0u8; 0x200];
    let mut memory = AArch64SliceMemory::new(&mut mem);
    cpu.execute_instruction(str_opcode, &mut memory).unwrap();

    assert_eq!(memory.read_u32(0x100).unwrap(), 0xCAFEBABE);

    // LDR W2, [X1] (32-bit load, same offset)
    let ldr_opcode: u32 = 0b10_111001_01_000000000000_00001_00010;
    cpu.execute_instruction(ldr_opcode, &mut memory).unwrap();

    assert_eq!(cpu.get_wreg(2), 0xCAFEBABE);
}

#[test]
fn aarch64_branch_unconditional() {
    let mut cpu = AArch64Cpu::new();
    cpu.registers_mut().pc = 0x1000;

    // B +16 (imm26 = 4 instructions)
    let opcode: u32 = 0b000101_00000000000000000000000100;
    let mut mem = vec![0u8; 0x2000];
    let mut memory = AArch64SliceMemory::new(&mut mem);
    cpu.execute_instruction(opcode, &mut memory).unwrap();

    assert_eq!(cpu.registers().pc, 0x1010);
}

#[test]
fn aarch64_branch_with_link() {
    let mut cpu = AArch64Cpu::new();
    cpu.registers_mut().pc = 0x1000;

    // BL +8 (imm26 = 2)
    let opcode: u32 = 0b100101_00000000000000000000000010;
    let mut mem = vec![0u8; 0x2000];
    let mut memory = AArch64SliceMemory::new(&mut mem);
    cpu.execute_instruction(opcode, &mut memory).unwrap();

    assert_eq!(cpu.registers().pc, 0x1008);
    // X30 (link register) should hold return address
    assert_eq!(cpu.get_xreg(30), 0x1004);
}

#[test]
fn aarch64_nop_advances_pc() {
    let mut cpu = AArch64Cpu::new();
    let pc_before = cpu.registers().pc;

    // NOP
    let opcode: u32 = 0b1101010100_0_00_011_0011_0000_000_11111;
    let mut mem = vec![0u8; 0x100];
    let mut memory = AArch64SliceMemory::new(&mut mem);
    cpu.execute_instruction(opcode, &mut memory).unwrap();

    assert_eq!(cpu.registers().pc, pc_before + 4);
}

#[test]
fn aarch64_wfi_sets_waiting() {
    let mut cpu = AArch64Cpu::new();

    // WFI
    let opcode: u32 = 0b1101010100_0_00_011_0011_0010_001_11111;
    let mut mem = vec![0u8; 0x100];
    let mut memory = AArch64SliceMemory::new(&mut mem);
    cpu.execute_instruction(opcode, &mut memory).unwrap();

    assert!(cpu.is_waiting());
}

// ─── AArch64: Register Access ──────────────────────────────────────

#[test]
fn aarch64_xzr_reads_zero_writes_silent() {
    let mut cpu = AArch64Cpu::new();

    // XZR (register 31) always reads as zero
    assert_eq!(cpu.get_xreg(31), 0);

    // Writing to XZR is silently discarded
    cpu.set_xreg(31, 0xFFFF_FFFF_FFFF_FFFF);
    assert_eq!(cpu.get_xreg(31), 0);
}

#[test]
fn aarch64_wreg_zero_extends() {
    let mut cpu = AArch64Cpu::new();

    // Set 64-bit value first
    cpu.set_xreg(5, 0xFFFF_FFFF_FFFF_FFFF);
    // Setting 32-bit view should zero-extend to 64 bits
    cpu.set_wreg(5, 0xCAFE_BABE);
    assert_eq!(cpu.get_xreg(5), 0xCAFE_BABE);
    assert_eq!(cpu.get_wreg(5), 0xCAFE_BABE);
}

// ─── AArch64: Reset & Lifecycle ────────────────────────────────────

#[test]
fn aarch64_reset_restores_initial_state() {
    let mut cpu = AArch64Cpu::new();
    cpu.set_xreg(0, 0xDEAD);
    cpu.registers_mut().pc = 0x9999;
    cpu.reset();

    assert_eq!(cpu.registers().pc, 0);
    assert_eq!(cpu.get_xreg(0), 0);
    assert!(!cpu.is_waiting());
}

// ─── AArch64: Interrupts ───────────────────────────────────────────

#[test]
fn aarch64_irq_wakes_from_wfi() {
    use hv2_cpu::aarch64::pstate;
    let mut cpu = AArch64Cpu::new();

    // Unmask IRQs so raise_irq can wake the CPU
    cpu.registers_mut().pstate &= !pstate::I;

    // Put CPU in WFI state
    let wfi_opcode: u32 = 0b1101010100_0_00_011_0011_0010_001_11111;
    let mut mem = vec![0u8; 0x100];
    let mut memory = AArch64SliceMemory::new(&mut mem);
    cpu.execute_instruction(wfi_opcode, &mut memory).unwrap();
    assert!(cpu.is_waiting());

    // Raise IRQ — should wake the CPU
    cpu.raise_irq();
    assert!(!cpu.is_waiting(), "IRQ should wake CPU from WFI");
}

#[test]
fn aarch64_fiq_wakes_from_wfi() {
    use hv2_cpu::aarch64::pstate;
    let mut cpu = AArch64Cpu::new();

    // Unmask FIQs so raise_fiq can wake the CPU
    cpu.registers_mut().pstate &= !pstate::F;

    // Put CPU in WFI state
    let wfi_opcode: u32 = 0b1101010100_0_00_011_0011_0010_001_11111;
    let mut mem = vec![0u8; 0x100];
    let mut memory = AArch64SliceMemory::new(&mut mem);
    cpu.execute_instruction(wfi_opcode, &mut memory).unwrap();
    assert!(cpu.is_waiting());

    cpu.raise_fiq();
    assert!(!cpu.is_waiting(), "FIQ should wake CPU from WFI");
}

// ─── AArch64: System Registers ─────────────────────────────────────

#[test]
fn aarch64_system_register_roundtrip() {
    let mut cpu = AArch64Cpu::new();

    cpu.write_sys_reg(aarch64::SystemRegId::SCTLR_EL1, 0x0000_0000_3050_5070);
    assert_eq!(
        cpu.read_sys_reg(aarch64::SystemRegId::SCTLR_EL1),
        0x0000_0000_3050_5070
    );

    cpu.write_sys_reg(aarch64::SystemRegId::MAIR_EL1, 0xFF440C0400);
    assert_eq!(
        cpu.read_sys_reg(aarch64::SystemRegId::MAIR_EL1),
        0xFF440C0400
    );
}

// ─── AArch64: SliceMemory ──────────────────────────────────────────

#[test]
fn aarch64_slice_memory_roundtrip() {
    let mut data = vec![0u8; 256];
    {
        let mut mem = AArch64SliceMemory::new(&mut data);
        mem.write_u32(0x10, 0xDEADBEEF).unwrap();
        assert_eq!(mem.read_u32(0x10).unwrap(), 0xDEADBEEF);

        mem.write_u64(0x20, 0x1234_5678_9ABC_DEF0).unwrap();
        assert_eq!(mem.read_u64(0x20).unwrap(), 0x1234_5678_9ABC_DEF0);
    }
}

// ─── Cross-architecture: Both CPUs initialize consistently ─────────

#[test]
fn both_cpus_start_not_halted() {
    let x86 = X86_64Cpu::new();
    let aarch64 = AArch64Cpu::new();

    assert!(!x86.is_halted());
    assert!(!aarch64.is_waiting());
}

#[test]
fn both_cpus_reset_idempotent() {
    let mut x86 = X86_64Cpu::new();
    let mut aarch64 = AArch64Cpu::new();

    // Double reset should be fine
    x86.reset();
    x86.reset();
    aarch64.reset();
    aarch64.reset();

    assert_eq!(x86.registers().rip, 0xFFF0);
    assert_eq!(aarch64.registers().pc, 0);
}
