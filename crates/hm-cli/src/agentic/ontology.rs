//! Programming Language Ontology for HyperMachine
//!
//! Defines a generalizable ontology that exposes the full VM control syntax
//! in a way that's transparent and discoverable by AI agents.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The complete HyperMachine ontology - describes all concepts, types, and operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperMachineOntology {
    /// API version
    pub version: String,
    /// Human-readable description
    pub description: String,
    /// Core concepts in the domain
    pub concepts: Vec<Concept>,
    /// Primitive types
    pub types: Vec<TypeDefinition>,
    /// Available operations grouped by category
    pub operations: HashMap<String, Vec<Operation>>,
    /// Relationships between concepts
    pub relationships: Vec<Relationship>,
    /// Usage examples
    pub examples: Vec<Example>,
}

/// A domain concept (e.g., VM, CPU, Memory)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Concept {
    /// Unique identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description of the concept
    pub description: String,
    /// Properties of this concept
    pub properties: Vec<Property>,
    /// Valid states for this concept
    #[serde(default)]
    pub states: Vec<State>,
    /// Constraints on this concept
    #[serde(default)]
    pub constraints: Vec<Constraint>,
}

/// A property of a concept
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Property {
    /// Property name
    pub name: String,
    /// Property type (reference to TypeDefinition)
    #[serde(rename = "type")]
    pub prop_type: String,
    /// Description
    pub description: String,
    /// Whether this property is required
    #[serde(default)]
    pub required: bool,
    /// Whether this property is read-only
    #[serde(default)]
    pub readonly: bool,
    /// Default value (as JSON)
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    /// Valid range for numeric types
    #[serde(default)]
    pub range: Option<Range>,
}

/// A numeric range constraint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub min: Option<i64>,
    pub max: Option<i64>,
}

/// A valid state for a concept
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    /// State name
    pub name: String,
    /// Description
    pub description: String,
    /// Valid transitions from this state
    pub transitions: Vec<String>,
}

/// A constraint on a concept or operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
    /// Constraint type
    #[serde(rename = "type")]
    pub constraint_type: ConstraintType,
    /// Description
    pub description: String,
    /// Constraint expression (for complex constraints)
    #[serde(default)]
    pub expression: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintType {
    Required,
    Unique,
    Range,
    Pattern,
    Dependency,
    Mutex,
    Custom,
}

/// A type definition in the ontology
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDefinition {
    /// Type name
    pub name: String,
    /// Base type (string, integer, boolean, object, array, enum)
    pub base: String,
    /// Description
    pub description: String,
    /// For enums: valid values
    #[serde(default)]
    pub values: Vec<EnumValue>,
    /// For objects: schema
    #[serde(default)]
    pub schema: Option<serde_json::Value>,
    /// For strings: pattern
    #[serde(default)]
    pub pattern: Option<String>,
}

/// An enum value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumValue {
    pub value: String,
    pub description: String,
}

/// An operation that can be performed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    /// Operation ID (e.g., "vm.create")
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description
    pub description: String,
    /// Input parameters
    pub parameters: Vec<Parameter>,
    /// Return type
    pub returns: ReturnType,
    /// Required concept states (preconditions)
    #[serde(default)]
    pub preconditions: Vec<Precondition>,
    /// State changes caused by this operation
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// Whether this operation is idempotent
    #[serde(default)]
    pub idempotent: bool,
    /// Whether this operation is safe (no side effects)
    #[serde(default)]
    pub safe: bool,
    /// Usage examples
    #[serde(default)]
    pub examples: Vec<OperationExample>,
}

/// A parameter for an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    /// Parameter name
    pub name: String,
    /// Parameter type
    #[serde(rename = "type")]
    pub param_type: String,
    /// Description
    pub description: String,
    /// Whether required
    #[serde(default)]
    pub required: bool,
    /// Default value
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    /// Valid values (for enums)
    #[serde(default)]
    pub enum_values: Vec<String>,
    /// Range constraints
    #[serde(default)]
    pub range: Option<Range>,
}

/// Return type for an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnType {
    /// Type name
    #[serde(rename = "type")]
    pub return_type: String,
    /// Description
    pub description: String,
    /// Schema for complex types
    #[serde(default)]
    pub schema: Option<serde_json::Value>,
}

