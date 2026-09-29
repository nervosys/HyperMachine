//! Verify a tamper-evident audit log.
//!
//! ```text
//! cargo run -p hv2-core --example verify_audit_log -- <audit.jsonl> <key-file>
//! ```
//!
//! `key-file` holds the 32-byte chain key as 64 hex characters, the same file
//! `HV2_AUDIT_KEY_FILE` names. Prints the number of records on success; on
//! failure, the first line that does not follow from the one before it, and
//! exits non-zero, so it can gate a pipeline.
//!
//! A log that verifies has not been edited, reordered or spliced, and no
//! record was removed from its middle. It can still have been cut short at
//! the end. Compare the last record against the copy your SIEM holds.

use std::process::ExitCode;

use hv2_core::security::audit_chain::verify;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [_, log, key_file] = args.as_slice() else {
        eprintln!("usage: verify_audit_log <audit.jsonl> <key-file>");
        return ExitCode::from(2);
    };

    let key_text = match std::fs::read_to_string(key_file) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("cannot read {key_file}: {e}");
            return ExitCode::from(2);
        }
    };
    let key_hex = key_text.trim();
    if key_hex.len() != 64 {
        eprintln!(
            "{key_file}: expected 64 hex characters, found {}",
            key_hex.len()
        );
        return ExitCode::from(2);
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        match u8::from_str_radix(&key_hex[2 * i..2 * i + 2], 16) {
            Ok(b) => *byte = b,
            Err(_) => {
                eprintln!("{key_file}: not hex");
                return ExitCode::from(2);
            }
        }
    }

    let text = match std::fs::read_to_string(log) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("cannot read {log}: {e}");
            return ExitCode::from(2);
        }
    };
    match verify(text.lines(), &key) {
        Ok(count) => {
            println!("{log}: {count} records, chain intact");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("{log}: TAMPERED -- {e}");
            ExitCode::FAILURE
        }
    }
}
