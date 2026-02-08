# GUI Automation

HyperMachine includes a desktop GUI with a **semantic automation API** that enables AI agents to control the application programmatically.

## Why Semantic Automation?

Traditional GUI automation (screen-based) has problems:

| Approach                                | Deterministic | Fast | Layout Independent |
| --------------------------------------- | ------------- | ---- | ------------------ |
| Screen capture (Anthropic Computer Use) | ❌             | ❌    | ❌                  |
| DOM scraping                            | ⚠️             | ⚠️    | ❌                  |
| **Semantic API (HyperMachine)**         | ✅             | ✅    | ✅                  |

HyperMachine's semantic API provides:

- **Deterministic execution** - Commands always produce the same result
- **Fast response** - No image processing or screen capture
- **Layout independent** - Works regardless of window size or theme

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      AI Agent                            │
│                (LLM with tool calling)                   │
└────────────────────────┬────────────────────────────────┘
                         │ JSON commands
                         ▼
┌─────────────────────────────────────────────────────────┐
│               AutomationHandle                           │
│          (Async command channel)                         │
└────────────────────────┬────────────────────────────────┘
                         │ GuiCommand
                         ▼
┌─────────────────────────────────────────────────────────┐
│                   GUI Event Loop                         │
│             (egui/eframe application)                    │
└─────────────────────────────────────────────────────────┘
```

## Available Tools (13 total)

| Tool                  | Description                                                 |
| --------------------- | ----------------------------------------------------------- |
| `gui.navigate`        | Navigate to a view (welcome, vm_details, console, settings) |
| `gui.dialog.open`     | Open a dialog (create_vm, settings, about)                  |
| `gui.dialog.close`    | Close current dialog                                        |
| `gui.dialog.submit`   | Submit dialog form                                          |
| `gui.vm.select`       | Select VM by id, name, or partial match                     |
| `gui.vm.action`       | Perform VM action (start, stop, pause, delete, console)     |
| `gui.form.set_field`  | Set a form field value                                      |
| `gui.form.get_field`  | Get a form field value                                      |
| `gui.get_state`       | Query current GUI state                                     |
| `gui.list_vms`        | List VMs visible in GUI                                     |
| `gui.get_selected_vm` | Get currently selected VM                                   |
| `gui.refresh`         | Refresh the GUI state                                       |
| `gui.screenshot`      | Capture current GUI state as description                    |

## Rust API

### Basic Usage

```rust
use hm_gui::{AutomationHandle, GuiCommand, DialogType, FormType};

// Create automation handle
let (handle, receiver) = AutomationHandle::new();

// AI agent controls the GUI
handle.open_dialog(DialogType::CreateVm)?;
handle.set_field(FormType::CreateVm, "name", "ai-sandbox")?;
handle.set_field(FormType::CreateVm, "cpus", 4)?;
handle.set_field(FormType::CreateVm, "memory_mb", 8192)?;
handle.execute(GuiCommand::SubmitDialog(DialogType::CreateVm))?;
```

### Complete Example

```rust
use hm_gui::{App, AutomationHandle, GuiCommand};
use std::thread;

fn main() {
    // Create automation handle
    let (handle, receiver) = AutomationHandle::new();
    
    // Spawn GUI thread
    let gui_handle = handle.clone();
    thread::spawn(move || {
        let app = App::new_with_automation(receiver);
        eframe::run_native("HyperMachine", options, Box::new(|_| Ok(Box::new(app))));
    });
    
    // AI agent thread
    thread::spawn(move || {
        // Wait for GUI to initialize
        thread::sleep(Duration::from_secs(1));
        
        // Navigate to VM list
        handle.execute(GuiCommand::Navigate("vm_list".into())).unwrap();
        
        // Open create VM dialog
        handle.execute(GuiCommand::OpenDialog("create_vm".into())).unwrap();
        
        // Fill form
        handle.execute(GuiCommand::SetFormField {
            form: "create_vm".into(),
            field: "name".into(),
            value: "ai-created-vm".into(),
        }).unwrap();
        
        handle.execute(GuiCommand::SetFormField {
            form: "create_vm".into(),
            field: "cpus".into(),
            value: "4".into(),
        }).unwrap();
        
        // Submit
        handle.execute(GuiCommand::SubmitDialog("create_vm".into())).unwrap();
        
        // Get state
        let state = handle.get_state().unwrap();
        println!("Current view: {}", state.current_view);
    });
}
```

## JSON Command Protocol

AI agents send commands as JSON:

### Navigate

```json
{"type": "Navigate", "params": "vm_list"}
```

### Open Dialog

```json
{"type": "OpenDialog", "params": "create_vm"}
```

### Set Form Field

```json
{
  "type": "SetFormField",
  "params": {
    "form": "create_vm",
    "field": "name",
    "value": "my-vm"
  }
}
```

### Submit Dialog

```json
{"type": "SubmitDialog", "params": "create_vm"}
```

### Select VM

```json
{"type": "SelectVm", "params": {"by": "name", "value": "my-vm"}}
```

### VM Action

```json
{"type": "VmAction", "params": {"action": "start", "vm_id": "vm-123"}}
```

### Get State

```json
{"type": "GetState"}
```

Response:

```json
{
  "current_view": "vm_list",
  "selected_vm": "vm-123",
  "dialogs_open": [],
  "vms": [
    {"id": "vm-123", "name": "my-vm", "status": "running"}
  ]
}
```

## LLM Tool Definitions

### OpenAI Format

```json
{
  "type": "function",
  "function": {
    "name": "gui_navigate",
    "description": "Navigate to a view in the HyperMachine GUI",
    "parameters": {
      "type": "object",
      "properties": {
        "view": {
          "type": "string",
          "enum": ["welcome", "vm_list", "vm_details", "console", "settings"],
          "description": "Target view to navigate to"
        }
      },
      "required": ["view"]
    }
  }
}
```

### Fetch GUI Tools

```bash
# Get GUI tools in OpenAI format
curl http://localhost:8080/agentic/gui-tools/openai

