//! `context.*` over the MCP tool surface.
//!
//! The claim these tests exist to check is one sentence: an agent can let go
//! of something and get it back word for word. Everything else here -- the
//! ranking, the eviction, the confined runtime -- is machinery in service of
//! that, and machinery that quietly returns a summary instead of the original
//! would satisfy every other test while defeating the point.

use std::sync::Arc;

use hv2_agent::context_host::LocalContextHost;
use hv2_agent::mcp::{AgentCapabilities, AgentCapability, AgentSession, McpServer};
use hv2_context::Budget;
use hv2_sandbox::ProcessSandbox;
use serde_json::json;

fn session(server: &McpServer) -> Arc<AgentSession> {
    server
        .create_session("agent", AgentCapabilities::full())
        .unwrap()
}

async fn call(
    server: &McpServer,
    session: &AgentSession,
    tool: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let response = session.call_tool(server, tool, params).await;
    if response.success {
        Ok(response.result.unwrap_or(serde_json::Value::Null))
    } else {
        Err(response.error.unwrap_or_default())
    }
}

/// A server with a record installed, and a runtime to compute in.
fn server_with_context(dir: &tempfile::TempDir) -> McpServer {
    let host = LocalContextHost::open(dir.path(), "s1", Budget::new(400, 0.5))
        .unwrap()
        .with_sandbox_runtime(Box::new(ProcessSandbox::new()), dir.path().join("work"))
        .unwrap();
    let server = McpServer::new();
    server.set_context_host(Arc::new(host));
    server
}

#[tokio::test]
async fn without_a_host_the_tools_refuse_rather_than_pretend() {
    // A record that accepts every write and loses it is worse than none: the
    // agent is told it succeeded.
    let server = McpServer::new();
    let session = session(&server);

    let err = call(
        &server,
        &session,
        "context.record",
        json!({ "role": "system", "kind": "note", "text": "anything" }),
    )
    .await
    .unwrap_err();
    assert!(err.contains("no context host"), "got: {err}");
}

#[tokio::test]
async fn a_recorded_result_is_found_by_content_past_its_preview() {
    // The trap: index the preview and a long result is findable only by its
    // opening lines, which is where nothing interesting ever is.
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_context(&dir);
    let session = session(&server);

    let mut text = "routine opening line\n".repeat(400);
    text.push_str("the failure was an unset CPUID leaf\n");

    let recorded = call(
        &server,
        &session,
        "context.record",
        json!({ "role": "tool", "kind": "tool_result", "text": text }),
    )
    .await
    .unwrap();
    let seq = recorded["seq"].as_u64().unwrap();

    let hits = call(
        &server,
        &session,
        "context.search",
        json!({ "query": "unset cpuid leaf" }),
    )
    .await
    .unwrap();

    assert_eq!(hits[0]["seq"].as_u64(), Some(seq), "got: {hits}");
    assert!(
        !hits[0]["preview"]
            .as_str()
            .unwrap()
            .contains("unset CPUID leaf"),
        "the point of this test is that the match was not in the preview"
    );
}

#[tokio::test]
async fn expanding_returns_the_original_and_not_a_description_of_it() {
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_context(&dir);
    let session = session(&server);

    let original = "the guest triple-faulted at 0xfff0, which is the reset vector";
    let recorded = call(
        &server,
        &session,
        "context.record",
        json!({ "role": "tool", "kind": "tool_result", "text": original }),
    )
    .await
    .unwrap();

    let events = call(
        &server,
        &session,
        "context.expand",
        json!({ "from": recorded["seq"], "into_view": false }),
    )
    .await
    .unwrap();

    assert_eq!(events[0]["text"].as_str(), Some(original));
}

#[tokio::test]
async fn a_search_never_returns_content() {
    // If search returned payloads it would fill the context with the thing it
    // was asked to find a way around.
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_context(&dir);
    let session = session(&server);

    let text = format!("marker {}", "body ".repeat(500));
    call(
        &server,
        &session,
        "context.record",
        json!({ "role": "tool", "kind": "tool_result", "text": text.clone() }),
    )
    .await
    .unwrap();

    let hits = call(
        &server,
        &session,
        "context.search",
        json!({ "query": "marker" }),
    )
    .await
    .unwrap();

    let preview = hits[0]["preview"].as_str().unwrap();
    assert!(
        preview.len() < text.len(),
        "search returned the whole payload: {} bytes",
        preview.len()
    );
}

#[tokio::test]
async fn the_answer_can_be_computed_without_reading_the_data() {
    // The whole reason for the third layer. The question is "how many lines
    // mention failed", and the answer is a number -- there is no reason for
    // 400 lines to pass through the context to produce it.
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_context(&dir);
    let session = session(&server);

    let mut lines = String::new();
    for i in 0..400 {
        lines.push_str(if i % 7 == 0 {
            "case failed\n"
        } else {
            "case ok\n"
        });
    }
    call(
        &server,
        &session,
        "context.record",
        json!({ "role": "tool", "kind": "tool_result", "text": lines.clone() }),
    )
    .await
    .unwrap();

    let (program, args) = if cfg!(windows) {
        ("findstr.exe".to_string(), vec!["/C:failed"])
    } else {
        // Not hardcoded: grep lives in /usr/bin on macOS and /bin on most
        // Linux distributions, and the sandbox runs a program directly rather
        // than through a shell, so nothing is going to search a PATH on our
        // behalf.
        let grep = ["/bin/grep", "/usr/bin/grep"]
            .into_iter()
            .find(|path| std::path::Path::new(path).exists())
            .expect("a host with no grep anywhere is not one these tests can run on");
        (grep.to_string(), vec!["-c", "failed"])
    };

    let result = call(
        &server,
        &session,
        "context.exec",
        json!({ "program": program, "args": args, "stdin": lines, "best_effort": true }),
    )
    .await
    .unwrap();

    let stdout = result["stdout"].as_str().unwrap();
    let matches = if cfg!(windows) {
        stdout.lines().filter(|l| l.contains("failed")).count()
    } else {
        stdout.trim().parse::<usize>().unwrap_or(0)
    };
    assert_eq!(matches, 58, "got: {stdout:?}");
}

