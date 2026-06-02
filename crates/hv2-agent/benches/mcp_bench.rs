//! Benchmarks for the MCP tool-dispatch path — the agent's per-step hot loop.
//!
//! An agent calls tools continuously, so the fixed per-call overhead of
//! `call_tool` (lookup, capability check, dispatch) is paid on every reasoning
//! step. Run with: `cargo bench -p hv2-agent --bench mcp_bench`.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hv2_agent::{AgentCapabilities, McpConfig, McpServer};
use serde_json::json;

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

criterion_group!(benches, bench_call_tool);
criterion_main!(benches);
