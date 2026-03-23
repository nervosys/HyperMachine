//! JSON Schema Generation for AI Agents
//!
//! Generates complete, self-describing schemas that AI agents can use
//! to understand and validate their tool calls.

use super::adapters::LlmProvider;
use super::ontology::HyperMachineOntology;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Complete schema bundle for AI agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSchema {
    /// Schema version
    #[serde(rename = "$schema")]
    pub schema: String,
    /// Schema ID
    #[serde(rename = "$id")]
    pub id: String,
    /// Title
    pub title: String,
    /// Description
    pub description: String,
    /// API version
    pub version: String,
    /// Type definitions
    pub definitions: serde_json::Value,
    /// Available operations
    pub operations: serde_json::Value,
    /// Usage examples
    pub examples: serde_json::Value,
    /// Quick reference
    pub quick_reference: serde_json::Value,
}

impl AgentSchema {
    /// Build the complete schema
    pub fn build() -> Self {
        let ontology = HyperMachineOntology::build();

        Self {
            schema: "https://json-schema.org/draft/2020-12/schema".to_string(),
            id: "https://hypermachine.dev/schema/agent/v1".to_string(),
            title: "HyperMachine Agent API".to_string(),
            description: "Complete API schema for AI agents to manage virtual machines".to_string(),
            version: ontology.version.clone(),
            definitions: Self::build_definitions(&ontology),
            operations: Self::build_operations(&ontology),
            examples: Self::build_examples(&ontology),
            quick_reference: Self::build_quick_reference(),
        }
    }

    fn build_definitions(ontology: &HyperMachineOntology) -> serde_json::Value {
        let mut defs = serde_json::Map::new();

        for type_def in &ontology.types {
            let mut def = serde_json::Map::new();
            def.insert("description".to_string(), json!(type_def.description));

            match type_def.base.as_str() {
                "enum" => {
                    def.insert("type".to_string(), json!("string"));
                    let values: Vec<&str> =
                        type_def.values.iter().map(|v| v.value.as_str()).collect();
                    def.insert("enum".to_string(), json!(values));

                    // Add descriptions as x-enum-descriptions
                    let descriptions: Vec<&str> = type_def
                        .values
                        .iter()
                        .map(|v| v.description.as_str())
                        .collect();
                    def.insert("x-enum-descriptions".to_string(), json!(descriptions));
                }
                "object" => {
                    def.insert("type".to_string(), json!("object"));
                    if let Some(ref schema) = type_def.schema {
                        if let Some(props) = schema.get("properties") {
                            def.insert("properties".to_string(), props.clone());
                        }
                    }
                }
                "string" => {
                    def.insert("type".to_string(), json!("string"));
                    if let Some(ref pattern) = type_def.pattern {
                        def.insert("pattern".to_string(), json!(pattern));
                    }
                }
                base => {
                    def.insert("type".to_string(), json!(base));
                }
            }

            defs.insert(type_def.name.clone(), serde_json::Value::Object(def));
        }

        serde_json::Value::Object(defs)
    }

    fn build_operations(ontology: &HyperMachineOntology) -> serde_json::Value {
        let mut ops = serde_json::Map::new();

        for (category, operations) in &ontology.operations {
            let mut cat_ops = Vec::new();

            for op in operations {
                let mut params = serde_json::Map::new();
                let mut required = Vec::new();

                for param in &op.parameters {
                    let mut p = serde_json::Map::new();
                    p.insert("type".to_string(), json!(Self::map_type(&param.param_type)));
                    p.insert("description".to_string(), json!(param.description));

                    if let Some(ref default) = param.default {
                        p.insert("default".to_string(), default.clone());
                    }
                    if let Some(ref range) = param.range {
                        if let Some(min) = range.min {
                            p.insert("minimum".to_string(), json!(min));
                        }
                        if let Some(max) = range.max {
                            p.insert("maximum".to_string(), json!(max));
                        }
                    }
                    if !param.enum_values.is_empty() {
                        p.insert("enum".to_string(), json!(param.enum_values));
                    }

                    params.insert(param.name.clone(), serde_json::Value::Object(p));

                    if param.required {
                        required.push(param.name.clone());
                    }
                }

                cat_ops.push(json!({
                    "id": op.id,
                    "name": op.name,
                    "description": op.description,
                    "parameters": {
                        "type": "object",
                        "properties": params,
                        "required": required,
                        "additionalProperties": false
                    },
                    "returns": {
                        "type": op.returns.return_type,
                        "description": op.returns.description
                    },
                    "idempotent": op.idempotent,
                    "safe": op.safe,
                    "examples": op.examples.iter().map(|ex| json!({
                        "description": ex.description,
                        "input": ex.input,
                        "output": ex.output
                    })).collect::<Vec<_>>()
                }));
            }

            ops.insert(category.clone(), json!(cat_ops));
        }

        serde_json::Value::Object(ops)
    }

