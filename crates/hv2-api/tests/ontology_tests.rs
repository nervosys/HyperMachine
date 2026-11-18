//! Integration tests for the Agentic Ontology API endpoints

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hv2_api::rest::create_router;
use tower::ServiceExt;

/// Helper to create a test request
fn test_request(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

// ============================================================================
// Ontology Endpoint Tests
// ============================================================================

#[tokio::test]
async fn test_get_ontology_json_ld() {
    let router = create_router();
    let response = router
        .oneshot(test_request("/agentic/ontology"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let content_type = response
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or(""));
    assert!(
        content_type
            .map(|ct| ct.contains("application/ld+json") || ct.contains("application/json"))
            .unwrap_or(false),
        "Expected JSON-LD or JSON content type, got {:?}",
        content_type
    );

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify JSON-LD structure
    assert!(
        json.get("@context").is_some(),
        "Missing @context in JSON-LD"
    );
    assert!(json.get("system").is_some(), "Missing system info");
    assert!(json.get("capabilities").is_some(), "Missing capabilities");
}

#[tokio::test]
async fn test_get_ontology_openai_format() {
    let router = create_router();
    let response = router
        .oneshot(test_request("/agentic/ontology?format=openai"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // OpenAI format should have tools array
    assert!(
        json.get("tools").is_some(),
        "Missing tools array in OpenAI format"
    );

    let tools = json.get("tools").unwrap().as_array().unwrap();
    assert!(!tools.is_empty(), "Tools array should not be empty");

    // Each tool should have type: "function" and function object
    for tool in tools {
        assert_eq!(tool.get("type").unwrap(), "function");
        assert!(tool.get("function").is_some());

        let func = tool.get("function").unwrap();
        assert!(func.get("name").is_some());
        assert!(func.get("description").is_some());
        assert!(func.get("parameters").is_some());
    }
}

#[tokio::test]
async fn test_get_ontology_anthropic_format() {
    let router = create_router();
    let response = router
        .oneshot(test_request("/agentic/ontology?format=anthropic"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Anthropic format should have tools array
    assert!(
        json.get("tools").is_some(),
        "Missing tools array in Anthropic format"
    );

    let tools = json.get("tools").unwrap().as_array().unwrap();
    assert!(!tools.is_empty(), "Tools array should not be empty");

    // Each tool should have name, description, input_schema
    for tool in tools {
        assert!(tool.get("name").is_some());
        assert!(tool.get("description").is_some());
        assert!(tool.get("input_schema").is_some());
    }
}

#[tokio::test]
async fn test_get_ontology_gemini_format() {
    let router = create_router();
    let response = router
        .oneshot(test_request("/agentic/ontology?format=gemini"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Gemini format should have function_declarations array
    assert!(
        json.get("functionDeclarations").is_some(),
        "Missing functionDeclarations array in Gemini format"
    );

    let declarations = json
        .get("functionDeclarations")
        .unwrap()
        .as_array()
        .unwrap();
    assert!(
        !declarations.is_empty(),
        "Function declarations should not be empty"
    );

    // Each declaration should have name, description, parameters
    for decl in declarations {
        assert!(decl.get("name").is_some());
        assert!(decl.get("description").is_some());
        assert!(decl.get("parameters").is_some());
    }
}

// ============================================================================
// Tool Format Endpoint Tests
// ============================================================================

#[tokio::test]
async fn test_openai_tools_endpoint() {
    let router = create_router();
    let response = router
        .oneshot(test_request("/agentic/tools/openai"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json.get("tools").is_some());

    // Verify tool structure matches OpenAI spec
    let tools = json.get("tools").unwrap().as_array().unwrap();
    for tool in tools {
        let func = tool.get("function").unwrap();
        let params = func.get("parameters").unwrap();

        // Parameters should be JSON Schema format
        assert_eq!(params.get("type").unwrap(), "object");
        assert!(params.get("properties").is_some());
    }
}

#[tokio::test]
async fn test_anthropic_tools_endpoint() {
    let router = create_router();
    let response = router
        .oneshot(test_request("/agentic/tools/anthropic"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json.get("tools").is_some());

    // Verify tool structure matches Anthropic spec
    let tools = json.get("tools").unwrap().as_array().unwrap();
    for tool in tools {
        let schema = tool.get("input_schema").unwrap();

        // Input schema should be JSON Schema format
        assert_eq!(schema.get("type").unwrap(), "object");
        assert!(schema.get("properties").is_some());
    }
}

#[tokio::test]
async fn test_gemini_tools_endpoint() {
    let router = create_router();
    let response = router
        .oneshot(test_request("/agentic/tools/gemini"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json.get("functionDeclarations").is_some());

    // Verify structure matches Gemini spec
    let declarations = json
        .get("functionDeclarations")
        .unwrap()
        .as_array()
        .unwrap();
    for decl in declarations {
        let params = decl.get("parameters").unwrap();
        assert_eq!(params.get("type").unwrap(), "object");
    }
}

// ============================================================================
// AI Plugin Manifest Tests
// ============================================================================

#[tokio::test]
async fn test_ai_plugin_manifest() {
    let router = create_router();
    let response = router
        .oneshot(test_request("/.well-known/ai-plugin.json"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify OpenAI plugin manifest structure
    assert_eq!(json.get("schema_version").unwrap(), "v1");
    assert!(json.get("name_for_human").is_some());
    assert!(json.get("name_for_model").is_some());
    assert!(json.get("description_for_human").is_some());
    assert!(json.get("description_for_model").is_some());
    assert!(json.get("auth").is_some());
    assert!(json.get("api").is_some());

    // API should reference OpenAPI spec
    let api = json.get("api").unwrap();
    assert_eq!(api.get("type").unwrap(), "openapi");
    assert!(api.get("url").is_some());
}

// ============================================================================
// Tool Content Validation Tests
// ============================================================================

#[tokio::test]
async fn test_vm_management_tools_present() {
    let router = create_router();
    let response = router
        .oneshot(test_request("/agentic/tools/openai"))
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let tools = json.get("tools").unwrap().as_array().unwrap();
    let tool_names: Vec<&str> = tools
        .iter()
        .map(|t| {
            t.get("function")
                .unwrap()
                .get("name")
                .unwrap()
                .as_str()
                .unwrap()
        })
        .collect();

    // Verify essential VM management tools are present (MCP tool names).
    assert!(tool_names.contains(&"vm.create"), "Missing vm.create tool");
    assert!(tool_names.contains(&"vm.delete"), "Missing vm.delete tool");
    assert!(tool_names.contains(&"vm.start"), "Missing vm.start tool");
    assert!(tool_names.contains(&"vm.stop"), "Missing vm.stop tool");
    assert!(tool_names.contains(&"vm.list"), "Missing vm.list tool");
}

#[tokio::test]
async fn test_tool_parameters_have_descriptions() {
    let router = create_router();
    let response = router
        .oneshot(test_request("/agentic/tools/openai"))
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let tools = json.get("tools").unwrap().as_array().unwrap();

    let mut missing = Vec::new();
    for tool in tools {
        let func = tool.get("function").unwrap();
        let name = func.get("name").unwrap().as_str().unwrap();
        let params = func.get("parameters").unwrap();
        let properties = params.get("properties").unwrap().as_object().unwrap();

        // Each property should have a description.
        for (prop_name, prop_value) in properties {
            if prop_value.get("description").is_none() {
                missing.push(format!("{name}.{prop_name}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "tool parameters missing descriptions: {missing:?}"
    );
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_invalid_format_parameter() {
    let router = create_router();
    let response = router
        .oneshot(test_request("/agentic/ontology?format=invalid"))
        .await
        .unwrap();

    // Should return default JSON-LD format for unknown format
    assert_eq!(response.status(), StatusCode::OK);

    let content_type = response
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or(""));
    assert!(
        content_type
            .map(|ct| ct.contains("application/ld+json") || ct.contains("application/json"))
            .unwrap_or(false),
        "Should fall back to JSON-LD for invalid format"
    );
}

// ============================================================================
// Cross-Format Consistency Tests
// ============================================================================

#[tokio::test]
async fn test_tool_count_consistency() {
    let router = create_router();

    // Get OpenAI tools
    let openai_response = router
        .clone()
        .oneshot(test_request("/agentic/tools/openai"))
        .await
        .unwrap();
    let openai_body = axum::body::to_bytes(openai_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let openai_json: serde_json::Value = serde_json::from_slice(&openai_body).unwrap();
    let openai_count = openai_json.get("tools").unwrap().as_array().unwrap().len();

    // Get Anthropic tools
    let anthropic_response = router
        .clone()
        .oneshot(test_request("/agentic/tools/anthropic"))
        .await
        .unwrap();
    let anthropic_body = axum::body::to_bytes(anthropic_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let anthropic_json: serde_json::Value = serde_json::from_slice(&anthropic_body).unwrap();
    let anthropic_count = anthropic_json
        .get("tools")
        .unwrap()
        .as_array()
        .unwrap()
        .len();

    // Get Gemini tools
    let gemini_response = router
        .clone()
        .oneshot(test_request("/agentic/tools/gemini"))
        .await
        .unwrap();
    let gemini_body = axum::body::to_bytes(gemini_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let gemini_json: serde_json::Value = serde_json::from_slice(&gemini_body).unwrap();
    let gemini_count = gemini_json
        .get("functionDeclarations")
        .unwrap()
        .as_array()
        .unwrap()
        .len();

    // Get native MCP manifest tools
    let mcp_response = router.oneshot(test_request("/agentic/mcp")).await.unwrap();
    let mcp_body = axum::body::to_bytes(mcp_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let mcp_json: serde_json::Value = serde_json::from_slice(&mcp_body).unwrap();
    let mcp_count = mcp_json.get("tools").unwrap().as_array().unwrap().len();

    // Every agent-facing transport must expose the same tool count.
    assert_eq!(
        openai_count, anthropic_count,
        "OpenAI and Anthropic tool counts differ: {} vs {}",
        openai_count, anthropic_count
    );
    assert_eq!(
        openai_count, gemini_count,
        "OpenAI and Gemini tool counts differ: {} vs {}",
        openai_count, gemini_count
    );
    assert_eq!(
        openai_count, mcp_count,
        "OpenAI and native MCP manifest tool counts differ: {} vs {}",
        openai_count, mcp_count
    );
}

#[tokio::test]
async fn test_mcp_manifest_endpoint_serves_full_surface() {
    let router = create_router();
    let response = router.oneshot(test_request("/agentic/mcp")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let tools = json.get("tools").unwrap().as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .map(|t| t.get("name").unwrap().as_str().unwrap())
        .collect();

    // The native MCP surface must carry the same dotted-name registry tools as
    // the LLM projections — including the lifecycle tools and every category.
    for expected in [
        "vm.create",
        "vm.pause",
        "vm.resume",
        "guest.exec",
        "snapshot.create",
        "network.attach",
        "gpu.register",
        "gpu.attach",
        "gpu.detach",
        "agent.broadcast",
        "system.info",
    ] {
        assert!(
            names.contains(&expected),
            "MCP manifest endpoint missing '{expected}'"
        );
    }

    // Each tool must expose a real JSON Schema input_schema.
    for tool in tools {
        let schema = tool.get("input_schema").unwrap();
        assert_eq!(schema.get("type").unwrap(), "object");
    }
}

// ============================================================================
// Guardrails and the managed API key interface
// ============================================================================

#[tokio::test]
async fn test_served_ontology_carries_guardrails_and_key_interface() {
    let response = create_router()
        .oneshot(test_request("/agentic/ontology"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let ids = |key: &str| -> Vec<String> {
        json[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_str().unwrap().to_string())
            .collect()
    };
    let (operations, capabilities) = (ids("operations"), ids("capabilities"));
    let guardrails = json["guardrails"].as_array().expect("guardrails list");
    assert_eq!(guardrails.len(), 9);
    for id in ["redis-shared-user-revocation", "pre-tombstone-revocations"] {
        let g = guardrails
            .iter()
            .find(|g| g["id"] == id)
            .unwrap_or_else(|| panic!("missing guardrail {id}"));
        assert!(matches!(
            g["severity"].as_str(),
            Some("info" | "warning" | "critical")
        ));
        for field in [
            "title",
            "description",
            "condition",
            "mitigation",
            "reference",
        ] {
            assert!(
                !g[field].as_str().unwrap_or("").is_empty(),
                "{id}.{field} empty"
            );
        }
        for target in g["applies_to"].as_array().unwrap() {
            let target = target.as_str().unwrap();
            assert!(
                operations.iter().chain(&capabilities).any(|i| i == target),
                "{id} applies_to unknown id {target}"
            );
        }
    }
    for op in [
        "create_managed_key",
        "list_managed_keys",
        "revoke_managed_key",
        "list_team_members",
    ] {
        assert!(operations.iter().any(|o| o == op), "missing operation {op}");
    }
    let revoke = json["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == "revoke_managed_key")
        .unwrap();
    assert_eq!(revoke["http_method"], "DELETE");
    assert_eq!(revoke["path"], "/keys/{id}");
}

// ============================================================================
// Declared-operations manifest (read by tools/check-ontology-coverage.py)
// ============================================================================

/// `docs/ontology-operations.json` lists every operation the ontology
/// declares (component, id, method, path), so the CI coverage gate can read
/// it without building Rust. This test keeps it identical to the built
/// ontology. After changing an operation, regenerate it with
/// `UPDATE_ONTOLOGY_OPERATIONS=1 cargo test -p hv2-api --test ontology_tests`.
#[test]
fn ontology_operations_manifest_is_current() {
    use hv2_api::ontology::HyperMachineOntology;

    let onto = HyperMachineOntology::build();
    let mut operations: Vec<serde_json::Value> = onto
        .operations
        .iter()
        .map(|op| {
            serde_json::json!({
                "component": HyperMachineOntology::operation_component(&op.id),
                "id": op.id,
                "method": op.http_method.to_uppercase(),
                "path": op.path,
            })
        })
        .collect();
    operations.sort_by_key(|o| {
        (
            o["component"].as_str().unwrap().to_string(),
            o["path"].as_str().unwrap().to_string(),
            o["method"].as_str().unwrap().to_string(),
            o["id"].as_str().unwrap().to_string(),
        )
    });
    let mut expected =
        serde_json::to_string_pretty(&serde_json::json!({ "operations": operations })).unwrap();
    expected.push('\n');

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/ontology-operations.json");
    if std::env::var_os("UPDATE_ONTOLOGY_OPERATIONS").is_some() {
        std::fs::write(&path, &expected).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| {
            panic!(
                "{}: {e}; regenerate with UPDATE_ONTOLOGY_OPERATIONS=1",
                path.display()
            )
        })
        .replace("\r\n", "\n");
    assert!(
        on_disk == expected,
        "docs/ontology-operations.json is stale; regenerate it with \
         UPDATE_ONTOLOGY_OPERATIONS=1 cargo test -p hv2-api --test ontology_tests"
    );
}

/// The sign-in and team routes the control plane serves are described, with
/// the facts an agent needs to call them.
#[test]
fn control_plane_sign_in_and_sharing_operations_are_described() {
    use hv2_api::ontology::HyperMachineOntology;

    let onto = HyperMachineOntology::build();
    let want = [
        ("sso_login", "GET", "/auth/login"),
        ("sso_callback", "GET", "/auth/callback"),
        ("sso_session", "GET", "/auth/session"),
        ("sso_guest_handoff", "GET", "/auth/guest"),
        ("sso_logout", "POST", "/auth/logout"),
        ("sso_cli_token", "POST", "/auth/cli-token"),
        ("list_template_shares", "GET", "/templates/{id}/shares"),
        (
            "share_template_snapshot",
            "PUT",
            "/templates/{id}/shares/{team}",
        ),
        (
            "unshare_template_snapshot",
            "DELETE",
            "/templates/{id}/shares/{team}",
        ),
    ];
    for (id, method, path) in want {
        let op = onto
            .operations
            .iter()
            .find(|o| o.id == id)
            .unwrap_or_else(|| panic!("missing operation {id}"));
        assert_eq!((op.http_method.as_str(), op.path.as_str()), (method, path));
        assert!(op.description.len() > 150, "{id} needs a full description");
        assert!(!op.responses.is_empty(), "{id} lists no responses");
        assert_eq!(HyperMachineOntology::operation_component(id), "hv2-cluster");
        // Every capability that lists it is a real capability.
        assert!(
            onto.capabilities
                .iter()
                .any(|c| c.operations.iter().any(|o| o == id)),
            "{id} belongs to no capability"
        );
    }
    // Operations that are declared with a body or parameters say so.
    let cli = onto
        .operations
        .iter()
        .find(|o| o.id == "sso_cli_token")
        .unwrap();
    assert!(cli.request_body.is_some());
    let share = onto
        .operations
        .iter()
        .find(|o| o.id == "share_template_snapshot")
        .unwrap();
    assert!(share.request_body.as_ref().unwrap().schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r == "acknowledgeContents"));
    // The guardrails that name them.
    assert!(!onto.guardrails_for("share_template_snapshot").is_empty());
    assert!(!onto.guardrails_for("sso_logout").is_empty());
    assert!(!onto.guardrails_for("sso_guest_handoff").is_empty());
    // A path-less operation would escape the coverage gate.
    for op in &onto.operations {
        assert!(op.path.starts_with('/'), "{} has path {:?}", op.id, op.path);
    }
}

/// The sandbox routes are modelled once for the control plane and once for the
/// node, with the node's ids prefixed `node_`; each operation belongs to one
/// component, and the routes both serve do not collapse into one.
#[test]
fn sandbox_operations_are_described_for_both_components() {
    use hv2_api::ontology::HyperMachineOntology;

    let onto = HyperMachineOntology::build();
    let base = [
        ("create_sandbox", "POST", "/sandboxes"),
        ("create_sandbox_v2", "POST", "/v2/sandboxes"),
        ("list_sandboxes", "GET", "/sandboxes"),
        ("list_sandboxes_v2", "GET", "/v2/sandboxes"),
        ("get_sandbox", "GET", "/sandboxes/{id}"),
        ("delete_sandbox", "DELETE", "/sandboxes/{id}"),
        ("connect_sandbox", "POST", "/sandboxes/{id}/connect"),
        ("connect_sandbox_v2", "POST", "/v2/sandboxes/{id}/connect"),
        ("set_sandbox_timeout", "POST", "/sandboxes/{id}/timeout"),
        ("pause_sandbox", "POST", "/sandboxes/{id}/pause"),
        ("resume_sandbox", "POST", "/sandboxes/{id}/resume"),
        ("standby_sandbox", "POST", "/sandboxes/{id}/standby"),
        ("get_sandbox_standby", "GET", "/sandboxes/{id}/standby"),
        ("resize_sandbox", "POST", "/sandboxes/{id}/resize"),
        ("adopt_sandbox_owner", "POST", "/sandboxes/{id}/owner"),
        ("exec_in_sandbox", "POST", "/sandboxes/{id}/exec"),
        ("list_sandbox_commands", "GET", "/sandboxes/{id}/commands"),
        ("get_sandbox_command", "GET", "/sandboxes/{id}/commands/{n}"),
        ("list_sandbox_processes", "GET", "/sandboxes/{id}/processes"),
    ];
    for (id, method, path) in base {
        for (id, component, path) in [
            (id.to_string(), "hv2-cluster", path.to_string()),
            (
                format!("node_{id}"),
                "hv2-sandboxd",
                path.replace("{id}", "{sandboxID}"),
            ),
        ] {
            let op = onto
                .operations
                .iter()
                .find(|o| o.id == id)
                .unwrap_or_else(|| panic!("missing operation {id}"));
            assert_eq!(
                (op.http_method.as_str(), op.path.as_str()),
                (method, path.as_str())
            );
            assert_eq!(HyperMachineOntology::operation_component(&id), component);
            assert!(op.description.len() > 150, "{id} needs a full description");
            assert!(!op.responses.is_empty(), "{id} lists no responses");
            assert!(
                onto.capabilities.iter().any(|c| c.operations.contains(&id)),
                "{id} belongs to no capability"
            );
        }
    }
    // Only the node serves a reboot.
    assert_eq!(
        HyperMachineOntology::operation_component("node_reboot_sandbox"),
        "hv2-sandboxd"
    );
    assert!(onto.operations.iter().all(|o| o.id != "reboot_sandbox"));
    // The control plane's responses name its own access failures; the node's,
    // the cluster token.
    let status = |id: &str, code: u16| {
        onto.operations
            .iter()
            .find(|o| o.id == id)
            .unwrap()
            .responses
            .iter()
            .any(|r| r.status_code == code)
    };
    assert!(status("pause_sandbox", 403) && status("pause_sandbox", 502));
    assert!(status("node_pause_sandbox", 401) && !status("node_pause_sandbox", 502));
    // A paused sandbox is a 404 for exec on a node, with its own body shape.
    assert!(status("node_exec_in_sandbox", 404));
    // The guardrails that name them.
    assert!(!onto.guardrails_for("node_exec_in_sandbox").is_empty());
    assert!(!onto.guardrails_for("exec_in_sandbox").is_empty());
    assert!(!onto.guardrails_for("get_sandbox_command").is_empty());
    assert!(!onto.guardrails_for("node_connect_sandbox").is_empty());
    // Owner adoption is refused on an open control plane, so it is not named.
    assert!(onto.guardrails_for("adopt_sandbox_owner").is_empty());
}
