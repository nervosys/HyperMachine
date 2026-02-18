//! AI Agent VM Creation Test
//!
//! This test demonstrates how an AI agent would use the semantic automation
//! API to create a VM through the HyperMachine GUI.
//!
//! Run with: `cargo test -p hm-gui --test ai_create_vm_test`

use hm_gui::{
    get_gui_tools, get_openai_tools, AutomationHandle, CommandResult, DialogType, FormType,
    GuiCommand, VmActionType,
};
use std::thread;
use std::time::Duration;

/// Simulates an AI agent creating a VM through the GUI
#[test]
fn test_ai_creates_vm_through_gui() {
    // Step 1: AI agent discovers available tools
    let tools = get_gui_tools();
    println!("\n🤖 AI Agent: Discovering available GUI tools...");
    println!("   Found {} tools:", tools.len());
    for tool in &tools {
        println!("   - {}: {}", tool.name, tool.description);
    }

    // Step 2: Create automation handle (this would be passed to the GUI)
    let (handle, receiver) = AutomationHandle::new();

    // Simulate the GUI processing commands in a background thread
    let gui_thread = thread::spawn(move || {
        // Simulate GUI event loop processing commands
        let mut processed = 0;
        loop {
            if let Some(request) = receiver.try_recv() {
                println!(
                    "   📺 GUI received command: {:?}",
                    std::mem::discriminant(&request.command)
                );

                // Simulate command processing and send success response
                let result = match &request.command {
                    GuiCommand::OpenDialog(DialogType::CreateVm) => CommandResult::success(
                        "gui.dialog.open",
                        Some(serde_json::json!({
                            "dialog": "create_vm",
                            "status": "opened"
                        })),
                    ),
                    GuiCommand::SetFormField(params) => CommandResult::success(
                        "gui.form.set_field",
                        Some(serde_json::json!({
                            "form": params.form,
                            "field": &params.field,
                            "value": &params.value,
                            "status": "set"
                        })),
                    ),
                    GuiCommand::SubmitDialog(DialogType::CreateVm) => CommandResult::success(
                        "gui.dialog.submit",
                        Some(serde_json::json!({
                            "dialog": "create_vm",
                            "status": "submitted",
                            "vm_id": "vm-001",
                            "vm_name": "ai-test-vm"
                        })),
                    ),
                    GuiCommand::GetState => CommandResult::success(
                        "gui.get_state",
                        Some(serde_json::json!({
                            "current_view": "vm_details",
                            "connected": true,
                            "selected_vm": {
                                "id": "vm-001",
                                "name": "ai-test-vm",
                                "state": "Stopped",
                                "cpus": 4,
                                "memory_mb": 8192
                            },
                            "vm_counts": {
                                "total": 1,
                                "running": 0,
                                "stopped": 1
                            }
                        })),
                    ),
                    GuiCommand::VmAction(VmActionType::Start) => CommandResult::success(
                        "gui.vm.action",
                        Some(serde_json::json!({
                            "action": "start",
                            "vm_id": "vm-001",
                            "status": "starting"
                        })),
                    ),
                    _ => CommandResult::success("gui.unknown", None),
                };

                let _ = request.response_tx.send(result);
                processed += 1;

                // Exit after processing expected commands
                if processed >= 7 {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        processed
    });

    // Step 3: AI agent executes the workflow to create a VM
    println!("\n🤖 AI Agent: Creating a new VM through the GUI...\n");

    // 3a: Open the Create VM dialog
    println!("   Step 1: Opening Create VM dialog...");
    let result = handle.open_dialog(DialogType::CreateVm).unwrap();
    assert!(result.success);
    println!("   ✅ Dialog opened: {:?}", result.data);

    // 3b: Fill in the form fields
    println!("\n   Step 2: Setting VM configuration...");

    let result = handle
        .set_field(FormType::CreateVm, "name", "ai-test-vm")
        .unwrap();
    assert!(result.success);
    println!("   ✅ Name set to 'ai-test-vm'");

    let result = handle.set_field(FormType::CreateVm, "cpus", 4).unwrap();
    assert!(result.success);
    println!("   ✅ CPUs set to 4");

    let result = handle
        .set_field(FormType::CreateVm, "memory_mb", 8192)
        .unwrap();
    assert!(result.success);
    println!("   ✅ Memory set to 8192 MB");

    let result = handle
        .set_field(FormType::CreateVm, "network_enabled", true)
        .unwrap();
    assert!(result.success);
    println!("   ✅ Network enabled");

    // 3c: Submit the form
    println!("\n   Step 3: Submitting VM creation...");
    let result = handle
        .execute(GuiCommand::SubmitDialog(DialogType::CreateVm))
        .unwrap();
    assert!(result.success);
    println!("   ✅ VM created: {:?}", result.data);

    // 3d: Verify the VM was created by checking state
    println!("\n   Step 4: Verifying VM creation...");
    let result = handle.execute(GuiCommand::GetState).unwrap();
    assert!(result.success);
    if let Some(data) = &result.data {
        let vm = data.get("selected_vm").unwrap();
        println!("   ✅ VM verified:");
        println!("      - Name: {}", vm.get("name").unwrap());
        println!("      - CPUs: {}", vm.get("cpus").unwrap());
        println!("      - Memory: {} MB", vm.get("memory_mb").unwrap());
        println!("      - State: {}", vm.get("state").unwrap());
    }

    // Wait for GUI thread to finish
    let commands_processed = gui_thread.join().unwrap();
    println!(
        "\n🎉 Success! AI agent created VM through {} GUI commands",
        commands_processed
    );
}

/// Test that AI can get OpenAI-compatible tool definitions
#[test]
fn test_ai_discovers_openai_tools() {
    let tools = get_openai_tools();

    println!("\n🤖 AI Agent: Discovering OpenAI-compatible tools...\n");

    // Verify the tools are properly formatted for OpenAI
    for tool in &tools {
        assert_eq!(tool.get("type").unwrap(), "function");
        let function = tool.get("function").unwrap();
        let name = function.get("name").unwrap().as_str().unwrap();
        let description = function.get("description").unwrap().as_str().unwrap();

        println!("   Tool: {}", name);
        println!("   Description: {}", description);
        println!();
    }

    // Verify essential tools exist
    let tool_names: Vec<&str> = tools
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();

    assert!(tool_names.contains(&"gui.dialog.open"));
    assert!(tool_names.contains(&"gui.form.set_field"));
    assert!(tool_names.contains(&"gui.dialog.submit"));
    assert!(tool_names.contains(&"gui.vm.action"));

    println!("   ✅ All essential tools available for VM creation workflow");
}

/// Test the complete VM creation workflow as JSON commands (like an LLM would send)
#[test]
fn test_ai_vm_creation_via_json() {
    println!("\n🤖 AI Agent: Executing VM creation via JSON commands...\n");

    // These are the JSON commands an LLM would generate
    let commands = [
        r#"{"type":"OpenDialog","params":"create_vm"}"#,
        r#"{"type":"SetFormField","params":{"form":"create_vm","field":"name","value":"my-ai-vm"}}"#,
        r#"{"type":"SetFormField","params":{"form":"create_vm","field":"cpus","value":2}}"#,
        r#"{"type":"SetFormField","params":{"form":"create_vm","field":"memory_mb","value":4096}}"#,
        r#"{"type":"SubmitDialog","params":"create_vm"}"#,
    ];

    // Parse each command to verify they're valid
    for (i, json) in commands.iter().enumerate() {
        let cmd: Result<GuiCommand, _> = serde_json::from_str(json);
        match cmd {
            Ok(parsed) => {
                println!(
                    "   Command {}: ✅ Valid - {:?}",
                    i + 1,
                    std::mem::discriminant(&parsed)
                );
            }
            Err(e) => {
                panic!("   Command {}: ❌ Parse error: {}", i + 1, e);
            }
        }
    }

    println!("\n   ✅ All JSON commands are valid and parseable");
}
