//! Reusable UI widgets

use crate::state::VmState;
use crate::theme::{danger_button, icons, primary_button, success_button, AppColors};
use egui::{Color32, CornerRadius, RichText, Ui};

pub fn vm_list_item(ui: &mut Ui, vm: &VmState, selected: bool) -> VmAction {
    let mut action = VmAction::None;
    let colors = AppColors::default();
    let bg_color = if selected {
        colors.primary.linear_multiply(0.2)
    } else {
        Color32::TRANSPARENT
    };

    egui::Frame::new()
        .fill(bg_color)
        .inner_margin(8.0)
        .outer_margin(2.0)
        .corner_radius(CornerRadius::same(4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(icons::COMPUTER)
                        .size(24.0)
                        .color(vm.state_color()),
                );
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&vm.name).strong().size(16.0));
                        ui.add_space(8.0);
                        let badge_color = vm.state_color();
                        egui::Frame::new()
                            .fill(badge_color.linear_multiply(0.2))
                            .inner_margin(egui::vec2(6.0, 2.0))
                            .corner_radius(CornerRadius::same(3))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(vm.state.to_string())
                                        .color(badge_color)
                                        .small(),
                                );
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{} CPU, {} MB", vm.cpus, vm.memory_mb))
                                .small()
                                .color(colors.text_secondary),
                        );
                        if let Some(ip) = &vm.ip_address {
                            ui.label(RichText::new("|").small().color(colors.text_secondary));
                            ui.label(RichText::new(ip).small().color(colors.text_secondary));
                        }
                    });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if vm.operation_pending.is_some() {
                        ui.spinner();
                    } else {
                        if vm.state == crate::api::VmStateApi::Running
                            && ui
                                .button(RichText::new(icons::CONSOLE).size(18.0))
                                .clicked()
                        {
                            action = VmAction::OpenConsole;
                        }
                        if vm.can_start() {
                            if ui.add(success_button(icons::PLAY)).clicked() {
                                action = VmAction::Start;
                            }
                        } else if vm.can_stop() && ui.add(danger_button(icons::STOP)).clicked() {
                            action = VmAction::Stop;
                        }
                        if vm.can_pause() && ui.button(icons::PAUSE).clicked() {
                            action = VmAction::Pause;
                        }
                    }
                });
            });
        });

    let response = ui.interact(ui.min_rect(), ui.id().with(&vm.id), egui::Sense::click());
    if response.clicked() && action == VmAction::None {
        action = VmAction::Select;
    }
    action
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum VmAction {
    None,
    Select,
    Start,
    Stop,
    Pause,
    Resume,
    Delete,
    OpenConsole,
}

pub fn vm_details_panel(ui: &mut Ui, vm: &VmState) -> VmAction {
    let mut action = VmAction::None;
    let colors = AppColors::default();

    ui.horizontal(|ui| {
        ui.heading(&vm.name);
        ui.add_space(8.0);
        let badge_color = vm.state_color();
        egui::Frame::new()
            .fill(badge_color.linear_multiply(0.2))
            .inner_margin(egui::vec2(8.0, 4.0))
            .corner_radius(CornerRadius::same(4))
            .show(ui, |ui| {
                ui.label(RichText::new(vm.state.to_string()).color(badge_color));
            });
    });

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        if vm.operation_pending.is_some() {
            ui.spinner();
            if let Some(op) = &vm.operation_pending {
                ui.label(format!("{}...", op));
            }
        } else {
            if vm.can_start()
                && ui
                    .add(success_button(&format!("{} Start", icons::PLAY)))
                    .clicked()
            {
                action = VmAction::Start;
            }
            if vm.can_pause() && ui.button(format!("{} Pause", icons::PAUSE)).clicked() {
                action = VmAction::Pause;
            }
            if vm.can_stop()
                && ui
                    .add(danger_button(&format!("{} Stop", icons::STOP)))
                    .clicked()
            {
                action = VmAction::Stop;
            }
            if vm.state == crate::api::VmStateApi::Running
                && ui
                    .add(primary_button(&format!("{} Console", icons::CONSOLE)))
                    .clicked()
            {
                action = VmAction::OpenConsole;
            }
            ui.add_space(16.0);
            if vm.can_delete()
                && ui
                    .add(danger_button(&format!("{} Delete", icons::DELETE)))
                    .clicked()
            {
                action = VmAction::Delete;
            }
        }
    });

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);

    egui::Grid::new("vm_details_grid")
        .num_columns(2)
        .spacing([20.0, 8.0])
        .show(ui, |ui| {
            ui.label(RichText::new("ID:").color(colors.text_secondary));
            ui.label(&vm.id);
            ui.end_row();
            ui.label(RichText::new("CPUs:").color(colors.text_secondary));
            ui.label(format!("{}", vm.cpus));
            ui.end_row();
            ui.label(RichText::new("Memory:").color(colors.text_secondary));
            ui.label(format!("{} MB", vm.memory_mb));
            ui.end_row();
            if let Some(disk) = &vm.disk_path {
                ui.label(RichText::new("Disk:").color(colors.text_secondary));
                ui.label(disk);
                ui.end_row();
            }
            if let Some(ip) = &vm.ip_address {
                ui.label(RichText::new("IP:").color(colors.text_secondary));
                ui.label(ip);
                ui.end_row();
            }
            ui.label(RichText::new("Created:").color(colors.text_secondary));
            ui.label(&vm.created_at);
            ui.end_row();
            if let Some(started) = &vm.started_at {
                ui.label(RichText::new("Started:").color(colors.text_secondary));
                ui.label(started);
                ui.end_row();
            }
        });

    action
}

