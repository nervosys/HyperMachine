//! Main application struct and event loop

use crate::api::{ApiClient, VmConfig};
use crate::components::{
    about_dialog, main_content, settings_dialog, toolbar, vm_list_sidebar, ContentAction,
    SidebarAction, ToolbarAction,
};
use crate::state::AppState;
use crate::theme::configure_dark_theme;
use crate::widgets::create_vm_dialog;

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use tokio::runtime::Runtime;

/// Main application
pub struct HyperMachineApp {
    /// Application state
    state: AppState,
    /// Tokio runtime for async operations
    runtime: Arc<Runtime>,
    /// Channel for receiving API responses
    response_rx: Receiver<ApiResponse>,
    /// Channel for sending API responses
    response_tx: Sender<ApiResponse>,
    /// API client
    api: ApiClient,
}

/// API response types
#[derive(Debug)]
enum ApiResponse {
    Connected(bool),
    VmList(Vec<crate::api::VmInfo>),
    VmCreated(Result<crate::api::VmInfo, String>),
    VmStarted(String, Result<crate::api::VmInfo, String>),
    VmStopped(String, Result<crate::api::VmInfo, String>),
    VmPaused(String, Result<crate::api::VmInfo, String>),
    VmDeleted(String, Result<(), String>),
    Framebuffer(String, Result<crate::api::FramebufferData, String>),
    Error(String),
}

impl HyperMachineApp {
    /// Create new application
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Configure theme
        configure_dark_theme(&cc.egui_ctx);

        // Create channels for async communication
        let (response_tx, response_rx) = channel();

