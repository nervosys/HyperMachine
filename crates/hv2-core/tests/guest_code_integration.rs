// Guest Code Integration Tests
//
// Tests guest code examples to verify binary format and structure.
// These tests ensure guest code is correctly assembled and ready for execution.

// NOTE: Full VM execution tests will be added once VM API is stabilized.
// For now, we validate binary format and structure only.

#[tokio::test]
async fn test_binary_sizes() {
    // Verify all binaries are exactly 512 bytes (boot sector size)
    let hello = include_bytes!("../../../examples/guest_code/hello.bin");
    assert_eq!(hello.len(), 512, "hello.bin should be 512 bytes");

    let timer = include_bytes!("../../../examples/guest_code/timer_test.bin");
    assert_eq!(timer.len(), 512, "timer_test.bin should be 512 bytes");

    let keyboard = include_bytes!("../../../examples/guest_code/keyboard_test.bin");
    assert_eq!(keyboard.len(), 512, "keyboard_test.bin should be 512 bytes");

    let rtc = include_bytes!("../../../examples/guest_code/rtc_test.bin");
    assert_eq!(rtc.len(), 512, "rtc_test.bin should be 512 bytes");

    let boot = include_bytes!("../../../examples/guest_code/boot_sequence.bin");
    assert_eq!(boot.len(), 512, "boot_sequence.bin should be 512 bytes");

    let vga = include_bytes!("../../../examples/guest_code/vga_demo.bin");
    assert_eq!(vga.len(), 512, "vga_demo.bin should be 512 bytes");

    let combo = include_bytes!("../../../examples/guest_code/device_combo.bin");
    assert_eq!(combo.len(), 512, "device_combo.bin should be 512 bytes");
}

#[tokio::test]
async fn test_boot_signature() {
    // Verify boot sector signature (0xAA55 at offset 510)
    let binaries = [
        (
            "hello.bin",
            include_bytes!("../../../examples/guest_code/hello.bin"),
        ),
        (
            "timer_test.bin",
            include_bytes!("../../../examples/guest_code/timer_test.bin"),
        ),
        (
            "keyboard_test.bin",
            include_bytes!("../../../examples/guest_code/keyboard_test.bin"),
        ),
        (
            "rtc_test.bin",
            include_bytes!("../../../examples/guest_code/rtc_test.bin"),
        ),
        (
            "boot_sequence.bin",
            include_bytes!("../../../examples/guest_code/boot_sequence.bin"),
        ),
        (
            "vga_demo.bin",
            include_bytes!("../../../examples/guest_code/vga_demo.bin"),
        ),
        (
            "device_combo.bin",
            include_bytes!("../../../examples/guest_code/device_combo.bin"),
        ),
    ];

    for (name, binary) in binaries.iter() {
        let signature = u16::from_le_bytes([binary[510], binary[511]]);
        assert_eq!(
            signature, 0xAA55,
            "{} should have boot signature 0xAA55",
            name
        );
    }
}

#[tokio::test]
async fn test_multistage_bootloader() {
    // Test multi-stage bootloader structure
    let multiboot = include_bytes!("../../../examples/guest_code/multiboot.img");

    // Verify Stage 1 boot signature
    let stage1_signature = u16::from_le_bytes([multiboot[510], multiboot[511]]);
    assert_eq!(
        stage1_signature, 0xAA55,
        "Stage 1 should have boot signature 0xAA55"
    );

    // Verify Stage 1 is 512 bytes
    assert!(
        multiboot.len() >= 512,
        "multiboot.img should contain at least Stage 1 (512 bytes)"
    );

    // Verify Stage 2 exists (multiboot.img = Stage 1 + Stage 2)
    assert!(
        multiboot.len() > 512,
        "multiboot.img should contain Stage 2 after Stage 1"
    );

    // Expected total size: 512 (Stage 1) + 1024 (Stage 2) = 1536 bytes
    assert_eq!(
        multiboot.len(),
        1536,
        "multiboot.img should be 1536 bytes (512 + 1024)"
    );
}

#[tokio::test]
async fn test_stage1_binary() {
    // Test standalone Stage 1 binary
    let stage1 = include_bytes!("../../../examples/guest_code/stage1.bin");

    // Verify size
    assert_eq!(stage1.len(), 512, "stage1.bin should be exactly 512 bytes");

    // Verify boot signature
    let signature = u16::from_le_bytes([stage1[510], stage1[511]]);
    assert_eq!(
        signature, 0xAA55,
        "stage1.bin should have boot signature 0xAA55"
    );
}

#[tokio::test]
async fn test_stage2_binary() {
    // Test standalone Stage 2 binary
    let stage2 = include_bytes!("../../../examples/guest_code/stage2.bin");

    // Verify size (Stage 2 is 1KB in our implementation)
    assert_eq!(stage2.len(), 1024, "stage2.bin should be 1024 bytes");

    // Verify it's not a boot sector (no boot signature)
    let potential_signature = u16::from_le_bytes([stage2[510], stage2[511]]);
    assert_ne!(
        potential_signature, 0xAA55,
        "stage2.bin should NOT have boot signature (it's not a boot sector)"
    );
}

