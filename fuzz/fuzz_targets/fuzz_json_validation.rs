#![no_main]
use hv2_api::middleware::{validate_json_body, BodyValidationConfig};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let config = BodyValidationConfig::default();
    let _ = validate_json_body(data, &config);
});