    fn build_examples(ontology: &HyperMachineOntology) -> serde_json::Value {
        json!(ontology
            .examples
            .iter()
            .map(|ex| json!({
                "title": ex.title,
                "description": ex.description,
                "steps": ex.operations.iter().map(|step| json!({
                    "operation": step.operation,
                    "arguments": step.arguments,
                    "expected": step.expected,
                    "comment": step.comment
                })).collect::<Vec<_>>()
            }))
            .collect::<Vec<_>>())
    }

    fn build_quick_reference() -> serde_json::Value {
        json!({
            "lifecycle": {
                "create": "vm.create(name, cpu_cores?, memory_gb?, gpu_enabled?, network_enabled?)",
                "start": "vm.start(name)",
                "stop": "vm.stop(name)",
                "delete": "vm.delete(name)"
            },
            "query": {
                "list": "vm.list()",
                "get": "vm.get(name)",
                "metrics": "vm.metrics(name)"
            },
            "execution": {
                "script": "vm.execute_script(name, script, timeout_seconds?)"
            },
            "state_machine": {
                "Created": ["-> Running"],
                "Running": ["-> Stopped", "-> Paused"],
                "Paused": ["-> Running"],
                "Stopped": ["-> Running"],
                "Error": ["-> Stopped"]
            },
            "common_patterns": {
                "create_and_start": [
                    {"operation": "vm.create", "args": {"name": "X"}},
                    {"operation": "vm.start", "args": {"name": "X"}}
                ],
                "safe_delete": [
                    {"operation": "vm.stop", "args": {"name": "X"}},
                    {"operation": "vm.delete", "args": {"name": "X"}}
                ],
                "check_before_create": [
                    {"operation": "vm.list", "args": {}},
                    {"operation": "vm.create", "args": {"name": "X"}}
                ]
            }
        })
    }

    fn map_type(t: &str) -> &str {
        match t {
            "VmName" => "string",
            "VmState" => "string",
            "integer" => "integer",
            "float" | "number" => "number",
            "boolean" => "boolean",
            "array" => "array",
            _ => "string",
        }
    }
}