/// A precondition for an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Precondition {
    /// Concept this applies to
    pub concept: String,
    /// Required state
    pub state: Option<String>,
    /// Custom condition
    pub condition: Option<String>,
    /// Error message if not met
    pub error_message: String,
}

/// An effect of an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Effect {
    /// Concept affected
    pub concept: String,
    /// State transition
    pub transition: Option<StateTransition>,
    /// Property changes
    #[serde(default)]
    pub property_changes: Vec<PropertyChange>,
}

/// A state transition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    pub from: Option<String>,
    pub to: String,
}

/// A property change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyChange {
    pub property: String,
    pub change: String,
}

/// An example of using an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationExample {
    /// Description of what this example does
    pub description: String,
    /// Input arguments
    pub input: serde_json::Value,
    /// Expected output
    pub output: serde_json::Value,
}

/// A relationship between concepts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    /// Relationship type
    #[serde(rename = "type")]
    pub rel_type: RelationshipType,
    /// Source concept
    pub from: String,
    /// Target concept
    pub to: String,
    /// Description
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipType {
    Contains,
    DependsOn,
    Manages,
    Connects,
    Inherits,
}

/// A usage example
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Example {
    /// Example title
    pub title: String,
    /// Description
    pub description: String,
    /// Sequence of operations
    pub operations: Vec<ExampleOperation>,
}

/// An operation in an example
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExampleOperation {
    /// Operation ID
    pub operation: String,
    /// Arguments
    pub arguments: serde_json::Value,
    /// Expected result
    #[serde(default)]
    pub expected: Option<serde_json::Value>,
    /// Comment
    #[serde(default)]
    pub comment: Option<String>,
}

impl HyperMachineOntology {
    /// Create the complete HyperMachine ontology
    pub fn build() -> Self {
        Self {
            version: "1.0.0".to_string(),
            description: "HyperMachine VM Control Ontology - A complete, discoverable API for AI agents to manage virtual machines".to_string(),
            concepts: Self::build_concepts(),
            types: Self::build_types(),
            operations: Self::build_operations(),
            relationships: Self::build_relationships(),
            examples: Self::build_examples(),
        }
    }

