//! Agent-driven VM workflow.
//!
//! Demonstrates how an AI agent discovers and drives a full VM lifecycle
//! through HyperMachine's agentic tool interface — no shell scraping or
//! brittle CLI wrappers. The flow has two parts:
//!
//! 1. **Tool discovery** — build the [`HyperMachineOntology`] and project it
//!    into OpenAI, Anthropic, and Gemini tool-schema formats, plus a
//!    provider-tuned [`ProviderConfig`] (tools + system prompt + hints).
//! 2. **Tool execution** — replay the structured tool calls an agent would
//!    emit (`vm.create` → `vm.list` → ... → `vm.delete`) against a real
//!    [`ToolExecutor`], printing each typed [`ToolCallResponse`].
//!
//! Run with:
//!
//! ```bash
//! cargo run -p hm-cli --example agent_vm_workflow
//! ```

use hm_cli::agentic::{
    HyperMachineOntology, LlmProvider, ProviderAdapter, ProviderConfig, ToolCallRequest,
    ToolExecutor,
};
use hm_cli::vm_manager::VmManager;
use serde_json::json;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ---------------------------------------------------------------------
    // Part 1 — capability discovery
    // ---------------------------------------------------------------------
    let ontology = HyperMachineOntology::build();

    let anthropic = ProviderAdapter::to_anthropic(&ontology);
    let openai = ProviderAdapter::to_openai(&ontology);
    let gemini = ProviderAdapter::to_gemini(&ontology);

    println!("== Agent tool discovery ==");
    println!(
        "Schemas emitted — OpenAI: {} fns, Anthropic: {} tools, Gemini: {} declarations",
        openai.len(),
        anthropic.len(),
        gemini.function_declarations.len()
    );
    println!("Tools an agent can call:");
    for tool in &anthropic {
        let first_line = tool.description.lines().next().unwrap_or("");
        println!("  - {:<20} {}", tool.name, first_line);
    }

    // A provider bundle pairs the tool schemas with a tuned system prompt.
    let config = ProviderConfig::for_provider(LlmProvider::Anthropic, &ontology);
    println!(
        "\nAnthropic config: temperature={}, parallel_tool_calls={}",
        config.hints.temperature, config.hints.parallel_tool_calls
    );

    // ---------------------------------------------------------------------
    // Part 2 — execute a tool-call sequence as an agent would
    // ---------------------------------------------------------------------
    // An in-memory manager keeps the example self-contained (no host state).
    let vm_manager = Arc::new(VmManager::new_in_memory()?);
    let executor = ToolExecutor::new(vm_manager);

    // The ordered tool calls a planning agent would emit to provision, probe,
    // exercise, and tear down an isolated sandbox VM.
    let workflow = vec![
        (
            "vm.create",
            json!({
                "name": "ai-sandbox",
                "cpu_cores": 4,
                "memory_gb": 8,
                "gpu_enabled": false,
                "network_enabled": true
            }),
        ),
        ("vm.list", json!({})),
        ("vm.get", json!({ "name": "ai-sandbox" })),
        ("vm.metrics", json!({ "name": "ai-sandbox" })),
        ("vm.start", json!({ "name": "ai-sandbox" })),
        (
            "vm.execute_script",
            json!({ "name": "ai-sandbox", "script": "print(\"hello from the agent\")" }),
        ),
        ("vm.stop", json!({ "name": "ai-sandbox" })),
        ("vm.delete", json!({ "name": "ai-sandbox" })),
    ];

    println!("\n== Agent tool execution ==");
    for (index, (tool, arguments)) in workflow.into_iter().enumerate() {
        let request = ToolCallRequest {
            tool: tool.to_string(),
            arguments,
            call_id: Some(format!("call-{index}")),
        };

        let response = executor.execute(request).await;

        // Every call returns a typed, structured result — success or a
        // machine-readable error — never free-form text for the agent to parse.
        if response.success {
            let result = response
                .result
                .map(|r| serde_json::to_string(&r).unwrap_or_default())
                .unwrap_or_default();
            println!("[ ok ] {tool} => {result}");
        } else {
            let error = response.error.unwrap_or_default();
            // Backend-dependent steps (start/exec) may report a structured
            // error on hosts without a hypervisor backend; the agent can react
            // to it programmatically rather than scraping logs.
            println!("[fail] {tool} !! {error}");
        }
    }

    Ok(())
}
