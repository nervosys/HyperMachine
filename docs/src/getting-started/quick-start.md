# Quick Start

Get up and running with HyperMachine in under 5 minutes.

## Create Your First VM

### Using the CLI

```bash
# Create a VM with 4 CPUs, 8GB RAM, and GPU support
hm t2 create --name my-first-vm --cpu 4 --memory 8G --gpu

# List VMs
hm t2 list

# Start the VM
hm t2 start my-first-vm

# Connect to console
hm t2 console my-first-vm

# Stop the VM
hm t2 stop my-first-vm

# Delete the VM
hm t2 delete my-first-vm
```

### Using the GUI

1. Launch the HyperMachine GUI:
   ```bash
   hm-gui
   ```

2. Click **Create VM** button

3. Fill in the VM settings:
   - **Name:** `my-first-vm`
   - **CPUs:** `4`
   - **Memory:** `8192` MB
   - **Enable GPU:** ✓

4. Click **Create**

5. Select the VM and click **Start**

## Start the MCP Server

The MCP (Model Context Protocol) server allows AI agents to control HyperMachine:

```bash
# Start the server with an API key
hm mcp serve --api-key "your-secret-key" --port 8080

# Or use environment variable
export HM_API_KEY="your-secret-key"
hm mcp serve
```

### Discover Available Tools

```bash
# List all available tools
curl http://localhost:8080/mcp/tools | jq

# Get tools in OpenAI format (for GPT-4, o1, o3)
curl http://localhost:8080/agentic/tools/openai

# Get tools in Anthropic format (for Claude)
curl http://localhost:8080/agentic/tools/anthropic

# Get tools in Google format (for Gemini)
curl http://localhost:8080/agentic/tools/gemini
```

### Execute a Tool

```bash
curl -X POST http://localhost:8080/mcp/call \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-secret-key" \
  -d '{
    "tool": "vm.create",
    "arguments": {
      "name": "ai-sandbox",
      "cpu_cores": 4,
      "memory_mb": 8192,
      "enable_gpu": true
    }
  }'
```

## Python SDK

Install the Python SDK:

```bash
pip install hypermachine
```

Use it in your code:

```python
from hypermachine import HyperMachine

# Connect to HyperMachine
hm = HyperMachine("http://localhost:8080", api_key="your-secret-key")

# Create a VM
vm = hm.create_vm(
    name="python-vm",
    cpu=4,
    memory="8G",
    gpu=True
)

# Start the VM
vm.start()

# Execute a command
result = vm.exec("uname -a")
print(result.stdout)

# Stop and delete
vm.stop()
vm.delete()
```

## AI Agent Integration

### OpenAI GPT-4

```python
import openai
import requests

# Get HyperMachine tools in OpenAI format
tools = requests.get("http://localhost:8080/agentic/tools/openai").json()

# Use with OpenAI
client = openai.OpenAI()
response = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "Create a VM named ai-test with 4 CPUs"}],
    tools=tools
)

# Execute the tool call
if response.choices[0].message.tool_calls:
    tool_call = response.choices[0].message.tool_calls[0]
    result = requests.post(
        "http://localhost:8080/mcp/call",
        headers={"Authorization": "Bearer your-secret-key"},
        json={
            "tool": tool_call.function.name,
            "arguments": json.loads(tool_call.function.arguments)
        }
    )
```

### Anthropic Claude

```python
import anthropic
import requests

# Get HyperMachine tools in Anthropic format
tools = requests.get("http://localhost:8080/agentic/tools/anthropic").json()

# Use with Claude
client = anthropic.Anthropic()
response = client.messages.create(
    model="claude-sonnet-4-20250514",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Create a VM for running ML experiments"}],
    tools=tools
)
```

## Next Steps

- [Configuration](./configuration.md) - Customize HyperMachine settings
- [Architecture Overview](../architecture/overview.md) - Understand how HyperMachine works
- [AI Integration Guide](../ai/overview.md) - Deep dive into AI agent support
