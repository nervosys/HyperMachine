//! HyperMachine CLI Library
//!
//! This crate provides the core functionality for the HyperMachine CLI,
//! including VM management, the MCP HTTP server for AI agent integration,
//! and an agentic interface for LLM-based VM control.

#![allow(dead_code)]

pub mod agentic;
pub mod mcp_server;
pub mod t1_manager;
pub mod vm_manager;
