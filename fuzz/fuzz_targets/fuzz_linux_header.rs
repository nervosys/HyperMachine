#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = hv2_core::boot::linux::LinuxBootProtocol::parse_header(data);
});