/// Compact schema for bandwidth-constrained scenarios
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactSchema {
    pub version: String,
    pub tools: Vec<CompactTool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactTool {
    pub id: String,
    pub desc: String,
    pub params: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<serde_json::Value>,
}

impl CompactSchema {
    /// Build a minimal schema
    pub fn build() -> Self {
        let ontology = HyperMachineOntology::build();
        let mut tools = Vec::new();

        for operations in ontology.operations.values() {
            for op in operations {
                let mut params = serde_json::Map::new();
                for param in &op.parameters {
                    params.insert(
                        param.name.clone(),
                        if param.required {
                            json!({"t": Self::short_type(&param.param_type), "r": true})
                        } else {
                            json!({"t": Self::short_type(&param.param_type)})
                        },
                    );
                }

                tools.push(CompactTool {
                    id: op.id.clone(),
                    desc: op.description.clone(),
                    params: json!(params),
                    example: op.examples.first().map(|ex| ex.input.clone()),
                });
            }
        }

        Self {
            version: ontology.version,
            tools,
        }
    }

    fn short_type(t: &str) -> &str {
        match t {
            "VmName" | "string" => "s",
            "integer" => "i",
            "boolean" => "b",
            "float" | "number" => "n",
            _ => "s",
        }
    }
}

/// Schema endpoints for HTTP API
pub struct SchemaEndpoints;

impl SchemaEndpoints {
    /// Get the full ontology
    pub fn ontology() -> serde_json::Value {
        serde_json::to_value(HyperMachineOntology::build()).expect("schema serialization failed")
    }

    /// Get the full schema
    pub fn full_schema() -> serde_json::Value {
        serde_json::to_value(AgentSchema::build()).expect("schema serialization failed")
    }

    /// Get compact schema
    pub fn compact_schema() -> serde_json::Value {
        serde_json::to_value(CompactSchema::build()).expect("schema serialization failed")
    }

    /// Get provider-specific configuration
    pub fn provider_config(provider: LlmProvider) -> serde_json::Value {
        use super::adapters::ProviderConfig;
        let ontology = HyperMachineOntology::build();
        serde_json::to_value(ProviderConfig::for_provider(provider, &ontology)).expect("schema serialization failed")
    }

    /// Get tools in OpenAI format
    pub fn openai_tools() -> serde_json::Value {
        use super::adapters::ProviderAdapter;
        let ontology = HyperMachineOntology::build();
        serde_json::to_value(ProviderAdapter::to_openai(&ontology)).expect("schema serialization failed")
    }

    /// Get tools in Anthropic format
    pub fn anthropic_tools() -> serde_json::Value {
        use super::adapters::ProviderAdapter;
        let ontology = HyperMachineOntology::build();
        serde_json::to_value(ProviderAdapter::to_anthropic(&ontology)).expect("schema serialization failed")
    }

    /// Get tools in Gemini format
    pub fn gemini_tools() -> serde_json::Value {
        use super::adapters::ProviderAdapter;
        let ontology = HyperMachineOntology::build();
        serde_json::to_value(ProviderAdapter::to_gemini(&ontology)).expect("schema serialization failed")
    }

    /// Get capabilities summary
    pub fn capabilities() -> serde_json::Value {
        use super::tools::Introspection;
        Introspection::capabilities_summary()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_schema_build() {
        let schema = AgentSchema::build();
        assert_eq!(schema.version, "1.0.0");
        assert!(!schema.definitions.as_object().unwrap().is_empty());
        assert!(!schema.operations.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_compact_schema_build() {
        let schema = CompactSchema::build();
        assert!(!schema.tools.is_empty());

        let create = schema.tools.iter().find(|t| t.id == "vm.create").unwrap();
        assert!(create.example.is_some());
    }

    #[test]
    fn test_schema_endpoints() {
        let ontology = SchemaEndpoints::ontology();
        assert_eq!(ontology["version"], "1.0.0");

        let openai = SchemaEndpoints::openai_tools();
        assert!(!openai.as_array().unwrap().is_empty());

        let caps = SchemaEndpoints::capabilities();
        assert!(caps["operations"].as_array().is_some());
    }

    #[test]
    fn test_agent_schema_fields() {
        let schema = AgentSchema::build();
        assert!(schema.schema.contains("json-schema.org"));
        assert!(schema.id.contains("hypermachine"));
        assert_eq!(schema.title, "HyperMachine Agent API");
    }

    #[test]
    fn test_compact_schema_short_types() {
        assert_eq!(CompactSchema::short_type("VmName"), "s");
        assert_eq!(CompactSchema::short_type("string"), "s");
        assert_eq!(CompactSchema::short_type("integer"), "i");
        assert_eq!(CompactSchema::short_type("boolean"), "b");
        assert_eq!(CompactSchema::short_type("float"), "n");
        assert_eq!(CompactSchema::short_type("number"), "n");
        assert_eq!(CompactSchema::short_type("unknown"), "s");
    }
}
