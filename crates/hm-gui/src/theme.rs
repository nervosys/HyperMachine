//! Theme and styling for HyperMachine GUI

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, Style, TextStyle, Visuals};

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct AppColors {
    pub primary: Color32,
    pub primary_dark: Color32,
    pub background: Color32,
    pub surface: Color32,
    pub surface_light: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub border: Color32,
}

impl Default for AppColors {
    fn default() -> Self {
        Self {
            primary: Color32::from_rgb(0x42, 0xa5, 0xf5),
            primary_dark: Color32::from_rgb(0x1e, 0x88, 0xe5),
            background: Color32::from_rgb(0x1a, 0x1a, 0x2e),
            surface: Color32::from_rgb(0x25, 0x25, 0x3a),
            surface_light: Color32::from_rgb(0x2d, 0x2d, 0x44),
            text: Color32::from_rgb(0xea, 0xea, 0xea),
            text_secondary: Color32::from_rgb(0xa0, 0xa0, 0xa0),
            success: Color32::from_rgb(0x4c, 0xaf, 0x50),
            warning: Color32::from_rgb(0xff, 0x98, 0x00),
            error: Color32::from_rgb(0xf4, 0x43, 0x36),
            border: Color32::from_rgb(0x3a, 0x3a, 0x5a),
        }
    }
}

pub fn configure_dark_theme(ctx: &egui::Context) {
    let colors = AppColors::default();
    let mut style = Style::default();
    let mut visuals = Visuals::dark();

    visuals.panel_fill = colors.background;
    visuals.window_fill = colors.surface;
    visuals.extreme_bg_color = colors.background;
    visuals.faint_bg_color = colors.surface_light;

    visuals.widgets.noninteractive.bg_fill = colors.surface;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, colors.text);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(4);

    visuals.widgets.inactive.bg_fill = colors.surface_light;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, colors.text);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(4);

    visuals.widgets.hovered.bg_fill = colors.primary.linear_multiply(0.3);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, colors.text);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(4);

    visuals.widgets.active.bg_fill = colors.primary;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.active.corner_radius = CornerRadius::same(4);

    visuals.widgets.open.bg_fill = colors.surface_light;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, colors.text);
    visuals.widgets.open.corner_radius = CornerRadius::same(4);

    visuals.selection.bg_fill = colors.primary.linear_multiply(0.4);
    visuals.selection.stroke = Stroke::new(1.0_f32, colors.primary);
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.window_stroke = Stroke::new(1.0_f32, colors.border);
    visuals.window_shadow = egui::Shadow::NONE;
    visuals.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.5 };

    style.visuals = visuals;
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(20.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
        (
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        ),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(12);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);

    ctx.set_style(style);
}

pub mod icons {
    pub const ADD: &str = "+";
    pub const DELETE: &str = "X";
    pub const PLAY: &str = ">";
    pub const STOP: &str = "#";
    pub const PAUSE: &str = "||";
    pub const REFRESH: &str = "@";
    pub const SETTINGS: &str = "*";
    pub const INFO: &str = "i";
    pub const COMPUTER: &str = "[PC]";
    pub const CONSOLE: &str = "[>_]";
    pub const FOLDER: &str = "[D]";
    pub const CONNECTED: &str = "[+]";
    pub const DISCONNECTED: &str = "[-]";
    pub const ERROR: &str = "!";
}

pub fn primary_button(text: &str) -> egui::Button<'_> {
    let colors = AppColors::default();
    egui::Button::new(egui::RichText::new(text).color(Color32::WHITE))
        .fill(colors.primary)
        .corner_radius(CornerRadius::same(4))
}

pub fn danger_button(text: &str) -> egui::Button<'_> {
    let colors = AppColors::default();
    egui::Button::new(egui::RichText::new(text).color(Color32::WHITE))
        .fill(colors.error)
        .corner_radius(CornerRadius::same(4))
}

pub fn success_button(text: &str) -> egui::Button<'_> {
    let colors = AppColors::default();
    egui::Button::new(egui::RichText::new(text).color(Color32::WHITE))
        .fill(colors.success)
        .corner_radius(CornerRadius::same(4))
}
