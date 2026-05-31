//! Example: export the MCP tool surface to each LLM provider's tool-use format.
//!
//! HyperMachine's MCP tools are defined once (name + description + JSON-Schema
//! parameters) and projected into the three major provider formats so the same
//! VM-control surface works whether an agent is built on OpenAI, Anthropic, or
//! Google models. This example prints the `vm.create` tool in all three.
//!
//! Run with:
//! ```bash
//! cargo run -p hv2-agent --example llm_tool_schemas
//! ```

use hv2_agent::{AgentCapabilities, McpServer, McpTool};
use serde_json::{json, Value};

/// OpenAI Chat Completions / Responses `tools` entry.
fn to_openai(tool: &McpTool) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters,
        }
    })
}

/// Anthropic Messages API `tools` entry.
fn to_anthropic(tool: &McpTool) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "input_schema": tool.parameters,
    })
}

/// Google Gemini `functionDeclarations` entry.
fn to_gemini(tool: &McpTool) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "parameters": tool.parameters,
    })
}

fn main() {
    let server = McpServer::new();
    // Use operator scope so the listing reflects a realistic agent's tool set.
    let caps = AgentCapabilities::operator();
    let tools = server.list_tools(&caps);

    println!(
        "HyperMachine exposes {} tools to an operator-scoped agent.\n",
        tools.len()
    );

    let sample = tools
        .iter()
        .find(|t| t.name == "vm.create")
        .expect("vm.create is always registered");

    let pretty = |label: &str, v: Value| {
        println!("── {label} ──");
        println!("{}\n", serde_json::to_string_pretty(&v).unwrap());
    };

    pretty("OpenAI (tools[])", to_openai(sample));
    pretty("Anthropic (tools[])", to_anthropic(sample));
    pretty("Gemini (functionDeclarations[])", to_gemini(sample));

    // A real integration ships the whole array; show the per-provider counts.
    let openai: Vec<Value> = tools.iter().map(to_openai).collect();
    let anthropic: Vec<Value> = tools.iter().map(to_anthropic).collect();
    let gemini: Vec<Value> = tools.iter().map(to_gemini).collect();
    println!(
        "Full tool arrays — openai: {}, anthropic: {}, gemini: {} entries.",
        openai.len(),
        anthropic.len(),
        gemini.len()
    );
}
