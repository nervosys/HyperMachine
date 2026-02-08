//! Automation Handler
//!
//! Provides the `AutomationHandle` for external agents to send commands
//! and receive results, plus the internal handler for processing commands.

use super::commands::*;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};

/// Handle for external agents to interact with the GUI
///
/// This is the primary interface for AI agents to control the GUI.
/// It is thread-safe and can be cloned for use across multiple agent instances.
#[derive(Clone)]
pub struct AutomationHandle {
    /// Channel for sending commands to the GUI
    command_tx: Sender<AutomationRequest>,
    /// Shared response storage (last response)
    #[allow(dead_code)]
    response_store: Arc<Mutex<Option<CommandResult>>>,
}

/// An automation request with callback
pub struct AutomationRequest {
    /// The command to execute
    pub command: GuiCommand,
    /// Channel to send the response
    pub response_tx: Sender<CommandResult>,
}

impl AutomationHandle {
    /// Create a new automation handle and receiver pair
    ///
    /// Returns:
    /// - `AutomationHandle`: For external agents to send commands
    /// - `AutomationReceiver`: For the GUI to receive and process commands
    pub fn new() -> (Self, AutomationReceiver) {
        let (command_tx, command_rx) = channel();
        let response_store = Arc::new(Mutex::new(None));

        let handle = Self {
            command_tx,
            response_store,
        };

        let receiver = AutomationReceiver { command_rx };

        (handle, receiver)
    }

    /// Send a command and wait for the result (blocking)
    pub fn execute(&self, command: GuiCommand) -> Result<CommandResult, AutomationError> {
        let (response_tx, response_rx) = channel();

        self.command_tx
            .send(AutomationRequest {
                command,
                response_tx,
            })
            .map_err(|_| AutomationError::ChannelClosed)?;

        response_rx
            .recv()
            .map_err(|_| AutomationError::NoResponse)
    }

    /// Send a command without waiting for result (non-blocking)
    pub fn execute_async(&self, command: GuiCommand) -> Result<ResponseWaiter, AutomationError> {
        let (response_tx, response_rx) = channel();

        self.command_tx
            .send(AutomationRequest {
                command,
                response_tx,
            })
            .map_err(|_| AutomationError::ChannelClosed)?;

        Ok(ResponseWaiter { response_rx })
    }

    /// Execute a command from JSON
    pub fn execute_json(&self, json: &str) -> Result<CommandResult, AutomationError> {
        let command: GuiCommand =
            serde_json::from_str(json).map_err(|e| AutomationError::InvalidCommand(e.to_string()))?;
        self.execute(command)
    }

    /// Navigate to a view
    pub fn navigate(&self, view: ViewType) -> Result<CommandResult, AutomationError> {
        self.execute(GuiCommand::Navigate(NavigateParams { view }))
    }

    /// Open a dialog
    pub fn open_dialog(&self, dialog: DialogType) -> Result<CommandResult, AutomationError> {
        self.execute(GuiCommand::OpenDialog(dialog))
    }

    /// Close a dialog
    pub fn close_dialog(&self, dialog: DialogType) -> Result<CommandResult, AutomationError> {
        self.execute(GuiCommand::CloseDialog(dialog))
    }

    /// Select a VM by ID
    pub fn select_vm(&self, id: &str) -> Result<CommandResult, AutomationError> {
        self.execute(GuiCommand::SelectVm(SelectVmParams {
            identifier: id.to_string(),
            by: SelectionMode::Id,
        }))
    }

    /// Select a VM by name
    pub fn select_vm_by_name(&self, name: &str) -> Result<CommandResult, AutomationError> {
        self.execute(GuiCommand::SelectVm(SelectVmParams {
            identifier: name.to_string(),
            by: SelectionMode::Name,
        }))
    }

    /// Set a form field value
    pub fn set_field(
        &self,
        form: FormType,
        field: &str,
        value: impl serde::Serialize,
    ) -> Result<CommandResult, AutomationError> {
        let value = serde_json::to_value(value)
            .map_err(|e| AutomationError::InvalidCommand(e.to_string()))?;
        self.execute(GuiCommand::SetFormField(FormFieldParams {
            form,
            field: field.to_string(),
            value,
        }))
    }

    /// Trigger a VM action
    pub fn vm_action(&self, action: VmActionType) -> Result<CommandResult, AutomationError> {
        self.execute(GuiCommand::VmAction(action))
    }

