//! Application state management

use crate::api::{VmInfo, VmStateApi};
use std::collections::HashMap;

/// Main application state
#[derive(Debug, Clone)]
pub struct AppState {
    /// Connected to backend
    pub connected: bool,
    /// Backend URL
    pub backend_url: String,
    /// Last error message
    pub last_error: Option<String>,
    /// Host information
    pub host_info: Option<HostState>,
    /// All VMs
    pub vms: HashMap<String, VmState>,
    /// Currently selected VM
    pub selected_vm: Option<String>,
    /// VM being viewed in console
    pub console_vm: Option<String>,
    /// Show create VM dialog
    pub show_create_dialog: bool,
    /// Show settings dialog
    pub show_settings: bool,
    /// Show about dialog
    pub show_about: bool,
    /// Create VM form state
    pub create_form: CreateVmForm,
    /// Auto-refresh enabled
    pub auto_refresh: bool,
    /// Refresh interval in seconds
    pub refresh_interval: u32,
    /// Last refresh time
    pub last_refresh: std::time::Instant,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            connected: false,
            backend_url: "http://localhost:8080".to_string(),
            last_error: None,
            host_info: None,
            vms: HashMap::new(),
            selected_vm: None,
            console_vm: None,
            show_create_dialog: false,
            show_settings: false,
            show_about: false,
            create_form: CreateVmForm::default(),
            auto_refresh: true,
            refresh_interval: 5,
            last_refresh: std::time::Instant::now(),
        }
    }
}

impl AppState {
    /// Check if it's time to refresh
    pub fn should_refresh(&self) -> bool {
        self.auto_refresh
            && self.connected
            && self.last_refresh.elapsed().as_secs() >= self.refresh_interval as u64
    }

    /// Mark refresh complete
    pub fn mark_refreshed(&mut self) {
        self.last_refresh = std::time::Instant::now();
    }

    /// Update VMs from API response
    pub fn update_vms(&mut self, vms: Vec<VmInfo>) {
        // Keep track of which VMs still exist
        let existing_ids: std::collections::HashSet<_> = vms.iter().map(|v| v.id.clone()).collect();

        // Remove VMs that no longer exist
        self.vms.retain(|id, _| existing_ids.contains(id));

        // Update or add VMs
        for vm in vms {
            self.vms
                .entry(vm.id.clone())
                .and_modify(|v| v.update_from(&vm))
                .or_insert_with(|| VmState::from(vm));
        }
    }

    /// Get sorted list of VMs
    pub fn sorted_vms(&self) -> Vec<&VmState> {
        let mut vms: Vec<_> = self.vms.values().collect();
        vms.sort_by(|a, b| a.name.cmp(&b.name));
        vms
    }

    /// Get selected VM state
    pub fn selected_vm_state(&self) -> Option<&VmState> {
        self.selected_vm.as_ref().and_then(|id| self.vms.get(id))
    }

    /// Count VMs by state
    pub fn vm_counts(&self) -> VmCounts {
        let mut counts = VmCounts::default();
        for vm in self.vms.values() {
            match vm.state {
                VmStateApi::Running => counts.running += 1,
                VmStateApi::Stopped => counts.stopped += 1,
                VmStateApi::Paused => counts.paused += 1,
                VmStateApi::Error => counts.error += 1,
                _ => counts.other += 1,
            }
        }
        counts.total = self.vms.len();
        counts
    }
}

/// VM counts summary
#[derive(Debug, Default, Clone, Copy)]
pub struct VmCounts {
    pub total: usize,
    pub running: usize,
    pub stopped: usize,
    pub paused: usize,
    pub error: usize,
    pub other: usize,
}

/// Host system state
#[derive(Debug, Clone)]
pub struct HostState {
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub cpus: u32,
    pub memory_total_mb: u64,
    pub memory_free_mb: u64,
    pub hypervisor: String,
}

