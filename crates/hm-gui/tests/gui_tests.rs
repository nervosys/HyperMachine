//! Automated GUI tests for HyperMachine
//!
//! Tests the GUI components.
//! Run with: `cargo test -p hm-gui`

mod state_tests {
    fn default_state() -> hm_gui::state::AppState {
        hm_gui::state::AppState::default()
    }

    #[test]
    fn test_app_state_default() {
        let state = default_state();
        assert!(!state.connected);
        assert_eq!(state.backend_url, "http://localhost:8080");
        assert!(state.vms.is_empty());
        assert!(state.selected_vm.is_none());
        assert!(!state.show_create_dialog);
        assert!(state.auto_refresh);
    }

    #[test]
    fn test_vm_counts_empty() {
        let state = default_state();
        let counts = state.vm_counts();
        assert_eq!(counts.total, 0);
        assert_eq!(counts.running, 0);
    }

    #[test]
    fn test_should_refresh_when_disconnected() {
        let state = default_state();
        assert!(!state.should_refresh());
    }
}

mod create_vm_form_tests {
    #[test]
    fn test_create_vm_form_default() {
        let form = hm_gui::state::CreateVmForm::default();
        assert!(form.name.is_empty());
        assert_eq!(form.cpus, 2);
        assert_eq!(form.memory_mb, 2048);
        assert!(!form.cancelled);
    }

    #[test]
    fn test_validate_empty_name() {
        let form = hm_gui::state::CreateVmForm::default();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_success() {
        let mut form = hm_gui::state::CreateVmForm::default();
        form.name = "test-vm".to_string();
        assert!(form.validate().is_ok());
    }

    #[test]
    fn test_form_reset() {
        let mut form = hm_gui::state::CreateVmForm::default();
        form.name = "modified".to_string();
        form.cancelled = true;
        form.reset();
        assert!(form.name.is_empty());
        assert!(!form.cancelled);
    }
}

mod api_tests {
    #[test]
    fn test_vm_state_display() {
        assert_eq!(format!("{}", hm_gui::api::VmStateApi::Running), "Running");
        assert_eq!(format!("{}", hm_gui::api::VmStateApi::Stopped), "Stopped");
        assert_eq!(format!("{}", hm_gui::api::VmStateApi::Paused), "Paused");
    }
}

mod dialog_workflow_tests {
    #[test]
    fn test_create_dialog_flow() {
        let mut state = hm_gui::state::AppState::default();
        assert!(!state.show_create_dialog);
        state.show_create_dialog = true;
        assert!(state.show_create_dialog);
        state.create_form.name = "My VM".to_string();
        assert!(state.create_form.validate().is_ok());
    }

    #[test]
    fn test_cancel_dialog_flow() {
        let mut state = hm_gui::state::AppState::default();
        state.show_create_dialog = true;
        state.create_form.name = "Test".to_string();
        state.create_form.cancelled = true;
        if state.create_form.cancelled {
            state.show_create_dialog = false;
            state.create_form.reset();
        }
        assert!(!state.show_create_dialog);
        assert!(!state.create_form.cancelled);
    }
}

mod theme_tests {
    #[test]
    fn test_app_colors_default() {
        let colors = hm_gui::theme::AppColors::default();
        assert!(colors.primary.r() > 0 || colors.primary.g() > 0 || colors.primary.b() > 0);
    }
}
