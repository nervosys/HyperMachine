#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    // Need at least 1 byte for RIP offset + 1 byte for opcode
    let mut memory = data.to_vec();
    // Pad to minimum 256 bytes so RIP=0 has room to read
    memory.resize(memory.len().max(256), 0);
    let mut cpu = hv2_cpu::x86_64::X86_64Cpu::new();
    cpu.regs.rip = 0;
    let _ = cpu.execute_with_memory(&mut memory);
});
