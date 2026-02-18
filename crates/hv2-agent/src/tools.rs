//! Agent Tool System
//!
//! This module provides a tool-use framework for AI agents:
//! - Tool registration and discovery
//! - Function calling with parameter validation
//! - Tool execution with result handling
//! - Tool categories and permissions
//! - Tool chaining and composition
//! - Execution history and auditing

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

/// Tool error types
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ToolError {
    /// Tool not found
    #[error("Tool not found: {0}")]
    NotFound(String),
    /// Tool already registered
    #[error("Tool already registered: {0}")]
    AlreadyRegistered(String),
    /// Invalid parameters
    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),
    /// Missing required parameter
    #[error("Missing required parameter: {0}")]
    MissingParameter(String),
    /// Parameter type mismatch
    #[error("Type mismatch for '{param}': expected {expected}, got {got}")]
    TypeMismatch {
        param: String,
        expected: String,
        got: String,
    },
    /// Execution failed
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    /// Tool disabled
    #[error("Tool disabled: {0}")]
    ToolDisabled(String),
    /// Timeout
    #[error("Timeout after {0:?}")]
    Timeout(Duration),
}

/// Result type for tool operations
pub type ToolResult<T> = Result<T, ToolError>;

/// Tool category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolCategory {
    /// File system operations
    FileSystem,
    /// Network operations
    Network,
    /// System operations
    System,
    /// Data processing
    Data,
    /// Web browsing/scraping
    Web,
    /// Code execution
    Code,
    /// Database operations
    Database,
    /// API calls
    Api,
    /// Custom/other
    Custom,
}

impl fmt::Display for ToolCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileSystem => write!(f, "filesystem"),
            Self::Network => write!(f, "network"),
            Self::System => write!(f, "system"),
            Self::Data => write!(f, "data"),
            Self::Web => write!(f, "web"),
            Self::Code => write!(f, "code"),
            Self::Database => write!(f, "database"),
            Self::Api => write!(f, "api"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

/// Parameter type for tool parameters
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParameterType {
    /// String value
    String,
    /// Integer value
    Integer,
    /// Floating point value
    Float,
    /// Boolean value
    Boolean,
    /// Array of values
    Array(Box<ParameterType>),
    /// Object/map
    Object,
    /// Any JSON value
    Any,
}

impl fmt::Display for ParameterType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String => write!(f, "string"),
            Self::Integer => write!(f, "integer"),
            Self::Float => write!(f, "float"),
            Self::Boolean => write!(f, "boolean"),
            Self::Array(inner) => write!(f, "array<{}>", inner),
            Self::Object => write!(f, "object"),
            Self::Any => write!(f, "any"),
        }
    }
}

impl ParameterType {
    /// Check if a JSON value matches this type
    pub fn matches(&self, value: &JsonValue) -> bool {
        match (self, value) {
            (Self::String, JsonValue::String(_)) => true,
            (Self::Integer, JsonValue::Number(n)) => n.is_i64() || n.is_u64(),
            (Self::Float, JsonValue::Number(_)) => true,
            (Self::Boolean, JsonValue::Bool(_)) => true,
            (Self::Array(_), JsonValue::Array(_)) => true,
            (Self::Object, JsonValue::Object(_)) => true,
            (Self::Any, _) => true,
            _ => false,
        }
    }
}

/// Tool parameter definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParameter {
    /// Parameter name
    pub name: String,
    /// Parameter description
    pub description: String,
    /// Parameter type
    pub param_type: ParameterType,
    /// Whether required
    pub required: bool,
    /// Default value (if not required)
    pub default: Option<JsonValue>,
    /// Enum values (if constrained)
    pub enum_values: Option<Vec<JsonValue>>,
}