    fn build_concepts() -> Vec<Concept> {
        vec![
            Concept {
                id: "vm".to_string(),
                name: "Virtual Machine".to_string(),
                description:
                    "A virtual machine instance that can be created, started, stopped, and deleted"
                        .to_string(),
                properties: vec![
                    Property {
                        name: "name".to_string(),
                        prop_type: "VmName".to_string(),
                        description: "Unique identifier for the VM".to_string(),
                        required: true,
                        readonly: true,
                        default: None,
                        range: None,
                    },
                    Property {
                        name: "cpu_cores".to_string(),
                        prop_type: "integer".to_string(),
                        description: "Number of virtual CPU cores".to_string(),
                        required: false,
                        readonly: false,
                        default: Some(serde_json::json!(2)),
                        range: Some(Range {
                            min: Some(1),
                            max: Some(128),
                        }),
                    },
                    Property {
                        name: "memory_gb".to_string(),
                        prop_type: "integer".to_string(),
                        description: "Memory allocation in gigabytes".to_string(),
                        required: false,
                        readonly: false,
                        default: Some(serde_json::json!(4)),
                        range: Some(Range {
                            min: Some(1),
                            max: Some(1024),
                        }),
                    },
                    Property {
                        name: "gpu_enabled".to_string(),
                        prop_type: "boolean".to_string(),
                        description: "Whether GPU passthrough is enabled".to_string(),
                        required: false,
                        readonly: false,
                        default: Some(serde_json::json!(false)),
                        range: None,
                    },
                    Property {
                        name: "network_enabled".to_string(),
                        prop_type: "boolean".to_string(),
                        description: "Whether networking is enabled".to_string(),
                        required: false,
                        readonly: false,
                        default: Some(serde_json::json!(false)),
                        range: None,
                    },
                    Property {
                        name: "state".to_string(),
                        prop_type: "VmState".to_string(),
                        description: "Current state of the VM".to_string(),
                        required: false,
                        readonly: true,
                        default: None,
                        range: None,
                    },
                    Property {
                        name: "created_at".to_string(),
                        prop_type: "datetime".to_string(),
                        description: "When the VM was created".to_string(),
                        required: false,
                        readonly: true,
                        default: None,
                        range: None,
                    },
                ],
                states: vec![
                    State {
                        name: "Created".to_string(),
                        description: "VM has been created but never started".to_string(),
                        transitions: vec!["Running".to_string()],
                    },
                    State {
                        name: "Running".to_string(),
                        description: "VM is currently executing".to_string(),
                        transitions: vec!["Stopped".to_string(), "Paused".to_string()],
                    },
                    State {
                        name: "Paused".to_string(),
                        description: "VM execution is suspended".to_string(),
                        transitions: vec!["Running".to_string()],
                    },
                    State {
                        name: "Stopped".to_string(),
                        description: "VM has been stopped".to_string(),
                        transitions: vec!["Running".to_string()],
                    },
                    State {
                        name: "Error".to_string(),
                        description: "VM encountered an error".to_string(),
                        transitions: vec!["Stopped".to_string()],
                    },
                ],
                constraints: vec![
                    Constraint {
                        constraint_type: ConstraintType::Unique,
                        description: "VM names must be unique".to_string(),
                        expression: None,
                    },
                    Constraint {
                        constraint_type: ConstraintType::Pattern,
                        description: "VM names must be alphanumeric with hyphens".to_string(),
                        expression: Some("^[a-zA-Z][a-zA-Z0-9-]*$".to_string()),
                    },
                ],
            },
            Concept {
                id: "metrics".to_string(),
                name: "VM Metrics".to_string(),
                description: "Runtime metrics for a virtual machine".to_string(),
                properties: vec![
                    Property {
                        name: "name".to_string(),
                        prop_type: "string".to_string(),
                        description: "VM name".to_string(),
                        required: true,
                        readonly: true,
                        default: None,
                        range: None,
                    },
                    Property {
                        name: "state".to_string(),
                        prop_type: "string".to_string(),
                        description: "Current VM state".to_string(),
                        required: true,
                        readonly: true,
                        default: None,
                        range: None,
                    },
                    Property {
                        name: "cpu_cores".to_string(),
                        prop_type: "integer".to_string(),
                        description: "Number of CPU cores allocated".to_string(),
                        required: true,
                        readonly: true,
                        default: None,
                        range: None,
                    },
                    Property {
                        name: "memory_gb".to_string(),
                        prop_type: "integer".to_string(),
                        description: "Memory allocated in GB".to_string(),
                        required: true,
                        readonly: true,
                        default: None,
                        range: None,
                    },
                    Property {
                        name: "cpu_usage_percent".to_string(),
                        prop_type: "float".to_string(),
                        description: "CPU usage as a percentage (0-100)".to_string(),
                        required: false,
                        readonly: true,
                        default: None,
                        range: Some(Range {
                            min: Some(0),
                            max: Some(100),
                        }),
                    },
                    Property {
                        name: "memory_used_gb".to_string(),
                        prop_type: "float".to_string(),
                        description: "Memory currently in use (GB)".to_string(),
                        required: false,
                        readonly: true,
                        default: None,
                        range: None,
                    },
                    Property {
                        name: "uptime_seconds".to_string(),
                        prop_type: "integer".to_string(),
                        description: "VM uptime in seconds".to_string(),
                        required: false,
                        readonly: true,
                        default: None,
                        range: None,
                    },
                ],
                states: vec![],
                constraints: vec![],
            },
            Concept {
                id: "script".to_string(),
                name: "Script Execution".to_string(),
                description: "A script to be executed within a VM".to_string(),
                properties: vec![
                    Property {
                        name: "content".to_string(),
                        prop_type: "string".to_string(),
                        description: "The script content to execute".to_string(),
                        required: true,
                        readonly: false,
                        default: None,
                        range: None,
                    },
                    Property {
                        name: "timeout_seconds".to_string(),
                        prop_type: "integer".to_string(),
                        description: "Maximum execution time".to_string(),
                        required: false,
                        readonly: false,
                        default: Some(serde_json::json!(300)),
                        range: Some(Range {
                            min: Some(1),
                            max: Some(3600),
                        }),
                    },
                ],
                states: vec![],
                constraints: vec![],
            },
        ]
    }

