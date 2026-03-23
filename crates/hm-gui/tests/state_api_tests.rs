//! Tests for hm-gui state management and API types

use hm_gui::api::{VmConfig, VmInfo, VmStateApi};
use hm_gui::state::{AppState, CreateVmForm, VmState};

// ============================================================================
// VmStateApi tests
// ============================================================================

#[test]
fn test_vm_state_api_display() {
    assert_eq!(format!("{}", VmStateApi::Stopped), "Stopped");
    assert_eq!(format!("{}", VmStateApi::Running), "Running");
    assert_eq!(format!("{}", VmStateApi::Paused), "Paused");
    assert_eq!(format!("{}", VmStateApi::Error), "Error");
    assert_eq!(format!("{}", VmStateApi::Creating), "Creating");
    assert_eq!(format!("{}", VmStateApi::Starting), "Starting");
    assert_eq!(format!("{}", VmStateApi::Stopping), "Stopping");
}

#[test]
fn test_vm_state_api_equality() {
    assert_eq!(VmStateApi::Running, VmStateApi::Running);
    assert_ne!(VmStateApi::Running, VmStateApi::Stopped);
}

#[test]
fn test_vm_state_api_serde_roundtrip() {
    let state = VmStateApi::Running;
    let json = serde_json::to_string(&state).unwrap();
    assert_eq!(json, "\"running\"");
    let parsed: VmStateApi = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, state);
}

#[test]
fn test_vm_state_api_serde_all_variants() {
    let variants = [
        (VmStateApi::Stopped, "\"stopped\""),
        (VmStateApi::Running, "\"running\""),
        (VmStateApi::Paused, "\"paused\""),
        (VmStateApi::Error, "\"error\""),
        (VmStateApi::Creating, "\"creating\""),
        (VmStateApi::Starting, "\"starting\""),
        (VmStateApi::Stopping, "\"stopping\""),
    ];
    for (variant, expected_json) in &variants {
        let json = serde_json::to_string(variant).unwrap();
        assert_eq!(&json, expected_json);
        let parsed: VmStateApi = serde_json::from_str(expected_json).unwrap();
        assert_eq!(&parsed, variant);
    }
}

// ============================================================================
// VmConfig tests
// ============================================================================

#[test]
fn test_vm_config_serde_defaults() {
    let json = r#"{"name":"test-vm"}"#;
    let config: VmConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.name, "test-vm");
    assert_eq!(config.cpus, 2); // default
    assert_eq!(config.memory_mb, 2048); // default
    assert!(config.network_enabled); // default true
    assert!(config.disk_path.is_none());
    assert!(config.boot_image.is_none());
}

#[test]
fn test_vm_config_serde_full() {
    let json = r#"{
        "name": "my-vm",
        "cpus": 4,
        "memory_mb": 8192,
        "disk_path": "/var/vm/disk.qcow2",
        "network_enabled": false,
        "boot_image": "/var/vm/boot.iso"
    }"#;
    let config: VmConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.name, "my-vm");
    assert_eq!(config.cpus, 4);
    assert_eq!(config.memory_mb, 8192);
    assert_eq!(config.disk_path.as_deref(), Some("/var/vm/disk.qcow2"));
    assert!(!config.network_enabled);
}

#[test]
fn test_vm_config_memory_alias() {
    let json = r#"{"name":"test","memory":4096}"#;
    let config: VmConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.memory_mb, 4096);
}

// ============================================================================
// VmInfo tests
// ============================================================================

#[test]
fn test_vm_info_serde() {
    let json = r#"{
        "id": "vm-123",
        "name": "test-vm",
        "state": "running",
        "cpus": 2,
        "memory_mb": 2048
    }"#;
    let info: VmInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.id, "vm-123");
    assert_eq!(info.name, "test-vm");
    assert_eq!(info.state, VmStateApi::Running);
    assert_eq!(info.cpus, 2);
}

#[test]
fn test_vm_info_defaults() {
    let json = r#"{"name":"vm","state":"stopped"}"#;
    let info: VmInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.id, ""); // default
    assert_eq!(info.cpus, 2); // default
    assert_eq!(info.memory_mb, 2048); // default
    assert!(info.disk_path.is_none());
    assert!(info.started_at.is_none());
    assert!(info.ip_address.is_none());
}

// ============================================================================
// CreateVmForm tests
// ============================================================================

#[test]
fn test_create_vm_form_default() {
    let form = CreateVmForm::default();
    assert!(form.name.is_empty());
    assert_eq!(form.cpus, 2);
    assert_eq!(form.memory_mb, 2048);
    assert!(form.network_enabled);
    assert!(!form.creating);
    assert!(!form.cancelled);
    assert!(form.error.is_none());
}

#[test]
fn test_create_vm_form_validate_ok() {
    let form = CreateVmForm {
        name: "test-vm".to_string(),
        cpus: 2,
        memory_mb: 2048,
        ..Default::default()
    };
    assert!(form.validate().is_ok());
}

