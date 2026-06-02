//! Benchmarks for the MCP tool-dispatch path — the agent's per-step hot loop.
//!
//! An agent calls tools continuously, so the fixed per-call overhead of
//! `call_tool` (lookup, capability check, dispatch) is paid on every reasoning
//! step. Run with: `cargo bench -p hv2-agent --bench mcp_bench`.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use hv2_agent::{AgentCapabilities, McpConfig, McpServer};
use serde_json::json;
use std::sync::Arc;

fn bench_call_tool(c: &mut Criterion) {
    // High rate limit keeps the tight loop out of the limiter; audit off so we
    // measure pure dispatch rather than unbounded audit-log growth.
    let server = McpServer::with_config(McpConfig {
        rate_limit: u32::MAX,
        audit_enabled: false,
        ..Default::default()
    });
    let session = server
        .create_session("bench-agent", AgentCapabilities::full())
        .expect("create session");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // `vm.list` is a cheap, read-only tool, so this isolates the dispatch
    // overhead an agent pays per call rather than handler work.
    c.bench_function("mcp_call_tool/vm.list", |b| {
        b.iter(|| {
            let resp = rt.block_on(session.call_tool(&server, black_box("vm.list"), json!({})));
            black_box(resp.success);
        });
    });
}

/// Concurrent dispatch: N agents each issue a tool call at once on a
/// multi-threaded runtime. With per-session state and audit off, calls should
/// parallelize — batch latency growing far slower than N indicates the dispatch
/// path has no hidden global serialization.
fn bench_concurrent_dispatch(c: &mut Criterion) {
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .expect("runtime");

    let server = Arc::new(McpServer::with_config(McpConfig {
        rate_limit: u32::MAX,
        audit_enabled: false,
        ..Default::default()
    }));
    let sessions: Vec<_> = (0..64)
        .map(|i| {
            server
                .create_session(&format!("agent-{i}"), AgentCapabilities::full())
                .expect("session")
        })
        .collect();

    let mut group = c.benchmark_group("mcp_concurrent_dispatch");
    for &n in &[1usize, 8, 64] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| {
                rt.block_on(async {
                    let mut handles = Vec::with_capacity(n);
                    for i in 0..n {
                        let server = Arc::clone(&server);
                        let session = Arc::clone(&sessions[i % sessions.len()]);
                        handles.push(tokio::spawn(async move {
                            session
                                .call_tool(&server, "vm.list", json!({}))
                                .await
                                .success
                        }));
                    }
                    let mut ok = 0usize;
                    for h in handles {
                        if h.await.unwrap_or(false) {
                            ok += 1;
                        }
                    }
                    black_box(ok);
                });
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_call_tool, bench_concurrent_dispatch);
criterion_main!(benches);