        // Create tokio runtime
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to create Tokio runtime"),
        );

        // Load saved state or use defaults
        let state = cc
            .storage
            .and_then(|s| eframe::get_value(s, "app_state"))
            .unwrap_or_default();

        let api = ApiClient::new(&AppState::default().backend_url);

        let mut app = Self {
            state,
            runtime,
            response_rx,
            response_tx,
            api,
        };

        // Start initial connection check
        app.check_connection();
        app.refresh_vms();

        app
    }

    /// Check connection to backend
    fn check_connection(&self) {
        let api = self.api.clone();
        let tx = self.response_tx.clone();

        self.runtime.spawn(async move {
            let connected = api.health().await.is_ok();
            let _ = tx.send(ApiResponse::Connected(connected));
        });
    }

    /// Refresh VM list
    fn refresh_vms(&mut self) {
        let api = self.api.clone();
        let tx = self.response_tx.clone();

        self.runtime.spawn(async move {
            match api.list_vms().await {
                Ok(vms) => {
                    let _ = tx.send(ApiResponse::VmList(vms));
                }
                Err(e) => {
                    let _ = tx.send(ApiResponse::Error(e.to_string()));
                }
            }
        });
    }

    /// Create a new VM
    fn create_vm(&self, config: VmConfig) {
        let api = self.api.clone();
        let tx = self.response_tx.clone();

        self.runtime.spawn(async move {
            let result = api.create_vm(config).await;
            let _ = tx.send(ApiResponse::VmCreated(result.map_err(|e| e.to_string())));
        });
    }

    /// Start a VM
    fn start_vm(&mut self, vm_id: String) {
        if let Some(vm) = self.state.vms.get_mut(&vm_id) {
            vm.operation_pending = Some("Starting".to_string());
        }

        let api = self.api.clone();
        let tx = self.response_tx.clone();
        let id = vm_id.clone();

        self.runtime.spawn(async move {
            let result = api.start_vm(&id).await;
            let _ = tx.send(ApiResponse::VmStarted(
                id,
                result.map_err(|e| e.to_string()),
            ));
        });
    }

    /// Stop a VM
    fn stop_vm(&mut self, vm_id: String) {
        if let Some(vm) = self.state.vms.get_mut(&vm_id) {
            vm.operation_pending = Some("Stopping".to_string());
        }

        let api = self.api.clone();
        let tx = self.response_tx.clone();
        let id = vm_id.clone();

        self.runtime.spawn(async move {
            let result = api.stop_vm(&id).await;
            let _ = tx.send(ApiResponse::VmStopped(
                id,
                result.map_err(|e| e.to_string()),
            ));
        });
    }

    /// Pause a VM
    fn pause_vm(&mut self, vm_id: String) {
        if let Some(vm) = self.state.vms.get_mut(&vm_id) {
            vm.operation_pending = Some("Pausing".to_string());
        }

        let api = self.api.clone();
        let tx = self.response_tx.clone();
        let id = vm_id.clone();

        self.runtime.spawn(async move {
            let result = api.pause_vm(&id).await;
            let _ = tx.send(ApiResponse::VmPaused(id, result.map_err(|e| e.to_string())));
        });
    }

    /// Delete a VM
    fn delete_vm(&mut self, vm_id: String) {
        if let Some(vm) = self.state.vms.get_mut(&vm_id) {
            vm.operation_pending = Some("Deleting".to_string());
        }

        let api = self.api.clone();
        let tx = self.response_tx.clone();
        let id = vm_id.clone();

        self.runtime.spawn(async move {
            let result = api.delete_vm(&id).await;
            let _ = tx.send(ApiResponse::VmDeleted(
                id,
                result.map_err(|e| e.to_string()),
            ));
        });
    }

    /// Fetch framebuffer for a VM
    fn fetch_framebuffer(&self, vm_id: String) {
        let api = self.api.clone();
        let tx = self.response_tx.clone();

        self.runtime.spawn(async move {
            let result = api.get_framebuffer(&vm_id).await;
            let _ = tx.send(ApiResponse::Framebuffer(
                vm_id,
                result.map_err(|e| e.to_string()),
            ));
        });
    }

    /// Process API responses
    fn process_responses(&mut self, ctx: &egui::Context) {
        while let Ok(response) = self.response_rx.try_recv() {
            match response {
                ApiResponse::Connected(connected) => {
                    self.state.connected = connected;
                    if !connected {
                        self.state.last_error =
                            Some("Cannot connect to HyperMachine backend".to_string());
                    }
                }
                ApiResponse::VmList(vms) => {
                    self.state.update_vms(vms);
                    self.state.mark_refreshed();
                }
                ApiResponse::VmCreated(result) => {
                    self.state.create_form.creating = false;
                    match result {
                        Ok(vm) => {
                            self.state.show_create_dialog = false;
                            self.state.create_form.reset();
                            self.state
                                .vms
                                .insert(vm.id.clone(), crate::state::VmState::from(vm));
                        }
                        Err(e) => {
                            self.state.create_form.error = Some(e);
                        }
                    }
                }
                ApiResponse::VmStarted(id, result) => {
                    if let Some(vm) = self.state.vms.get_mut(&id) {
                        vm.operation_pending = None;
                        if let Ok(info) = result {
                            vm.update_from(&info);
                        }
                    }
                }
                ApiResponse::VmStopped(id, result) => {
                    if let Some(vm) = self.state.vms.get_mut(&id) {
                        vm.operation_pending = None;
                        if let Ok(info) = result {
                            vm.update_from(&info);
                        }
                    }
                }
                ApiResponse::VmPaused(id, result) => {
                    if let Some(vm) = self.state.vms.get_mut(&id) {
                        vm.operation_pending = None;
                        if let Ok(info) = result {
                            vm.update_from(&info);
                        }
                    }
                }
                ApiResponse::VmDeleted(id, result) => {
                    if result.is_ok() {
                        self.state.vms.remove(&id);
                        if self.state.selected_vm.as_ref() == Some(&id) {
                            self.state.selected_vm = None;
                        }
                        if self.state.console_vm.as_ref() == Some(&id) {
                            self.state.console_vm = None;
                        }
                    } else if let Some(vm) = self.state.vms.get_mut(&id) {
                        vm.operation_pending = None;
                    }
                }
                ApiResponse::Framebuffer(id, result) => {
                    if let Ok(fb) = result {
                        if let Some(vm) = self.state.vms.get_mut(&id) {
                            // Convert framebuffer data to texture
                            let image = egui::ColorImage::from_rgba_unmultiplied(
                                [fb.width as usize, fb.height as usize],
                                &convert_to_rgba(&fb.data, &fb.format),
                            );
                            vm.framebuffer_handle = Some(ctx.load_texture(
                                format!("fb_{}", id),
                                image,
                                egui::TextureOptions::LINEAR,
                            ));
                            vm.framebuffer_size = Some((fb.width, fb.height));
                            vm.last_fb_update = std::time::Instant::now();
                        }
                    }
                }
                ApiResponse::Error(e) => {
                    // Connection errors are expected when backend is offline
                    if e.contains("error sending request") || e.contains("connection refused") {
                        tracing::debug!("Backend unavailable: {}", e);
                    } else {
                        tracing::warn!("API error: {}", e);
                    }
                    self.state.last_error = Some(e);
                }
            }
        }
    }

    /// Handle sidebar action
    fn handle_sidebar_action(&mut self, action: SidebarAction) {
        match action {
            SidebarAction::SelectVm(id) => {
                self.state.selected_vm = Some(id);
                self.state.console_vm = None;
            }
            SidebarAction::StartVm(id) => {
                self.start_vm(id);
            }
            SidebarAction::StopVm(id) => {
                self.stop_vm(id);
            }
            SidebarAction::PauseVm(id) => {
                self.pause_vm(id);
            }
            SidebarAction::ResumeVm(id) => {
                // Resume is same as start
                self.start_vm(id);
            }
            SidebarAction::OpenConsole(id) => {
                self.state.console_vm = Some(id.clone());
                self.state.selected_vm = Some(id.clone());
                self.fetch_framebuffer(id);
            }
            SidebarAction::None => {}
        }
    }

    /// Handle content action
    fn handle_content_action(&mut self, action: ContentAction) {
        match action {
            ContentAction::StartVm(id) => {
                self.start_vm(id);
            }
            ContentAction::StopVm(id) => {
                self.stop_vm(id);
            }
            ContentAction::PauseVm(id) => {
                self.pause_vm(id);
            }
            ContentAction::ResumeVm(id) => {
                self.start_vm(id);
            }
            ContentAction::DeleteVm(id) => {
                self.delete_vm(id);
            }
            ContentAction::CloseConsole => {
                self.state.console_vm = None;
            }
            ContentAction::SendKey(vm_id, keycode, pressed) => {
                let api = self.api.clone();
                self.runtime.spawn(async move {
                    let _ = api.send_key(&vm_id, keycode, pressed).await;
                });
            }
            ContentAction::SendMouse(vm_id, x, y, buttons, scroll) => {
                let api = self.api.clone();
                self.runtime.spawn(async move {
                    let _ = api.send_mouse(&vm_id, x, y, buttons, scroll).await;
                });
            }
            ContentAction::None => {}
        }
    }
}

