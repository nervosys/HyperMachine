//! Performance benchmarks for virtual switch operations
//!
//! Run with: cargo bench -p hv2-core --bench vswitch_bench

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::time::Duration;

use hv2_core::networking::vswitch::{MacAddress, MacTable, Port, PortType, VlanId, VlanSet};

fn bench_mac_table_learn(c: &mut Criterion) {
    let mut group = c.benchmark_group("mac_table/learn");

    for table_size in [100, 1_000, 10_000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(table_size),
            table_size,
            |b, &size| {
                b.iter(|| {
                    let mut table = MacTable::new(size + 1, Duration::from_secs(300));
                    let vlan = VlanId::new(1).unwrap();
                    for i in 0..size {
                        let mac = MacAddress::from_bytes([
                            0x02,
                            0x00,
                            (i >> 24) as u8,
                            (i >> 16) as u8,
                            (i >> 8) as u8,
                            i as u8,
                        ]);
                        table.learn(black_box(mac), black_box(i as u32), black_box(vlan));
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_mac_table_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("mac_table/lookup");

    for table_size in [100, 1_000, 10_000].iter() {
        let mut table = MacTable::new(*table_size + 1, Duration::from_secs(300));
        let vlan = VlanId::new(1).unwrap();

        for i in 0..*table_size {
            let mac = MacAddress::from_bytes([
                0x02,
                0x00,
                (i >> 24) as u8,
                (i >> 16) as u8,
                (i >> 8) as u8,
                i as u8,
            ]);
            table.learn(mac, i as u32, vlan);
        }

        group.bench_with_input(
            BenchmarkId::from_parameter(table_size),
            table_size,
            |b, &size| {
                let target = MacAddress::from_bytes([
                    0x02,
                    0x00,
                    ((size / 2) >> 24) as u8,
                    ((size / 2) >> 16) as u8,
                    ((size / 2) >> 8) as u8,
                    (size / 2) as u8,
                ]);
                b.iter(|| table.lookup(black_box(target), black_box(vlan)));
            },
        );
    }
    group.finish();
}

fn bench_mac_table_age(c: &mut Criterion) {
    let mut group = c.benchmark_group("mac_table/age");

    for table_size in [100, 1_000, 10_000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(table_size),
            table_size,
            |b, &size| {
                let mut table = MacTable::new(size + 1, Duration::from_secs(0));
                let vlan = VlanId::new(1).unwrap();
                for i in 0..size {
                    let mac = MacAddress::from_bytes([
                        0x02,
                        0x00,
                        (i >> 24) as u8,
                        (i >> 16) as u8,
                        (i >> 8) as u8,
                        i as u8,
                    ]);
                    table.learn(mac, i as u32, vlan);
                }
                b.iter(|| {
                    let _ = table.age();
                });
            },
        );
    }
    group.finish();
}

fn bench_vlan_set_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("vlan_set");

    group.bench_function("add_100_vlans", |b| {
        b.iter(|| {
            let mut set = VlanSet::empty();
            for i in 1..=100 {
                if let Some(vlan) = VlanId::new(i) {
                    set.add(black_box(vlan));
                }
            }
        });
    });

    let mut full_set = VlanSet::empty();
    for i in 1..=100 {
        if let Some(vlan) = VlanId::new(i) {
            full_set.add(vlan);
        }
    }

    group.bench_function("contains_check", |b| {
        let vlan = VlanId::new(50).unwrap();
        b.iter(|| full_set.contains(black_box(vlan)));
    });

    group.finish();
}

fn bench_mac_address_random(c: &mut Criterion) {
    c.bench_function("mac_address/random_local", |b| {
        b.iter(MacAddress::random_local);
    });
}

fn bench_port_vlan_check(c: &mut Criterion) {
    let port = Port::new(1, "bench-port", PortType::Internal);

    c.bench_function("port/allows_vlan", |b| {
        let vlan = VlanId::new(1).unwrap();
        b.iter(|| port.allows_vlan(black_box(vlan)));
    });
}

criterion_group!(
    benches,
    bench_mac_table_learn,
    bench_mac_table_lookup,
    bench_mac_table_age,
    bench_vlan_set_operations,
    bench_mac_address_random,
    bench_port_vlan_check,
);

criterion_main!(benches);
