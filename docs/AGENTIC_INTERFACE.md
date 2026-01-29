# HyperMachine Agentic AI Interface Design

This document describes HyperMachine's design for use by agentic AI systems, providing
effective tool-use interfaces for one-to-many agent orchestration.

## Overview

HyperMachine provides a comprehensive interface for AI agents to manage virtual machines.
The design follows these principles:

1. **Tool-Use First**: All operations are exposed as structured tools with JSON Schema
   definitions, compatible with OpenAI function calling and Anthropic tool use.

2. **Multi-Agent Safe**: Built-in coordination primitives prevent conflicts when multiple
   agents operate on the same infrastructure.

3. **Capability-Based Security**: Fine-grained permissions control what each agent can do.

4. **Observable**: Full audit logging and telemetry for traceability.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           AI Agent Systems                                  │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
│  │  GPT-4      │  │  Claude     │  │  Gemini     │  │  Local LLM  │         │
│  │  Agent      │  │  Agent      │  │  Agent      │  │  Agent      │         │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘         │
│         │                │                │                │                │
│  ┌──────┴────────────────┴────────────────┴────────────────┴───────┐        │
│  │                    MCP Interface Layer                          │        │
│  │  • OpenAI function calling compatible                           │        │
│  │  • Anthropic tool use compatible                                │        │
│  │  • JSON Schema definitions for all tools                        │        │
│  └──────────────────────────────┬──────────────────────────────────┘        │
└─────────────────────────────────┼─────────────────────────────────────────-─┘
                                  │
┌─────────────────────────────────┼──────────────────────────────────────────┐
│                    HyperMachine │                                          │
│  ┌──────────────────────────────┴───────────────────────────────────┐      │
│  │                   Agent Orchestration Layer                      │      │
│  │  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐         │      │
│  │  │ Session       │  │ Resource      │  │ Conflict      │         │      │
│  │  │ Management    │  │ Locking       │  │ Resolution    │         │      │
│  │  └───────────────┘  └───────────────┘  └───────────────┘         │      │
│  │  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐         │      │
│  │  │ Capability    │  │ Rate          │  │ Audit         │         │      │
│  │  │ Enforcement   │  │ Limiting      │  │ Logging       │         │      │
│  │  └───────────────┘  └───────────────┘  └───────────────┘         │      │
│  └──────────────────────────────┬───────────────────────────────────┘      │
│                                 │                                          │
│  ┌──────────────────────────────┴───────────────────────────────────┐      │
│  │                   VM Management Layer                            │      │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐              │      │
│  │  │ Create  │  │ Start   │  │ Stop    │  │ Delete  │              │      │
│  │  │ VM      │  │ VM      │  │ VM      │  │ VM      │              │      │
│  │  └─────────┘  └─────────┘  └─────────┘  └─────────┘              │      │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐              │      │
│  │  │Snapshot │  │ Network │  │ Storage │  │ Guest   │              │      │
│  │  │ Mgmt    │  │ Mgmt    │  │ Mgmt    │  │ Exec    │              │      │
│  │  └─────────┘  └─────────┘  └─────────┘  └─────────┘              │      │
│  └──────────────────────────────────────────────────────────────────┘      │
│                                                                            │
│  ┌──────────────────────────────────────────────────────────────────┐      │
│  │                   Hypervisor Backend (T1/T2)                     │      │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐               │      │
│  │  │ KVM (Linux) │  │ WHPX (Win)  │  │ HVF (macOS) │               │      │
│  │  └─────────────┘  └─────────────┘  └─────────────┘               │      │
│  └──────────────────────────────────────────────────────────────────┘      │
└────────────────────────────────────────────────────────────────────────────┘
```

## MCP Tool Interface

### Tool Discovery

Agents can discover available tools with their JSON Schema definitions:

```json
{
  "name": "vm.create",
  "description": "Create a new virtual machine",
  "parameters": {
    "type": "object",
    "properties": {
      "name": {
        "type": "string",
        "description": "VM name (unique identifier)"
      },
      "cpu_cores": {
        "type": "integer",
        "description": "Number of virtual CPU cores",
        "minimum": 1,
        "maximum": 128,
        "default": 2
      },
      "memory_gb": {
        "type": "integer",
        "description": "Memory size in gigabytes",
        "minimum": 1,
        "maximum": 1024,
        "default": 4
      }
    },
    "required": ["name"]
  }
}
```

### Tool Categories

| Category     | Description                 | Example Tools                                       |
| ------------ | --------------------------- | --------------------------------------------------- |
| VmLifecycle  | VM create/start/stop/delete | `vm.create`, `vm.start`, `vm.stop`, `vm.delete`     |
| Resources    | CPU/memory management       | `vm.resize`, `vm.metrics`                           |
| Snapshots    | State management            | `snapshot.create`, `snapshot.restore`               |
| Network      | Network configuration       | `network.attach`, `network.detach`                  |
| System       | Guest execution             | `guest.exec`, `guest.file.read`, `guest.file.write` |
| Coordination | Multi-agent ops             | `agent.broadcast`, `agent.claim`, `agent.release`   |
| Monitoring   | Observability               | `system.health`, `system.info`                      |

## Multi-Agent Orchestration

### Agent Roles

| Role     | Description           | Capabilities                    |
| -------- | --------------------- | ------------------------------- |
| Operator | Full VM management    | Create, delete, modify, execute |
| Monitor  | Read-only observation | View status, metrics, logs      |
| Security | Audit and enforcement | View all, alert on violations   |
| Backup   | Snapshot management   | Create/restore snapshots        |
| Network  | Network configuration | Manage networking only          |
| Scaler   | Resource management   | Resize, migrate VMs             |

### Resource Claiming

Agents can claim exclusive access to VMs to prevent conflicts:

```rust
// Agent 1 claims VM for exclusive access
orchestrator.claim_vm("agent-1", "production-db", Some("maintenance"), None)?;

