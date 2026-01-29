//! TAP/TUN network device support

use crate::Result;

/// TAP device configuration
pub struct TapConfig {
    pub name: String,
    pub mac_address: Option<String>,
}

/// TAP network device
pub struct TapDevice {
    config: TapConfig,
}

impl TapDevice {
    pub fn new(config: TapConfig) -> Self {
        Self { config }
    }

    pub async fn create(&mut self) -> Result<()> {
        tracing::info!("Creating TAP device: {}", self.config.name);
        // TODO: Implement TAP device creation
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }
}
