#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = hv2_core::boot::linux::LinuxHeader::parse_header(data);
});
