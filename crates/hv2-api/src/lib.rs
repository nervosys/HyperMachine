//! Remote API for HV2
//!
//! Provides gRPC and REST APIs for remote VM control

pub mod grpc;
pub mod rest;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("VM not found: {0}")]
    VmNotFound(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Agent error: {0}")]
    Agent(#[from] hv2_agent::AgentError),

    #[error("Core error: {0}")]
    Core(#[from] hv2_core::Error),

    #[error("Transport error: {0}")]
    Transport(String),
}

pub type Result<T> = std::result::Result<T, ApiError>;
