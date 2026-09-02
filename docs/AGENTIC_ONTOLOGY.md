# HyperMachine Agentic Ontology

**Version:** 1.0.0  
**Last Updated:** 2026-02-02  

---

## Overview

HyperMachine is designed as an **agent-first** hypervisor. AI agents (Claude, GPT, Gemini, etc.) are primary users, with human operators providing oversight and strategic direction.

This document describes the **Agentic Ontology** - a machine-readable API description that enables AI agents to discover and use HyperMachine capabilities autonomously.

---

## Quick Start for AI Agents

### 1. Discover Capabilities

```bash
# Get the full ontology (JSON-LD)
curl https://hypermachine.local/agentic/ontology

# Get OpenAI function calling format
curl https://hypermachine.local/agentic/tools/openai

# Get Anthropic MCP format
curl https://hypermachine.local/agentic/tools/anthropic

# Get Google Gemini format
curl https://hypermachine.local/agentic/tools/gemini
```

### 2. Authenticate

```bash
# Request a token
curl -X POST https://hypermachine.local/api/v1/auth/token \
  -H "Content-Type: application/json" \
  -d '{"api_key": "your-api-key"}'

# Use the token
curl https://hypermachine.local/api/v1/vms \
  -H "Authorization: Bearer <token>"
```

### 3. Perform Operations

```bash
# Create a VM
curl -X POST https://hypermachine.local/api/v1/vms \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"name": "ai-worker", "vcpu_count": 4, "memory_gb": 16, "enable_gpu": true}'

# Execute an agent script
curl -X POST https://hypermachine.local/api/v1/vms/ai-worker/script \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"script": "let metrics = get_metrics(); metrics.cpu_usage"}'
```

---

## Ontology Structure

### JSON-LD Context

The ontology uses JSON-LD for semantic web compatibility:

```json
{
  "@context": {
    "@vocab": "https://schema.nervosys.ai/hypermachine#",
    "schema": "https://schema.org/",
    "hm": "https://nervosys.ai/hypermachine/ontology/",
    "dcterms": "http://purl.org/dc/terms/"
  }
}
```

### Core Components

| Component        | Description                                           |
| ---------------- | ----------------------------------------------------- |
| `system`         | API identification and authentication info            |
| `capabilities`   | High-level features the system provides               |
| `resources`      | Resource types that can be managed (VM, Agent, etc.)  |
| `operations`     | Specific API operations with parameters and responses |
| `state_machines` | Valid state transitions for resources                 |
| `events`         | Event types for subscriptions                         |

---

## Capabilities

### Virtual Machine Management (`vm_management`)

Create, configure, and manage virtual machines.

**Operations:**
- `create_vm` - Create a new VM
- `delete_vm` - Delete an existing VM
- `get_vm` - Get VM details
- `list_vms` - List all VMs
- `update_vm` - Update VM configuration

**Permissions Required:** `vm:create`, `vm:read`

### VM Lifecycle Control (`vm_lifecycle`)

Control VM execution state.

**Operations:**
- `start_vm` - Start a stopped VM
- `stop_vm` - Gracefully stop a running VM
- `pause_vm` - Pause a running VM
- `resume_vm` - Resume a paused VM
- `snapshot_vm` - Create a snapshot
- `restore_vm` - Restore from snapshot

**Permissions Required:** `vm:control`

### AI Agent Execution (`agent_execution`)

Execute sandboxed scripts within VMs.

**Operations:**
- `execute_script` - Run a Rhai script on the host against a read-only view of a VM
- `list_agents` - List active agents
- `get_agent_logs` - Get agent execution logs

**Permissions Required:** `agent:execute`

### GPU Passthrough (`gpu_passthrough`)

Manage GPU devices for AI/ML workloads.

**Operations:**
- `attach_gpu` - Attach a GPU to a VM
- `detach_gpu` - Detach a GPU from a VM
- `list_gpus` - List available GPUs

**Permissions Required:** `gpu:manage`

### Metrics & Monitoring (`metrics_monitoring`)

Collect and query performance metrics.

**Operations:**
- `get_metrics` - Get current VM metrics
- `query_metrics` - Query historical metrics
- `set_alert` - Configure metric alerts

