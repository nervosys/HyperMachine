//! A small builder for the operations that the cluster control plane and the
//! node daemon serve alike.
//!
//! Each operation is written once, from the handler code, and built twice:
//! `build(false)` for the control plane (`hv2-cluster`) and `build(true)` for
//! the node (`hv2-sandboxd`), whose ids start `node_`. Where the two differ in
//! more than their access failures, the caller branches on `node` while it
//! writes the text.

use super::{
    Capability, CapabilityCategory, Operation, OperationResponse, Parameter, ParameterLocation,
    RequestBody,
};
use serde_json::{json, Value};

/// A path parameter.
pub(super) fn path_param(name: &str, description: &str, schema: Value) -> Parameter {
    Parameter {
        name: name.to_string(),
        location: ParameterLocation::Path,
        required: true,
        description: description.to_string(),
        schema,
        default: None,
    }
}

/// An optional query parameter.
pub(super) fn query_param(name: &str, description: &str, schema: Value) -> Parameter {
    Parameter {
        name: name.to_string(),
        location: ParameterLocation::Query,
        required: false,
        description: description.to_string(),
        schema,
        default: None,
    }
}

/// A required query parameter.
pub(super) fn required_query(name: &str, description: &str, schema: Value) -> Parameter {
    Parameter {
        required: true,
        ..query_param(name, description, schema)
    }
}

/// A response in the `{"code", "message"}` shape both components use.
pub(super) fn fail(status_code: u16, description: &str) -> OperationResponse {
    OperationResponse {
        status_code,
        description: description.to_string(),
        schema: Some(json!({
            "type": "object",
            "properties": { "code": { "type": "integer" }, "message": { "type": "string" } }
        })),
    }
}

/// A response in the `{"error", "code", "request_id"}` shape `hv2-api` uses.
pub(super) fn api_fail(status_code: u16, description: &str) -> OperationResponse {
    OperationResponse {
        status_code,
        description: description.to_string(),
        schema: Some(json!({
            "type": "object",
            "properties": {
                "error": { "type": "string" },
                "code": { "type": "string" },
                "request_id": { "type": "string", "description": "present when the request carried X-Request-Id" }
            }
        })),
    }
}

/// The two answers the API key middleware gives, when one is configured.
pub(super) fn api_key_failures() -> Vec<OperationResponse> {
    vec![
        api_fail(
            401,
            "an API key is configured and the request carried none (`UNAUTHORIZED`); never returned when no key is configured, which is the default",
        ),
        api_fail(
            403,
            "an API key is configured and either the one sent is not among them (`FORBIDDEN`), or its role may not send this method to this path (`ROLE_DENIED`), or it is bound to a tenant and `X-Tenant-Id` names another (`TENANT_MISMATCH`)",
        ),
    ]
}

/// What the content-type middleware answers a POST, PUT or PATCH that is not
/// `application/json`, even one with no body.
pub(super) fn json_content_type_required() -> OperationResponse {
    api_fail(
        415,
        "the request does not carry `Content-Type: application/json` (`UNSUPPORTED_MEDIA_TYPE`); the middleware checks every POST, PUT and PATCH outside /health and /agentic, whether or not it has a body, while content-type validation is on (the default)",
    )
}

/// A capability of `hv2-api`'s own routes, in `category`.
pub(super) fn api_capability(
    id: &str,
    name: &str,
    description: &str,
    category: CapabilityCategory,
    ids: &[&str],
    prerequisites: &[&str],
    permissions: &[&str],
) -> Capability {
    Capability {
        id: id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        category,
        operations: ids.iter().map(|op| op.to_string()).collect(),
        prerequisites: prerequisites.iter().map(|p| p.to_string()).collect(),
        permissions_required: permissions.iter().map(|p| p.to_string()).collect(),
    }
}

/// A response whose body the framework wrote, or that has none.
pub(super) fn plain(status_code: u16, description: &str) -> OperationResponse {
    OperationResponse {
        status_code,
        description: description.to_string(),
        schema: None,
    }
}

