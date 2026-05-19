//! Headless CI tests for the agentic automation interface.
//!
//! These tests exercise the command dispatch pipeline (AutomationHandle →
//! channel → AutomationReceiver) entirely in-process without any GUI context,
//! making them safe for headless CI environments.

use hm_gui::agentic::{
    AutomationError, AutomationHandle, CommandResult, DialogType, GuiCommand,
    NavigateParams, SelectVmParams, SelectionMode, ViewType, VmActionType,
};
use serde_json::json;
use std::thread;

// ─── Channel Lifecycle ─────────────────────────────────────────────

#[test]
fn automation_handle_creates_connected_pair() {
    let (handle, receiver) = AutomationHandle::new();

    // Send a command through the channel
    let waiter = handle.execute_async(GuiCommand::GetState).unwrap();

    // Receiver should see it
    let req = receiver.try_recv();
    assert!(req.is_some());
    let req = req.unwrap();
    assert!(matches!(req.command, GuiCommand::GetState));

    // Reply so the waiter can resolve
    req.response_tx
        .send(CommandResult::success("get_state", None))
        .unwrap();

    let result = waiter.wait().unwrap();
    assert!(result.success);
}

#[test]
fn automation_handle_detects_closed_channel() {
    let (handle, receiver) = AutomationHandle::new();
    drop(receiver); // Close the receiving end

    let result = handle.execute_async(GuiCommand::Refresh);
    assert!(result.is_err());
    match result {
        Err(AutomationError::ChannelClosed) => {} // expected
        other => panic!("Expected ChannelClosed, got: {:?}", other.err()),
    }
}

#[test]
fn automation_receiver_drain_collects_all_pending() {
    let (handle, receiver) = AutomationHandle::new();

    // Queue 3 commands without consuming
    for _ in 0..3 {
        let _ = handle.execute_async(GuiCommand::Refresh).unwrap();
    }

    let drained = receiver.drain();
    assert_eq!(drained.len(), 3);
    for req in &drained {
        assert!(matches!(req.command, GuiCommand::Refresh));
    }
}

#[test]
fn automation_receiver_try_recv_returns_none_when_empty() {
    let (_handle, receiver) = AutomationHandle::new();
    assert!(receiver.try_recv().is_none());
}

// ─── Command Serialization (JSON round-trip) ───────────────────────

#[test]
fn gui_command_navigate_serde_roundtrip() {
    let cmd = GuiCommand::Navigate(NavigateParams {
        view: ViewType::Settings,
    });
    let json = serde_json::to_string(&cmd).unwrap();
    let parsed: GuiCommand = serde_json::from_str(&json).unwrap();

    if let GuiCommand::Navigate(params) = parsed {
        assert!(matches!(params.view, ViewType::Settings));
    } else {
        panic!("Expected Navigate variant");
    }
}

#[test]
fn gui_command_all_variants_serialize() {
    let commands = vec![
        GuiCommand::Navigate(NavigateParams {
            view: ViewType::Welcome,
        }),
        GuiCommand::OpenDialog(DialogType::CreateVm),
        GuiCommand::CloseDialog(DialogType::Settings),
        GuiCommand::SubmitDialog(DialogType::About),
        GuiCommand::SelectVm(SelectVmParams {
            identifier: "vm-1".into(),
            by: SelectionMode::Id,
        }),
        GuiCommand::DeselectVm,
        GuiCommand::VmAction(VmActionType::Start),
        GuiCommand::Refresh,
        GuiCommand::GetState,
        GuiCommand::GetAvailableActions,
    ];

    for cmd in commands {
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(!json.is_empty());
        let _parsed: GuiCommand = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn execute_json_parses_and_dispatches() {
    let (handle, receiver) = AutomationHandle::new();

    let json_cmd = serde_json::to_string(&GuiCommand::Refresh).unwrap();

    // Spawn a thread to respond since execute() is blocking
    let t = thread::spawn(move || {
        let req = loop {
            if let Some(r) = receiver.try_recv() {
                break r;
            }
            thread::yield_now();
        };
        req.response_tx
            .send(CommandResult::success("refresh", None))
            .unwrap();
    });

    let result = handle.execute_json(&json_cmd).unwrap();
    assert!(result.success);
    t.join().unwrap();
}

#[test]
fn execute_json_rejects_invalid_json() {
    let (handle, _receiver) = AutomationHandle::new();
    let result = handle.execute_json("not valid json{{{");
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        AutomationError::InvalidCommand(_)
    ));
}

// ─── Convenience Methods ───────────────────────────────────────────

#[test]
fn navigate_sends_correct_command() {
    let (handle, receiver) = AutomationHandle::new();

    let t = thread::spawn(move || {
        let req = loop {
            if let Some(r) = receiver.try_recv() {
                break r;
            }
            thread::yield_now();
        };
        assert!(matches!(req.command, GuiCommand::Navigate(_)));
        req.response_tx
            .send(CommandResult::success("navigate", None))
            .unwrap();
    });

    let result = handle.navigate(ViewType::VmDetails).unwrap();
    assert!(result.success);
    t.join().unwrap();
}

