//! Performance benchmarks for VM operations
//!
//! Run with: cargo bench -p hv2-core --bench vm_bench

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use hv2_core::memory::GuestMemory;
use hv2_core::snapshot::device::DeviceStateSerializer;
use hv2_core::vcpu::ControlRegisters;

fn bench_guest_memory_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("guest_memory/allocate");
    for size in [4096, 65536, 1048576].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, sz| {
            b.iter(|| {
                let memory = GuestMemory::new(*sz as u64).unwrap();
                let _ = memory.allocate_region(black_box(*sz as u64), false);
            });
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
            b.iter(|| memory.read_bytes(black_box(0), black_box(*sz)));
        });
    }
    group.finish();
}

fn bench_control_registers_serialization(c: &mut Criterion) {
    let regs = ControlRegisters::default();
    c.bench_function("control_registers/serialize", |b| {
        b.iter(|| bincode::serialize(black_box(&regs)).unwrap());
    });
    let serialized = bincode::serialize(&regs).unwrap();
    c.bench_function("control_registers/deserialize", |b| {
        b.iter(|| bincode::deserialize::<ControlRegisters>(black_box(&serialized)).unwrap());
    });
}

fn bench_guest_memory_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("guest_memory/write");
    for size in [64, 256, 1024, 4096].iter() {
        let memory = GuestMemory::new(1024 * 1024).expect("Failed");
        let _ = memory.allocate_region(*size as u64 + 4096, false);
        let data = vec![0xABu8; *size];
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| memory.write_bytes(black_box(0), black_box(&data)));
        });
    }
    group.finish();
}

fn bench_device_state_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot/device_state");

    for field_count in [10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("serialize", field_count),
            field_count,
            |b, &count| {
                b.iter(|| {
                    let mut ser = DeviceStateSerializer::with_capacity(count * 8);
                    for i in 0..count {
                        ser.write_u64(black_box(i as u64));
                    }
                    let _ = ser.into_bytes();
                });
            },
        );
    }

    group.bench_function("mixed_types", |b| {
        b.iter(|| {
            let mut ser = DeviceStateSerializer::new();
            ser.write_u8(black_box(0xFF));
            ser.write_u16(black_box(0x1234));
            ser.write_u32(black_box(0xDEADBEEF));
            ser.write_u64(black_box(0xCAFEBABE_DEADBEEF));
            ser.write_bool(black_box(true));
            ser.write_string(black_box("device-state-benchmark"));
            ser.write_bytes(black_box(&[0u8; 256]));
            let _ = ser.into_bytes();
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_guest_memory_allocation,
    bench_guest_memory_read,
    bench_guest_memory_write,
    bench_control_registers_serialization,
    bench_device_state_serialization,
);
criterion_main!(benches);
