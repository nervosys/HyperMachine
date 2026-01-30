//! Agentic AI Interface for HyperMachine
//!
//! Provides a structured, discoverable API for AI agents to control VMs
//! without error-prone multi-line text blocks. Supports OpenAI, Anthropic,
//! Google Gemini, and other major LLM providers.
//!
//! # Design Principles
//!
//! 1. **Atomic Operations**: Single-purpose, composable tools
//! 2. **Type-Safe**: Strong typing with JSON Schema validation
//! 3. **Discoverable**: Self-documenting API with examples
//! 4. **Model-Native**: First-class support for major LLM providers

pub mod adapters;
pub mod ontology;
pub mod schema;
pub mod tools;

pub use adapters::*;
pub use ontology::*;
pub use schema::*;
pub use tools::*;
