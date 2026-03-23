//! Network virtualization for HyperMachine

#![allow(dead_code)]

pub mod tap;
pub mod virtio;
pub mod vswitch;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum NetError {
    #[error("Network error: {0}")]
    Network(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, NetError>;
