//! MCP HTTP Server Integration Tests
//!
//! Tests the full HTTP request/response cycle for the MCP server endpoints.

use hm_cli::mcp_server::{McpServerState, RateLimiter};
use hm_cli::vm_manager::VmManager;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

// =============================================================================
// Unit Tests for VmManager (these don't need HTTP)
// =============================================================================

#[tokio::test]
async fn test_vm_lifecycle_create_start_stop_delete() {
    let vm_manager = Arc::new(VmManager::new_in_memory().expect("Failed to create VM manager"));

    let state = Arc::new(McpServerState {
        vm_manager: vm_manager.clone(),
        sessions: RwLock::new(HashMap::new()),
        api_key: None,
        rate_limiter: RateLimiter::new(100, Duration::from_secs(60)),
        trusted_proxies: Vec::new(),
    });

    // Create VM
    let result = state
        .vm_manager
        .create_vm("lifecycle-vm", 2, 4, false, false)
        .await;
    assert!(result.is_ok(), "Should create VM");

    // Verify it's in Created state
    let vm = state.vm_manager.get_vm("lifecycle-vm").await.unwrap();
    assert_eq!(format!("{}", vm.state), "Created");

    // Start VM. Starting actually runs the VM via the hypervisor backend, so
    // skip the run/stop portion when no backend is available (e.g. CI or WSL2
    // without /dev/kvm access). create/delete do not need a backend and are
    // exercised by other tests.
    if state.vm_manager.start_vm("lifecycle-vm").await.is_err() {
        eprintln!("skipping: hypervisor backend unavailable (VM start failed)");
        return;
    }

    // Verify it's running
    let vm = state.vm_manager.get_vm("lifecycle-vm").await.unwrap();
    assert_eq!(format!("{}", vm.state), "Running");

    // Stop VM
    let result = state.vm_manager.stop_vm("lifecycle-vm").await;
    assert!(result.is_ok(), "Should stop VM");

    // Verify it's stopped
    let vm = state.vm_manager.get_vm("lifecycle-vm").await.unwrap();
    assert_eq!(format!("{}", vm.state), "Stopped");

    // Delete VM
    let result = state.vm_manager.delete_vm("lifecycle-vm").await;
    assert!(result.is_ok(), "Should delete VM");

    // Verify it's gone
    let result = state.vm_manager.get_vm("lifecycle-vm").await;
    assert!(result.is_err(), "VM should not exist");
}

#[tokio::test]
async fn test_create_duplicate_vm_fails() {
    let vm_manager = Arc::new(VmManager::new_in_memory().expect("Failed to create VM manager"));

    // Create first VM
    let result = vm_manager
        .create_vm("duplicate-test", 2, 4, false, false)
        .await;
    assert!(result.is_ok());

    // Try to create duplicate
    let result = vm_manager
        .create_vm("duplicate-test", 2, 4, false, false)
        .await;
    assert!(result.is_err(), "Should reject duplicate VM name");
}

#[tokio::test]
async fn test_start_already_running_vm_fails() {
    let vm_manager = Arc::new(VmManager::new_in_memory().expect("Failed to create VM manager"));

    // Create and start VM
    vm_manager
        .create_vm("running-vm", 2, 4, false, false)
        .await
        .unwrap();
    // Needs a hypervisor backend to actually start; skip if unavailable.
    if vm_manager.start_vm("running-vm").await.is_err() {
        eprintln!("skipping: hypervisor backend unavailable (VM start failed)");
        return;
    }

    // Try to start again
    let result = vm_manager.start_vm("running-vm").await;
    assert!(result.is_err(), "Should reject starting already running VM");
}

#[tokio::test]
async fn test_stop_vm_changes_state_to_stopped() {
    let vm_manager = Arc::new(VmManager::new_in_memory().expect("Failed to create VM manager"));

    // Create VM (starts in Created state)
    vm_manager
        .create_vm("stop-test-vm", 2, 4, false, false)
        .await
        .unwrap();

    // Stop it (idempotent - sets state to Stopped)
    let result = vm_manager.stop_vm("stop-test-vm").await;
    assert!(result.is_ok(), "Stop should succeed");

    // Verify state is now Stopped
    let vm = vm_manager.get_vm("stop-test-vm").await.unwrap();
    assert_eq!(format!("{}", vm.state), "Stopped");
}

#[tokio::test]
async fn test_get_nonexistent_vm_fails() {
    let vm_manager = Arc::new(VmManager::new_in_memory().expect("Failed to create VM manager"));

    let result = vm_manager.get_vm("nonexistent").await;
    assert!(result.is_err(), "Should fail for nonexistent VM");
}

