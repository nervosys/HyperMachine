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
    assert!(limiter.check("ip-1").await.is_ok());
    assert!(limiter.check("ip-1").await.is_ok());
    assert!(limiter.check("ip-1").await.is_ok());
    assert!(limiter.check("ip-1").await.is_err()); // Exhausted

    // Second IP should still have full quota
    assert!(limiter.check("ip-2").await.is_ok());
    assert!(limiter.check("ip-2").await.is_ok());
    assert!(limiter.check("ip-2").await.is_ok());
    assert!(limiter.check("ip-2").await.is_err()); // Exhausted

    // Third IP unaffected
    assert!(limiter.check("ip-3").await.is_ok());
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
    };

    assert!(state.api_key.is_none());
    assert_eq!(state.rate_limiter.max_requests, 100); // Default
}