impl ToolParameter {
    /// Create a new required parameter
    pub fn required(name: &str, param_type: ParameterType, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            param_type,
            required: true,
            default: None,
            enum_values: None,
        }
    }

    /// Create a new optional parameter
    pub fn optional(name: &str, param_type: ParameterType, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            param_type,
            required: false,
            default: None,
            enum_values: None,
        }
    }

    /// Set default value
    pub fn with_default(mut self, default: JsonValue) -> Self {
        self.default = Some(default);
        self
    }

    /// Set enum constraint
    pub fn with_enum(mut self, values: Vec<JsonValue>) -> Self {
        self.enum_values = Some(values);
        self
    }

    /// Validate a value against this parameter
    pub fn validate(&self, value: Option<&JsonValue>) -> ToolResult<JsonValue> {
        match value {
            Some(v) => {
                // Check type
                if !self.param_type.matches(v) {
                    return Err(ToolError::TypeMismatch {
                        param: self.name.clone(),
                        expected: self.param_type.to_string(),
                        got: json_type_name(v),
                    });
                }

                // Check enum constraint
                if let Some(ref allowed) = self.enum_values {
                    if !allowed.contains(v) {
                        return Err(ToolError::InvalidParameters(format!(
                            "Value for '{}' must be one of: {:?}",
                            self.name, allowed
                        )));
                    }
                }

                Ok(v.clone())
            }
            None => {
                if self.required {
                    Err(ToolError::MissingParameter(self.name.clone()))
                } else {
                    Ok(self.default.clone().unwrap_or(JsonValue::Null))
                }
            }
        }
    }
}

/// Get JSON type name for error messages
fn json_type_name(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(_) => "boolean".to_string(),
        JsonValue::Number(_) => "number".to_string(),
        JsonValue::String(_) => "string".to_string(),
        JsonValue::Array(_) => "array".to_string(),
        JsonValue::Object(_) => "object".to_string(),
    }
}

/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name (unique identifier)
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Tool category
    pub category: ToolCategory,
    /// Parameters
    pub parameters: Vec<ToolParameter>,
    /// Whether tool is enabled
    pub enabled: bool,
    /// Required permissions
    pub permissions: Vec<String>,
    /// Maximum execution time
    pub timeout: Option<Duration>,
}

impl ToolDefinition {
    /// Create a new tool definition
    pub fn new(name: &str, description: &str, category: ToolCategory) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            category,
            parameters: Vec::new(),
            enabled: true,
            permissions: Vec::new(),
            timeout: None,
        }
    }

    /// Add a parameter
    pub fn with_parameter(mut self, param: ToolParameter) -> Self {
        self.parameters.push(param);
        self
    }

    /// Add a permission
    pub fn with_permission(mut self, permission: &str) -> Self {
        self.permissions.push(permission.to_string());
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Disable the tool
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Validate arguments against parameters
    pub fn validate_args(
        &self,
        args: &HashMap<String, JsonValue>,
    ) -> ToolResult<HashMap<String, JsonValue>> {
        let mut validated = HashMap::new();

        for param in &self.parameters {
            let value = args.get(&param.name);
            let validated_value = param.validate(value)?;
            validated.insert(param.name.clone(), validated_value);
        }

        Ok(validated)
    }

    /// Get required parameters
    pub fn required_params(&self) -> Vec<&ToolParameter> {
        self.parameters.iter().filter(|p| p.required).collect()
    }

    /// Get optional parameters
    pub fn optional_params(&self) -> Vec<&ToolParameter> {
        self.parameters.iter().filter(|p| !p.required).collect()
    }
}

/// Tool call request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique call ID
    pub id: String,
    /// Tool name
    pub tool_name: String,
    /// Arguments
    pub arguments: HashMap<String, JsonValue>,
    /// Caller identifier
    pub caller: String,
    /// Request timestamp
    pub timestamp: SystemTime,
}

impl ToolCall {
    /// Create a new tool call
    pub fn new(id: &str, tool_name: &str, caller: &str) -> Self {
        Self {
            id: id.to_string(),
            tool_name: tool_name.to_string(),
            arguments: HashMap::new(),
            caller: caller.to_string(),
            timestamp: SystemTime::now(),
        }
    }

    /// Add an argument
    pub fn with_arg(mut self, name: &str, value: JsonValue) -> Self {
        self.arguments.insert(name.to_string(), value);
        self
    }

    /// Set all arguments
    pub fn with_args(mut self, args: HashMap<String, JsonValue>) -> Self {
        self.arguments = args;
        self
    }
}