// Agent 2 cannot modify (but can still read)
let result = orchestrator.can_access_vm("agent-2", "production-db", true);
assert_eq!(result, Ok(false));  // Write denied

let result = orchestrator.can_access_vm("agent-2", "production-db", false);
assert_eq!(result, Ok(true));   // Read allowed
```

### Inter-Agent Communication

Agents can communicate via:

1. **Direct Messages**: Point-to-point communication
2. **Channels**: Pub/sub for broadcast communication
3. **Events**: System-wide event notifications

```rust
// Subscribe to VM events
orchestrator.subscribe_event("monitor-agent", EventType::VmStarted);
orchestrator.subscribe_event("monitor-agent", EventType::VmStopped);

// Broadcast to a channel
orchestrator.broadcast("operator-agent", "ops-channel", json!({
    "action": "maintenance-starting",
    "vms": ["vm-1", "vm-2"]
}))?;
```

## Capability-Based Security

### Capability Levels

```rust
// Read-only agent (monitoring)
let caps = AgentCapabilities::read_only();
// Grants: VmRead, MetricsRead, StorageRead

// Operator agent (standard operations)
let caps = AgentCapabilities::operator();
// Grants: VmRead, VmWrite, VmManage, MetricsRead, SnapshotManage

// Full access (admin)
let caps = AgentCapabilities::full();
// Grants: All capabilities
```

### Tool Access Control

Tools declare their required capabilities:

```rust
McpTool {
    name: "vm.create",
    required_capabilities: vec![AgentCapability::VmManage],
    // ...
}
```

Agents can only call tools for which they have all required capabilities.

## Usage Examples

### Single Agent Example

```rust
use hv2_agent::{McpServer, AgentCapabilities};
use serde_json::json;

// Create MCP server
let mcp = McpServer::new();

// Register agent session
let session = mcp.create_session("my-agent", AgentCapabilities::operator())?;

// List available tools
let tools = session.list_tools(&mcp);
for tool in &tools {
    println!("Tool: {} - {}", tool.name, tool.description);
}

// Create a VM
let result = session.call_tool(&mcp, "vm.create", json!({
    "name": "test-vm",
    "cpu_cores": 4,
    "memory_gb": 8
})).await;

if result.success {
    println!("VM created: {:?}", result.result);
}
```

### Multi-Agent Workflow Example

```rust
use hv2_agent::{AgentOrchestrator, AgentRole};
use serde_json::json;

let orch = AgentOrchestrator::new();

// Register multiple agents with different roles
orch.register_agent("planner", "Planning Agent", AgentRole::Operator)?;
orch.register_agent("executor", "Execution Agent", AgentRole::Operator)?;
orch.register_agent("monitor", "Monitoring Agent", AgentRole::Monitor)?;

// Subscribe monitor to events
orch.subscribe_event("monitor", EventType::VmStarted);
orch.subscribe_event("monitor", EventType::VmStopped);

// Set up communication channel
orch.subscribe("planner", "coordination")?;
orch.subscribe("executor", "coordination")?;

