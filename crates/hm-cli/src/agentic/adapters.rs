//! LLM Provider Adapters
//!
//! Provides first-class support for major LLM providers (as of January 2026):
//! - OpenAI (GPT-5, GPT-5-turbo, GPT-4o, o1, o3, o3-mini)
//! - Anthropic (Claude 4.5 Opus/Sonnet, Claude 4, Claude 3.5)
//! - Google (Gemini 2.5, Gemini 2.0 Ultra/Pro, Gemini Flash)
//! - Other providers following OpenAI-compatible formats

use super::ontology::{HyperMachineOntology, Operation, Parameter, Range};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Supported LLM providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    OpenAI,
    Anthropic,
    Google,
    Generic,
}

impl std::str::FromStr for LlmProvider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" | "gpt" | "chatgpt" => Ok(LlmProvider::OpenAI),
            "anthropic" | "claude" => Ok(LlmProvider::Anthropic),
            "google" | "gemini" => Ok(LlmProvider::Google),
            "generic" | "other" => Ok(LlmProvider::Generic),
            _ => Err(format!("Unknown provider: {}", s)),
        }
    }
}

/// OpenAI Function Calling Format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAITool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OpenAIFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// Anthropic Tool Use Format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<AnthropicCacheControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicCacheControl {
    #[serde(rename = "type")]
    pub cache_type: String,
}

/// Google Gemini Function Declaration Format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiTool {
    pub function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiFunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: GeminiParameters,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiParameters {
    #[serde(rename = "type")]
    pub param_type: String,
    pub properties: serde_json::Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
}

/// Tool call request format (unified)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    /// Tool/function name
    pub tool: String,
    /// Arguments as JSON
    pub arguments: serde_json::Value,
    /// Optional call ID (for tracking)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
}

/// Tool call response format (unified)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResponse {
    /// Whether the call succeeded
    pub success: bool,
    /// Result data (on success)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error message (on failure)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Original call ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
}

/// Adapter for converting ontology to provider-specific formats
pub struct ProviderAdapter;

impl ProviderAdapter {
    /// Get tools in OpenAI function calling format
    pub fn to_openai(ontology: &HyperMachineOntology) -> Vec<OpenAITool> {
        let mut tools = Vec::new();

        for (_category, operations) in &ontology.operations {
            for op in operations {
                tools.push(Self::operation_to_openai(op));
            }
        }

        tools
    }

    /// Get tools in Anthropic tool use format
    pub fn to_anthropic(ontology: &HyperMachineOntology) -> Vec<AnthropicTool> {
        let mut tools = Vec::new();

        for (_category, operations) in &ontology.operations {
            for op in operations {
                tools.push(Self::operation_to_anthropic(op));
            }
        }

        tools
    }

    /// Get tools in Google Gemini format
    pub fn to_gemini(ontology: &HyperMachineOntology) -> GeminiTool {
        let mut declarations = Vec::new();

        for (_category, operations) in &ontology.operations {
            for op in operations {
                declarations.push(Self::operation_to_gemini(op));
            }
        }

        GeminiTool {
            function_declarations: declarations,
        }
    }

    /// Get tools in a generic format (similar to OpenAI but simpler)
    pub fn to_generic(ontology: &HyperMachineOntology) -> Vec<serde_json::Value> {
        let mut tools = Vec::new();

        for (_category, operations) in &ontology.operations {
            for op in operations {
                tools.push(json!({
                    "name": op.id,
                    "description": op.description,
                    "parameters": Self::params_to_json_schema(&op.parameters),
                    "returns": {
                        "type": op.returns.return_type,
                        "description": op.returns.description
                    },
                    "idempotent": op.idempotent,
                    "safe": op.safe
                }));
            }
        }

        tools
    }

    fn operation_to_openai(op: &Operation) -> OpenAITool {
        OpenAITool {
            tool_type: "function".to_string(),
            function: OpenAIFunction {
                name: op.id.clone(),
                description: Self::build_rich_description(op),
                parameters: Self::params_to_json_schema(&op.parameters),
                strict: Some(true),
            },
        }
    }

    fn operation_to_anthropic(op: &Operation) -> AnthropicTool {
        AnthropicTool {
            name: op.id.clone(),
            description: Self::build_rich_description(op),
            input_schema: Self::params_to_json_schema(&op.parameters),
            cache_control: None,
        }
    }

    fn operation_to_gemini(op: &Operation) -> GeminiFunctionDeclaration {
        let schema = Self::params_to_json_schema(&op.parameters);
        let properties = schema.get("properties").cloned().unwrap_or(json!({}));
        let required = schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        GeminiFunctionDeclaration {
            name: op.id.clone(),
            description: Self::build_rich_description(op),
            parameters: GeminiParameters {
                param_type: "object".to_string(),
                properties,
                required,
            },
        }
    }