/// Tool execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    /// Call ID
    pub call_id: String,
    /// Whether execution succeeded
    pub success: bool,
    /// Result value (if success)
    pub result: Option<JsonValue>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Execution duration
    pub duration: Duration,
    /// Completion timestamp
    pub completed_at: SystemTime,
}

impl ToolCallResult {
    /// Create a success result
    pub fn success(call_id: &str, result: JsonValue, duration: Duration) -> Self {
        Self {
            call_id: call_id.to_string(),
            success: true,
            result: Some(result),
            error: None,
            duration,
            completed_at: SystemTime::now(),
        }
    }

    /// Create a failure result
    pub fn failure(call_id: &str, error: &str, duration: Duration) -> Self {
        Self {
            call_id: call_id.to_string(),
            success: false,
            result: None,
            error: Some(error.to_string()),
            duration,
            completed_at: SystemTime::now(),
        }
    }
}

/// Tool handler function type
pub type ToolHandler =
    Box<dyn Fn(&HashMap<String, JsonValue>) -> ToolResult<JsonValue> + Send + Sync>;

/// Registered tool with handler
pub struct RegisteredTool {
    /// Tool definition
    pub definition: ToolDefinition,
    /// Handler function
    handler: ToolHandler,
}

impl fmt::Debug for RegisteredTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisteredTool")
            .field("definition", &self.definition)
            .field("handler", &"<function>")
            .finish()
    }
}

impl RegisteredTool {
    /// Create a new registered tool
    pub fn new<F>(definition: ToolDefinition, handler: F) -> Self
    where
        F: Fn(&HashMap<String, JsonValue>) -> ToolResult<JsonValue> + Send + Sync + 'static,
    {
        Self {
            definition,
            handler: Box::new(handler),
        }
    }

    /// Execute the tool
    pub fn execute(&self, args: &HashMap<String, JsonValue>) -> ToolResult<JsonValue> {
        if !self.definition.enabled {
            return Err(ToolError::ToolDisabled(self.definition.name.clone()));
        }

        // Validate arguments
        let validated = self.definition.validate_args(args)?;

        // Execute handler
        (self.handler)(&validated)
    }
}

/// Tool registry
#[derive(Debug, Default)]
pub struct ToolRegistry {
    /// Registered tools
    tools: HashMap<String, RegisteredTool>,
    /// Tools by category
    by_category: HashMap<ToolCategory, Vec<String>>,
    /// Execution history
    history: Vec<ToolCallResult>,
    /// Maximum history size
    max_history: usize,
}

