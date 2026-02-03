//! Performance benchmarks for VM operations
//!
//! Run with: cargo bench -p hv2-core --bench vm_bench

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use hv2_core::memory::GuestMemory;
use hv2_core::vcpu::ControlRegisters;

fn bench_guest_memory_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("guest_memory/allocate");
    for size in [4096, 65536, 1048576].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, sz| {
            b.iter(|| {
                let memory = GuestMemory::new(*sz as u64).unwrap();
                memory.allocate_region(black_box(*sz as u64), false)
            })
        });
    }
    group.finish();
}

fn bench_guest_memory_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("guest_memory/read");
    for size in [64, 256, 1024, 4096].iter() {
        let memory = GuestMemory::new(1024 * 1024).expect("Failed");
        let _ = memory.allocate_region(*size as u64 + 4096, false);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, sz| {
            b.iter(|| memory.read_bytes(black_box(0), black_box(*sz)))
        });
    }
    group.finish();
}

fn bench_control_registers_serialization(c: &mut Criterion) {
    let regs = ControlRegisters::default();
    c.bench_function("control_registers/serialize", |b| {
        b.iter(|| bincode::serialize(black_box(&regs)).unwrap())
    });
    let serialized = bincode::serialize(&regs).unwrap();
    c.bench_function("control_registers/deserialize", |b| {
        b.iter(|| bincode::deserialize::<ControlRegisters>(black_box(&serialized)).unwrap())
    });
}

criterion_group!(benches, bench_guest_memory_allocation, bench_guest_memory_read, bench_control_registers_serialization);
criterion_main!(benches);
