use eframe::egui::{self, Color32, FontFamily, FontId, Stroke, TextStyle};

pub const INK: Color32 = Color32::from_rgb(39, 53, 46);
pub const MUTED: Color32 = Color32::from_rgb(122, 132, 123);
pub const ACCENT: Color32 = Color32::from_rgb(217, 72, 54);
pub const BORDER: Color32 = Color32::from_rgb(225, 228, 220);
pub const PAPER: Color32 = Color32::from_rgb(250, 251, 247);
pub const CANVAS: Color32 = Color32::from_rgb(240, 242, 234);
pub const GREEN: Color32 = Color32::from_rgb(78, 120, 91);

const PREFERENCE_KEY: &str = "ketchup.theme.v1";

// UI colors are independent of the colors used for model geometry and inference.
#[derive(Clone, Copy)]
pub struct Palette {
    pub ink: Color32,
    pub muted: Color32,
    pub border: Color32,
    pub paper: Color32,
    pub canvas: Color32,
    pub surface: Color32,
    pub green: Color32,
    pub hovered: Color32,
    pub selected: Color32,
}

impl Palette {
    pub fn for_ui(ui: &egui::Ui) -> Self {
        Self::new(ui.visuals().dark_mode)
    }

    fn new(dark: bool) -> Self {
        if dark {
            Self {
                ink: Color32::from_rgb(229, 234, 226),
                muted: Color32::from_rgb(163, 175, 163),
                border: Color32::from_rgb(65, 77, 67),
                paper: Color32::from_rgb(30, 37, 32),
                canvas: Color32::from_rgb(39, 47, 41),
                surface: Color32::from_rgb(35, 43, 37),
                green: Color32::from_rgb(143, 187, 151),
                hovered: Color32::from_rgb(51, 64, 53),
                selected: Color32::from_rgb(83, 49, 41),
            }
        } else {
            Self {
                ink: INK,
                muted: MUTED,
                border: BORDER,
                paper: PAPER,
                canvas: CANVAS,
                surface: Color32::WHITE,
                green: GREEN,
                hovered: Color32::from_rgb(235, 239, 229),
                selected: Color32::from_rgb(248, 229, 218),
            }
        }
    }
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_style_of(egui::Theme::Light, style(false));
    ctx.set_style_of(egui::Theme::Dark, style(true));
    ctx.options_mut(|options| options.fallback_theme = egui::Theme::Light);
    ctx.set_theme(egui::ThemePreference::System);
}

pub fn restore_preference(ctx: &egui::Context, storage: Option<&dyn eframe::Storage>) {
    let saved = storage.and_then(|storage| storage.get_string(PREFERENCE_KEY));
    let preference = match saved.as_deref() {
        Some("light") => egui::ThemePreference::Light,
        Some("dark") => egui::ThemePreference::Dark,
        _ => egui::ThemePreference::System,
    };
    ctx.set_theme(preference);
}

pub fn save_preference(ctx: &egui::Context, storage: &mut dyn eframe::Storage) {
    let preference = ctx.options(|options| options.theme_preference);
    storage.set_string(PREFERENCE_KEY, preference.name().to_owned());
    storage.flush();
}

fn style(dark: bool) -> egui::Style {
    let colors = Palette::new(dark);
    let mut style = egui::Style::default();
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.override_text_color = Some(colors.ink);
    visuals.panel_fill = colors.paper;
    visuals.window_fill = colors.paper;
    visuals.faint_bg_color = colors.canvas;
    visuals.extreme_bg_color = colors.surface;
    visuals.selection.bg_fill = colors.selected;
    visuals.selection.stroke = Stroke::new(1., ACCENT);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1., colors.border);
    visuals.widgets.inactive.bg_fill = colors.canvas;
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1., colors.border);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1., colors.ink);
    visuals.widgets.hovered.bg_fill = colors.hovered;
    visuals.widgets.hovered.weak_bg_fill = colors.hovered;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1., colors.muted);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1., colors.ink);
    visuals.widgets.active.bg_fill = colors.selected;
    visuals.widgets.active.weak_bg_fill = colors.selected;
    visuals.widgets.active.bg_stroke = Stroke::new(1., ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1., colors.ink);
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
    style
}