impl ToolRegistry {
    /// Create a new tool registry
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            by_category: HashMap::new(),
            history: Vec::new(),
            max_history: 1000,
        }
    }

    /// Set maximum history size
    pub fn with_max_history(mut self, max: usize) -> Self {
        self.max_history = max;
        self
    }

    /// Register a tool
    pub fn register(&mut self, tool: RegisteredTool) -> ToolResult<()> {
        let name = tool.definition.name.clone();
        let category = tool.definition.category;

        if self.tools.contains_key(&name) {
            return Err(ToolError::AlreadyRegistered(name));
        }

        self.by_category
            .entry(category)
            .or_default()
            .push(name.clone());
        self.tools.insert(name, tool);

        Ok(())
    }

    /// Unregister a tool
    pub fn unregister(&mut self, name: &str) -> ToolResult<RegisteredTool> {
        let tool = self
            .tools
            .remove(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;

        // Remove from category index
        if let Some(tools) = self.by_category.get_mut(&tool.definition.category) {
            tools.retain(|n| n != name);
        }

        Ok(tool)
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<&RegisteredTool> {
        self.tools.get(name)
    }

    /// List all tools
    pub fn list(&self) -> Vec<&ToolDefinition> {
        self.tools.values().map(|t| &t.definition).collect()
    }

    /// List tools by category
    pub fn list_by_category(&self, category: ToolCategory) -> Vec<&ToolDefinition> {
        self.by_category
            .get(&category)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|n| self.tools.get(n))
                    .map(|t| &t.definition)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// List enabled tools only
    pub fn list_enabled(&self) -> Vec<&ToolDefinition> {
        self.tools
            .values()
            .filter(|t| t.definition.enabled)
            .map(|t| &t.definition)
            .collect()
    }

    /// Execute a tool call
    pub fn execute(&mut self, call: &ToolCall) -> ToolResult<ToolCallResult> {
        let start = std::time::Instant::now();

        let result = match self.tools.get(&call.tool_name) {
            Some(tool) => match tool.execute(&call.arguments) {
                Ok(value) => ToolCallResult::success(&call.id, value, start.elapsed()),
                Err(e) => ToolCallResult::failure(&call.id, &e.to_string(), start.elapsed()),
            },
            None => ToolCallResult::failure(
                &call.id,
                &format!("Tool not found: {}", call.tool_name),
                start.elapsed(),
            ),
        };

        // Store in history
        self.history.push(result.clone());
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }

        if result.success {
            Ok(result)
        } else {
            Err(ToolError::ExecutionFailed(
                result.error.clone().unwrap_or_default(),
            ))
        }
    }

    /// Get tool count
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Get execution history
    pub fn history(&self) -> &[ToolCallResult] {
        &self.history
    }

    /// Clear history
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Enable a tool
    pub fn enable(&mut self, name: &str) -> ToolResult<()> {
        let tool = self
            .tools
            .get_mut(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        tool.definition.enabled = true;
        Ok(())
    }

    /// Disable a tool
    pub fn disable(&mut self, name: &str) -> ToolResult<()> {
        let tool = self
            .tools
            .get_mut(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        tool.definition.enabled = false;
        Ok(())
    }
}

/// Tool chain for sequential execution
#[derive(Debug, Clone)]
pub struct ToolChain {
    /// Chain name
    pub name: String,
    /// Steps in the chain
    steps: Vec<ToolChainStep>,
}

/// A step in a tool chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolChainStep {
    /// Step name
    pub name: String,
    /// Tool to call
    pub tool_name: String,
    /// Argument mappings (param name -> source)
    pub arg_mappings: HashMap<String, ArgSource>,
    /// Output variable name
    pub output_var: String,
}

/// Source for argument values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArgSource {
    /// Literal value
    Literal(JsonValue),
    /// From chain input
    Input(String),
    /// From previous step output
    StepOutput(String),
    /// From previous step output field
    StepField(String, String),
}

impl ToolChain {
    /// Create a new tool chain
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            steps: Vec::new(),
        }
    }

    /// Add a step
    pub fn add_step(&mut self, step: ToolChainStep) {
        self.steps.push(step);
    }

    /// Get step count
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Execute the chain
    pub fn execute(
        &self,
        registry: &mut ToolRegistry,
        inputs: &HashMap<String, JsonValue>,
        caller: &str,
    ) -> ToolResult<HashMap<String, JsonValue>> {
        let mut outputs: HashMap<String, JsonValue> = HashMap::new();

        for (i, step) in self.steps.iter().enumerate() {
            // Build arguments
            let mut args = HashMap::new();
            for (param_name, source) in &step.arg_mappings {
                let value = match source {
                    ArgSource::Literal(v) => v.clone(),
                    ArgSource::Input(name) => inputs
                        .get(name)
                        .cloned()
                        .ok_or_else(|| ToolError::MissingParameter(name.clone()))?,
                    ArgSource::StepOutput(step_name) => {
                        outputs.get(step_name).cloned().ok_or_else(|| {
                            ToolError::InvalidParameters(format!(
                                "Step output '{}' not found",
                                step_name
                            ))
                        })?
                    }
                    ArgSource::StepField(step_name, field) => {
                        let output = outputs.get(step_name).ok_or_else(|| {
                            ToolError::InvalidParameters(format!(
                                "Step output '{}' not found",
                                step_name
                            ))
                        })?;
                        output.get(field).cloned().ok_or_else(|| {
                            ToolError::InvalidParameters(format!(
                                "Field '{}' not found in step '{}' output",
                                field, step_name
                            ))
                        })?
                    }
                };
                args.insert(param_name.clone(), value);
            }

            // Execute step
            let call = ToolCall::new(
                &format!("{}-step-{}", self.name, i),
                &step.tool_name,
                caller,
            )
            .with_args(args);

            let result = registry.execute(&call)?;
            if let Some(value) = result.result {
                outputs.insert(step.output_var.clone(), value);
            }
        }

        Ok(outputs)
    }
}

