//! HyperMachine Desktop GUI Library
//!
//! A virt-manager style graphical interface for managing virtual machines.
//! Provides screen passthrough, VM management, and system monitoring.
//!
//! ## Agent Automation
//!
//! This crate includes a semantic automation API for AI agents to control
//! the GUI programmatically. See the [`agentic`] module for details.

#![warn(clippy::all, rust_2018_idioms)]

pub mod agentic;
pub mod api;
pub mod app;
pub mod components;
pub mod state;
pub mod theme;
pub mod widgets;

pub use agentic::{
    ActionInfo, AgentCapabilities, AutomationError, AutomationHandle, AutomationReceiver,
    AvailableActions, CommandResult, DialogType, FormFieldParams, FormType, GuiCommand,
    GuiStateSnapshot, GuiToolDefinition, NavigateParams, SelectVmParams, SelectionMode,
    ToggleParams, ToggleSetting, ViewType, VmActionType,
};
pub use agentic::{get_anthropic_tools, get_gemini_tools, get_gui_tools, get_openai_tools};
pub use app::HyperMachineApp;
pub use state::{AppState, CreateVmForm, VmCounts, VmState};