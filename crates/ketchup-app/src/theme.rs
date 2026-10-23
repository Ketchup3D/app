use eframe::egui::{self, Color32, FontFamily, FontId, Stroke, TextStyle};

pub const INK: Color32 = Color32::from_rgb(39, 53, 46);
pub const MUTED: Color32 = Color32::from_rgb(122, 132, 123);
pub const ACCENT: Color32 = Color32::from_rgb(217, 72, 54);
pub const BORDER: Color32 = Color32::from_rgb(225, 228, 220);
pub const PAPER: Color32 = Color32::from_rgb(250, 251, 247);
pub const CANVAS: Color32 = Color32::from_rgb(240, 242, 234);
pub const GREEN: Color32 = Color32::from_rgb(78, 120, 91);

pub fn apply(ctx: &egui::Context) {
    let mut style = egui::Style::default();
    let mut visuals = egui::Visuals::light();
    visuals.override_text_color = Some(INK);
    visuals.panel_fill = PAPER;
    visuals.window_fill = PAPER;
    visuals.faint_bg_color = Color32::from_rgb(241, 243, 236);
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.selection.bg_fill = Color32::from_rgb(249, 224, 214);
    visuals.selection.stroke = Stroke::new(1., ACCENT);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1., BORDER);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(241, 243, 236);
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1., BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1., INK);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(232, 236, 225);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(232, 236, 225);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1., Color32::from_rgb(189, 200, 184));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1., INK);
    visuals.widgets.active.bg_fill = Color32::from_rgb(248, 225, 216);
    visuals.widgets.active.bg_stroke = Stroke::new(1., ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1., INK);
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8., 8.);
    style.spacing.button_padding = egui::vec2(10., 7.);
    style.spacing.interact_size = egui::vec2(32., 30.);
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(13., FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(13., FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Small, FontId::new(11., FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(20., FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12., FontFamily::Monospace),
    );
    ctx.set_global_style(style);
}