/// Thread-safe shared tool registry
#[derive(Debug, Clone)]
pub struct SharedToolRegistry {
    inner: Arc<RwLock<ToolRegistry>>,
}

impl Default for SharedToolRegistry {
    fn default() -> Self {
        Self::new(ToolRegistry::new())
    }
}

impl SharedToolRegistry {
    /// Create a new shared registry
    pub fn new(registry: ToolRegistry) -> Self {
        Self {
            inner: Arc::new(RwLock::new(registry)),
        }
    }

    /// Register a tool
    pub fn register(&self, tool: RegisteredTool) -> ToolResult<()> {
        self.inner.write().expect("lock poisoned").register(tool)
    }

    /// Execute a tool call
    pub fn execute(&self, call: &ToolCall) -> ToolResult<ToolCallResult> {
        self.inner.write().expect("lock poisoned").execute(call)
    }

    /// List all tools
    pub fn list(&self) -> Vec<ToolDefinition> {
        self.inner
            .read()
            .expect("lock poisoned")
            .list()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get tool count
    pub fn len(&self) -> usize {
        self.inner.read().expect("lock poisoned").len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.inner.read().expect("lock poisoned").is_empty()
    }

    /// Enable a tool
    pub fn enable(&self, name: &str) -> ToolResult<()> {
        self.inner.write().expect("lock poisoned").enable(name)
    }

    /// Disable a tool
    pub fn disable(&self, name: &str) -> ToolResult<()> {
        self.inner.write().expect("lock poisoned").disable(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameter_type_matches() {
        assert!(ParameterType::String.matches(&JsonValue::String("test".to_string())));
        assert!(!ParameterType::String.matches(&JsonValue::Number(42.into())));

        assert!(ParameterType::Integer.matches(&JsonValue::Number(42.into())));
        assert!(ParameterType::Float.matches(&JsonValue::Number(
            serde_json::Number::from_f64(std::f64::consts::PI).unwrap()
        )));

        assert!(ParameterType::Boolean.matches(&JsonValue::Bool(true)));
        assert!(ParameterType::Any.matches(&JsonValue::Null));
    }

    #[test]
    fn test_parameter_type_display() {
        assert_eq!(ParameterType::String.to_string(), "string");
        assert_eq!(ParameterType::Integer.to_string(), "integer");
        assert_eq!(
            ParameterType::Array(Box::new(ParameterType::String)).to_string(),
            "array<string>"
        );
    }

    #[test]
    fn test_tool_parameter_required() {
        let param = ToolParameter::required("name", ParameterType::String, "User name");

        assert!(param.required);
        assert!(param
            .validate(Some(&JsonValue::String("Alice".to_string())))
            .is_ok());
        assert!(param.validate(None).is_err());
    }

    #[test]
    fn test_tool_parameter_optional() {
        let param = ToolParameter::optional("count", ParameterType::Integer, "Count")
            .with_default(JsonValue::Number(10.into()));

        assert!(!param.required);
        let result = param.validate(None).unwrap();
        assert_eq!(result, JsonValue::Number(10.into()));
    }

    #[test]
    fn test_tool_parameter_enum() {
        let param =
            ToolParameter::required("color", ParameterType::String, "Color").with_enum(vec![
                JsonValue::String("red".to_string()),
                JsonValue::String("green".to_string()),
                JsonValue::String("blue".to_string()),
            ]);

        assert!(param
            .validate(Some(&JsonValue::String("red".to_string())))
            .is_ok());
        assert!(param
            .validate(Some(&JsonValue::String("yellow".to_string())))
            .is_err());
    }

    #[test]
    fn test_tool_definition_creation() {
        let tool = ToolDefinition::new("read_file", "Read a file", ToolCategory::FileSystem)
            .with_parameter(ToolParameter::required(
                "path",
                ParameterType::String,
                "File path",
            ))
            .with_permission("file.read")
            .with_timeout(Duration::from_secs(30));

        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.category, ToolCategory::FileSystem);
        assert_eq!(tool.parameters.len(), 1);
        assert_eq!(tool.permissions.len(), 1);
        assert_eq!(tool.timeout, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_tool_definition_validate_args() {
        let tool = ToolDefinition::new("test", "Test", ToolCategory::Custom)
            .with_parameter(ToolParameter::required(
                "name",
                ParameterType::String,
                "Name",
            ))
            .with_parameter(
                ToolParameter::optional("count", ParameterType::Integer, "Count")
                    .with_default(JsonValue::Number(1.into())),
            );

        let mut args = HashMap::new();
        args.insert("name".to_string(), JsonValue::String("test".to_string()));

        let validated = tool.validate_args(&args).unwrap();
        assert_eq!(
            validated.get("name"),
            Some(&JsonValue::String("test".to_string()))
        );
        assert_eq!(validated.get("count"), Some(&JsonValue::Number(1.into())));
    }

    #[test]
    fn test_tool_call_creation() {
        let call = ToolCall::new("call-1", "read_file", "agent-1")
            .with_arg("path", JsonValue::String("/tmp/test.txt".to_string()));

        assert_eq!(call.id, "call-1");
        assert_eq!(call.tool_name, "read_file");
        assert_eq!(call.caller, "agent-1");
        assert!(call.arguments.contains_key("path"));
    }

    #[test]
    fn test_tool_call_result_success() {
        let result = ToolCallResult::success(
            "call-1",
            JsonValue::String("content".to_string()),
            Duration::from_millis(100),
        );

        assert!(result.success);
        assert!(result.result.is_some());
        assert!(result.error.is_none());
    }

    #[test]
    fn test_tool_call_result_failure() {
        let result = ToolCallResult::failure("call-1", "File not found", Duration::from_millis(50));

        assert!(!result.success);
        assert!(result.result.is_none());
        assert_eq!(result.error, Some("File not found".to_string()));
    }

    #[test]
    fn test_registered_tool_execute() {
        let def = ToolDefinition::new("echo", "Echo back", ToolCategory::Custom).with_parameter(
            ToolParameter::required("message", ParameterType::String, "Message"),
        );

        let tool = RegisteredTool::new(def, |args| {
            let msg = args.get("message").cloned().unwrap_or(JsonValue::Null);
            Ok(msg)
        });

        let mut args = HashMap::new();
        args.insert(
            "message".to_string(),
            JsonValue::String("hello".to_string()),
        );

        let result = tool.execute(&args).unwrap();
        assert_eq!(result, JsonValue::String("hello".to_string()));
    }

    #[test]
    fn test_registered_tool_disabled() {
        let def = ToolDefinition::new("test", "Test", ToolCategory::Custom).disabled();

        let tool = RegisteredTool::new(def, |_| Ok(JsonValue::Null));

        let result = tool.execute(&HashMap::new());
        assert!(matches!(result, Err(ToolError::ToolDisabled(_))));
    }

    #[test]
    fn test_tool_registry_register() {
        let mut registry = ToolRegistry::new();

        let tool = RegisteredTool::new(
            ToolDefinition::new("test", "Test", ToolCategory::Custom),
            |_| Ok(JsonValue::Null),
        );

        registry.register(tool).unwrap();
        assert_eq!(registry.len(), 1);
        assert!(registry.get("test").is_some());
    }

    #[test]
    fn test_tool_registry_duplicate() {
        let mut registry = ToolRegistry::new();

        let tool1 = RegisteredTool::new(
            ToolDefinition::new("test", "Test 1", ToolCategory::Custom),
            |_| Ok(JsonValue::Null),
        );
        let tool2 = RegisteredTool::new(
            ToolDefinition::new("test", "Test 2", ToolCategory::Custom),
            |_| Ok(JsonValue::Null),
        );

        registry.register(tool1).unwrap();
        assert!(registry.register(tool2).is_err());
    }

    #[test]
    fn test_tool_registry_list_by_category() {
        let mut registry = ToolRegistry::new();

        registry
            .register(RegisteredTool::new(
                ToolDefinition::new("read", "Read", ToolCategory::FileSystem),
                |_| Ok(JsonValue::Null),
            ))
            .unwrap();

        registry
            .register(RegisteredTool::new(
                ToolDefinition::new("http_get", "GET", ToolCategory::Network),
                |_| Ok(JsonValue::Null),
            ))
            .unwrap();

        let fs_tools = registry.list_by_category(ToolCategory::FileSystem);
        assert_eq!(fs_tools.len(), 1);
        assert_eq!(fs_tools[0].name, "read");
    }

    #[test]
    fn test_tool_registry_execute() {
        let mut registry = ToolRegistry::new();

        let tool = RegisteredTool::new(
            ToolDefinition::new("add", "Add numbers", ToolCategory::Custom)
                .with_parameter(ToolParameter::required(
                    "a",
                    ParameterType::Integer,
                    "First",
                ))
                .with_parameter(ToolParameter::required(
                    "b",
                    ParameterType::Integer,
                    "Second",
                )),
            |args| {
                let a = args.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
                let b = args.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
                Ok(JsonValue::Number((a + b).into()))
            },
        );

        registry.register(tool).unwrap();

        let call = ToolCall::new("call-1", "add", "test")
            .with_arg("a", JsonValue::Number(3.into()))
            .with_arg("b", JsonValue::Number(5.into()));

        let result = registry.execute(&call).unwrap();
        assert!(result.success);
        assert_eq!(result.result, Some(JsonValue::Number(8.into())));
    }

    #[test]
    fn test_tool_registry_enable_disable() {
        let mut registry = ToolRegistry::new();

        registry
            .register(RegisteredTool::new(
                ToolDefinition::new("test", "Test", ToolCategory::Custom),
                |_| Ok(JsonValue::Null),
            ))
            .unwrap();

        registry.disable("test").unwrap();
        assert!(!registry.get("test").unwrap().definition.enabled);

        registry.enable("test").unwrap();
        assert!(registry.get("test").unwrap().definition.enabled);
    }

    #[test]
    fn test_tool_chain_creation() {
        let mut chain = ToolChain::new("process_file");

        chain.add_step(ToolChainStep {
            name: "read".to_string(),
            tool_name: "read_file".to_string(),
            arg_mappings: {
                let mut m = HashMap::new();
                m.insert(
                    "path".to_string(),
                    ArgSource::Input("file_path".to_string()),
                );
                m
            },
            output_var: "content".to_string(),
        });

        assert_eq!(chain.len(), 1);
        assert_eq!(chain.name, "process_file");
    }

    #[test]
    fn test_shared_registry() {
        let registry = SharedToolRegistry::default();

        registry
            .register(RegisteredTool::new(
                ToolDefinition::new("test", "Test", ToolCategory::Custom),
                |_| Ok(JsonValue::Null),
            ))
            .unwrap();

        assert_eq!(registry.len(), 1);

        let tools = registry.list();
        assert_eq!(tools.len(), 1);
    }

    #[test]
    fn test_tool_error_display() {
        let err = ToolError::NotFound("test_tool".to_string());
        assert!(err.to_string().contains("not found"));

        let err = ToolError::TypeMismatch {
            param: "count".to_string(),
            expected: "integer".to_string(),
            got: "string".to_string(),
        };
        assert!(err.to_string().contains("count"));
        assert!(err.to_string().contains("integer"));
    }

    #[test]
    fn test_tool_category_display() {
        assert_eq!(ToolCategory::FileSystem.to_string(), "filesystem");
        assert_eq!(ToolCategory::Network.to_string(), "network");
        assert_eq!(ToolCategory::Custom.to_string(), "custom");
    }

    #[test]
    fn test_tool_registry_history() {
        let mut registry = ToolRegistry::new();

        registry
            .register(RegisteredTool::new(
                ToolDefinition::new("test", "Test", ToolCategory::Custom),
                |_| Ok(JsonValue::Bool(true)),
            ))
            .unwrap();

        let call = ToolCall::new("call-1", "test", "agent");
        registry.execute(&call).unwrap();

        assert_eq!(registry.history().len(), 1);
        assert!(registry.history()[0].success);

        registry.clear_history();
        assert!(registry.history().is_empty());
    }

    #[test]
    fn test_arg_source_variants() {
        let _literal = ArgSource::Literal(JsonValue::String("test".to_string()));
        let _input = ArgSource::Input("key".to_string());
        let _step_output = ArgSource::StepOutput("step1".to_string());
        let _step_field = ArgSource::StepField("step1".to_string(), "field".to_string());
    }
}
