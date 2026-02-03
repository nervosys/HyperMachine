//! Performance benchmarks for API operations
//!
//! Run with: cargo bench -p hv2-api

use criterion::{black_box, criterion_group, criterion_main, Criterion};

/// Benchmark JSON serialization of ontology structures
fn bench_ontology_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("ontology");

    // Simulate ontology data structure
    let ontology = serde_json::json!({
        "@context": {
            "hm": "https://hypermachine.io/ontology#",
            "schema": "https://schema.org/",
        },
        "@graph": (0..50).map(|i| {
            serde_json::json!({
                "@id": format!("hm:operation_{}", i),
                "@type": "hm:Operation",
                "hm:name": format!("operation_{}", i),
                "hm:parameters": [
                    {"name": "param1", "type": "string"},
                    {"name": "param2", "type": "integer"},
                ]
            })
        }).collect::<Vec<_>>()
    });

    group.bench_function("serialize_ontology", |b| {
        b.iter(|| serde_json::to_string(black_box(&ontology)).unwrap())
    });

    let ontology_str = serde_json::to_string(&ontology).unwrap();
    group.bench_function("deserialize_ontology", |b| {
        b.iter(|| {
            serde_json::from_str::<serde_json::Value>(black_box(&ontology_str)).unwrap()
        })
    });

    group.finish();
}

/// Benchmark tool format conversions
fn bench_tool_format_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("tool_formats");

    // OpenAI format
    let openai_tools: Vec<serde_json::Value> = (0..20)
        .map(|i| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": format!("tool_{}", i),
                    "description": format!("Description for tool {}", i),
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "param1": {"type": "string", "description": "First parameter"},
                            "param2": {"type": "integer", "description": "Second parameter"},
                        },
                        "required": ["param1"]
                    }
                }
            })
        })
        .collect();

    group.bench_function("serialize_openai_tools", |b| {
        b.iter(|| serde_json::to_string(black_box(&openai_tools)).unwrap())
    });

    // Anthropic format
    let anthropic_tools: Vec<serde_json::Value> = (0..20)
        .map(|i| {
            serde_json::json!({
                "name": format!("tool_{}", i),
                "description": format!("Description for tool {}", i),
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "param1": {"type": "string", "description": "First parameter"},
                        "param2": {"type": "integer", "description": "Second parameter"},
                    },
                    "required": ["param1"]
                }
            })
        })
        .collect();

    group.bench_function("serialize_anthropic_tools", |b| {
        b.iter(|| serde_json::to_string(black_box(&anthropic_tools)).unwrap())
    });

    group.finish();
}

/// Benchmark request/response parsing
fn bench_request_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("request_parsing");

    let create_vm_request = serde_json::json!({
        "name": "test-vm",
        "cpu_cores": 4,
        "memory_gb": 8,
        "gpu_enabled": true,
        "network": {
            "type": "bridge",
            "ip": "192.168.1.100",
            "gateway": "192.168.1.1"
        },
        "storage": [
            {"type": "disk", "size_gb": 100, "path": "/dev/sda"},
            {"type": "cdrom", "iso": "/path/to/image.iso"}
        ]
    });

    let request_str = serde_json::to_string(&create_vm_request).unwrap();

    group.bench_function("parse_create_vm_request", |b| {
        b.iter(|| {
            serde_json::from_str::<serde_json::Value>(black_box(&request_str)).unwrap()
        })
    });

    let list_response = serde_json::json!({
        "vms": (0..100).map(|i| {
            serde_json::json!({
                "id": format!("vm-{}", i),
                "name": format!("vm-{}", i),
                "state": "running",
                "cpu_cores": 4,
                "memory_gb": 8,
                "uptime_seconds": 3600 + i * 100
            })
        }).collect::<Vec<_>>()
    });

    group.bench_function("serialize_list_response", |b| {
        b.iter(|| serde_json::to_string(black_box(&list_response)).unwrap())
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_ontology_serialization,
    bench_tool_format_conversion,
    bench_request_parsing,
);

criterion_main!(benches);