#[tokio::test]
async fn test_delete_nonexistent_vm_succeeds_idempotent() {
    let vm_manager = Arc::new(VmManager::new_in_memory().expect("Failed to create VM manager"));

    // Delete is idempotent - succeeds even for non-existent VMs
    let result = vm_manager.delete_vm("nonexistent").await;
    assert!(result.is_ok(), "Delete should be idempotent");
}

#[tokio::test]
async fn test_list_vms_empty() {
    let vm_manager = Arc::new(VmManager::new_in_memory().expect("Failed to create VM manager"));

    let vms = vm_manager.list_vms().await;
    assert!(vms.is_empty(), "Should start with no VMs");
}

#[tokio::test]
async fn test_list_vms_with_vms() {
    let vm_manager = Arc::new(VmManager::new_in_memory().expect("Failed to create VM manager"));

    // Create some VMs
    vm_manager
        .create_vm("vm1", 2, 4, false, false)
        .await
        .unwrap();
    vm_manager.create_vm("vm2", 4, 8, true, true).await.unwrap();

    let vms = vm_manager.list_vms().await;
    assert_eq!(vms.len(), 2, "Should have 2 VMs");

    let names: Vec<&str> = vms.iter().map(|v| v.name.as_str()).collect();
    assert!(names.contains(&"vm1"));
    assert!(names.contains(&"vm2"));
}

#[tokio::test]
async fn test_vm_creation_with_all_options() {
    let vm_manager = Arc::new(VmManager::new_in_memory().expect("Failed to create VM manager"));

    let result = vm_manager
        .create_vm("full-options", 16, 64, true, true)
        .await;
    assert!(result.is_ok());

    let vm = result.unwrap();
    assert_eq!(vm.name, "full-options");
    assert_eq!(vm.cpu_cores, 16);
    assert_eq!(vm.memory_gb, 64);
    assert!(vm.gpu_enabled);
    assert!(vm.network_enabled);
}

// =============================================================================
// Rate Limiter Tests (extended)
// =============================================================================

#[tokio::test]
async fn test_rate_limiter_multiple_ips_isolated() {
    let limiter = RateLimiter::new(3, Duration::from_secs(60));

    // First IP uses 3 requests
    assert!(limiter.check("ip-1").await.is_some());
    assert!(limiter.check("ip-1").await.is_some());
    assert!(limiter.check("ip-1").await.is_some());
    assert!(limiter.check("ip-1").await.is_none()); // Exhausted

    // Second IP should still have full quota
    assert!(limiter.check("ip-2").await.is_some());
    assert!(limiter.check("ip-2").await.is_some());
    assert!(limiter.check("ip-2").await.is_some());
    assert!(limiter.check("ip-2").await.is_none()); // Exhausted

    // Third IP unaffected
    assert!(limiter.check("ip-3").await.is_some());
}

// =============================================================================
// MCP State Tests
// =============================================================================

#[tokio::test]
async fn test_mcp_state_creation() {
    let vm_manager = Arc::new(VmManager::new_in_memory().expect("Failed to create VM manager"));

    let state = McpServerState {
        vm_manager,
        sessions: RwLock::new(HashMap::new()),
        api_key: Some("test-key".to_string()),
        rate_limiter: RateLimiter::new(50, Duration::from_secs(30)),
        trusted_proxies: Vec::new(),
    };

    assert!(state.api_key.is_some());
    assert_eq!(state.api_key.as_ref().unwrap(), "test-key");
    assert_eq!(state.rate_limiter.max_requests, 50);
}

#[tokio::test]
async fn test_mcp_state_without_auth() {
    let vm_manager = Arc::new(VmManager::new_in_memory().expect("Failed to create VM manager"));

    let state = McpServerState {
        vm_manager,
        sessions: RwLock::new(HashMap::new()),
        api_key: None,
        rate_limiter: RateLimiter::default(),
        trusted_proxies: Vec::new(),
    };

    assert!(state.api_key.is_none());
    assert_eq!(state.rate_limiter.max_requests, 100); // Default
}

// =============================================================================
// `hm serve` exposure, authentication and rate-limit keying
// =============================================================================

use hm_cli::mcp_server::{self, ServeConfig};
use std::net::{IpAddr, SocketAddr};

/// A key built at runtime, so no secret literal sits in the source.
fn test_key() -> String {
    "k".repeat(32)
}

