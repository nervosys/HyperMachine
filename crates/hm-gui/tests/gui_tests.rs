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
        let form = hm_gui::state::CreateVmForm {
            name: "test-vm".to_string(),
            ..hm_gui::state::CreateVmForm::default()
        };
        assert!(form.validate().is_ok());
    }

    #[test]
    fn test_form_reset() {
        let mut form = hm_gui::state::CreateVmForm {
            name: "modified".to_string(),
            cancelled: true,
            ..hm_gui::state::CreateVmForm::default()
        };
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
        let mut state = hm_gui::state::AppState {
            show_create_dialog: true,
            ..hm_gui::state::AppState::default()
        };
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

mod agentic_tests {
    use hm_gui::{
        get_anthropic_tools, get_gemini_tools, get_gui_tools, get_openai_tools, AgentCapabilities,
        AutomationHandle, CommandResult, DialogType, FormFieldParams, FormType, GuiCommand,
        NavigateParams, SelectVmParams, SelectionMode, ViewType,
    };

    #[test]
    fn test_automation_handle_creation() {
        let (handle, _receiver) = AutomationHandle::new();
        let _handle2 = handle.clone();
    }

    #[test]
    fn test_gui_command_serialization() {
        let cmd = GuiCommand::Navigate(NavigateParams {
            view: ViewType::VmDetails,
        });
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("Navigate"));
        assert!(json.contains("vm_details"));
    }

    #[test]
    fn test_gui_command_deserialization() {
        let json = r#"{"type":"OpenDialog","params":"create_vm"}"#;
        let cmd: GuiCommand = serde_json::from_str(json).unwrap();
        match cmd {
            GuiCommand::OpenDialog(DialogType::CreateVm) => {}
            _ => panic!("Expected OpenDialog(CreateVm)"),
        }
    }

    #[test]
    fn test_select_vm_params() {
        let params = SelectVmParams {
            identifier: "test-vm".to_string(),
            by: SelectionMode::Name,
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("test-vm"));
        assert!(json.contains("name"));
    }

    #[test]
    fn test_form_field_params() {
        let params = FormFieldParams {
            form: FormType::CreateVm,
            field: "cpus".to_string(),
            value: serde_json::json!(4),
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("create_vm"));
        assert!(json.contains("cpus"));
    }

    #[test]
    fn test_command_result_success() {
        let result = CommandResult::success("test_cmd", Some(serde_json::json!({"key": "value"})));
        assert!(result.success);
        assert!(result.error.is_none());
        assert!(result.data.is_some());
        assert_eq!(result.command, "test_cmd");
    }

    #[test]
    fn test_command_result_error() {
        let result = CommandResult::error("test_cmd", "Something went wrong");
        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result.data.is_none());
    }

    #[test]
    fn test_gui_tools_available() {
        let tools = get_gui_tools();
        assert!(!tools.is_empty());

        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"gui.navigate"));
        assert!(tool_names.contains(&"gui.dialog.open"));
        assert!(tool_names.contains(&"gui.dialog.close"));
        assert!(tool_names.contains(&"gui.vm.select"));
        assert!(tool_names.contains(&"gui.vm.action"));
        assert!(tool_names.contains(&"gui.form.set_field"));
        assert!(tool_names.contains(&"gui.get_state"));
        assert!(tool_names.contains(&"gui.refresh"));
    }

    #[test]
    fn test_openai_tools_format() {
        let tools = get_openai_tools();
        assert!(!tools.is_empty());

        let first = &tools[0];
        assert_eq!(first.get("type").unwrap(), "function");
        assert!(first.get("function").is_some());
        assert!(first["function"].get("name").is_some());
    }

    #[test]
    fn test_anthropic_tools_format() {
        let tools = get_anthropic_tools();
        assert!(!tools.is_empty());

        let first = &tools[0];
        assert!(first.get("name").is_some());
        assert!(first.get("input_schema").is_some());
    }

    #[test]
    fn test_gemini_tools_format() {
        let tools = get_gemini_tools();
        assert!(!tools.is_empty());

        let first = &tools[0];
        assert!(first.get("function_declarations").is_some());
    }

    #[test]
    fn test_agent_capabilities() {
        let caps = AgentCapabilities::build();
        assert!(!caps.gui_tools.is_empty());
        assert!(!caps.examples.is_empty());
        assert!(!caps.description.is_empty());
        assert!(!caps.version.is_empty());
    }
}
