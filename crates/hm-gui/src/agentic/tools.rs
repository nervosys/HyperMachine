//! Tool Definitions for AI Agents
//!
//! Provides OpenAI/Anthropic-compatible tool definitions that describe
//! the GUI automation capabilities for LLM function calling.

use serde::{Deserialize, Serialize};
use serde_json::json;

/// Tool definition compatible with OpenAI/Anthropic function calling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiToolDefinition {
    /// Tool name (e.g., "gui.navigate")
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for parameters
    pub parameters: serde_json::Value,
}

/// Get all GUI automation tool definitions
pub fn get_gui_tools() -> Vec<GuiToolDefinition> {
    vec![
        // Navigation
        GuiToolDefinition {
            name: "gui.navigate".to_string(),
            description: "Navigate to a specific view in the HyperMachine GUI".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "view": {
                        "type": "string",
                        "enum": ["welcome", "vm_details", "vm_console", "settings"],
                        "description": "Target view to navigate to"
                    }
                },
                "required": ["view"]
            }),
        },
        // Dialog management
        GuiToolDefinition {
            name: "gui.dialog.open".to_string(),
            description: "Open a dialog in the HyperMachine GUI".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "dialog": {
                        "type": "string",
                        "enum": ["create_vm", "settings", "about"],
                        "description": "Type of dialog to open"
                    }
                },
                "required": ["dialog"]
            }),
        },
        GuiToolDefinition {
            name: "gui.dialog.close".to_string(),
            description: "Close a dialog in the HyperMachine GUI".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "dialog": {
                        "type": "string",
                        "enum": ["create_vm", "settings", "about"],
                        "description": "Type of dialog to close"
                    }
                },
                "required": ["dialog"]
            }),
        },
        GuiToolDefinition {
            name: "gui.dialog.submit".to_string(),
            description: "Submit/confirm the current dialog in the HyperMachine GUI".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "dialog": {
                        "type": "string",
                        "enum": ["create_vm", "settings"],
                        "description": "Type of dialog to submit"
                    }
                },
                "required": ["dialog"]
            }),
        },
        // VM selection
        GuiToolDefinition {
            name: "gui.vm.select".to_string(),
            description: "Select a VM in the HyperMachine GUI sidebar".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "identifier": {
                        "type": "string",
                        "description": "VM ID or name to select"
                    },
                    "by": {
                        "type": "string",
                        "enum": ["id", "name", "name_contains"],
                        "default": "id",
                        "description": "How to match the identifier"
                    }
                },
                "required": ["identifier"]
            }),
        },
        GuiToolDefinition {
            name: "gui.vm.deselect".to_string(),
            description: "Deselect the currently selected VM".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // Form manipulation
        GuiToolDefinition {
            name: "gui.form.set_field".to_string(),
            description: "Set a form field value in the HyperMachine GUI".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "form": {
                        "type": "string",
                        "enum": ["create_vm", "settings"],
                        "description": "Which form to modify"
                    },
                    "field": {
                        "type": "string",
                        "description": "Field name to set (e.g., 'name', 'cpus', 'memory_mb')"
                    },
                    "value": {
                        "description": "Value to set (type depends on field)"
                    }
                },
                "required": ["form", "field", "value"]
            }),
        },
        GuiToolDefinition {
            name: "gui.form.reset".to_string(),
            description: "Reset a form to its default values".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "form": {
                        "type": "string",
                        "enum": ["create_vm", "settings"],
                        "description": "Which form to reset"
                    }
                },
                "required": ["form"]
            }),
        },
        // VM actions
        GuiToolDefinition {
            name: "gui.vm.action".to_string(),
            description: "Execute an action on the selected VM".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["start", "stop", "pause", "resume", "delete", "open_console", "close_console"],
                        "description": "Action to perform on the selected VM"
                    }
                },
                "required": ["action"]
            }),
        },
        // Toggle settings
        GuiToolDefinition {
            name: "gui.toggle".to_string(),
            description: "Toggle a boolean setting in the HyperMachine GUI".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "setting": {
                        "type": "string",
                        "enum": ["auto_refresh", "dark_mode"],
                        "description": "Setting to toggle"
                    },
                    "value": {
                        "type": "boolean",
                        "description": "Explicit value to set (optional, toggles if not provided)"
                    }
                },
                "required": ["setting"]
            }),
        },
        // State queries
        GuiToolDefinition {
            name: "gui.refresh".to_string(),
            description: "Refresh the VM list from the backend".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        },
        GuiToolDefinition {
            name: "gui.get_state".to_string(),
            description: "Get the current state of the HyperMachine GUI including selected VM, open dialogs, and VM counts".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        },
        GuiToolDefinition {
            name: "gui.get_available_actions".to_string(),
            description: "Get all actions available in the current GUI context".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}

