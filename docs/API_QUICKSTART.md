# HyperMachine API Quickstart

Get your AI agents managing virtual machines in under 5 minutes.

## Prerequisites

- HyperMachine server running (default: `http://localhost:8080`)
- API key (for production deployments)

## Quick Start

### 1. Verify Connection

```bash
curl http://localhost:8080/health
```

Response:
```json
{
  "status": "healthy",
  "version": "0.1.0",
  "uptime_seconds": 3600
}
```

### 2. Create Your First VM

```bash
curl -X POST http://localhost:8080/api/v1/vms \
  -H "Content-Type: application/json" \
  -d '{
    "name": "my-first-vm",
    "cpus": 2,
    "memory_mb": 2048,
    "disk_gb": 20,
    "image": "ubuntu-22.04"
  }'
```

Response:
```json
{
  "id": "vm-abc123",
  "name": "my-first-vm",
  "state": "creating",
  "created_at": "2026-02-02T12:00:00Z"
}
```

### 3. Start the VM

```bash
curl -X POST http://localhost:8080/api/v1/vms/vm-abc123/start
```

### 4. Check VM Status

```bash
curl http://localhost:8080/api/v1/vms/vm-abc123
```

### 5. Execute a Script

```bash
curl -X POST http://localhost:8080/api/v1/vms/vm-abc123/script \
  -H "Content-Type: application/json" \
  -d '{
    "language": "rhai",
    "code": "print(\"Hello from HyperMachine!\");"
  }'
```

---

## AI Agent Integration

HyperMachine is designed for AI-first automation. Get tool definitions for your AI framework:

### OpenAI Function Calling

```bash
curl http://localhost:8080/agentic/tools/openai
```

Use directly in your OpenAI client:

```python
import openai
import requests

# Get HyperMachine tools
tools = requests.get("http://localhost:8080/agentic/tools/openai").json()["tools"]

# Use with OpenAI
response = openai.chat.completions.create(
    model="gpt-4",
    messages=[{"role": "user", "content": "Create a VM with 4 CPUs for testing"}],
    tools=tools
)
```

### Anthropic Claude

```bash
curl http://localhost:8080/agentic/tools/anthropic
```

```python
import anthropic
import requests

tools = requests.get("http://localhost:8080/agentic/tools/anthropic").json()["tools"]

client = anthropic.Anthropic()
response = client.messages.create(
    model="claude-3-opus",
    max_tokens=1024,
    tools=tools,
    messages=[{"role": "user", "content": "List all running VMs"}]
)
```

### Google Gemini

```bash
curl http://localhost:8080/agentic/tools/gemini
```

### Full Ontology (JSON-LD)

```bash
curl http://localhost:8080/agentic/ontology
```

---

## API Reference

### Virtual Machines

| Method | Endpoint                   | Description        |
| ------ | -------------------------- | ------------------ |
| GET    | `/api/v1/vms`              | List all VMs       |
| POST   | `/api/v1/vms`              | Create a new VM    |
| GET    | `/api/v1/vms/{id}`         | Get VM details     |
| DELETE | `/api/v1/vms/{id}`         | Delete a VM        |
| POST   | `/api/v1/vms/{id}/start`   | Start a VM         |
| POST   | `/api/v1/vms/{id}/stop`    | Stop a VM          |
| POST   | `/api/v1/vms/{id}/pause`   | Pause a VM         |
| POST   | `/api/v1/vms/{id}/resume`  | Resume a paused VM |
| GET    | `/api/v1/vms/{id}/metrics` | Get VM metrics     |
| POST   | `/api/v1/vms/{id}/script`  | Execute a script   |

### Agent Discovery

| Method | Endpoint                      | Description             |
| ------ | ----------------------------- | ----------------------- |
| GET    | `/agentic/ontology`           | Full JSON-LD ontology   |
| GET    | `/agentic/tools/openai`       | OpenAI function format  |
| GET    | `/agentic/tools/anthropic`    | Anthropic tool format   |
| GET    | `/agentic/tools/gemini`       | Gemini function format  |
| GET    | `/.well-known/ai-plugin.json` | ChatGPT plugin manifest |

---

## VM Configuration Options

```json
{
  "name": "string (required)",
  "cpus": "integer (1-128, default: 1)",
  "memory_mb": "integer (512-1048576, default: 1024)",
  "disk_gb": "integer (1-10000, default: 10)",
  "image": "string (required)",
  "network": {
    "type": "nat | bridge | none",
    "ip": "optional static IP"
  },
  "gpu": {
    "enabled": "boolean",
    "type": "passthrough | vgpu"
  },
  "security": {
    "secure_boot": "boolean",
    "vtpm": "boolean",
    "memory_encryption": "sev | tdx | none"
  }
}
```

---

## Error Handling

All errors return standard HTTP status codes with JSON body:

```json
{
  "error": {
    "code": "VM_NOT_FOUND",
    "message": "Virtual machine 'vm-xyz' not found",
    "details": {}
  }
}
```

| Code | Meaning                                           |
| ---- | ------------------------------------------------- |
| 400  | Bad Request - Invalid parameters                  |
| 401  | Unauthorized - Invalid API key                    |
| 404  | Not Found - Resource doesn't exist                |
| 409  | Conflict - Operation conflicts with current state |
| 500  | Internal Error - Server-side failure              |

---

## Rate Limits

| Tier       | Requests/min | Concurrent VMs |
| ---------- | ------------ | -------------- |
| Free       | 60           | 5              |
| Pro        | 600          | 50             |
| Enterprise | Unlimited    | Unlimited      |

---

## Next Steps

- [Deployment Guide](./DEPLOYMENT_GUIDE.md) - Production setup
- [Security Configuration](./security/SECURITY_AUDIT.md) - Hardening guide
- [AI Agent Integration](./AGENTIC_ONTOLOGY.md) - Deep dive on agent APIs
- [API Specification](./api/openapi.yaml) - Full OpenAPI spec

---

## Support

- **Documentation**: https://hypermachine.dev/docs
- **GitHub Issues**: https://github.com/nervosys/HyperMachine/issues
- **Enterprise Support**: support@nervosys.com