**Permissions Required:** `metrics:read`

---

## Resource Types

### Virtual Machine (`vm`)

```json
{
  "type": "object",
  "properties": {
    "id": { "type": "string", "format": "uuid" },
    "name": { "type": "string" },
    "state": { "enum": ["created", "starting", "running", "paused", "stopping", "stopped", "error"] },
    "vcpu_count": { "type": "integer", "minimum": 1, "maximum": 256 },
    "memory_gb": { "type": "integer", "minimum": 1, "maximum": 4096 },
    "enable_gpu": { "type": "boolean" },
    "enable_networking": { "type": "boolean" }
  }
}
```

**State Machine:**

```
created → starting → running ⇄ paused
                  ↓
              stopping
                  ↓
               stopped
```

### AI Agent (`agent`)

```json
{
  "type": "object",
  "properties": {
    "id": { "type": "string", "format": "uuid" },
    "vm_id": { "type": "string", "format": "uuid" },
    "script_type": { "enum": ["rhai", "wasm"] },
    "state": { "enum": ["pending", "running", "completed", "failed"] },
    "output": { "type": "string" },
    "error": { "type": "string" }
  }
}
```

---

## AI Agent Tool Formats

### OpenAI Function Calling

**Endpoint:** `GET /agentic/tools/openai`

```json
{
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "create_vm",
        "description": "Create a new virtual machine with specified configuration.",
        "parameters": {
          "type": "object",
          "properties": {
            "name": { "type": "string", "description": "VM name" },
            "vcpu_count": { "type": "integer", "default": 2 },
            "memory_gb": { "type": "integer", "default": 4 },
            "enable_gpu": { "type": "boolean", "default": false }
          },
          "required": ["name"]
        }
      }
    }
  ]
}
```

### Anthropic MCP (Model Context Protocol)

**Endpoint:** `GET /agentic/tools/anthropic`

```json
{
  "name": "hypermachine",
  "version": "0.1.0",
  "tools": [
    {
      "name": "create_vm",
      "description": "Create a new virtual machine with specified configuration.",
      "input_schema": {
        "type": "object",
        "properties": {
          "name": { "type": "string" },
          "vcpu_count": { "type": "integer" },
          "memory_gb": { "type": "integer" }
        },
        "required": ["name"]
      }
    }
  ]
}
```

### Google Gemini

**Endpoint:** `GET /agentic/tools/gemini`

```json
{
  "functionDeclarations": [
    {
      "name": "create_vm",
      "description": "Create a new virtual machine.",
      "parameters": {
        "type": "object",
        "properties": {
          "name": { "type": "string" },
          "vcpu_count": { "type": "integer" }
        },
        "required": ["name"]
      }
    }
  ]
}
```

### ChatGPT Plugin Manifest

**Endpoint:** `GET /.well-known/ai-plugin.json`

Compatible with OpenAI ChatGPT Plugin specification for marketplace integration.

---

## Event Subscriptions

AI agents can subscribe to real-time events via WebSocket:

```javascript
const ws = new WebSocket('wss://hypermachine.local/api/v1/events');
ws.send(JSON.stringify({
  action: 'subscribe',
  events: ['vm.state_changed', 'agent.completed']
}));

ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  console.log(`Event: ${data.type}`, data.payload);
};
```

### Event Types

| Event              | Description                     |
| ------------------ | ------------------------------- |
| `vm.state_changed` | VM transitioned between states  |
| `vm.metrics`       | Periodic metrics update         |
| `agent.completed`  | Agent script finished execution |

---

## Agent Scripting

### Rhai Scripts

Rhai is a safe, embeddable scripting language:

```rhai
// Get VM status
let status = vm_status();
print(`VM is ${status}`);

// Conditional logic
if status == "running" {
    let metrics = get_metrics();
    if metrics.cpu_usage > 80.0 {
        // Scale up
        scale_vcpus(4);
    }
}
```

**Available Functions:**
- `vm_status()` - Get current VM state
- `get_metrics()` - Get performance metrics
- `scale_vcpus(count)` - Adjust CPU count
- `scale_memory(gb)` - Adjust memory
- `attach_gpu(id)` - Attach a GPU
- `http_get(url)` - Make HTTP request (if network enabled)

