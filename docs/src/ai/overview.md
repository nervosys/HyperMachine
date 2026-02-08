# AI Integration Overview

HyperMachine is designed from the ground up for AI agent orchestration, providing first-class integration with major LLM providers.

## Why AI-First?

Modern AI agents need to:

- **Create isolated environments** for code execution
- **Manage compute resources** dynamically
- **Run untrusted code** safely
- **Scale GPU workloads** efficiently

HyperMachine provides all of this through a semantic API that AI agents can understand and use.

## Integration Methods

```
┌─────────────────────────────────────────────────────────┐
│                      AI Agent                            │
│              (GPT-4, Claude, Gemini, etc.)              │
└────────────────────────┬────────────────────────────────┘
                         │
        ┌────────────────┼────────────────┐
        │                │                │
        ▼                ▼                ▼
┌───────────────┐ ┌───────────────┐ ┌───────────────┐
│  MCP Server   │ │   REST API    │ │  Python SDK   │
│  (Native)     │ │  (HTTP/JSON)  │ │  (Wrapper)    │
└───────────────┘ └───────────────┘ └───────────────┘
        │                │                │
        └────────────────┼────────────────┘
                         │
                         ▼
              ┌─────────────────────┐
              │   HyperMachine Core │
              └─────────────────────┘
```

## Quick Comparison

| Method             | Best For               | Latency | Complexity |
| ------------------ | ---------------------- | ------- | ---------- |
| **MCP Server**     | Native LLM integration | Low     | Simple     |
| **REST API**       | Custom applications    | Medium  | Medium     |
| **Python SDK**     | Python AI frameworks   | Low     | Simple     |
| **GUI Automation** | Desktop control        | Medium  | Simple     |

## MCP Server

The Model Context Protocol server provides native LLM integration:

```bash
# Start MCP server
hm mcp serve --api-key "your-key"

# Discover tools
curl http://localhost:8080/mcp/tools
```

AI agents can discover and call tools directly:

```json
{
  "tools": [
    {
      "name": "vm.create",
      "description": "Create a new virtual machine",
      "inputSchema": {
        "type": "object",
        "properties": {
          "name": { "type": "string" },
          "cpu_cores": { "type": "integer" },
          "memory_mb": { "type": "integer" },
          "enable_gpu": { "type": "boolean" }
        }
      }
    }
  ]
}
```

## LLM-Specific Formats

HyperMachine provides tool definitions in native formats for each LLM:

### OpenAI (GPT-4, o1, o3)

```bash
curl http://localhost:8080/agentic/tools/openai
```

```json
{
  "type": "function",
  "function": {
    "name": "vm_create",
    "description": "Create a new virtual machine",
    "parameters": {
      "type": "object",
      "properties": {
        "name": { "type": "string" },
        "cpu_cores": { "type": "integer" }
      },
      "required": ["name"]
    }
  }
}
```

### Anthropic (Claude)

```bash
curl http://localhost:8080/agentic/tools/anthropic
```

```json
{
  "name": "vm_create",
  "description": "Create a new virtual machine",
  "input_schema": {
    "type": "object",
    "properties": {
      "name": { "type": "string" },
      "cpu_cores": { "type": "integer" }
    },
    "required": ["name"]
  }
}
```

### Google (Gemini)

```bash
curl http://localhost:8080/agentic/tools/gemini
```

```json
{
  "name": "vm_create",
  "description": "Create a new virtual machine",
  "parameters": {
    "type": "OBJECT",
    "properties": {
      "name": { "type": "STRING" },
      "cpu_cores": { "type": "INTEGER" }
    },
    "required": ["name"]
  }
}
```

## Available Tools

| Category         | Tools                                                           |
| ---------------- | --------------------------------------------------------------- |
| **VM Lifecycle** | `vm.create`, `vm.start`, `vm.stop`, `vm.delete`                 |
| **VM Info**      | `vm.list`, `vm.get`, `vm.status`                                |
| **Execution**    | `vm.exec`, `vm.exec_script`, `vm.upload`, `vm.download`         |
| **Snapshots**    | `vm.snapshot.create`, `vm.snapshot.restore`, `vm.snapshot.list` |
| **Network**      | `vm.network.configure`, `vm.network.port_forward`               |
| **GPU**          | `vm.gpu.attach`, `vm.gpu.detach`, `vm.gpu.list`                 |

## Example: AI Code Executor

```python
import openai
import requests

class AICodeExecutor:
    def __init__(self, hm_url: str, api_key: str):
        self.hm_url = hm_url
        self.api_key = api_key
        self.tools = self._fetch_tools()
    
    def _fetch_tools(self):
        return requests.get(
            f"{self.hm_url}/agentic/tools/openai"
        ).json()
    
    def execute_tool(self, name: str, arguments: dict):
        return requests.post(
            f"{self.hm_url}/mcp/call",
            headers={"Authorization": f"Bearer {self.api_key}"},
            json={"tool": name, "arguments": arguments}
        ).json()
    
    def run_code(self, code: str, language: str = "python"):
        # Create sandbox VM
        vm = self.execute_tool("vm.create", {
            "name": f"sandbox-{uuid.uuid4().hex[:8]}",
            "cpu_cores": 2,
            "memory_mb": 4096,
        })
        
        try:
            # Start VM
            self.execute_tool("vm.start", {"vm_id": vm["id"]})
            
            # Execute code
            result = self.execute_tool("vm.exec", {
                "vm_id": vm["id"],
                "command": f"{language} -c '{code}'"
            })
            
            return result
        finally:
            # Clean up
            self.execute_tool("vm.delete", {"vm_id": vm["id"]})
```

## Security Model

AI agents operate within defined security boundaries:

- **API Key Authentication** - All operations require valid API key
- **Rate Limiting** - Prevent resource exhaustion
- **Resource Quotas** - Limit VMs, memory, GPU per key
- **Audit Logging** - Track all AI operations
- **Network Isolation** - VMs isolated by default

## Next Steps

- [MCP Server](./mcp-server.md) - MCP protocol details
- [Tool Formats](./tool-formats.md) - LLM-specific formats
- [Python SDK](./python-sdk.md) - Python integration
- [GUI Automation](./gui-automation.md) - Desktop automation