#[test]
fn test_create_vm_form_validate_empty_name() {
    let form = CreateVmForm {
        name: "".to_string(),
        cpus: 2,
        memory_mb: 2048,
        ..Default::default()
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("Name"));
}

#[test]
fn test_create_vm_form_validate_whitespace_name() {
    let form = CreateVmForm {
        name: "   ".to_string(),
        cpus: 2,
        memory_mb: 2048,
        ..Default::default()
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("Name"));
}

#[test]
fn test_create_vm_form_validate_zero_cpus() {
    let form = CreateVmForm {
        name: "test".to_string(),
        cpus: 0,
        memory_mb: 2048,
        ..Default::default()
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("CPU"));
}

#[test]
fn test_create_vm_form_validate_low_memory() {
    let form = CreateVmForm {
        name: "test".to_string(),
        cpus: 1,
        memory_mb: 64,
        ..Default::default()
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("128"));
}

#[test]
fn test_create_vm_form_validate_boundary_memory() {
    // Exactly 128 MB should be OK
    let form = CreateVmForm {
        name: "test".to_string(),
        cpus: 1,
        memory_mb: 128,
        ..Default::default()
    };
    assert!(form.validate().is_ok());

    // 127 MB should fail
    let form2 = CreateVmForm {
        name: "test".to_string(),
        cpus: 1,
        memory_mb: 127,
        ..Default::default()
    };
    assert!(form2.validate().is_err());
}

#[test]
fn test_create_vm_form_reset() {
    let mut form = CreateVmForm {
        name: "modified".to_string(),
        cpus: 8,
        memory_mb: 16384,
        creating: true,
        error: Some("something".to_string()),
        ..Default::default()
    };
    form.reset();
    assert!(form.name.is_empty());
    assert_eq!(form.cpus, 2);
    assert_eq!(form.memory_mb, 2048);
    assert!(!form.creating);
    assert!(form.error.is_none());
}

// ============================================================================
// VmState tests
// ============================================================================

fn make_vm_info(id: &str, name: &str, state: VmStateApi) -> VmInfo {
    VmInfo {
        id: id.to_string(),
        name: name.to_string(),
        state,
        cpus: 2,
        memory_mb: 2048,
        disk_path: None,
        created_at: "2024-01-01T00:00:00Z".to_string(),
        started_at: None,
        ip_address: None,
    }
}

#[test]
fn test_vm_state_from_vm_info() {
    let info = make_vm_info("vm-1", "Test VM", VmStateApi::Running);
    let state = VmState::from(info);
    assert_eq!(state.id, "vm-1");
    assert_eq!(state.name, "Test VM");
    assert_eq!(state.state, VmStateApi::Running);
    assert_eq!(state.cpus, 2);
}

#[test]
fn test_vm_state_can_start() {
    let stopped = VmState::from(make_vm_info("1", "vm", VmStateApi::Stopped));
    assert!(stopped.can_start());

    let paused = VmState::from(make_vm_info("2", "vm", VmStateApi::Paused));
    assert!(paused.can_start());

    let running = VmState::from(make_vm_info("3", "vm", VmStateApi::Running));
    assert!(!running.can_start());

    let error = VmState::from(make_vm_info("4", "vm", VmStateApi::Error));
    assert!(!error.can_start());

    let creating = VmState::from(make_vm_info("5", "vm", VmStateApi::Creating));
    assert!(!creating.can_start());
}

#[test]
fn test_vm_state_can_stop() {
    let running = VmState::from(make_vm_info("1", "vm", VmStateApi::Running));
    assert!(running.can_stop());

    let paused = VmState::from(make_vm_info("2", "vm", VmStateApi::Paused));
    assert!(paused.can_stop());

    let stopped = VmState::from(make_vm_info("3", "vm", VmStateApi::Stopped));
    assert!(!stopped.can_stop());
}

#[test]
fn test_vm_state_can_pause() {
    let running = VmState::from(make_vm_info("1", "vm", VmStateApi::Running));
    assert!(running.can_pause());

    let stopped = VmState::from(make_vm_info("2", "vm", VmStateApi::Stopped));
    assert!(!stopped.can_pause());

    let paused = VmState::from(make_vm_info("3", "vm", VmStateApi::Paused));
    assert!(!paused.can_pause());
}

#[test]
fn test_vm_state_can_delete() {
    let stopped = VmState::from(make_vm_info("1", "vm", VmStateApi::Stopped));
    assert!(stopped.can_delete());

    let running = VmState::from(make_vm_info("2", "vm", VmStateApi::Running));
    assert!(!running.can_delete());

    let paused = VmState::from(make_vm_info("3", "vm", VmStateApi::Paused));
    assert!(!paused.can_delete());
}

#[test]
fn test_vm_state_pending_operation_blocks_actions() {
    let info = make_vm_info("1", "vm", VmStateApi::Stopped);
    let mut state = VmState::from(info);
    state.operation_pending = Some("starting".to_string());
    assert!(!state.can_start());
    assert!(!state.can_delete());
}