### WASM Modules

For compiled, high-performance agents:

```rust
// agent.rs - Compile to WASM
#[no_mangle]
pub fn run() -> i32 {
    let status = hm::vm_status();
    hm::log(&format!("VM status: {}", status));
    
    if status == "running" {
        let metrics = hm::get_metrics();
        if metrics.cpu_usage > 80.0 {
            hm::scale_vcpus(4);
        }
    }
    0
}
```

Compile and upload:
```bash
cargo build --target wasm32-wasi --release
base64 target/wasm32-wasi/release/agent.wasm > agent.b64
```

---

## Security Considerations

### Capability-Based Security

Agents run with minimal privileges:

```json
{
  "script": "...",
  "capabilities": ["vm:read", "metrics:read"]
}
```

### Sandboxing

- **WASM:** Hardware memory isolation, no direct syscalls
- **Rhai:** No FFI, no file system access, CPU time limits
- **Both:** Rate limiting, timeout enforcement

### Audit Logging

All agent actions are logged:

```json
{
  "timestamp": "2026-02-02T12:34:56Z",
  "agent_id": "agent-123",
  "vm_id": "vm-456",
  "action": "scale_vcpus",
  "parameters": {"count": 4},
  "result": "success"
}
```

---

## Best Practices for AI Agents

### 1. Check State Before Operations

```python
# Good: Check state first
vm = client.get_vm(vm_id)
if vm.state == "running":
    client.stop_vm(vm_id)
```

### 2. Handle Errors Gracefully

```python
try:
    result = client.execute_script(vm_id, script)
except RateLimitError:
    time.sleep(60)
    retry()
```

### 3. Use Idempotent Operations

Most operations are idempotent - calling `start_vm` on a running VM is a no-op.

### 4. Subscribe to Events

Instead of polling, subscribe to events for real-time updates.

### 5. Respect Rate Limits

| Operation        | Rate Limit |
| ---------------- | ---------- |
| `create_vm`      | 10/min     |
| `execute_script` | 60/min     |
| `get_metrics`    | 300/min    |

---

## API Discovery Endpoints

| Endpoint                             | Format  | Description              |
| ------------------------------------ | ------- | ------------------------ |
| `/agentic/ontology`                  | JSON-LD | Full semantic ontology   |
| `/agentic/ontology?format=openai`    | JSON    | OpenAI tools format      |
| `/agentic/ontology?format=anthropic` | JSON    | Anthropic MCP format     |
| `/agentic/ontology?format=gemini`    | JSON    | Gemini functions format  |
| `/agentic/tools/openai`              | JSON    | OpenAI tools (direct)    |
| `/agentic/tools/anthropic`           | JSON    | Anthropic tools (direct) |
| `/agentic/tools/gemini`              | JSON    | Gemini tools (direct)    |
| `/.well-known/ai-plugin.json`        | JSON    | ChatGPT plugin manifest  |

---

## Integration Examples

### OpenAI GPT-4

```python
from openai import OpenAI
import requests

# Fetch HyperMachine tools
tools = requests.get("https://hypermachine.local/agentic/tools/openai").json()

client = OpenAI()
response = client.chat.completions.create(
    model="gpt-4-turbo",
    messages=[{"role": "user", "content": "Create a VM with 4 CPUs and 8GB RAM"}],
    tools=tools["tools"],
    tool_choice="auto"
)
```

### Anthropic Claude

```python
import anthropic
import requests

# Fetch HyperMachine tools
tools_data = requests.get("https://hypermachine.local/agentic/tools/anthropic").json()

client = anthropic.Anthropic()
response = client.messages.create(
    model="claude-sonnet-4-20250514",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Create a VM for ML training"}],
    tools=tools_data["tools"]
)
```

### Google Gemini

```python
import google.generativeai as genai
import requests

# Fetch HyperMachine tools
tools = requests.get("https://hypermachine.local/agentic/tools/gemini").json()

model = genai.GenerativeModel('gemini-pro', tools=[tools])
response = model.generate_content("Create a VM with GPU support")
```

---

**Document Control:**
- Author: HyperMachine API Team
- Reviewers: AI Integration Lead, Security Team
- Next Review: 2026-05-02