    fn build_rich_description(op: &Operation) -> String {
        let mut desc = op.description.clone();

        // Add precondition info
        if !op.preconditions.is_empty() {
            desc.push_str("\n\nPreconditions:");
            for pre in &op.preconditions {
                desc.push_str(&format!("\n- {}", pre.error_message));
            }
        }

        // Add idempotency/safety info
        if op.idempotent {
            desc.push_str("\n\nThis operation is idempotent (safe to retry).");
        }
        if op.safe {
            desc.push_str("\nThis operation has no side effects.");
        }

        // Add example if available
        if let Some(example) = op.examples.first() {
            desc.push_str(&format!("\n\nExample: {}", example.description));
        }

        desc
    }

    fn params_to_json_schema(params: &[Parameter]) -> serde_json::Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in params {
            let mut prop = serde_json::Map::new();

            // Map type
            let json_type = match param.param_type.as_str() {
                "VmName" | "string" => "string",
                "integer" => "integer",
                "float" | "number" => "number",
                "boolean" => "boolean",
                "array" => "array",
                _ => "string",
            };
            prop.insert("type".to_string(), json!(json_type));
            prop.insert("description".to_string(), json!(param.description));

            // Add constraints
            if let Some(ref range) = param.range {
                Self::add_range_constraints(&mut prop, range);
            }

            // Add enum values
            if !param.enum_values.is_empty() {
                prop.insert("enum".to_string(), json!(param.enum_values));
            }

            // Add default
            if let Some(ref default) = param.default {
                prop.insert("default".to_string(), default.clone());
            }

            // Add pattern for VmName
            if param.param_type == "VmName" {
                prop.insert("pattern".to_string(), json!("^[a-zA-Z][a-zA-Z0-9-]{0,62}$"));
            }

            properties.insert(param.name.clone(), serde_json::Value::Object(prop));

            if param.required {
                required.push(param.name.clone());
            }
        }

        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        })
    }

    fn add_range_constraints(prop: &mut serde_json::Map<String, serde_json::Value>, range: &Range) {
        if let Some(min) = range.min {
            prop.insert("minimum".to_string(), json!(min));
        }
        if let Some(max) = range.max {
            prop.insert("maximum".to_string(), json!(max));
        }
    }
}

/// System prompts for different providers
pub struct SystemPrompts;

impl SystemPrompts {
    /// Get an optimized system prompt for OpenAI models
    pub fn for_openai() -> String {
        r#"You are an AI assistant with access to HyperMachine VM management tools.

CAPABILITIES:
- Create, start, stop, and delete virtual machines
- Monitor VM metrics and performance
- Execute scripts within VMs

GUIDELINES:
1. Use vm.list to discover existing VMs before operations
2. Use vm.get to check VM state before state-changing operations
3. VM names must be alphanumeric with hyphens (e.g., "my-vm-01")
4. Always verify operations completed by checking the result

TOOL USAGE:
- Call tools with the exact parameter names shown in the schema
- Required parameters must always be provided
- Optional parameters use defaults if omitted"#
            .to_string()
    }

    /// Get an optimized system prompt for Anthropic Claude
    pub fn for_anthropic() -> String {
        r#"You are an AI assistant with access to HyperMachine VM management tools.

<capabilities>
- Create, start, stop, and delete virtual machines
- Monitor VM metrics and performance  
- Execute scripts within VMs
</capabilities>

<guidelines>
1. Use vm.list to discover existing VMs before operations
2. Use vm.get to check VM state before state-changing operations
3. VM names must be alphanumeric with hyphens (e.g., "my-vm-01")
4. Always verify operations completed by checking the result
</guidelines>

<tool_usage>
- Call tools with the exact parameter names shown in the schema
- Required parameters must always be provided
- Optional parameters use defaults if omitted
</tool_usage>"#
            .to_string()
    }

    /// Get an optimized system prompt for Google Gemini
    pub fn for_gemini() -> String {
        r#"You are an AI assistant with access to HyperMachine VM management tools.

**Capabilities:**
- Create, start, stop, and delete virtual machines
- Monitor VM metrics and performance
- Execute scripts within VMs

**Guidelines:**
1. Use vm.list to discover existing VMs before operations
2. Use vm.get to check VM state before state-changing operations
3. VM names must be alphanumeric with hyphens (e.g., "my-vm-01")
4. Always verify operations completed by checking the result

**Tool Usage:**
- Call tools with the exact parameter names shown in the schema
- Required parameters must always be provided
- Optional parameters use defaults if omitted"#
            .to_string()
    }

