#![no_main]
use hv2_core::descriptors::{InterruptDescriptor64, SegmentDescriptor};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Fuzz SegmentDescriptor (8 bytes)
    if data.len() >= 8 {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&data[..8]);
        let desc = SegmentDescriptor::from_bytes(bytes);
        let roundtrip = desc.to_bytes();
        assert_eq!(bytes, roundtrip);
    }

    // Fuzz InterruptDescriptor64 (16 bytes)
    if data.len() >= 16 {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&data[..16]);
        let desc = InterruptDescriptor64::from_bytes(bytes);
        let roundtrip = desc.to_bytes();
        assert_eq!(bytes, roundtrip);
    }
});
