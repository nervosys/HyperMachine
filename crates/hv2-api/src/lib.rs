//! Remote API for HyperMachine
//!
//! Provides gRPC and REST APIs for remote VM control

#![allow(dead_code)]

pub mod agent_runtime_routes;
pub mod config;
pub mod events;
pub mod gpu_fabric_routes;
pub mod grpc;
pub mod health_routes;
pub mod image_registry_routes;
pub mod metrics_routes;
pub mod middleware;
pub mod ontology;
pub mod permission_middleware;
pub mod rest;
pub mod runtime_routes;
pub mod server;
pub mod snapshot_routes;
pub mod tls;
pub mod ws_routes;

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

    #[error("Runtime error: {0}")]
    Runtime(#[from] hv2_runtime::RuntimeError),

    #[error("Transport error: {0}")]
    Transport(String),
}

pub type Result<T> = std::result::Result<T, ApiError>;