    /// Get current GUI state
    pub fn get_state(&self) -> Result<GuiStateSnapshot, AutomationError> {
        let result = self.execute(GuiCommand::GetState)?;
        if result.success {
            if let Some(data) = result.data {
                serde_json::from_value(data)
                    .map_err(|e| AutomationError::InvalidCommand(e.to_string()))
            } else {
                Err(AutomationError::NoResponse)
            }
        } else {
            Err(AutomationError::CommandFailed(
                result.error.unwrap_or_default(),
            ))
        }
    }

    /// Refresh VM list
    pub fn refresh(&self) -> Result<CommandResult, AutomationError> {
        self.execute(GuiCommand::Refresh)
    }

    /// Get available actions
    pub fn get_available_actions(&self) -> Result<AvailableActions, AutomationError> {
        let result = self.execute(GuiCommand::GetAvailableActions)?;
        if result.success {
            if let Some(data) = result.data {
                serde_json::from_value(data)
                    .map_err(|e| AutomationError::InvalidCommand(e.to_string()))
            } else {
                Err(AutomationError::NoResponse)
            }
        } else {
            Err(AutomationError::CommandFailed(
                result.error.unwrap_or_default(),
            ))
        }
    }
}

impl Default for AutomationHandle {
    fn default() -> Self {
        Self::new().0
    }
}

/// Waiter for async command response
pub struct ResponseWaiter {
    response_rx: Receiver<CommandResult>,
}

impl ResponseWaiter {
    /// Wait for the response (blocking)
    pub fn wait(self) -> Result<CommandResult, AutomationError> {
        self.response_rx
            .recv()
            .map_err(|_| AutomationError::NoResponse)
    }

    /// Try to get response without blocking
    pub fn try_get(&self) -> Option<CommandResult> {
        self.response_rx.try_recv().ok()
    }
}

/// Receiver for the GUI to process automation commands
pub struct AutomationReceiver {
    command_rx: Receiver<AutomationRequest>,
}

impl AutomationReceiver {
    /// Try to receive a command without blocking
    pub fn try_recv(&self) -> Option<AutomationRequest> {
        match self.command_rx.try_recv() {
            Ok(request) => Some(request),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }

    /// Receive all pending commands
    pub fn drain(&self) -> Vec<AutomationRequest> {
        let mut requests = Vec::new();
        while let Some(request) = self.try_recv() {
            requests.push(request);
        }
        requests
    }
}

/// Errors that can occur during automation
#[derive(Debug, Clone)]
pub enum AutomationError {
    /// The command channel is closed
    ChannelClosed,
    /// No response received
    NoResponse,
    /// Invalid command format
    InvalidCommand(String),
    /// Command execution failed
    CommandFailed(String),
    /// VM not found
    VmNotFound(String),
    /// Action not available
    ActionNotAvailable(String),
}

impl std::fmt::Display for AutomationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChannelClosed => write!(f, "Automation channel closed"),
            Self::NoResponse => write!(f, "No response from GUI"),
            Self::InvalidCommand(msg) => write!(f, "Invalid command: {}", msg),
            Self::CommandFailed(msg) => write!(f, "Command failed: {}", msg),
            Self::VmNotFound(id) => write!(f, "VM not found: {}", id),
            Self::ActionNotAvailable(action) => write!(f, "Action not available: {}", action),
        }
    }
}

impl std::error::Error for AutomationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_automation_handle_creation() {
        let (handle, _receiver) = AutomationHandle::new();
        assert!(handle.command_tx.send(AutomationRequest {
            command: GuiCommand::GetState,
            response_tx: channel().0,
        }).is_ok());
    }

    #[test]
    fn test_command_serialization() {
        let cmd = GuiCommand::Navigate(NavigateParams {
            view: ViewType::VmDetails,
        });
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("Navigate"));
        assert!(json.contains("vm_details"));
    }

    #[test]
    fn test_command_result_success() {
        let result = CommandResult::success("test", Some(serde_json::json!({"key": "value"})));
        assert!(result.success);
        assert!(result.error.is_none());
        assert!(result.data.is_some());
    }

    #[test]
    fn test_command_result_error() {
        let result = CommandResult::error("test", "Something went wrong");
        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result.data.is_none());
    }
}
