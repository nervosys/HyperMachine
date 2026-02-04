# LLM Tool Formats

HyperMachine provides tool definitions in native formats for each major LLM provider.

## OpenAI Format

For GPT-4, GPT-4o, o1, o3, and other OpenAI models.

### Fetch Tools

```bash
curl http://localhost:8080/agentic/tools/openai
```

### Format Structure

```json
[
  {
    "type": "function",
    "function": {
      "name": "vm_create",
      "description": "Create a new virtual machine with specified resources including CPU, memory, and optional GPU support",
      "parameters": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string",
            "description": "Unique name for the virtual machine"
          },
          "cpu_cores": {
            "type": "integer",
            "description": "Number of CPU cores (1-64)",
            "minimum": 1,
            "maximum": 64
          },
          "memory_mb": {
            "type": "integer",
            "description": "Memory in megabytes (512-262144)",
            "minimum": 512
          },
          "enable_gpu": {
            "type": "boolean",
            "description": "Enable GPU passthrough or virtual GPU"
          }
        },
        "required": ["name"]
      }
    }
  }
]
```

### Usage with OpenAI SDK

```python
import openai
import requests
import json

# Fetch HyperMachine tools
tools = requests.get("http://localhost:8080/agentic/tools/openai").json()

# Create OpenAI client
client = openai.OpenAI()

# Chat with tool support
response = client.chat.completions.create(
    model="gpt-4o",
    messages=[
        {"role": "user", "content": "Create a VM named 'ml-sandbox' with 8 CPUs and 32GB RAM for machine learning"}
    ],
    tools=tools,
    tool_choice="auto"
)

# Handle tool calls
if response.choices[0].message.tool_calls:
    for tool_call in response.choices[0].message.tool_calls:
        # Execute the tool
        result = requests.post(
            "http://localhost:8080/mcp/call",
            headers={"Authorization": "Bearer your-api-key"},
            json={
                "tool": tool_call.function.name.replace("_", "."),
                "arguments": json.loads(tool_call.function.arguments)
            }
        ).json()
        
        print(f"Tool result: {result}")
```

## Anthropic Format

For Claude 4, Claude Sonnet, Claude Haiku, and other Anthropic models.

### Fetch Tools

```bash
curl http://localhost:8080/agentic/tools/anthropic
```

### Format Structure

```json
[
  {
    "name": "vm_create",
    "description": "Create a new virtual machine with specified resources including CPU, memory, and optional GPU support",
    "input_schema": {
      "type": "object",
      "properties": {
        "name": {
          "type": "string",
          "description": "Unique name for the virtual machine"
        },
        "cpu_cores": {
          "type": "integer",
          "description": "Number of CPU cores (1-64)"
        },
        "memory_mb": {
          "type": "integer",
          "description": "Memory in megabytes (512-262144)"
        },
        "enable_gpu": {
          "type": "boolean",
          "description": "Enable GPU passthrough or virtual GPU"
        }
      },
      "required": ["name"]
    }
  }
]
```

### Usage with Anthropic SDK

```python
import anthropic
import requests
import json

# Fetch HyperMachine tools
tools = requests.get("http://localhost:8080/agentic/tools/anthropic").json()

# Create Anthropic client
client = anthropic.Anthropic()

# Chat with tool support
response = client.messages.create(
    model="claude-sonnet-4-20250514",
    max_tokens=1024,
    messages=[
        {"role": "user", "content": "Create a VM for running Python experiments"}
    ],
    tools=tools
)

# Handle tool use
for content in response.content:
    if content.type == "tool_use":
        # Execute the tool
        result = requests.post(
            "http://localhost:8080/mcp/call",
            headers={"Authorization": "Bearer your-api-key"},
            json={
                "tool": content.name.replace("_", "."),
                "arguments": content.input
            }
        ).json()
        
        print(f"Tool result: {result}")
```

## Google Gemini Format

For Gemini 2.5, Gemini Pro, and other Google models.

### Fetch Tools

```bash
curl http://localhost:8080/agentic/tools/gemini
```

### Format Structure

```json
[
  {
    "name": "vm_create",
    "description": "Create a new virtual machine with specified resources including CPU, memory, and optional GPU support",
    "parameters": {
      "type": "OBJECT",
      "properties": {
        "name": {
          "type": "STRING",
          "description": "Unique name for the virtual machine"
        },
        "cpu_cores": {
          "type": "INTEGER",
          "description": "Number of CPU cores (1-64)"
        },
        "memory_mb": {
          "type": "INTEGER",
          "description": "Memory in megabytes (512-262144)"
        },
        "enable_gpu": {
          "type": "BOOLEAN",
          "description": "Enable GPU passthrough or virtual GPU"
        }
      },
      "required": ["name"]
    }
  }
]
```

### Usage with Google Generative AI SDK

```python
import google.generativeai as genai
import requests

# Fetch HyperMachine tools
tools_response = requests.get("http://localhost:8080/agentic/tools/gemini").json()

# Convert to Gemini format
tools = [genai.protos.Tool(function_declarations=tools_response)]

# Configure Gemini
genai.configure(api_key="your-gemini-api-key")
model = genai.GenerativeModel("gemini-2.5-pro", tools=tools)

# Chat with tool support
chat = model.start_chat()
response = chat.send_message("Create a VM for data analysis with GPU support")

# Handle function calls
for part in response.parts:
    if fn := part.function_call:
        # Execute the tool
        result = requests.post(
            "http://localhost:8080/mcp/call",
            headers={"Authorization": "Bearer your-api-key"},
            json={
                "tool": fn.name.replace("_", "."),
                "arguments": dict(fn.args)
            }
        ).json()
        
        print(f"Tool result: {result}")
```

## Tool Name Mapping

HyperMachine uses dot notation internally, but LLMs often prefer underscores:

| Internal Name | LLM Name |
|--------------|----------|
| `vm.create` | `vm_create` |
| `vm.start` | `vm_start` |
| `vm.exec` | `vm_exec` |
| `vm.snapshot.create` | `vm_snapshot_create` |

The MCP server automatically handles this translation.

## Complete Tool List

| Tool | Description |
|------|-------------|
| `vm_create` | Create a new VM |
| `vm_start` | Start a VM |
| `vm_stop` | Stop a VM |
| `vm_delete` | Delete a VM |
| `vm_list` | List all VMs |
| `vm_get` | Get VM details |
| `vm_exec` | Execute command in VM |
| `vm_exec_script` | Execute script in VM |
| `vm_upload` | Upload file to VM |
| `vm_download` | Download file from VM |
| `vm_snapshot_create` | Create snapshot |
| `vm_snapshot_restore` | Restore snapshot |
| `vm_snapshot_list` | List snapshots |
| `vm_gpu_attach` | Attach GPU to VM |
| `vm_gpu_detach` | Detach GPU from VM |

## Custom Tool Extensions

You can extend the tool set with custom operations:

```rust
// In your HyperMachine extension
#[mcp_tool]
pub fn my_custom_tool(
    #[description("Input parameter")]
    param: String,
) -> Result<String> {
    // Implementation
    Ok(format!("Result: {}", param))
}
```

## Next Steps

- [MCP Server](./mcp-server.md) - Server configuration
- [Python SDK](./python-sdk.md) - High-level Python interface
- [GUI Automation](./gui-automation.md) - Desktop control