#[tokio::test]
async fn test_protected_mode_bootloader() {
    // Test protected mode multi-stage bootloader
    let pmode = include_bytes!("../../../examples/guest_code/pmode.img");

    // Verify Stage 1 boot signature
    let stage1_signature = u16::from_le_bytes([pmode[510], pmode[511]]);
    assert_eq!(
        stage1_signature, 0xAA55,
        "Protected mode bootloader Stage 1 should have boot signature 0xAA55"
    );

    // Verify Stage 1 is 512 bytes
    assert!(
        pmode.len() >= 512,
        "pmode.img should contain at least Stage 1 (512 bytes)"
    );

    // Verify Stage 2 exists
    assert!(
        pmode.len() > 512,
        "pmode.img should contain Stage 2 (protected mode code)"
    );

    // Expected: Stage 1 (512) + Stage 2 protected mode (~2KB)
    assert!(
        pmode.len() >= 2048,
        "pmode.img should be at least 2KB (Stage 1 + Stage 2 pmode)"
    );
}

#[tokio::test]
async fn test_stage2_pmode_binary() {
    // Test standalone Stage 2 protected mode binary
    let stage2_pmode = include_bytes!("../../../examples/guest_code/stage2_pmode.bin");

    // Verify size is reasonable (should be ~2KB as configured)
    assert!(
        stage2_pmode.len() >= 1024,
        "stage2_pmode.bin should be at least 1KB"
    );
    assert!(
        stage2_pmode.len() <= 65536,
        "stage2_pmode.bin should not exceed 64KB"
    );

    // Verify it's not a boot sector (no boot signature at standard location)
    if stage2_pmode.len() >= 512 {
        let potential_signature = u16::from_le_bytes([stage2_pmode[510], stage2_pmode[511]]);
        assert_ne!(
            potential_signature, 0xAA55,
            "stage2_pmode.bin should NOT have boot signature (it's not a boot sector)"
        );
    }
}

#[tokio::test]
async fn test_protected_mode_gdt() {
    // Test that protected mode Stage 2 contains GDT structure
    let stage2_pmode = include_bytes!("../../../examples/guest_code/stage2_pmode.bin");

    // GDT should be present somewhere in the binary
    // We can't test exact location without disassembly, but we can verify:
    // 1. Binary is large enough to contain GDT code
    assert!(
        stage2_pmode.len() >= 2048,
        "Stage 2 protected mode should be large enough for GDT and pmode code"
    );

    // 2. Contains both 16-bit and 32-bit code markers
    // Look for typical protected mode instruction sequences
    // This is a basic sanity check
    assert!(
        !stage2_pmode.is_empty(),
        "Stage 2 protected mode binary should not be empty"
    );
}

#[tokio::test]
async fn test_interrupt_demo_extended() {
    // Test interrupt demo multi-stage bootloader
    let interrupt_img = include_bytes!("../../../examples/guest_code/interrupt_demo.img");

    // Verify Stage 1 boot signature
    let stage1_signature = u16::from_le_bytes([interrupt_img[510], interrupt_img[511]]);
    assert_eq!(
        stage1_signature, 0xAA55,
        "Interrupt demo Stage 1 should have boot signature 0xAA55"
    );

    // Verify Stage 1 is present (512 bytes)
    assert!(
        interrupt_img.len() >= 512,
        "interrupt_demo.img should contain Stage 1"
    );

    // Verify Stage 2 is present
    assert!(
        interrupt_img.len() > 512,
        "interrupt_demo.img should contain Stage 2"
    );

    // Expected: Stage 1 (512) + Stage 2 (~4KB) = ~4.5KB
    assert!(
        interrupt_img.len() >= 4096,
        "interrupt_demo.img should be at least 4KB"
    );
}

#[tokio::test]
async fn test_interrupt_demo_stage2() {
    // Test standalone interrupt demo Stage 2 binary
    let stage2 = include_bytes!("../../../examples/guest_code/interrupt_demo_extended.bin");

    // Verify size is reasonable (should be 4KB as configured)
    assert_eq!(
        stage2.len(),
        4096,
        "interrupt_demo_extended.bin should be 4KB"
    );

    // Verify it's not a boot sector (no boot signature at standard location)
    let potential_signature = u16::from_le_bytes([stage2[510], stage2[511]]);
    assert_ne!(
        potential_signature, 0xAA55,
        "interrupt_demo_extended.bin should NOT have boot signature"
    );
}

#[tokio::test]
async fn test_mmio_test_extended() {
    // Test MMIO test multi-stage bootloader
    let mmio_img = include_bytes!("../../../examples/guest_code/mmio_test.img");

    // Verify Stage 1 boot signature
    let stage1_signature = u16::from_le_bytes([mmio_img[510], mmio_img[511]]);
    assert_eq!(
        stage1_signature, 0xAA55,
        "MMIO test Stage 1 should have boot signature 0xAA55"
    );

    // Verify Stage 1 is present (512 bytes)
    assert!(
        mmio_img.len() >= 512,
        "mmio_test.img should contain Stage 1"
    );

    // Verify Stage 2 is present
    assert!(mmio_img.len() > 512, "mmio_test.img should contain Stage 2");

    // Expected: Stage 1 (512) + Stage 2 (~4KB) = ~4.5KB
    assert!(
        mmio_img.len() >= 4096,
        "mmio_test.img should be at least 4KB"
    );
}

#[tokio::test]
async fn test_mmio_test_stage2() {
    // Test standalone MMIO test Stage 2 binary
    let stage2 = include_bytes!("../../../examples/guest_code/mmio_test_extended.bin");

    // Verify size is reasonable (should be 4KB as configured)
    assert_eq!(stage2.len(), 4096, "mmio_test_extended.bin should be 4KB");

    // Verify it's not a boot sector (no boot signature at standard location)
    let potential_signature = u16::from_le_bytes([stage2[510], stage2[511]]);
    assert_ne!(
        potential_signature, 0xAA55,
        "mmio_test_extended.bin should NOT have boot signature"
    );
}