/// A successful response.
pub(super) fn ok(status_code: u16, description: &str, schema: Option<Value>) -> OperationResponse {
    OperationResponse {
        status_code,
        description: description.to_string(),
        schema,
    }
}

/// The refusal the framework gives a body that is not the JSON a route reads.
pub(super) fn body_rejection() -> OperationResponse {
    plain(
        400,
        "the body is not the JSON this route reads: the framework's own rejection (400, 415 \
         or 422, plain text)",
    )
}

/// A capability over the operations `ids`, which get the `node_` prefix when
/// `node` is true. Both components' capabilities are the same list of ids.
pub(super) fn capability(
    id: &str,
    name: &str,
    description: &str,
    ids: &[&str],
    node: bool,
    prerequisites: &[&str],
    permissions: &[&str],
) -> Capability {
    let prefix = if node { "node_" } else { "" };
    Capability {
        id: format!("{prefix}{id}"),
        name: name.to_string(),
        description: description.to_string(),
        category: CapabilityCategory::VirtualMachine,
        operations: ids.iter().map(|op| format!("{prefix}{op}")).collect(),
        prerequisites: prerequisites.iter().map(|p| p.to_string()).collect(),
        permissions_required: permissions.iter().map(|p| p.to_string()).collect(),
    }
}

/// One operation, before it is built for a component.
pub(super) struct Spec {
    id: &'static str,
    name: &'static str,
    method: &'static str,
    path: &'static str,
    description: String,
    parameters: Vec<Parameter>,
    body: Option<RequestBody>,
    responses: Vec<OperationResponse>,
    idempotent: bool,
}

impl Spec {
    /// `path` is the control plane's; `{SB}` stands for the sandbox's ID
    /// parameter, `{id}` there and `{sandboxID}` on the node.
    pub(super) fn new(
        id: &'static str,
        name: &'static str,
        method: &'static str,
        path: &'static str,
    ) -> Self {
        Self {
            id,
            name,
            method,
            path,
            description: String::new(),
            parameters: Vec::new(),
            body: None,
            responses: Vec::new(),
            idempotent: false,
        }
    }

    /// The operation's ID, before a node's prefix.
    pub(super) fn id(&self) -> &'static str {
        self.id
    }

    pub(super) fn about(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub(super) fn with(mut self, parameter: Parameter) -> Self {
        self.parameters.push(parameter);
        self
    }

    /// A JSON request body.
    pub(super) fn takes(mut self, schema: Value, required: bool) -> Self {
        self.body = Some(RequestBody {
            content_type: "application/json".to_string(),
            schema,
            required,
        });
        self
    }

    /// A request body of another type.
    pub(super) fn takes_raw(mut self, content_type: &str, schema: Value, required: bool) -> Self {
        self.body = Some(RequestBody {
            content_type: content_type.to_string(),
            schema,
            required,
        });
        self
    }

    pub(super) fn answers(mut self, response: OperationResponse) -> Self {
        self.responses.push(response);
        self
    }

    pub(super) fn answers_all(mut self, responses: Vec<OperationResponse>) -> Self {
        self.responses.extend(responses);
        self
    }

    /// Calling it again does no more than calling it once.
    pub(super) fn repeatable(mut self) -> Self {
        self.idempotent = true;
        self
    }

    /// The operation for the control plane (`node` false) or the node.
    pub(super) fn build(self, node: bool) -> Operation {
        let id = if node {
            format!("node_{}", self.id)
        } else {
            self.id.to_string()
        };
        let sandbox = if node { "{sandboxID}" } else { "{id}" };
        Operation {
            id,
            name: self.name.to_string(),
            description: self.description,
            http_method: self.method.to_string(),
            path: self.path.replace("{SB}", sandbox),
            parameters: self.parameters,
            request_body: self.body,
            responses: self.responses,
            idempotent: self.idempotent,
            async_operation: false,
            rate_limit: None,
            examples: vec![],
        }
    }
}
