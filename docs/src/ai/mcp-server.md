# MCP Server

The Model Context Protocol (MCP) server provides a standardized interface for AI agents to interact with HyperMachine.

## Starting the Server

```bash
# Basic startup
hm mcp serve --api-key "your-secret-key"

# With custom port and TLS
hm mcp serve \
  --port 8443 \
  --tls-cert /etc/ssl/cert.pem \
  --tls-key /etc/ssl/key.pem \
  --api-key "your-secret-key"

# Using environment variables
export HM_API_KEY="your-secret-key"
export HM_MCP_PORT=8080
hm mcp serve
```

## Endpoints

### Tool Discovery

```bash
# List all available tools
GET /mcp/tools

# Response
{
  "tools": [
    {
      "name": "vm.create",
      "description": "Create a new virtual machine with specified resources",
      "inputSchema": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string",
            "description": "Name of the virtual machine"
          },
          "cpu_cores": {
            "type": "integer",
            "description": "Number of CPU cores",
            "default": 2
          },
          "memory_mb": {
            "type": "integer",
            "description": "Memory in megabytes",
            "default": 4096
          },
          "enable_gpu": {
            "type": "boolean",
            "description": "Enable GPU support",
            "default": false
          }
        },
        "required": ["name"]
      }
    }
  ]
}
```

### Tool Execution

```bash
# Execute a tool
POST /mcp/call
Content-Type: application/json
Authorization: Bearer your-secret-key

{
  "tool": "vm.create",
  "arguments": {
    "name": "my-sandbox",
    "cpu_cores": 4,
    "memory_mb": 8192,
    "enable_gpu": true
  }
}

# Response
{
  "result": {
    "id": "vm-550e8400-e29b-41d4-a716-446655440000",
    "name": "my-sandbox",
    "status": "created",
    "cpu_cores": 4,
    "memory_mb": 8192,
    "gpu_enabled": true
  }
}
```

### LLM-Specific Tool Formats

```bash
# OpenAI format (GPT-4, o1, o3)
GET /agentic/tools/openai

# Anthropic format (Claude)
GET /agentic/tools/anthropic

# Google format (Gemini)
GET /agentic/tools/gemini
```

## Available Tools

### VM Lifecycle

#### `vm.create`

Create a new virtual machine.

```json
{
  "tool": "vm.create",
  "arguments": {
    "name": "my-vm",
    "cpu_cores": 4,
    "memory_mb": 8192,
    "disk_gb": 100,
    "enable_gpu": true,
    "network_mode": "nat",
    "image": "ubuntu-22.04"
  }
}
```

#### `vm.start`

Start a virtual machine.

```json
{
  "tool": "vm.start",
  "arguments": {
    "vm_id": "vm-550e8400-e29b-41d4-a716-446655440000"
  }
}
```

#### `vm.stop`

Stop a virtual machine.

```json
{
  "tool": "vm.stop",
  "arguments": {
    "vm_id": "vm-550e8400-e29b-41d4-a716-446655440000",
    "force": false
  }
}
```

#### `vm.delete`

Delete a virtual machine.

```json
{
  "tool": "vm.delete",
  "arguments": {
    "vm_id": "vm-550e8400-e29b-41d4-a716-446655440000"
  }
}
```

### VM Information

#### `vm.list`

List all virtual machines.

```json
{
  "tool": "vm.list",
  "arguments": {
    "status": "running",
    "limit": 10
  }
}
```

#### `vm.get`

Get details of a specific VM.

```json
{
  "tool": "vm.get",
  "arguments": {
    "vm_id": "vm-550e8400-e29b-41d4-a716-446655440000"
  }
}
```

### Code Execution

#### `vm.exec`

Execute a command in a VM.

```json
{
  "tool": "vm.exec",
  "arguments": {
    "vm_id": "vm-550e8400-e29b-41d4-a716-446655440000",
    "command": "python -c 'print(\"Hello, AI!\")'",
    "timeout_secs": 60
  }
}

// Response
{
  "result": {
    "exit_code": 0,
    "stdout": "Hello, AI!\n",
    "stderr": "",
    "duration_ms": 150
  }
}
```

#### `vm.exec_script`

Execute a script file in a VM.

```json
{
  "tool": "vm.exec_script",
  "arguments": {
    "vm_id": "vm-550e8400-e29b-41d4-a716-446655440000",
    "script": "#!/bin/bash\necho 'Hello'\ndate",
    "interpreter": "/bin/bash"
  }
}
```

### File Operations

#### `vm.upload`

Upload a file to a VM.

```json
{
  "tool": "vm.upload",
  "arguments": {
    "vm_id": "vm-550e8400-e29b-41d4-a716-446655440000",
    "path": "/home/user/data.json",
    "content": "{\"key\": \"value\"}",
    "encoding": "utf-8"
  }
}
```

#### `vm.download`

Download a file from a VM.

```json
{
  "tool": "vm.download",
  "arguments": {
    "vm_id": "vm-550e8400-e29b-41d4-a716-446655440000",
    "path": "/home/user/output.txt"
  }
}
```

### Snapshots

#### `vm.snapshot.create`

Create a VM snapshot.

```json
{
  "tool": "vm.snapshot.create",
  "arguments": {
    "vm_id": "vm-550e8400-e29b-41d4-a716-446655440000",
    "name": "before-experiment"
  }
}
```

#### `vm.snapshot.restore`

Restore a VM from snapshot.

```json
{
  "tool": "vm.snapshot.restore",
  "arguments": {
    "vm_id": "vm-550e8400-e29b-41d4-a716-446655440000",
    "snapshot_id": "snap-123456"
  }
}
```

## Error Handling

Errors are returned with structured information:

```json
{
  "error": {
    "code": "VM_NOT_FOUND",
    "message": "Virtual machine not found",
    "details": {
      "vm_id": "vm-invalid-id"
    }
  }
}
```

### Error Codes

| Code                 | Description           |
| -------------------- | --------------------- |
| `VM_NOT_FOUND`       | VM does not exist     |
| `VM_ALREADY_RUNNING` | VM is already running |
| `VM_NOT_RUNNING`     | VM is not running     |
| `RESOURCE_EXHAUSTED` | Quota exceeded        |
| `INVALID_ARGUMENT`   | Invalid parameter     |
| `UNAUTHORIZED`       | Invalid API key       |
| `INTERNAL_ERROR`     | Server error          |

## Rate Limiting

Default rate limits:

| Operation   | Limit      |
| ----------- | ---------- |
| `vm.create` | 10/minute  |
| `vm.exec`   | 100/minute |
| `vm.list`   | 60/minute  |
| Other       | 120/minute |

Rate limit headers:

```
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 95
X-RateLimit-Reset: 1640995200
```

## WebSocket Streaming

For long-running operations, use WebSocket:

```javascript
const ws = new WebSocket('wss://localhost:8080/mcp/stream');

ws.onopen = () => {
  ws.send(JSON.stringify({
    type: 'subscribe',
    vm_id: 'vm-550e8400-e29b-41d4-a716-446655440000',
    channels: ['console', 'metrics']
  }));
};

ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  console.log('Received:', data);
};
```

## Next Steps

- [Tool Formats](./tool-formats.md) - LLM-specific tool definitions
- [Python SDK](./python-sdk.md) - Python client library
- [GUI Automation](./gui-automation.md) - Desktop automation API