// Planner claims VMs for workflow
orch.claim_vm("planner", "vm-1", Some("deployment workflow"), None)?;
orch.claim_vm("planner", "vm-2", Some("deployment workflow"), None)?;

// Planner sends task to executor
orch.send_message(
    "planner",
    "executor",
    MessageType::Task,
    json!({
        "task": "update-application",
        "vms": ["vm-1", "vm-2"],
        "version": "2.0.0"
    })
)?;

// Executor receives and processes
let messages = orch.receive_messages("executor", 10);
for msg in messages {
    // Process task...
}

// Release claims when done
orch.release_vm("planner", "vm-1")?;
orch.release_vm("planner", "vm-2")?;
```

### OpenAI Function Calling Integration

```python
import openai
from hypermachine import HyperMachineClient

# Get tool definitions
hm = HyperMachineClient()
tools = hm.list_tools()

# Convert to OpenAI format
openai_tools = [
    {
        "type": "function",
        "function": {
            "name": tool["name"],
            "description": tool["description"],
            "parameters": tool["parameters"]
        }
    }
    for tool in tools
]

# Use with OpenAI
response = openai.chat.completions.create(
    model="gpt-4",
    messages=[{"role": "user", "content": "Create a VM with 4 cores and 8GB RAM"}],
    tools=openai_tools,
)

# Execute tool calls
for tool_call in response.choices[0].message.tool_calls:
    result = hm.call_tool(
        tool_call.function.name,
        json.loads(tool_call.function.arguments)
    )
```

## Future Enhancements

### Planned Features

1. **Workflow Engine**: Declarative multi-step workflows with conditional logic
2. **Policy Engine**: Automated enforcement of operational policies
3. **Learning System**: Agents can learn from past operations
4. **Cost Optimization**: AI-driven resource optimization
5. **Predictive Scaling**: Proactive VM scaling based on patterns

## MCP HTTP Server

HyperMachine includes a built-in MCP HTTP server for AI agent access:

```bash
# Start the MCP server
hm serve --rest-port 8080

# With authentication enabled
export HM_API_KEY="your-secret-key"
hm serve --rest-port 8080
```

### Security

**Authentication**: Set the `HM_API_KEY` environment variable to enable bearer token authentication. When enabled, protected endpoints require an `Authorization: Bearer <token>` header.

**Rate Limiting**: Built-in token bucket rate limiting (100 requests/minute per IP) protects against abuse.

| Endpoint Type | Authentication Required |
| ------------- | ----------------------- |
| GET endpoints | No (read-only)          |
| POST/DELETE   | Yes (if HM_API_KEY set) |
| /health       | No                      |

### Endpoints

| Method | Endpoint             | Description                                    | Auth |
| ------ | -------------------- | ---------------------------------------------- | ---- |
| GET    | `/mcp/tools`         | List available tools (OpenAI/Anthropic format) | No   |
| POST   | `/mcp/call`          | Execute a tool call                            | Yes  |
| GET    | `/vms`               | List all VMs                                   | No   |
| POST   | `/vms`               | Create a new VM                                | Yes  |
| GET    | `/vms/:name`         | Get VM details                                 | No   |
| DELETE | `/vms/:name`         | Delete a VM                                    | Yes  |
| POST   | `/vms/:name/start`   | Start a VM                                     | Yes  |
| POST   | `/vms/:name/stop`    | Stop a VM                                      | Yes  |
| GET    | `/vms/:name/metrics` | Get VM metrics                                 | No   |
| GET    | `/health`            | Health check                                   | No   |

### Tool Discovery

```bash
curl http://localhost:8080/mcp/tools
```

### Unified Tool Execution

```bash
# With authentication
curl -X POST http://localhost:8080/mcp/call \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-secret-key" \
  -d '{
    "tool": "vm.create",
    "arguments": {
      "name": "my-vm",
      "cpu_cores": 4,
      "memory_gb": 8
    }
  }'
```

## API Reference

See the Rust documentation:
- [`hv2_agent::mcp`](./crates/hv2-agent/src/mcp.rs) - MCP tool interface
- [`hv2_agent::orchestration`](./crates/hv2-agent/src/orchestration.rs) - Multi-agent orchestration
- [`hv2_agent::tools`](./crates/hv2-agent/src/tools.rs) - Tool definitions
- [`hv2_agent::communication`](./crates/hv2-agent/src/communication.rs) - Agent messaging
