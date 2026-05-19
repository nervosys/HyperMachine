#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let opcode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let mut memory = data[4..].to_vec();
    memory.resize(memory.len().max(4096), 0);
    let mut cpu = hv2_cpu::aarch64::AArch64Cpu::new();
    let mut mem = hv2_cpu::aarch64::SliceMemory::new(&mut memory);
    let _ = cpu.execute_instruction(opcode, &mut mem);
});