# Get GUI tools in Anthropic format
curl http://localhost:8080/agentic/gui-tools/anthropic
```

## Integration Example

### Python AI Agent Controlling GUI

```python
import openai
import requests
import json

# Get GUI tools
gui_tools = requests.get(
    "http://localhost:8080/agentic/gui-tools/openai"
).json()

client = openai.OpenAI()

def execute_gui_command(command: dict) -> dict:
    """Execute a GUI command via the automation API."""
    return requests.post(
        "http://localhost:8080/gui/execute",
        json=command
    ).json()

# Chat loop
messages = [
    {"role": "system", "content": "You control a VM management GUI. Use the provided tools to help the user manage VMs."},
    {"role": "user", "content": "Create a new VM called 'data-processor' with 8 CPUs and 16GB RAM"}
]

response = client.chat.completions.create(
    model="gpt-4o",
    messages=messages,
    tools=gui_tools,
    tool_choice="auto"
)

# Execute tool calls
for tool_call in response.choices[0].message.tool_calls or []:
    args = json.loads(tool_call.function.arguments)
    
    if tool_call.function.name == "gui_navigate":
        execute_gui_command({"type": "Navigate", "params": args["view"]})
    
    elif tool_call.function.name == "gui_dialog_open":
        execute_gui_command({"type": "OpenDialog", "params": args["dialog"]})
    
    elif tool_call.function.name == "gui_form_set_field":
        execute_gui_command({
            "type": "SetFormField",
            "params": {
                "form": args["form"],
                "field": args["field"],
                "value": args["value"]
            }
        })
    
    elif tool_call.function.name == "gui_dialog_submit":
        execute_gui_command({"type": "SubmitDialog", "params": args["dialog"]})
```

## Views and Dialogs

### Available Views

| View         | Description                       |
| ------------ | --------------------------------- |
| `welcome`    | Welcome screen with quick actions |
| `vm_list`    | List of all VMs                   |
| `vm_details` | Details of selected VM            |
| `console`    | VM console output                 |
| `settings`   | Application settings              |

### Available Dialogs

| Dialog      | Fields                                     |
| ----------- | ------------------------------------------ |
| `create_vm` | name, cpus, memory_mb, disk_gb, enable_gpu |
| `settings`  | theme, api_key, data_dir                   |
| `about`     | (read-only info)                           |

## Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ai_creates_vm_through_gui() {
        let (handle, receiver) = AutomationHandle::new();
        
        // Simulate AI creating a VM
        handle.execute(GuiCommand::OpenDialog("create_vm".into())).unwrap();
        handle.execute(GuiCommand::SetFormField {
            form: "create_vm".into(),
            field: "name".into(),
            value: "test-vm".into(),
        }).unwrap();
        handle.execute(GuiCommand::SubmitDialog("create_vm".into())).unwrap();
        
        // Verify state
        let state = handle.get_state().unwrap();
        assert!(state.vms.iter().any(|vm| vm.name == "test-vm"));
    }
}
```

## Next Steps

- [AI Overview](./overview.md) - AI integration overview
- [MCP Server](./mcp-server.md) - API-based control
- [Python SDK](./python-sdk.md) - Python integration