#[tokio::test]
async fn what_compaction_evicts_is_still_there_afterwards() {
    // The invariant the whole arrangement rests on. Eviction changes the view
    // and never the record.
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_context(&dir);
    let session = session(&server);

    for i in 0..40 {
        call(
            &server,
            &session,
            "context.expand",
            json!({
                "from": call(
                    &server,
                    &session,
                    "context.record",
                    json!({
                        "role": "assistant",
                        "kind": "turn",
                        "text": format!("turn {i} {}", "padding ".repeat(20)),
                    }),
                )
                .await
                .unwrap()["seq"],
                "into_view": true
            }),
        )
        .await
        .unwrap();
    }

    let compaction = call(
        &server,
        &session,
        "context.compact",
        json!({
            "task": "forty turns of setup",
            "state": "all of it recorded",
            "next_action": "get on with the actual work",
            "status": "done"
        }),
    )
    .await
    .unwrap();

    assert!(
        compaction["evicted"].as_u64().unwrap() > 0,
        "got: {compaction}"
    );

    let from = compaction["span_from"].as_u64().unwrap();
    let recovered = call(
        &server,
        &session,
        "context.expand",
        json!({ "from": from, "into_view": false }),
    )
    .await
    .unwrap();
    assert!(
        recovered[0]["text"].as_str().unwrap().contains("padding"),
        "got: {recovered}"
    );

    // ...and the view says what it is missing, so an agent reading it has a
    // reason to go looking.
    let view = call(&server, &session, "context.view", json!({}))
        .await
        .unwrap();
    let text = view["view"].as_str().unwrap();
    assert!(text.contains("forty turns of setup"), "got: {text}");
    assert!(text.contains("still addressable"), "got: {text}");
}

#[tokio::test]
async fn an_unfinished_span_is_not_recorded_as_finished() {
    // Omitting the status must not read as "done". An agent returning to this
    // session would see completed work and move on.
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_context(&dir);
    let session = session(&server);

    for i in 0..40 {
        call(
            &server,
            &session,
            "context.expand",
            json!({
                "from": call(
                    &server,
                    &session,
                    "context.record",
                    json!({
                        "role": "assistant",
                        "kind": "turn",
                        "text": format!("turn {i} {}", "padding ".repeat(20)),
                    }),
                )
                .await
                .unwrap()["seq"],
                "into_view": true
            }),
        )
        .await
        .unwrap();
    }

    call(
        &server,
        &session,
        "context.compact",
        json!({
            "task": "chasing the triple fault",
            "state": "e820 and CPUID both fixed",
            "next_action": "single-step to find where it dies"
        }),
    )
    .await
    .unwrap();

    let view = call(&server, &session, "context.view", json!({}))
        .await
        .unwrap();
    let text = view["view"].as_str().unwrap();
    assert!(text.contains("in progress"), "got: {text}");
}

#[tokio::test]
async fn reading_the_record_requires_the_capability() {
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_context(&dir);

    let mut capabilities = AgentCapabilities::none();
    capabilities.add(AgentCapability::VmRead);
    let limited = server.create_session("limited", capabilities).unwrap();

    let err = call(
        &server,
        &limited,
        "context.search",
        json!({ "query": "anything" }),
    )
    .await
    .unwrap_err();
    assert!(err.to_lowercase().contains("capab"), "got: {err}");
}

#[tokio::test]
async fn computing_needs_host_execution_as_well_as_context_access() {
    // Being able to read the record is not a reason to be able to run code on
    // the machine that holds it.
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_context(&dir);

    let mut capabilities = AgentCapabilities::none();
    capabilities.add(AgentCapability::ContextMemory);
    let reader = server.create_session("reader", capabilities).unwrap();

    // Reading is allowed...
    call(
        &server,
        &reader,
        "context.search",
        json!({ "query": "anything" }),
    )
    .await
    .unwrap();

    // ...and running a program is not.
    let err = call(
        &server,
        &reader,
        "context.exec",
        // Refused on the capability check, before anything looks for this
        // program, so the path only has to be plausible.
        json!({ "program": "/bin/echo", "args": ["hi"], "best_effort": true }),
    )
    .await
    .unwrap_err();
    assert!(err.to_lowercase().contains("capab"), "got: {err}");
}

#[tokio::test]
async fn a_request_with_an_unknown_field_is_refused() {
    // Silently ignoring a field that looked like a filter is how a search
    // comes back scoped to more than the caller asked for.
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_context(&dir);
    let session = session(&server);

    let err = call(
        &server,
        &session,
        "context.search",
        json!({ "query": "x", "role": "tool" }),
    )
    .await
    .unwrap_err();
    assert!(err.contains("invalid context.search request"), "got: {err}");
}

#[tokio::test]
async fn status_says_whether_there_is_anywhere_to_compute() {
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_context(&dir);
    let session = session(&server);

    let status = call(&server, &session, "context.status", json!({}))
        .await
        .unwrap();
    assert!(status["runtime"].is_string(), "got: {status}");
    assert_eq!(status["session"].as_str(), Some("s1"));
}
