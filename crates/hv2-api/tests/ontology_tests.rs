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

    // Verify essential VM management tools are present
    assert!(tool_names.contains(&"create_vm"), "Missing create_vm tool");
    assert!(tool_names.contains(&"delete_vm"), "Missing delete_vm tool");
    assert!(tool_names.contains(&"start_vm"), "Missing start_vm tool");
    assert!(tool_names.contains(&"stop_vm"), "Missing stop_vm tool");
    assert!(tool_names.contains(&"list_vms"), "Missing list_vms tool");
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

    for tool in tools {
        let func = tool.get("function").unwrap();
        let params = func.get("parameters").unwrap();
        let properties = params.get("properties").unwrap().as_object().unwrap();

        // Each property should have a description
        for (prop_name, prop_value) in properties {
            assert!(
                prop_value.get("description").is_some(),
                "Parameter '{}' in tool '{}' missing description",
                prop_name,
                func.get("name").unwrap()
            );
        }
    }
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

    // All formats should expose the same number of tools
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
}
