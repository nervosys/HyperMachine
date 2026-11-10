//! UI components - sidebar, toolbar, console, etc.

use crate::api::VmStateApi;
use crate::state::{AppState, View, VmState};
use crate::theme::{icons, primary_button, AppColors};
use crate::widgets::{connection_status, stats_bar, vm_details_panel, vm_list_item, VmAction};
use egui::{Color32, CornerRadius, RichText, Ui};

pub fn toolbar(ui: &mut Ui, state: &mut AppState) -> ToolbarAction {
    let mut action = ToolbarAction::None;
    egui::MenuBar::new().ui(ui, |ui| {
        ui.label(RichText::new("HyperMachine").strong().size(18.0));
        ui.separator();
        // Which backend's list the sidebar and main panel show. Switching
        // clears the other view's selection implicitly, since each view
        // reads only its own `selected_*` field.
        if ui
            .selectable_label(state.view == View::Vms, "VMs")
            .clicked()
        {
            state.view = View::Vms;
        }
        if ui
            .selectable_label(state.view == View::Sandboxes, "Sandboxes")
            .clicked()
        {
            state.view = View::Sandboxes;
        }
        ui.separator();
        if state.view == View::Vms
            && ui
                .add(primary_button(&format!("{} New VM", icons::ADD)))
                .clicked()
        {
            state.show_create_dialog = true;
        }
        ui.separator();
        if ui.button(format!("{} Refresh", icons::REFRESH)).clicked() {
            action = ToolbarAction::Refresh;
        }
        ui.checkbox(&mut state.auto_refresh, "Auto");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(icons::SETTINGS).clicked() {
                state.show_settings = true;
            }
            if ui.button(icons::INFO).clicked() {
                state.show_about = true;
            }
            ui.separator();
            let (url, label) = match state.view {
                View::Vms => (&state.backend_url, "API"),
                View::Sandboxes => (&state.sandbox_backend_url, "Sandboxd"),
            };
            ui.label(label);
            connection_status(ui, state.connected, url);
        });
    });
    action
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
    None,
    Refresh,
}

pub fn vm_list_sidebar(ui: &mut Ui, state: &mut AppState) -> SidebarAction {
    let mut action = SidebarAction::None;
    let colors = AppColors::default();

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.heading("Virtual Machines");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                stats_bar(ui, &state.vm_counts());
            });
        });
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if state.vms.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label(RichText::new("No virtual machines").color(colors.text_secondary));
                        ui.add_space(16.0);
                        if ui.add(primary_button("Create your first VM")).clicked() {
                            state.show_create_dialog = true;
                        }
                    });
                } else {
                    let vms = state.sorted_vms();
                    let selected_id = state.selected_vm.clone();
                    for vm in vms {
                        let is_selected = selected_id.as_ref() == Some(&vm.id);
                        let vm_action = vm_list_item(ui, vm, is_selected);
                        match vm_action {
                            VmAction::Select => {
                                action = SidebarAction::SelectVm(vm.id.clone());
                            }
                            VmAction::Start => {
                                action = SidebarAction::StartVm(vm.id.clone());
                            }
                            VmAction::Stop => {
                                action = SidebarAction::StopVm(vm.id.clone());
                            }
                            VmAction::Pause => {
                                action = SidebarAction::PauseVm(vm.id.clone());
                            }
                            VmAction::OpenConsole => {
                                action = SidebarAction::OpenConsole(vm.id.clone());
                            }
                            _ => {}
                        }
                    }
                }
            });
    });
    action
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SidebarAction {
    None,
    SelectVm(String),
    StartVm(String),
    StopVm(String),
    PauseVm(String),
    ResumeVm(String),
    OpenConsole(String),
    SelectSandbox(String),
}

/// The Sandboxes view's sidebar: a plain list, since sandbox VMs don't
/// carry a console or the VM lifecycle's pause/resume -- they are created
/// and deleted, not paused, so there is less to show per row than a VM.
pub fn sandbox_list_sidebar(ui: &mut Ui, state: &mut AppState) -> SidebarAction {
    let mut action = SidebarAction::None;
    let colors = AppColors::default();

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.heading("Sandbox VMs");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{} total", state.sandboxes.len()))
                        .color(colors.text_secondary),
                );
            });
        });
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if state.sandboxes.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label(
                            RichText::new("No sandboxes, or no sandboxd node at this URL")
                                .color(colors.text_secondary),
                        );
                    });
                } else {
                    let selected_id = state.selected_sandbox.clone();
                    for sandbox in &state.sandboxes {
                        let is_selected = selected_id.as_deref() == Some(&sandbox.sandbox_id);
                        let response = ui.selectable_label(
                            is_selected,
                            format!(
                                "{}  [{}]  {}",
                                sandbox.sandbox_id, sandbox.state, sandbox.template_id
                            ),
                        );
                        if response.clicked() {
                            action = SidebarAction::SelectSandbox(sandbox.sandbox_id.clone());
                        }
                    }
                }
            });
    });
    action
}