pub fn create_vm_dialog(
    ui: &mut Ui,
    form: &mut crate::state::CreateVmForm,
) -> Option<crate::api::VmConfig> {
    let mut result = None;
    ui.heading("Create New VM");
    ui.add_space(16.0);

    if let Some(error) = &form.error {
        egui::Frame::new()
            .fill(AppColors::default().error.linear_multiply(0.2))
            .inner_margin(8.0)
            .corner_radius(CornerRadius::same(4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icons::ERROR).color(AppColors::default().error));
                    ui.label(RichText::new(error).color(AppColors::default().error));
                });
            });
        ui.add_space(8.0);
    }

    egui::Grid::new("create_vm_form")
        .num_columns(2)
        .spacing([10.0, 10.0])
        .show(ui, |ui| {
            ui.label("Name:");
            ui.text_edit_singleline(&mut form.name);
            ui.end_row();
            ui.label("CPUs:");
            ui.add(egui::DragValue::new(&mut form.cpus).range(1..=64));
            ui.end_row();
            ui.label("Memory (MB):");
            ui.add(egui::DragValue::new(&mut form.memory_mb).range(128..=262144));
            ui.end_row();
            ui.label("Disk Path:");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut form.disk_path);
                if ui.button(icons::FOLDER).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Disk Images", &["qcow2", "img", "raw", "vhd", "vhdx"])
                        .pick_file()
                    {
                        form.disk_path = path.display().to_string();
                    }
                }
            });
            ui.end_row();
            ui.label("Boot Image:");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut form.boot_image);
                if ui.button(icons::FOLDER).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ISO Images", &["iso"])
                        .pick_file()
                    {
                        form.boot_image = path.display().to_string();
                    }
                }
            });
            ui.end_row();
            ui.label("Network:");
            ui.checkbox(&mut form.network_enabled, "Enable");
            ui.end_row();
        });

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        let create_enabled = !form.creating && !form.name.is_empty();
        if form.creating {
            ui.spinner();
            ui.label("Creating...");
        } else {
            if ui
                .add_enabled(create_enabled, primary_button("Create"))
                .clicked()
            {
                match form.validate() {
                    Ok(()) => {
                        result = Some(crate::api::VmConfig {
                            name: form.name.clone(),
                            cpus: form.cpus,
                            memory_mb: form.memory_mb,
                            disk_path: if form.disk_path.is_empty() {
                                None
                            } else {
                                Some(form.disk_path.clone())
                            },
                            network_enabled: form.network_enabled,
                            boot_image: if form.boot_image.is_empty() {
                                None
                            } else {
                                Some(form.boot_image.clone())
                            },
                            metadata: std::collections::HashMap::new(),
                        });
                    }
                    Err(e) => {
                        form.error = Some(e);
                    }
                }
            }
            if ui.button("Cancel").clicked() {
                form.cancelled = true;
            }
        }
    });
    result
}

pub fn connection_status(ui: &mut Ui, connected: bool, url: &str) {
    let (icon, color, text) = if connected {
        (icons::CONNECTED, AppColors::default().success, "Connected")
    } else {
        (
            icons::DISCONNECTED,
            AppColors::default().error,
            "Disconnected",
        )
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).color(color));
        ui.label(RichText::new(text).color(color).small());
        ui.label(
            RichText::new(format!("({})", url))
                .color(AppColors::default().text_secondary)
                .small(),
        );
    });
}

pub fn stats_bar(ui: &mut Ui, counts: &crate::state::VmCounts) {
    let colors = AppColors::default();
    ui.horizontal(|ui| {
        ui.label(format!("{} VMs", counts.total));
        ui.separator();
        ui.label(RichText::new(format!("{} Running", counts.running)).color(colors.success));
        ui.separator();
        if counts.paused > 0 {
            ui.label(RichText::new(format!("{} Paused", counts.paused)).color(colors.warning));
            ui.separator();
        }
        if counts.error > 0 {
            ui.label(RichText::new(format!("{} Error", counts.error)).color(colors.error));
            ui.separator();
        }
        ui.label(RichText::new(format!("{} Stopped", counts.stopped)).color(colors.text_secondary));
    });
}
