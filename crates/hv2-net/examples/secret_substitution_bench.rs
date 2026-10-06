//! Local synthetic component benchmark; never a competitor or KVM benchmark.
use hv2_net::secret_substitution::{Binding, Bindings};
use std::hint::black_box;
use std::time::Instant;

fn token(index: usize) -> String {
    format!("hms_{index:064x}")
}

fn bindings(count: usize) -> Bindings {
    Bindings::new(
        (0..count)
            .map(|index| Binding {
                placeholder: token(index),
                value: format!("fixture-{index:04}").into_bytes(),
                hosts: vec!["api.example.test".into()],
            })
            .collect(),
    )
    .unwrap()
}

fn input(size: usize, stride: Option<usize>, index: usize) -> Vec<u8> {
    let mut input = vec![b'x'; size];
    if let Some(stride) = stride {
        let placeholder = token(index);
        for offset in (0..size.saturating_sub(placeholder.len()) + 1).step_by(stride) {
            input[offset..offset + placeholder.len()].copy_from_slice(placeholder.as_bytes());
        }
    }
    input
}

fn main() {
    // A debug build's timings mean nothing; refuse rather than print them.
    if cfg!(debug_assertions) {
        eprintln!("run this benchmark with --release");
        std::process::exit(2);
    }
    let cases = [
        ("small-header", 256, 1, Some(128), 0, "api.example.test"),
        ("plain-64k", 65536, 128, None, 0, "api.example.test"),
        ("plain-1m", 1048576, 128, None, 0, "api.example.test"),
        ("unscoped-1m", 1048576, 128, None, 0, "other.example.test"),
        (
            "sparse-1m",
            1048576,
            128,
            Some(65536),
            127,
            "api.example.test",
        ),
        ("dense-one", 1048576, 1, Some(68), 0, "api.example.test"),
        (
            "dense-last-of-128",
            1048576,
            128,
            Some(68),
            127,
            "api.example.test",
        ),
        (
            "dense-unmatched",
            1048576,
            128,
            Some(68),
            256,
            "api.example.test",
        ),
    ];
    let mut results = Vec::new();
    for (label, size, count, stride, index, host) in cases {
        let policy = bindings(count);
        let input = input(size, stride, index);
        let expected = policy.replace(host, &input).unwrap();
        for _ in 0..10 {
            assert_eq!(policy.replace(host, &input).unwrap(), expected);
        }
        let mut samples = Vec::new();
        for _ in 0..9 {
            let started = Instant::now();
            for _ in 0..10 {
                black_box(policy.replace(black_box(host), black_box(&input)).unwrap());
            }
            samples.push(started.elapsed().as_secs_f64() * 1_000_000.0 / 10.0);
        }
        let output_checksum = expected.iter().fold(0u64, |checksum, byte| {
            checksum.wrapping_mul(131).wrapping_add(u64::from(*byte))
        });
        results.push(serde_json::json!({
            "case":label, "input_bytes":size, "bindings":count,
            "samples_us":samples, "output_bytes":expected.len(),
            "output_checksum":output_checksum
        }));
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "version":1, "samples_per_case":9, "iterations_per_sample":10,
        "cases":results, "limits":["Synthetic in-process raw substitution only",
            "No HTTP, TLS, KVM, concurrency or competitor timing", "Single process, unpinned CPU"]
    })).unwrap());
}
