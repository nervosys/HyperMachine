//! HyperMachine Desktop GUI
//!
//! A virt-manager style graphical interface for managing virtual machines.
//! Provides screen passthrough, VM management, and system monitoring.

#![warn(clippy::all, rust_2018_idioms)]

mod api;
mod app;
mod components;
mod state;
mod theme;
mod widgets;

pub use app::HyperMachineApp;
pub use state::{AppState, VmState};

fn main() -> eframe::Result<()> {
    // Initialize logging - suppress wgpu/Vulkan validation noise
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(
                    "hypermachine_gui=info"
                        .parse()
                        .expect("valid tracing directive"),
                )
                .add_directive("wgpu=off".parse().expect("valid tracing directive"))
                .add_directive("wgpu_hal=off".parse().expect("valid tracing directive"))
                .add_directive("wgpu_core=off".parse().expect("valid tracing directive"))
                .add_directive("naga=off".parse().expect("valid tracing directive")),
        )
        .init();

    tracing::info!("Starting HyperMachine GUI");

    // Configure native options
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0])
            .with_title("HyperMachine")
            .with_icon(load_icon()),
        vsync: true,
        multisampling: 4,
        hardware_acceleration: eframe::HardwareAcceleration::Preferred,
        ..Default::default()
    };

    // Run the application
    eframe::run_native(
        "HyperMachine",
        native_options,
        Box::new(|cc| Ok(Box::new(HyperMachineApp::new(cc)))),
    )
}

/// Load application icon
fn load_icon() -> egui::IconData {
    // Generate a simple colored icon (can be replaced with actual icon file)
    let size = 64;
    let mut rgba = vec![0u8; size * size * 4];

    for y in 0..size {
        for x in 0..size {
            let idx = (y * size + x) * 4;
            let dx = (x as i32 - size as i32 / 2).abs() as f32;
            let dy = (y as i32 - size as i32 / 2).abs() as f32;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist < size as f32 / 2.0 - 2.0 {
                // Blue gradient circle
                rgba[idx] = 0x2d; // R
                rgba[idx + 1] = 0x7d; // G
                rgba[idx + 2] = 0xf2; // B
                rgba[idx + 3] = 255; // A
            } else if dist < size as f32 / 2.0 {
                // Border
                rgba[idx] = 0x1a;
                rgba[idx + 1] = 0x56;
                rgba[idx + 2] = 0xb8;
                rgba[idx + 3] = 255;
            }
        }
    }

    egui::IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    }
}