impl eframe::App for HyperMachineApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process any pending API responses
        self.process_responses(ctx);

        // Auto-refresh
        if self.state.should_refresh() {
            self.refresh_vms();
        }

        // Update framebuffer for console view
        if let Some(console_vm_id) = &self.state.console_vm.clone() {
            if let Some(vm) = self.state.vms.get(console_vm_id) {
                if vm.state == crate::api::VmStateApi::Running
                    && vm.last_fb_update.elapsed().as_millis() > 33
                {
                    // ~30 fps
                    self.fetch_framebuffer(console_vm_id.clone());
                }
            }
        }

        // Top toolbar
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            let action = toolbar(ui, &mut self.state);
            if action == ToolbarAction::Refresh {
                self.refresh_vms();
            }
        });

        // Left sidebar with VM list
        egui::SidePanel::left("vm_list")
            .min_width(300.0)
            .default_width(350.0)
            .show(ctx, |ui| {
                let action = vm_list_sidebar(ui, &mut self.state);
                self.handle_sidebar_action(action);
            });

        // Main content area
        egui::CentralPanel::default().show(ctx, |ui| {
            let action = main_content(ui, &mut self.state);
            self.handle_content_action(action);
        });

        // Dialogs
        if self.state.show_create_dialog {
            egui::Window::new("Create VM")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if let Some(config) = create_vm_dialog(ui, &mut self.state.create_form) {
                        self.state.create_form.creating = true;
                        self.create_vm(config);
                    }
                    if self.state.create_form.cancelled {
                        self.state.show_create_dialog = false;
                        self.state.create_form.reset();
                    }
                });
        }

        if self.state.show_settings {
            egui::Window::new("Settings")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if !settings_dialog(ui, &mut self.state) {
                        self.state.show_settings = false;
                        // Update API client with new URL
                        self.api = ApiClient::new(&self.state.backend_url);
                        self.check_connection();
                    }
                });
        }

        if self.state.show_about {
            egui::Window::new("About")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if !about_dialog(ui) {
                        self.state.show_about = false;
                    }
                });
        }

        // Request repaint for animations
        ctx.request_repaint();
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "app_state", &self.state);
    }
}

/// Convert framebuffer data to RGBA
fn convert_to_rgba(data: &[u8], format: &str) -> Vec<u8> {
    match format {
        "XRGB32" | "ARGB32" => {
            // XRGB32/ARGB32 -> RGBA
            let mut rgba = Vec::with_capacity(data.len());
            for chunk in data.chunks(4) {
                if chunk.len() == 4 {
                    // BGRA -> RGBA
                    rgba.push(chunk[2]); // R
                    rgba.push(chunk[1]); // G
                    rgba.push(chunk[0]); // B
                    rgba.push(255); // A (ignore alpha from XRGB)
                }
            }
            rgba
        }
        "RGB24" => {
            // RGB24 -> RGBA
            let mut rgba = Vec::with_capacity((data.len() / 3) * 4);
            for chunk in data.chunks(3) {
                if chunk.len() == 3 {
                    rgba.push(chunk[0]); // R
                    rgba.push(chunk[1]); // G
                    rgba.push(chunk[2]); // B
                    rgba.push(255); // A
                }
            }
            rgba
        }
        _ => {
            // Assume RGBA or unknown
            data.to_vec()
        }
    }
}

// Make AppState serializable for persistence
impl serde::Serialize for AppState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AppState", 4)?;
        state.serialize_field("backend_url", &self.backend_url)?;
        state.serialize_field("auto_refresh", &self.auto_refresh)?;
        state.serialize_field("refresh_interval", &self.refresh_interval)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for AppState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct SavedState {
            backend_url: String,
            auto_refresh: bool,
            refresh_interval: u32,
        }

        let saved = SavedState::deserialize(deserializer)?;
        Ok(AppState {
            backend_url: saved.backend_url,
            auto_refresh: saved.auto_refresh,
            refresh_interval: saved.refresh_interval,
            ..Default::default()
        })
    }
}