/// The Sandboxes view's main panel: the selected sandbox's fields, or a
/// prompt to pick one.
pub fn sandbox_content(ui: &mut Ui, state: &AppState) -> ContentAction {
    let mut action = ContentAction::None;
    let colors = AppColors::default();
    match state.selected_sandbox_info() {
        Some(sandbox) => {
            ui.heading(&sandbox.sandbox_id);
            ui.add_space(8.0);
            egui::Grid::new("sandbox_details")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Template:");
                    ui.label(&sandbox.template_id);
                    ui.end_row();
                    ui.label("State:");
                    ui.label(&sandbox.state);
                    ui.end_row();
                    ui.label("CPUs:");
                    ui.label(sandbox.cpu_count.to_string());
                    ui.end_row();
                    ui.label("Memory:");
                    ui.label(format!("{} MB", sandbox.memory_mb));
                    ui.end_row();
                    ui.label("Started:");
                    ui.label(&sandbox.started_at);
                    ui.end_row();
                    ui.label("Node:");
                    ui.label(&sandbox.node_id);
                    ui.end_row();
                });
            ui.add_space(16.0);
            if ui.button(format!("{} Delete", icons::DELETE)).clicked() {
                action = ContentAction::DeleteSandbox(sandbox.sandbox_id.clone());
            }
        }
        None => {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.label(
                    RichText::new("Select a sandbox from the list to view its details")
                        .color(colors.text_secondary),
                );
            });
        }
    }
    action
}

pub fn main_content(ui: &mut Ui, state: &mut AppState) -> ContentAction {
    let mut action = ContentAction::None;
    if let Some(console_vm_id) = &state.console_vm.clone() {
        if let Some(vm) = state.vms.get(console_vm_id) {
            action = console_view(ui, vm);
        }
    } else if let Some(vm_id) = &state.selected_vm.clone() {
        if let Some(vm) = state.vms.get(vm_id) {
            let vm_action = vm_details_panel(ui, vm);
            action = ContentAction::from_vm_action(vm_action, vm_id.clone());
        }
    } else {
        welcome_screen(ui, state);
    }
    action
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ContentAction {
    None,
    StartVm(String),
    StopVm(String),
    PauseVm(String),
    ResumeVm(String),
    DeleteVm(String),
    CloseConsole,
    SendKey(String, u32, bool),
    SendMouse(String, i32, i32, u8, i32),
    DeleteSandbox(String),
}

impl ContentAction {
    fn from_vm_action(action: VmAction, vm_id: String) -> Self {
        match action {
            VmAction::Start => ContentAction::StartVm(vm_id),
            VmAction::Stop => ContentAction::StopVm(vm_id),
            VmAction::Pause => ContentAction::PauseVm(vm_id),
            VmAction::Resume => ContentAction::ResumeVm(vm_id),
            VmAction::Delete => ContentAction::DeleteVm(vm_id),
            _ => ContentAction::None,
        }
    }
}

fn welcome_screen(ui: &mut Ui, state: &AppState) {
    let colors = AppColors::default();
    ui.vertical_centered(|ui| {
        ui.add_space(60.0);
        ui.label(RichText::new("HyperMachine").size(64.0));
        ui.add_space(8.0);
        ui.label(
            RichText::new("A modern virtual machine manager")
                .color(colors.text_secondary)
                .size(16.0),
        );
        ui.add_space(32.0);
        if !state.vms.is_empty() {
            let counts = state.vm_counts();
            ui.horizontal(|ui| {
                stat_card(ui, "Total VMs", &counts.total.to_string(), colors.primary);
                stat_card(ui, "Running", &counts.running.to_string(), colors.success);
                stat_card(
                    ui,
                    "Stopped",
                    &counts.stopped.to_string(),
                    colors.text_secondary,
                );
            });
            ui.add_space(24.0);
            ui.label(
                RichText::new("Select a VM from the list to view details")
                    .color(colors.text_secondary),
            );
        } else {
            ui.label(
                RichText::new("No VMs yet. Create one to get started!")
                    .color(colors.text_secondary),
            );
            ui.add_space(24.0);
            egui::Frame::new()
                .fill(colors.surface)
                .inner_margin(24.0)
                .corner_radius(CornerRadius::same(8))
                .show(ui, |ui| {
                    ui.heading("Quick Start");
                    ui.add_space(8.0);
                    ui.label("1. Click New VM to create a virtual machine");
                    ui.label("2. Configure CPU, memory, and storage");
                    ui.label("3. Attach a boot ISO image");
                    ui.label("4. Start the VM and open the console");
                });
        }
    });
}

fn stat_card(ui: &mut Ui, label: &str, value: &str, color: Color32) {
    let colors = AppColors::default();
    egui::Frame::new()
        .fill(colors.surface)
        .inner_margin(16.0)
        .corner_radius(CornerRadius::same(8))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(value).size(32.0).color(color).strong());
                ui.label(RichText::new(label).color(colors.text_secondary));
            });
        });
}