#[test]
fn open_close_dialog_sends_correct_commands() {
    let (handle, receiver) = AutomationHandle::new();

    let t = thread::spawn(move || {
        // Respond to each command as it arrives to avoid deadlock
        // (handle methods block waiting for response)
        let req1 = loop {
            if let Some(r) = receiver.try_recv() {
                break r;
            }
            thread::yield_now();
        };
        assert!(matches!(req1.command, GuiCommand::OpenDialog(_)));
        req1.response_tx
            .send(CommandResult::success("dialog", None))
            .unwrap();

        let req2 = loop {
            if let Some(r) = receiver.try_recv() {
                break r;
            }
            thread::yield_now();
        };
        assert!(matches!(req2.command, GuiCommand::CloseDialog(_)));
        req2.response_tx
            .send(CommandResult::success("dialog", None))
            .unwrap();
    });

    let r1 = handle.open_dialog(DialogType::CreateVm).unwrap();
    let r2 = handle.close_dialog(DialogType::CreateVm).unwrap();
    assert!(r1.success);
    assert!(r2.success);
    t.join().unwrap();
}

#[test]
fn select_vm_by_id_and_name() {
    let (handle, receiver) = AutomationHandle::new();

    let t = thread::spawn(move || {
        for _ in 0..2 {
            let req = loop {
                if let Some(r) = receiver.try_recv() {
                    break r;
                }
                thread::yield_now();
            };
            if let GuiCommand::SelectVm(ref params) = req.command {
                assert!(!params.identifier.is_empty());
            } else {
                panic!("Expected SelectVm");
            }
            req.response_tx
                .send(CommandResult::success("select_vm", None))
                .unwrap();
        }
    });

    handle.select_vm("vm-123").unwrap();
    handle.select_vm_by_name("my-vm").unwrap();
    t.join().unwrap();
}

#[test]
fn vm_action_sends_correct_command() {
    let (handle, receiver) = AutomationHandle::new();

    let t = thread::spawn(move || {
        let req = loop {
            if let Some(r) = receiver.try_recv() {
                break r;
            }
            thread::yield_now();
        };
        assert!(matches!(req.command, GuiCommand::VmAction(VmActionType::Start)));
        req.response_tx
            .send(CommandResult::success("vm_action", None))
            .unwrap();
    });

    let result = handle.vm_action(VmActionType::Start).unwrap();
    assert!(result.success);
    t.join().unwrap();
}

// ─── CommandResult ─────────────────────────────────────────────────

#[test]
fn command_result_success_and_error_constructors() {
    let ok = CommandResult::success("test_cmd", Some(json!({"key": "val"})));
    assert!(ok.success);
    assert_eq!(ok.command, "test_cmd");
    assert!(ok.data.is_some());
    assert!(ok.error.is_none());
    assert!(!ok.timestamp.is_empty());

    let err = CommandResult::error("test_cmd", "something broke");
    assert!(!err.success);
    assert_eq!(err.command, "test_cmd");
    assert!(err.data.is_none());
    assert_eq!(err.error.as_deref(), Some("something broke"));
}

// ─── AutomationError ──────────────────────────────────────────────

#[test]
fn automation_error_display() {
    let errors = vec![
        (AutomationError::ChannelClosed, "channel closed"),
        (AutomationError::NoResponse, "No response"),
        (
            AutomationError::InvalidCommand("bad".into()),
            "Invalid command",
        ),
        (
            AutomationError::CommandFailed("oops".into()),
            "Command failed",
        ),
        (AutomationError::VmNotFound("vm-1".into()), "VM not found"),
        (
            AutomationError::ActionNotAvailable("delete".into()),
            "not available",
        ),
    ];

    for (err, expected_substr) in errors {
        let msg = format!("{}", err);
        assert!(
            msg.to_lowercase().contains(&expected_substr.to_lowercase()),
            "Error '{}' should contain '{}'",
            msg,
            expected_substr
        );
    }
}

// ─── Cross-thread Command Pipeline ─────────────────────────────────

#[test]
fn multi_command_pipeline_across_threads() {
    let (handle, receiver) = AutomationHandle::new();

    // Simulate a GUI processing loop in a background thread
    let gui_thread = thread::spawn(move || {
        let mut processed = 0;
        loop {
            if let Some(req) = receiver.try_recv() {
                let reply = match req.command {
                    GuiCommand::Refresh => CommandResult::success("refresh", None),
                    GuiCommand::GetState => CommandResult::success(
                        "get_state",
                        Some(json!({"connected": true, "vm_count": 3})),
                    ),
                    _ => CommandResult::error("unknown", "unhandled command"),
                };
                let _ = req.response_tx.send(reply);
                processed += 1;
                if processed >= 3 {
                    break;
                }
            }
            thread::yield_now();
        }
        processed
    });

    // Agent side: send multiple commands
    let r1 = handle.execute(GuiCommand::Refresh).unwrap();
    assert!(r1.success);

    let r2 = handle.execute(GuiCommand::GetState).unwrap();
    assert!(r2.success);
    assert!(r2.data.is_some());

    let r3 = handle
        .execute(GuiCommand::VmAction(VmActionType::Delete))
        .unwrap();
    assert!(!r3.success);

    let count = gui_thread.join().unwrap();
    assert_eq!(count, 3);
}