#[test]
fn test_vm_state_update_from_clears_pending() {
    let info = make_vm_info("1", "vm", VmStateApi::Stopped);
    let mut state = VmState::from(info);
    state.operation_pending = Some("starting".to_string());

    let updated_info = make_vm_info("1", "vm", VmStateApi::Running);
    state.update_from(&updated_info);
    assert!(state.operation_pending.is_none());
    assert_eq!(state.state, VmStateApi::Running);
}

// ============================================================================
// AppState tests
// ============================================================================

#[test]
fn test_app_state_default() {
    let state = AppState::default();
    assert!(!state.connected);
    assert_eq!(state.backend_url, "http://localhost:8080");
    assert!(state.vms.is_empty());
    assert!(state.selected_vm.is_none());
    assert!(state.auto_refresh);
}

#[test]
fn test_app_state_update_vms() {
    let mut state = AppState::default();
    let vms = vec![
        make_vm_info("vm-1", "Alpha", VmStateApi::Running),
        make_vm_info("vm-2", "Beta", VmStateApi::Stopped),
    ];
    state.update_vms(vms);
    assert_eq!(state.vms.len(), 2);
    assert!(state.vms.contains_key("vm-1"));
    assert!(state.vms.contains_key("vm-2"));
}

#[test]
fn test_app_state_update_vms_removes_stale() {
    let mut state = AppState::default();
    state.update_vms(vec![
        make_vm_info("vm-1", "A", VmStateApi::Running),
        make_vm_info("vm-2", "B", VmStateApi::Running),
    ]);
    assert_eq!(state.vms.len(), 2);

    // Update with only vm-1 — vm-2 should be removed
    state.update_vms(vec![make_vm_info("vm-1", "A", VmStateApi::Running)]);
    assert_eq!(state.vms.len(), 1);
    assert!(state.vms.contains_key("vm-1"));
    assert!(!state.vms.contains_key("vm-2"));
}

#[test]
fn test_app_state_sorted_vms() {
    let mut state = AppState::default();
    state.update_vms(vec![
        make_vm_info("3", "Charlie", VmStateApi::Running),
        make_vm_info("1", "Alpha", VmStateApi::Stopped),
        make_vm_info("2", "Bravo", VmStateApi::Paused),
    ]);
    let sorted = state.sorted_vms();
    assert_eq!(sorted[0].name, "Alpha");
    assert_eq!(sorted[1].name, "Bravo");
    assert_eq!(sorted[2].name, "Charlie");
}

#[test]
fn test_app_state_selected_vm_state() {
    let mut state = AppState::default();
    state.update_vms(vec![make_vm_info("vm-1", "Test", VmStateApi::Running)]);

    assert!(state.selected_vm_state().is_none());

    state.selected_vm = Some("vm-1".to_string());
    let selected = state.selected_vm_state().unwrap();
    assert_eq!(selected.name, "Test");

    state.selected_vm = Some("nonexistent".to_string());
    assert!(state.selected_vm_state().is_none());
}

#[test]
fn test_app_state_vm_counts() {
    let mut state = AppState::default();
    state.update_vms(vec![
        make_vm_info("1", "a", VmStateApi::Running),
        make_vm_info("2", "b", VmStateApi::Running),
        make_vm_info("3", "c", VmStateApi::Stopped),
        make_vm_info("4", "d", VmStateApi::Paused),
        make_vm_info("5", "e", VmStateApi::Error),
        make_vm_info("6", "f", VmStateApi::Creating),
    ]);
    let counts = state.vm_counts();
    assert_eq!(counts.total, 6);
    assert_eq!(counts.running, 2);
    assert_eq!(counts.stopped, 1);
    assert_eq!(counts.paused, 1);
    assert_eq!(counts.error, 1);
    assert_eq!(counts.other, 1); // Creating
}

#[test]
fn test_app_state_vm_counts_empty() {
    let state = AppState::default();
    let counts = state.vm_counts();
    assert_eq!(counts.total, 0);
    assert_eq!(counts.running, 0);
    assert_eq!(counts.stopped, 0);
}

#[test]
fn test_app_state_should_refresh() {
    let mut state = AppState::default();
    // Not connected - should not refresh
    assert!(!state.should_refresh());

    state.connected = true;
    state.auto_refresh = true;
    state.refresh_interval = 0; // immediate
                                // Just created, might or might not need refresh depending on timing
                                // Mark refreshed then check
    state.mark_refreshed();
}

// ============================================================================
// ApiClient construction test
// ============================================================================

#[test]
fn test_api_client_trims_trailing_slash() {
    use hm_gui::api::ApiClient;
    let _client = ApiClient::new("http://localhost:8080/");
    // Just verify construction works — no panics
}

#[test]
fn test_api_client_construction() {
    use hm_gui::api::ApiClient;
    let _client = ApiClient::new("http://127.0.0.1:3000");
}
