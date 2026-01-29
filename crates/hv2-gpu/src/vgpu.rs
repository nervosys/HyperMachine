//! Virtual GPU implementation

use std::sync::Arc;
use wgpu::{Device, Instance, Queue};

use crate::Result;

/// Virtual GPU device
pub struct VirtualGpu {
    name: String,
}

impl VirtualGpu {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub async fn init(&mut self) -> Result<()> {
        tracing::info!("Initializing virtual GPU: {}", self.name);
        // TODO: Initialize WGPU device
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}
