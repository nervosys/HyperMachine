//! Performance benchmarks for FIPS crypto operations
//!
//! Run with: cargo bench -p hv2-core

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use hv2_core::crypto::fips::{AesKeySize, FipsCrypto, FipsMode};

fn bench_aes_gcm_encrypt(c: &mut Criterion) {
    let crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();
    let key = crypto.generate_aes_key(AesKeySize::Aes256).unwrap();
    let aad = b"benchmark";

    let mut group = c.benchmark_group("aes_gcm_encrypt");

    for size in [64, 256, 1024, 4096, 16384, 65536].iter() {
        let plaintext = vec![0u8; *size];
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                crypto
                    .aes_gcm_encrypt(black_box(key.as_bytes()), black_box(&plaintext), black_box(aad))
                    .unwrap()
            })
        });
    }
    group.finish();
}

fn bench_aes_gcm_decrypt(c: &mut Criterion) {
    let crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();
    let key = crypto.generate_aes_key(AesKeySize::Aes256).unwrap();
    let aad = b"benchmark";

    let mut group = c.benchmark_group("aes_gcm_decrypt");

    for size in [64, 256, 1024, 4096, 16384, 65536].iter() {
        let plaintext = vec![0u8; *size];
        let ciphertext = crypto.aes_gcm_encrypt(key.as_bytes(), &plaintext, aad).unwrap();

        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                crypto
                    .aes_gcm_decrypt(black_box(key.as_bytes()), black_box(&ciphertext), black_box(aad))
                    .unwrap()
            })
        });
    }
    group.finish();
}

fn bench_sha256(c: &mut Criterion) {
    let crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();

    let mut group = c.benchmark_group("sha256");

    for size in [64, 256, 1024, 4096, 16384, 65536, 1048576].iter() {
        let data = vec![0u8; *size];
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| crypto.sha256(black_box(&data)).unwrap())
        });
    }
    group.finish();
}

fn bench_sha512(c: &mut Criterion) {
    let crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();

    let mut group = c.benchmark_group("sha512");

    for size in [64, 256, 1024, 4096, 16384, 65536, 1048576].iter() {
        let data = vec![0u8; *size];
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| crypto.sha512(black_box(&data)).unwrap())
        });
    }
    group.finish();
}

fn bench_hmac_sha256(c: &mut Criterion) {
    let crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();
    let key = vec![0u8; 32];

    let mut group = c.benchmark_group("hmac_sha256");

    for size in [64, 256, 1024, 4096, 16384].iter() {
        let data = vec![0u8; *size];
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| crypto.hmac_sha256(black_box(&key), black_box(&data)).unwrap())
        });
    }
    group.finish();
}

fn bench_hkdf(c: &mut Criterion) {
    let crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();
    let ikm = vec![0u8; 32];
    let salt = vec![0u8; 32];
    let info = b"benchmark";

    let mut group = c.benchmark_group("hkdf");

    for output_len in [32, 64, 128, 256].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(output_len),
            output_len,
            |b, &len| {
                b.iter(|| {
                    crypto
                        .hkdf_sha256(black_box(&salt), black_box(&ikm), black_box(info), len)
                        .unwrap()
                })
            },
        );
    }
    group.finish();
}

fn bench_random_bytes(c: &mut Criterion) {
    let crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();

    let mut group = c.benchmark_group("random_bytes");

    for size in [16, 32, 64, 256, 1024, 4096].iter() {
        let mut buffer = vec![0u8; *size];
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| crypto.random_bytes(black_box(&mut buffer)).unwrap())
        });
    }
    group.finish();
}

fn bench_key_generation(c: &mut Criterion) {
    let crypto = FipsCrypto::new(FipsMode::Enabled).unwrap();

    c.bench_function("generate_aes128_key", |b| {
        b.iter(|| crypto.generate_aes_key(black_box(AesKeySize::Aes128)).unwrap())
    });

    c.bench_function("generate_aes256_key", |b| {
        b.iter(|| crypto.generate_aes_key(black_box(AesKeySize::Aes256)).unwrap())
    });
}

fn bench_self_tests(c: &mut Criterion) {
    c.bench_function("fips_self_tests", |b| {
        b.iter(|| {
            let mut crypto = FipsCrypto::new(black_box(FipsMode::Enabled)).unwrap();
            crypto.run_self_tests().unwrap()
        })
    });
}

criterion_group!(
    benches,
    bench_aes_gcm_encrypt,
    bench_aes_gcm_decrypt,
    bench_sha256,
    bench_sha512,
    bench_hmac_sha256,
    bench_hkdf,
    bench_random_bytes,
    bench_key_generation,
    bench_self_tests,
);

criterion_main!(benches);