fn console_view(ui: &mut Ui, vm: &VmState) -> ContentAction {
    let mut action = ContentAction::None;
    let colors = AppColors::default();

    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} Console - {}", icons::CONSOLE, vm.name)).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                action = ContentAction::CloseConsole;
            }
            ui.separator();
            ui.button("Fullscreen").clicked();
            if ui.button("Ctrl+Alt+Del").clicked() {}
        });
    });
    ui.separator();
    let available = ui.available_size();

    egui::Frame::new().fill(Color32::BLACK).show(ui, |ui| {
        ui.set_min_size(available);
        if vm.state != VmStateApi::Running {
            ui.vertical_centered(|ui| {
                ui.add_space(available.y / 3.0);
                ui.label(
                    RichText::new("VM is not running")
                        .color(colors.text_secondary)
                        .size(20.0),
                );
                ui.label(
                    RichText::new("Start the VM to see the console").color(colors.text_secondary),
                );
            });
        } else if let Some(texture) = &vm.framebuffer_handle {
            let fb_size = vm.framebuffer_size.unwrap_or((800, 600));
            let scale = (available.x / fb_size.0 as f32).min(available.y / fb_size.1 as f32);
            let display_size = egui::vec2(fb_size.0 as f32 * scale, fb_size.1 as f32 * scale);
            let offset = (available - display_size) / 2.0;
            ui.add_space(offset.y);
            ui.horizontal(|ui| {
                ui.add_space(offset.x);
                let _response = ui.image((texture.id(), display_size));
            });
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(available.y / 3.0);
                ui.spinner();
                ui.label(RichText::new("Connecting to display...").color(colors.text_secondary));
            });
        }
    });
    action
}

pub fn settings_dialog(ui: &mut Ui, state: &mut AppState) -> bool {
    let mut open = true;
    ui.heading("Settings");
    ui.add_space(16.0);
    egui::Grid::new("settings_grid")
        .num_columns(2)
        .spacing([10.0, 10.0])
        .show(ui, |ui| {
            ui.label("Backend URL:");
            ui.text_edit_singleline(&mut state.backend_url);
            ui.end_row();
            ui.label("Sandboxd URL:");
            ui.text_edit_singleline(&mut state.sandbox_backend_url);
            ui.end_row();
            ui.label("Auto-refresh:");
            ui.checkbox(&mut state.auto_refresh, "Enabled");
            ui.end_row();
            ui.label("Refresh interval:");
            ui.add(
                egui::DragValue::new(&mut state.refresh_interval)
                    .range(1..=60)
                    .suffix("s"),
            );
            ui.end_row();
        });
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        if ui.button("Save").clicked() {
            open = false;
        }
        if ui.button("Cancel").clicked() {
            open = false;
        }
    });
    open
}

pub fn about_dialog(ui: &mut Ui) -> bool {
    let mut open = true;
    let colors = AppColors::default();
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("HyperMachine").size(48.0));
        ui.add_space(8.0);
        ui.heading("HyperMachine");
        ui.label(
            RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                .color(colors.text_secondary),
        );
        ui.add_space(16.0);
        ui.label("A modern hypervisor and virtual machine manager");
        ui.add_space(8.0);
        ui.hyperlink_to("GitHub", "https://github.com/nervosys/AetherVM");
        ui.add_space(16.0);
        ui.label(RichText::new("2024 Nervosys").color(colors.text_secondary));
    });
    ui.add_space(16.0);
    if ui.button("Close").clicked() {
        open = false;
    }
    open
}