    /// Get a generic system prompt
    pub fn generic() -> String {
        r#"You are an AI assistant with access to HyperMachine VM management tools.

Available operations:
- vm.create: Create a new VM
- vm.start: Start a stopped VM
- vm.stop: Stop a running VM
- vm.delete: Delete a VM
- vm.list: List all VMs
- vm.get: Get VM details
- vm.metrics: Get VM performance metrics
- vm.execute_script: Run a script in a VM

Guidelines:
1. Always check vm.list before creating to avoid name conflicts
2. Check vm.get to verify VM state before operations
3. VM names: alphanumeric with hyphens, starting with a letter
4. Verify operation success by checking the result"#
            .to_string()
    }
}

/// Configuration bundle for a specific provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider name
    pub provider: LlmProvider,
    /// Tools in provider-specific format
    pub tools: serde_json::Value,
    /// Recommended system prompt
    pub system_prompt: String,
    /// Provider-specific configuration hints
    pub hints: ProviderHints,
}

/// Provider-specific configuration hints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderHints {
    /// Recommended temperature setting
    pub temperature: f32,
    /// Whether to use parallel tool calls
    pub parallel_tool_calls: bool,
    /// Maximum tokens for response
    pub max_tokens: Option<u32>,
    /// Additional provider-specific settings
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl ProviderConfig {
    /// Create configuration for a specific provider
    pub fn for_provider(provider: LlmProvider, ontology: &HyperMachineOntology) -> Self {
        match provider {
            LlmProvider::OpenAI => Self {
                provider,
                tools: serde_json::to_value(ProviderAdapter::to_openai(ontology)).unwrap(),
                system_prompt: SystemPrompts::for_openai(),
                hints: ProviderHints {
                    temperature: 0.0,
                    parallel_tool_calls: true,
                    max_tokens: Some(4096),
                    extra: serde_json::Map::new(),
                },
            },
            LlmProvider::Anthropic => Self {
                provider,
                tools: serde_json::to_value(ProviderAdapter::to_anthropic(ontology)).unwrap(),
                system_prompt: SystemPrompts::for_anthropic(),
                hints: ProviderHints {
                    temperature: 0.0,
                    parallel_tool_calls: true,
                    max_tokens: Some(4096),
                    extra: serde_json::Map::new(),
                },
            },
            LlmProvider::Google => Self {
                provider,
                tools: serde_json::to_value(ProviderAdapter::to_gemini(ontology)).unwrap(),
                system_prompt: SystemPrompts::for_gemini(),
                hints: ProviderHints {
                    temperature: 0.0,
                    parallel_tool_calls: true,
                    max_tokens: Some(8192),
                    extra: serde_json::Map::new(),
                },
            },
            LlmProvider::Generic => Self {
                provider,
                tools: serde_json::to_value(ProviderAdapter::to_generic(ontology)).unwrap(),
                system_prompt: SystemPrompts::generic(),
                hints: ProviderHints {
                    temperature: 0.0,
                    parallel_tool_calls: false,
                    max_tokens: None,
                    extra: serde_json::Map::new(),
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_format() {
        let ontology = HyperMachineOntology::build();
        let tools = ProviderAdapter::to_openai(&ontology);

        assert!(!tools.is_empty());

        // Find vm.create
        let create = tools
            .iter()
            .find(|t| t.function.name == "vm.create")
            .unwrap();
        assert_eq!(create.tool_type, "function");
        assert!(create.function.description.contains("Create"));
    }

    #[test]
    fn test_anthropic_format() {
        let ontology = HyperMachineOntology::build();
        let tools = ProviderAdapter::to_anthropic(&ontology);

        assert!(!tools.is_empty());

        let create = tools.iter().find(|t| t.name == "vm.create").unwrap();
        assert!(create.description.contains("Create"));
        assert!(create.input_schema.get("properties").is_some());
    }

    #[test]
    fn test_gemini_format() {
        let ontology = HyperMachineOntology::build();
        let tool = ProviderAdapter::to_gemini(&ontology);

        assert!(!tool.function_declarations.is_empty());

        let create = tool
            .function_declarations
            .iter()
            .find(|f| f.name == "vm.create")
            .unwrap();
        assert_eq!(create.parameters.param_type, "object");
    }

    #[test]
    fn test_provider_config() {
        let ontology = HyperMachineOntology::build();

        let openai_config = ProviderConfig::for_provider(LlmProvider::OpenAI, &ontology);
        assert_eq!(openai_config.provider, LlmProvider::OpenAI);
        assert!(openai_config.hints.parallel_tool_calls);

        let anthropic_config = ProviderConfig::for_provider(LlmProvider::Anthropic, &ontology);
        assert!(anthropic_config.system_prompt.contains("<capabilities>"));
    }
}
