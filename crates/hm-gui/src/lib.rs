//! HyperMachine Desktop GUI Library
//!
//! A virt-manager style graphical interface for managing virtual machines.
//! Provides screen passthrough, VM management, and system monitoring.

#![warn(clippy::all, rust_2018_idioms)]

pub mod api;
pub mod app;
pub mod components;
pub mod state;
pub mod theme;
pub mod widgets;

pub use app::HyperMachineApp;
pub use state::{AppState, CreateVmForm, VmCounts, VmState};
