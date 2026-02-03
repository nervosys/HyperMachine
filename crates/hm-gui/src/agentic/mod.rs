//! Agentic Automation Interface for HyperMachine GUI
//!
//! Provides a semantic, tool-based API for AI agents to interact with the GUI
//! programmatically. This approach is superior to screen-based automation (like
//! Anthropic's Computer Use) because it:
//!
//! 1. **Exposes semantic actions** - Agents call `gui.create_vm_dialog.open()` not
//!    `mouse.click(x, y)`
//! 2. **Is deterministic** - No OCR errors or visual ambiguity
//! 3. **Is fast** - Direct function calls vs. screen capture/analysis cycles
//! 4. **Is reliable** - Layout changes don't break automation
//!
//! # Design Principles
//!
//! - **Command-based**: All GUI interactions are discrete commands
//! - **Type-safe**: Strong typing with JSON Schema validation
//! - **Observable**: State queries return structured data
//! - **Async-friendly**: Non-blocking command dispatch via channels
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────────┐     ┌─────────────────┐
//! │  AI Agent   │────▶│ AutomationHandle │────▶│ HyperMachineApp │
//! │  (External) │◀────│   (Commands)     │◀────│   (GUI State)   │
//! └─────────────┘     └──────────────────┘     └─────────────────┘
//! ```

mod commands;
mod handler;
mod tools;

pub use commands::*;
pub use handler::*;
pub use tools::*;