    fn build_types() -> Vec<TypeDefinition> {
        vec![
            TypeDefinition {
                name: "VmName".to_string(),
                base: "string".to_string(),
                description: "A valid VM name (alphanumeric with hyphens, starting with a letter)"
                    .to_string(),
                values: vec![],
                schema: None,
                pattern: Some("^[a-zA-Z][a-zA-Z0-9-]{0,62}$".to_string()),
            },
            TypeDefinition {
                name: "VmState".to_string(),
                base: "enum".to_string(),
                description: "The current state of a virtual machine".to_string(),
                values: vec![
                    EnumValue {
                        value: "Created".to_string(),
                        description: "VM created but never started".to_string(),
                    },
                    EnumValue {
                        value: "Running".to_string(),
                        description: "VM is executing".to_string(),
                    },
                    EnumValue {
                        value: "Paused".to_string(),
                        description: "VM is paused".to_string(),
                    },
                    EnumValue {
                        value: "Stopped".to_string(),
                        description: "VM is stopped".to_string(),
                    },
                    EnumValue {
                        value: "Error".to_string(),
                        description: "VM in error state".to_string(),
                    },
                ],
                schema: None,
                pattern: None,
            },
            TypeDefinition {
                name: "VmInfo".to_string(),
                base: "object".to_string(),
                description: "Complete information about a virtual machine".to_string(),
                values: vec![],
                schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "cpu_cores": { "type": "integer" },
                        "memory_gb": { "type": "integer" },
                        "gpu_enabled": { "type": "boolean" },
                        "network_enabled": { "type": "boolean" },
                        "state": { "type": "string" },
                        "created_at": { "type": "string", "format": "date-time" },
                        "updated_at": { "type": "string", "format": "date-time" }
                    }
                })),
                pattern: None,
            },
            TypeDefinition {
                name: "ScriptResult".to_string(),
                base: "object".to_string(),
                description: "Result of script execution".to_string(),
                values: vec![],
                schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "success": { "type": "boolean" },
                        "output": { "type": "string" },
                        "exit_code": { "type": "integer" },
                        "execution_time_ms": { "type": "integer" }
                    }
                })),
                pattern: None,
            },
        ]
    }

    fn build_operations() -> HashMap<String, Vec<Operation>> {
        let mut ops = HashMap::new();

        // VM Lifecycle Operations
        ops.insert(
            "lifecycle".to_string(),
            vec![
                Operation {
                    id: "vm.create".to_string(),
                    name: "Create VM".to_string(),
                    description: "Create a new virtual machine with specified configuration"
                        .to_string(),
                    parameters: vec![
                        Parameter {
                            name: "name".to_string(),
                            param_type: "VmName".to_string(),
                            description: "Unique name for the VM".to_string(),
                            required: true,
                            default: None,
                            enum_values: vec![],
                            range: None,
                        },
                        Parameter {
                            name: "cpu_cores".to_string(),
                            param_type: "integer".to_string(),
                            description: "Number of virtual CPU cores".to_string(),
                            required: false,
                            default: Some(serde_json::json!(2)),
                            enum_values: vec![],
                            range: Some(Range {
                                min: Some(1),
                                max: Some(128),
                            }),
                        },
                        Parameter {
                            name: "memory_gb".to_string(),
                            param_type: "integer".to_string(),
                            description: "Memory size in gigabytes".to_string(),
                            required: false,
                            default: Some(serde_json::json!(4)),
                            enum_values: vec![],
                            range: Some(Range {
                                min: Some(1),
                                max: Some(1024),
                            }),
                        },
                        Parameter {
                            name: "gpu_enabled".to_string(),
                            param_type: "boolean".to_string(),
                            description: "Enable GPU passthrough".to_string(),
                            required: false,
                            default: Some(serde_json::json!(false)),
                            enum_values: vec![],
                            range: None,
                        },
                        Parameter {
                            name: "network_enabled".to_string(),
                            param_type: "boolean".to_string(),
                            description: "Enable networking".to_string(),
                            required: false,
                            default: Some(serde_json::json!(false)),
                            enum_values: vec![],
                            range: None,
                        },
                    ],
                    returns: ReturnType {
                        return_type: "VmInfo".to_string(),
                        description: "The created VM information".to_string(),
                        schema: None,
                    },
                    preconditions: vec![Precondition {
                        concept: "vm".to_string(),
                        state: None,
                        condition: Some("!exists(name)".to_string()),
                        error_message: "A VM with this name already exists".to_string(),
                    }],
                    effects: vec![Effect {
                        concept: "vm".to_string(),
                        transition: Some(StateTransition {
                            from: None,
                            to: "Created".to_string(),
                        }),
                        property_changes: vec![],
                    }],
                    idempotent: false,
                    safe: false,
                    examples: vec![
                        OperationExample {
                            description: "Create a basic VM".to_string(),
                            input: serde_json::json!({"name": "my-vm"}),
                            output: serde_json::json!({
                                "name": "my-vm",
                                "cpu_cores": 2,
                                "memory_gb": 4,
                                "gpu_enabled": false,
                                "network_enabled": false,
                                "state": "Created"
                            }),
                        },
                        OperationExample {
                            description: "Create a GPU-enabled VM with 8 cores".to_string(),
                            input: serde_json::json!({
                                "name": "gpu-vm",
                                "cpu_cores": 8,
                                "memory_gb": 32,
                                "gpu_enabled": true,
                                "network_enabled": true
                            }),
                            output: serde_json::json!({
                                "name": "gpu-vm",
                                "cpu_cores": 8,
                                "memory_gb": 32,
                                "gpu_enabled": true,
                                "network_enabled": true,
                                "state": "Created"
                            }),
                        },
                    ],
                },
                Operation {
                    id: "vm.start".to_string(),
                    name: "Start VM".to_string(),
                    description: "Start a stopped or newly created virtual machine".to_string(),
                    parameters: vec![Parameter {
                        name: "name".to_string(),
                        param_type: "VmName".to_string(),
                        description: "Name of the VM to start".to_string(),
                        required: true,
                        default: None,
                        enum_values: vec![],
                        range: None,
                    }],
                    returns: ReturnType {
                        return_type: "object".to_string(),
                        description: "Operation result".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "status": { "type": "string" },
                                "name": { "type": "string" }
                            }
                        })),
                    },
                    preconditions: vec![Precondition {
                        concept: "vm".to_string(),
                        state: None,
                        condition: Some("state in ['Created', 'Stopped']".to_string()),
                        error_message: "VM must be in Created or Stopped state to start"
                            .to_string(),
                    }],
                    effects: vec![Effect {
                        concept: "vm".to_string(),
                        transition: Some(StateTransition {
                            from: None,
                            to: "Running".to_string(),
                        }),
                        property_changes: vec![],
                    }],
                    idempotent: false,
                    safe: false,
                    examples: vec![OperationExample {
                        description: "Start a VM".to_string(),
                        input: serde_json::json!({"name": "my-vm"}),
                        output: serde_json::json!({"status": "started", "name": "my-vm"}),
                    }],
                },
                Operation {
                    id: "vm.stop".to_string(),
                    name: "Stop VM".to_string(),
                    description: "Stop a running virtual machine".to_string(),
                    parameters: vec![Parameter {
                        name: "name".to_string(),
                        param_type: "VmName".to_string(),
                        description: "Name of the VM to stop".to_string(),
                        required: true,
                        default: None,
                        enum_values: vec![],
                        range: None,
                    }],
                    returns: ReturnType {
                        return_type: "object".to_string(),
                        description: "Operation result".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "status": { "type": "string" },
                                "name": { "type": "string" }
                            }
                        })),
                    },
                    preconditions: vec![],
                    effects: vec![Effect {
                        concept: "vm".to_string(),
                        transition: Some(StateTransition {
                            from: None,
                            to: "Stopped".to_string(),
                        }),
                        property_changes: vec![],
                    }],
                    idempotent: true,
                    safe: false,
                    examples: vec![OperationExample {
                        description: "Stop a VM".to_string(),
                        input: serde_json::json!({"name": "my-vm"}),
                        output: serde_json::json!({"status": "stopped", "name": "my-vm"}),
                    }],
                },
                Operation {
                    id: "vm.delete".to_string(),
                    name: "Delete VM".to_string(),
                    description: "Delete a virtual machine (stops it first if running)".to_string(),
                    parameters: vec![Parameter {
                        name: "name".to_string(),
                        param_type: "VmName".to_string(),
                        description: "Name of the VM to delete".to_string(),
                        required: true,
                        default: None,
                        enum_values: vec![],
                        range: None,
                    }],
                    returns: ReturnType {
                        return_type: "object".to_string(),
                        description: "Operation result".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "status": { "type": "string" },
                                "name": { "type": "string" }
                            }
                        })),
                    },
                    preconditions: vec![],
                    effects: vec![Effect {
                        concept: "vm".to_string(),
                        transition: None,
                        property_changes: vec![PropertyChange {
                            property: "existence".to_string(),
                            change: "removed".to_string(),
                        }],
                    }],
                    idempotent: true,
                    safe: false,
                    examples: vec![OperationExample {
                        description: "Delete a VM".to_string(),
                        input: serde_json::json!({"name": "my-vm"}),
                        output: serde_json::json!({"status": "deleted", "name": "my-vm"}),
                    }],
                },
            ],
        );

        // Query Operations
        ops.insert(
            "query".to_string(),
            vec![
                Operation {
                    id: "vm.list".to_string(),
                    name: "List VMs".to_string(),
                    description: "List all virtual machines".to_string(),
                    parameters: vec![],
                    returns: ReturnType {
                        return_type: "array".to_string(),
                        description: "Array of VM information".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "array",
                            "items": { "$ref": "#/types/VmInfo" }
                        })),
                    },
                    preconditions: vec![],
                    effects: vec![],
                    idempotent: true,
                    safe: true,
                    examples: vec![OperationExample {
                        description: "List all VMs".to_string(),
                        input: serde_json::json!({}),
                        output: serde_json::json!([
                            {"name": "vm-1", "state": "Running", "cpu_cores": 4},
                            {"name": "vm-2", "state": "Stopped", "cpu_cores": 2}
                        ]),
                    }],
                },
                Operation {
                    id: "vm.get".to_string(),
                    name: "Get VM".to_string(),
                    description: "Get information about a specific virtual machine".to_string(),
                    parameters: vec![Parameter {
                        name: "name".to_string(),
                        param_type: "VmName".to_string(),
                        description: "Name of the VM".to_string(),
                        required: true,
                        default: None,
                        enum_values: vec![],
                        range: None,
                    }],
                    returns: ReturnType {
                        return_type: "VmInfo".to_string(),
                        description: "VM information".to_string(),
                        schema: None,
                    },
                    preconditions: vec![Precondition {
                        concept: "vm".to_string(),
                        state: None,
                        condition: Some("exists(name)".to_string()),
                        error_message: "VM not found".to_string(),
                    }],
                    effects: vec![],
                    idempotent: true,
                    safe: true,
                    examples: vec![OperationExample {
                        description: "Get VM details".to_string(),
                        input: serde_json::json!({"name": "my-vm"}),
                        output: serde_json::json!({
                            "name": "my-vm",
                            "cpu_cores": 4,
                            "memory_gb": 8,
                            "state": "Running"
                        }),
                    }],
                },
                Operation {
                    id: "vm.metrics".to_string(),
                    name: "Get VM Metrics".to_string(),
                    description: "Get runtime metrics for a virtual machine".to_string(),
                    parameters: vec![Parameter {
                        name: "name".to_string(),
                        param_type: "VmName".to_string(),
                        description: "Name of the VM".to_string(),
                        required: true,
                        default: None,
                        enum_values: vec![],
                        range: None,
                    }],
                    returns: ReturnType {
                        return_type: "object".to_string(),
                        description: "VM metrics".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "state": { "type": "string" },
                                "cpu_cores": { "type": "integer" },
                                "memory_gb": { "type": "integer" },
                                "cpu_usage_percent": { "type": "number" },
                                "memory_used_gb": { "type": "number" },
                                "uptime_seconds": { "type": "integer" }
                            }
                        })),
                    },
                    preconditions: vec![],
                    effects: vec![],
                    idempotent: true,
                    safe: true,
                    examples: vec![OperationExample {
                        description: "Get VM metrics".to_string(),
                        input: serde_json::json!({"name": "my-vm"}),
                        output: serde_json::json!({
                            "name": "my-vm",
                            "state": "Running",
                            "cpu_cores": 4,
                            "memory_gb": 8,
                            "cpu_usage_percent": 45.5,
                            "memory_used_gb": 2.5,
                            "uptime_seconds": 3600
                        }),
                    }],
                },
            ],
        );

        // Execution Operations
        ops.insert(
            "execution".to_string(),
            vec![Operation {
                id: "vm.execute_script".to_string(),
                name: "Execute Script".to_string(),
                description: "Execute a script within a running virtual machine".to_string(),
                parameters: vec![
                    Parameter {
                        name: "name".to_string(),
                        param_type: "VmName".to_string(),
                        description: "Name of the VM".to_string(),
                        required: true,
                        default: None,
                        enum_values: vec![],
                        range: None,
                    },
                    Parameter {
                        name: "script".to_string(),
                        param_type: "string".to_string(),
                        description: "Script content to execute".to_string(),
                        required: true,
                        default: None,
                        enum_values: vec![],
                        range: None,
                    },
                    Parameter {
                        name: "timeout_seconds".to_string(),
                        param_type: "integer".to_string(),
                        description: "Maximum execution time in seconds".to_string(),
                        required: false,
                        default: Some(serde_json::json!(300)),
                        enum_values: vec![],
                        range: Some(Range {
                            min: Some(1),
                            max: Some(3600),
                        }),
                    },
                ],
                returns: ReturnType {
                    return_type: "ScriptResult".to_string(),
                    description: "Script execution result".to_string(),
                    schema: None,
                },
                preconditions: vec![Precondition {
                    concept: "vm".to_string(),
                    state: Some("Running".to_string()),
                    condition: None,
                    error_message: "VM must be running to execute scripts".to_string(),
                }],
                effects: vec![],
                idempotent: false,
                safe: false,
                examples: vec![OperationExample {
                    description: "Execute a simple script".to_string(),
                    input: serde_json::json!({
                        "name": "my-vm",
                        "script": "echo 'Hello, World!'"
                    }),
                    output: serde_json::json!({
                        "success": true,
                        "output": "Hello, World!\n",
                        "exit_code": 0,
                        "execution_time_ms": 15
                    }),
                }],
            }],
        );

        ops
    }

    fn build_relationships() -> Vec<Relationship> {
        vec![
            Relationship {
                rel_type: RelationshipType::Contains,
                from: "vm".to_string(),
                to: "metrics".to_string(),
                description: "A VM has associated metrics".to_string(),
            },
            Relationship {
                rel_type: RelationshipType::Manages,
                from: "vm".to_string(),
                to: "script".to_string(),
                description: "A VM can execute scripts".to_string(),
            },
        ]
    }

    fn build_examples() -> Vec<Example> {
        vec![
            Example {
                title: "Create and Start a VM".to_string(),
                description: "Basic workflow to create and start a virtual machine".to_string(),
                operations: vec![
                    ExampleOperation {
                        operation: "vm.create".to_string(),
                        arguments: serde_json::json!({
                            "name": "dev-vm",
                            "cpu_cores": 4,
                            "memory_gb": 8
                        }),
                        expected: Some(serde_json::json!({"state": "Created"})),
                        comment: Some("Create a new VM with 4 cores and 8GB RAM".to_string()),
                    },
                    ExampleOperation {
                        operation: "vm.start".to_string(),
                        arguments: serde_json::json!({"name": "dev-vm"}),
                        expected: Some(serde_json::json!({"status": "started"})),
                        comment: Some("Start the VM".to_string()),
                    },
                    ExampleOperation {
                        operation: "vm.get".to_string(),
                        arguments: serde_json::json!({"name": "dev-vm"}),
                        expected: Some(serde_json::json!({"state": "Running"})),
                        comment: Some("Verify the VM is running".to_string()),
                    },
                ],
            },
            Example {
                title: "Monitor VM Performance".to_string(),
                description: "Check VM metrics to monitor performance".to_string(),
                operations: vec![
                    ExampleOperation {
                        operation: "vm.list".to_string(),
                        arguments: serde_json::json!({}),
                        expected: None,
                        comment: Some("List all VMs to find the target".to_string()),
                    },
                    ExampleOperation {
                        operation: "vm.metrics".to_string(),
                        arguments: serde_json::json!({"name": "prod-vm"}),
                        expected: None,
                        comment: Some("Get performance metrics".to_string()),
                    },
                ],
            },
            Example {
                title: "Cleanup Workflow".to_string(),
                description: "Stop and delete a VM".to_string(),
                operations: vec![
                    ExampleOperation {
                        operation: "vm.stop".to_string(),
                        arguments: serde_json::json!({"name": "temp-vm"}),
                        expected: Some(serde_json::json!({"status": "stopped"})),
                        comment: Some("Stop the VM first".to_string()),
                    },
                    ExampleOperation {
                        operation: "vm.delete".to_string(),
                        arguments: serde_json::json!({"name": "temp-vm"}),
                        expected: Some(serde_json::json!({"status": "deleted"})),
                        comment: Some("Then delete it".to_string()),
                    },
                ],
            },
        ]
    }
}