/// Start the real router on an ephemeral loopback port. The socket peer of
/// every test request is therefore 127.0.0.1.
async fn spawn_server(
    api_key: Option<String>,
    trusted: Vec<IpAddr>,
    max_requests: u64,
) -> SocketAddr {
    let state = Arc::new(McpServerState {
        vm_manager: Arc::new(VmManager::new_in_memory().expect("Failed to create VM manager")),
        sessions: RwLock::new(HashMap::new()),
        api_key,
        rate_limiter: RateLimiter::new(max_requests, Duration::from_secs(60)),
        trusted_proxies: trusted,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let app = mcp_server::build_router(state).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

/// GET `path` and return the status code. No proxy from the environment.
async fn get_status(addr: SocketAddr, path: &str, headers: &[(&str, &str)]) -> u16 {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client");
    let mut req = client.get(format!("http://{addr}{path}"));
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    req.send().await.expect("request").status().as_u16()
}

#[tokio::test]
async fn serve_refuses_non_loopback_bind_without_key() {
    let cfg = ServeConfig {
        addr: "0.0.0.0:0".parse().unwrap(),
        api_key: None,
        allow_unauthenticated: false,
        trusted_proxies: Vec::new(),
    };
    // A server that starts would serve forever, so bound the wait.
    let outcome =
        tokio::time::timeout(Duration::from_secs(5), mcp_server::start_mcp_server(cfg)).await;
    match outcome {
        Ok(Err(e)) => assert!(
            e.to_string().contains("HM_API_KEY"),
            "unexpected error: {e}"
        ),
        Ok(Ok(())) => panic!("server returned Ok without a key on a non-loopback bind"),
        Err(_) => panic!("server started on 0.0.0.0 without a key"),
    }
}

#[test]
fn exposure_check_allows_loopback_key_or_explicit_opt_in() {
    let any: IpAddr = "0.0.0.0".parse().unwrap();
    let loopback: IpAddr = "127.0.0.1".parse().unwrap();
    let key = test_key();
    assert!(mcp_server::check_exposure(loopback, None, false).is_ok());
    assert!(mcp_server::check_exposure(any, Some(&key), false).is_ok());
    assert!(mcp_server::check_exposure(any, None, true).is_ok());
    // An empty key is no key.
    assert!(mcp_server::check_exposure(any, Some(""), false).is_err());
    assert!(mcp_server::check_exposure(any, None, false).is_err());
}

#[tokio::test]
async fn key_guards_reads_but_not_health() {
    let key = test_key();
    let addr = spawn_server(Some(key.clone()), Vec::new(), 1000).await;

    assert_eq!(get_status(addr, "/health", &[]).await, 200);
    assert_eq!(get_status(addr, "/vms", &[]).await, 401);
    assert_eq!(get_status(addr, "/mcp/tools", &[]).await, 401);
    assert_eq!(get_status(addr, "/agentic/capabilities", &[]).await, 401);

    let wrong = "Bearer not-the-key";
    assert_eq!(
        get_status(addr, "/vms", &[("Authorization", wrong)]).await,
        401
    );

    let right = format!("Bearer {key}");
    assert_eq!(
        get_status(addr, "/vms", &[("Authorization", right.as_str())]).await,
        200
    );
}

#[tokio::test]
async fn spoofed_forwarded_for_does_not_evade_rate_limit() {
    // No trusted proxies: the header must be ignored and the socket peer counted.
    let addr = spawn_server(None, Vec::new(), 3).await;

    for i in 0..3 {
        let xff = format!("198.51.100.{}", i + 1);
        assert_eq!(
            get_status(addr, "/health", &[("X-Forwarded-For", xff.as_str())]).await,
            200
        );
    }
    // A fresh spoofed address on the fourth request is still the same client.
    assert_eq!(
        get_status(addr, "/health", &[("X-Forwarded-For", "203.0.113.99")]).await,
        429
    );
}

#[tokio::test]
async fn trusted_proxy_forwarded_for_is_honoured() {
    // The test client connects from 127.0.0.1, so 127.0.0.1 is the proxy.
    let proxy: IpAddr = "127.0.0.1".parse().unwrap();
    let addr = spawn_server(None, vec![proxy], 2).await;

    // The proxy appends the real client (203.0.113.7). The client controls only
    // the entries to its left, so varying those must not change the bucket.
    for i in 0..2 {
        let xff = format!("10.9.9.{}, 203.0.113.7", i + 1);
        assert_eq!(
            get_status(addr, "/health", &[("X-Forwarded-For", xff.as_str())]).await,
            200
        );
    }
    assert_eq!(
        get_status(
            addr,
            "/health",
            &[("X-Forwarded-For", "10.9.9.9, 203.0.113.7")]
        )
        .await,
        429
    );

    // A different real client behind the same proxy has its own quota.
    assert_eq!(
        get_status(
            addr,
            "/health",
            &[("X-Forwarded-For", "10.9.9.9, 198.51.100.9")]
        )
        .await,
        200
    );
}
