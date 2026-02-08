//! GUI Automation Commands
//!
//! Defines all commands that AI agents can send to control the GUI.
//! Each command is atomic, validated, and returns a structured result.

use serde::{Deserialize, Serialize};

/// A command that can be sent to the GUI for automation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "params")]
pub enum GuiCommand {
    // ===== Navigation Commands =====
    /// Navigate to a specific view
    Navigate(NavigateParams),

    // ===== Dialog Commands =====
    /// Open a dialog
    OpenDialog(DialogType),

    /// Close a dialog
    CloseDialog(DialogType),

    /// Submit the current form dialog
    SubmitDialog(DialogType),

    // ===== VM Selection Commands =====
    /// Select a VM by ID or name
    SelectVm(SelectVmParams),

    /// Deselect all VMs
    DeselectVm,

    // ===== Form Commands =====
    /// Set a form field value
    SetFormField(FormFieldParams),

    /// Reset a form to default values
    ResetForm(FormType),

    // ===== Action Commands =====
    /// Execute an action on the selected VM
    VmAction(VmActionType),

    /// Toggle a boolean setting
    Toggle(ToggleParams),

    /// Refresh the VM list
    Refresh,

    // ===== Query Commands =====
    /// Get current GUI state
    GetState,

    /// Get available actions for current context
    GetAvailableActions,
}

/// Parameters for navigation command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigateParams {
    /// Target view to navigate to
    pub view: ViewType,
}

/// Available views in the GUI
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViewType {
    /// Welcome/home screen
    Welcome,
    /// VM details view
    VmDetails,
    /// VM console view
    VmConsole,
    /// Settings view
    Settings,
}

/// Types of dialogs that can be opened/closed
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DialogType {
    /// Create new VM dialog
    CreateVm,
    /// Settings dialog
    Settings,
    /// About dialog
    About,
}

/// Parameters for VM selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectVmParams {
    /// VM identifier (ID or name)
    pub identifier: String,
    /// How to interpret the identifier
    #[serde(default)]
    pub by: SelectionMode,
}

/// How to match VM for selection
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    /// Match by VM ID
    #[default]
    Id,
    /// Match by VM name (exact)
    Name,
    /// Match by VM name (partial/contains)
    NameContains,
}

/// Parameters for form field updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormFieldParams {
    /// Which form to update
    pub form: FormType,
    /// Field name
    pub field: String,
    /// New value (JSON value for flexibility)
    pub value: serde_json::Value,
}

/// Types of forms in the GUI
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FormType {
    /// Create VM form
    CreateVm,
    /// Settings form
    Settings,
}

/// VM actions that can be triggered
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VmActionType {
    /// Start the VM
    Start,
    /// Stop the VM
    Stop,
    /// Pause the VM
    Pause,
    /// Resume the VM
    Resume,
    /// Delete the VM
    Delete,
    /// Open console for the VM
    OpenConsole,
    /// Close console
    CloseConsole,
}

/// Parameters for toggle commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToggleParams {
    /// What to toggle
    pub setting: ToggleSetting,
    /// Optional explicit value (if not provided, toggles current)
    pub value: Option<bool>,
}

/// Settings that can be toggled
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToggleSetting {
    /// Auto-refresh of VM list
    AutoRefresh,
    /// Dark mode theme
    DarkMode,
}

/// Result of executing a GUI command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    /// Whether the command succeeded
    pub success: bool,
    /// Command that was executed
    pub command: String,
    /// Result data (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Error message (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Timestamp of execution
    pub timestamp: String,
}

impl CommandResult {
    /// Create a success result
    pub fn success(command: &str, data: Option<serde_json::Value>) -> Self {
        Self {
            success: true,
            command: command.to_string(),
            data,
            error: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Create an error result
    pub fn error(command: &str, error: impl Into<String>) -> Self {
        Self {
            success: false,
            command: command.to_string(),
            data: None,
            error: Some(error.into()),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Snapshot of GUI state for observability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiStateSnapshot {
    /// Current view
    pub current_view: ViewType,
    /// Connection status
    pub connected: bool,
    /// Selected VM (if any)
    pub selected_vm: Option<VmSnapshot>,
    /// Open dialogs
    pub open_dialogs: Vec<DialogType>,
    /// VM counts
    pub vm_counts: VmCountsSnapshot,
    /// Available actions in current context
    pub available_actions: Vec<String>,
}

/// Snapshot of a VM's state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmSnapshot {
    pub id: String,
    pub name: String,
    pub state: String,
    pub cpus: u32,
    pub memory_mb: u32,
}

/// VM counts snapshot
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VmCountsSnapshot {
    pub total: usize,
    pub running: usize,
    pub stopped: usize,
    pub paused: usize,
    pub error: usize,
}

/// Actions available in the GUI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableActions {
    /// Global actions always available
    pub global: Vec<ActionInfo>,
    /// VM-specific actions (if VM selected)
    pub vm_actions: Vec<ActionInfo>,
    /// Form actions (if form open)
    pub form_actions: Vec<ActionInfo>,
}

/// Information about an available action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionInfo {
    /// Action identifier
    pub id: String,
    /// Human-readable description
    pub description: String,
    /// Whether the action is currently enabled
    pub enabled: bool,
}
