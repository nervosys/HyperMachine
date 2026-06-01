//! Benchmarks for copy-on-write VM cloning (fast agent cold-start).
//!
//! Run with: cargo bench -p hv2-core --bench cow_bench

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use hv2_core::memory_cow::MemoryTemplate;

/// Spawning a CoW clone must be ~constant time in the baseline size, whereas a
/// full copy scales linearly. This is the agent cold-start win: instantiate a
/// fresh VM from a multi-megabyte warm image without a proportional memcpy.
fn bench_clone_vs_copy(c: &mut Criterion) {
    let mut group = c.benchmark_group("cow_spawn");

    for mib in [1usize, 16, 64] {
        let size = mib * 1024 * 1024;
        let baseline = vec![0xABu8; size];
        let tmpl = MemoryTemplate::from_bytes(&baseline);

        // O(1) copy-on-write clone — independent of baseline size.
        group.bench_with_input(BenchmarkId::new("cow_instantiate", mib), &mib, |b, _| {
            b.iter(|| {
                // black_box the whole clone so the Arc/overlay setup can't be elided.
                black_box(tmpl.instantiate());
            });
        });

        // Full copy of the same image — scales with size (the cost CoW avoids).
        // black_box the whole buffer, otherwise LLVM deletes the memcpy.
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("full_copy", mib), &mib, |b, _| {
            b.iter(|| {
                let copy: Vec<u8> = baseline.clone();
                black_box(copy);
            });
        });
    }

    group.finish();
}

/// The marginal cost a clone pays on the first write to a page (the software
/// "page fault": copy one 4 KiB page private, then write into it).
fn bench_first_write(c: &mut Criterion) {
    let baseline = vec![0u8; 16 * 1024 * 1024];
    let tmpl = MemoryTemplate::from_bytes(&baseline);

    c.bench_function("cow_first_write_page", |b| {
        b.iter_batched(
            || tmpl.instantiate(),
            |mut clone| {
                clone.write(black_box(0), black_box(&[0xFFu8; 64])).unwrap();
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_clone_vs_copy, bench_first_write);
criterion_main!(benches);