/// Get tool definitions in OpenAI function calling format
pub fn get_openai_tools() -> Vec<serde_json::Value> {
    get_gui_tools()
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters
                }
            })
        })
        .collect()
}

/// Get tool definitions in Anthropic format
pub fn get_anthropic_tools() -> Vec<serde_json::Value> {
    get_gui_tools()
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.parameters
            })
        })
        .collect()
}

/// Get tool definitions in Google Gemini format
pub fn get_gemini_tools() -> Vec<serde_json::Value> {
    vec![json!({
        "function_declarations": get_gui_tools()
            .into_iter()
            .map(|tool| json!({
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.parameters
            }))
            .collect::<Vec<_>>()
    })]
}

/// Combined GUI + VM tools for complete agent capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapabilities {
    /// API version
    pub version: String,
    /// GUI automation tools
    pub gui_tools: Vec<GuiToolDefinition>,
    /// Description of capabilities
    pub description: String,
    /// Usage examples
    pub examples: Vec<UsageExample>,
}

/// Example of tool usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageExample {
    /// Description of what the example does
    pub description: String,
    /// Sequence of tool calls
    pub tool_calls: Vec<serde_json::Value>,
}

impl AgentCapabilities {
    /// Build complete agent capabilities
    pub fn build() -> Self {
        Self {
            version: "1.0.0".to_string(),
            gui_tools: get_gui_tools(),
            description: "HyperMachine GUI automation tools for AI agents. These tools enable semantic control of the GUI without screen capture or mouse simulation.".to_string(),
            examples: vec![
                UsageExample {
                    description: "Create a new VM with 4 CPUs and 8GB RAM".to_string(),
                    tool_calls: vec![
                        json!({
                            "name": "gui.dialog.open",
                            "arguments": { "dialog": "create_vm" }
                        }),
                        json!({
                            "name": "gui.form.set_field",
                            "arguments": { "form": "create_vm", "field": "name", "value": "my-vm" }
                        }),
                        json!({
                            "name": "gui.form.set_field",
                            "arguments": { "form": "create_vm", "field": "cpus", "value": 4 }
                        }),
                        json!({
                            "name": "gui.form.set_field",
                            "arguments": { "form": "create_vm", "field": "memory_mb", "value": 8192 }
                        }),
                        json!({
                            "name": "gui.dialog.submit",
                            "arguments": { "dialog": "create_vm" }
                        }),
                    ],
                },
                UsageExample {
                    description: "Select a VM and open its console".to_string(),
                    tool_calls: vec![
                        json!({
                            "name": "gui.vm.select",
                            "arguments": { "identifier": "my-vm", "by": "name" }
                        }),
                        json!({
                            "name": "gui.vm.action",
                            "arguments": { "action": "open_console" }
                        }),
                    ],
                },
                UsageExample {
                    description: "Stop a running VM".to_string(),
                    tool_calls: vec![
                        json!({
                            "name": "gui.vm.select",
                            "arguments": { "identifier": "my-vm", "by": "name" }
                        }),
                        json!({
                            "name": "gui.vm.action",
                            "arguments": { "action": "stop" }
                        }),
                    ],
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_gui_tools() {
        let tools = get_gui_tools();
        assert!(!tools.is_empty());
        
        // Check for essential tools
        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"gui.navigate"));
        assert!(tool_names.contains(&"gui.dialog.open"));
        assert!(tool_names.contains(&"gui.vm.select"));
        assert!(tool_names.contains(&"gui.get_state"));
    }

    #[test]
    fn test_openai_format() {
        let tools = get_openai_tools();
        assert!(!tools.is_empty());
        
        let first = &tools[0];
        assert!(first.get("type").is_some());
        assert!(first.get("function").is_some());
    }

    #[test]
    fn test_anthropic_format() {
        let tools = get_anthropic_tools();
        assert!(!tools.is_empty());
        
        let first = &tools[0];
        assert!(first.get("name").is_some());
        assert!(first.get("input_schema").is_some());
    }

    #[test]
    fn test_agent_capabilities() {
        let caps = AgentCapabilities::build();
        assert!(!caps.gui_tools.is_empty());
        assert!(!caps.examples.is_empty());
    }
}