/// Per-VM state in the GUI
#[derive(Clone)]
pub struct VmState {
    pub id: String,
    pub name: String,
    pub state: VmStateApi,
    pub cpus: u32,
    pub memory_mb: u32,
    pub disk_path: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub ip_address: Option<String>,
    /// Framebuffer texture handle (for screen passthrough)
    pub framebuffer_handle: Option<egui::TextureHandle>,
    /// Framebuffer dimensions
    pub framebuffer_size: Option<(u32, u32)>,
    /// Last framebuffer update
    pub last_fb_update: std::time::Instant,
    /// Is expanded in list view
    pub expanded: bool,
    /// Operation in progress
    pub operation_pending: Option<String>,
}
impl std::fmt::Debug for VmState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VmState")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl From<VmInfo> for VmState {
    fn from(info: VmInfo) -> Self {
        Self {
            id: info.id,
            name: info.name,
            state: info.state,
            cpus: info.cpus,
            memory_mb: info.memory_mb,
            disk_path: info.disk_path,
            created_at: info.created_at,
            started_at: info.started_at,
            ip_address: info.ip_address,
            framebuffer_handle: None,
            framebuffer_size: None,
            last_fb_update: std::time::Instant::now(),
            expanded: false,
            operation_pending: None,
        }
    }
}

impl VmState {
    /// Update from API info
    pub fn update_from(&mut self, info: &VmInfo) {
        self.name = info.name.clone();
        self.state = info.state;
        self.cpus = info.cpus;
        self.memory_mb = info.memory_mb;
        self.disk_path = info.disk_path.clone();
        self.started_at = info.started_at.clone();
        self.ip_address = info.ip_address.clone();

        // Clear pending operation if state changed
        if self.operation_pending.is_some() {
            self.operation_pending = None;
        }
    }

    /// Check if VM can be started
    pub fn can_start(&self) -> bool {
        matches!(self.state, VmStateApi::Stopped | VmStateApi::Paused)
            && self.operation_pending.is_none()
    }

    /// Check if VM can be stopped
    pub fn can_stop(&self) -> bool {
        matches!(self.state, VmStateApi::Running | VmStateApi::Paused)
            && self.operation_pending.is_none()
    }

    /// Check if VM can be paused
    pub fn can_pause(&self) -> bool {
        self.state == VmStateApi::Running && self.operation_pending.is_none()
    }

    /// Check if VM can be deleted
    pub fn can_delete(&self) -> bool {
        self.state == VmStateApi::Stopped && self.operation_pending.is_none()
    }

    /// Get state color
    pub fn state_color(&self) -> egui::Color32 {
        match self.state {
            VmStateApi::Running => egui::Color32::from_rgb(0x4c, 0xaf, 0x50), // Green
            VmStateApi::Stopped => egui::Color32::from_rgb(0x9e, 0x9e, 0x9e), // Gray
            VmStateApi::Paused => egui::Color32::from_rgb(0xff, 0x98, 0x00),  // Orange
            VmStateApi::Error => egui::Color32::from_rgb(0xf4, 0x43, 0x36),   // Red
            VmStateApi::Creating
            | VmStateApi::Starting
            | VmStateApi::Stopping => egui::Color32::from_rgb(0x21, 0x96, 0xf3), // Blue
        }
    }
}

/// Form state for creating a new VM
#[derive(Debug, Clone)]
pub struct CreateVmForm {
    pub name: String,
    pub cpus: u32,
    pub memory_mb: u32,
    pub disk_path: String,
    pub boot_image: String,
    pub network_enabled: bool,
    pub error: Option<String>,
    pub creating: bool,
}

impl Default for CreateVmForm {
    fn default() -> Self {
        Self {
            name: String::new(),
            cpus: 2,
            memory_mb: 2048,
            disk_path: String::new(),
            boot_image: String::new(),
            network_enabled: true,
            error: None,
            creating: false,
        }
    }
}

impl CreateVmForm {
    /// Reset form to defaults
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Validate form
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Name is required".to_string());
        }
        if self.cpus == 0 {
            return Err("At least 1 CPU is required".to_string());
        }
        if self.memory_mb < 128 {
            return Err("At least 128 MB of memory is required".to_string());
        }
        Ok(())
    }
}

