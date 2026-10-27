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
    pub accent: Color32,
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

    pub fn for_painter(p: &egui::Painter) -> Self {
        Self::new(p.ctx().global_style().visuals.dark_mode)
    }

    fn new(dark: bool) -> Self {
        if dark {
            Self {
                ink: Color32::from_rgb(229, 234, 226),
                accent: Color32::from_rgb(246, 142, 125),
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
                accent: ACCENT,
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

/// Grid lines stay visible against each theme's environment without competing
/// with model edges.
#[derive(Clone, Copy)]
pub struct GridPalette {
    pub major: Color32,
    pub minor: Color32,
}

impl GridPalette {
    pub fn for_ui(ui: &egui::Ui) -> Self {
        if ui.visuals().dark_mode {
            Self {
                major: Color32::from_rgb(83, 96, 83),
                minor: Color32::from_rgb(61, 72, 61),
            }
        } else {
            Self {
                major: Color32::from_rgb(161, 168, 148),
                minor: Color32::from_rgb(188, 194, 175),
            }
        }
    }
}

/// Theme defaults for model faces and their style previews. User paint and tag
/// colors stay independent of the theme.
#[derive(Clone, Copy)]
pub struct FacePalette {
    pub front: Color32,
    pub side: Color32,
    pub shaded_side: Color32,
    pub back: Color32,
    pub inactive: Color32,
    pub selected: Color32,
    pub hovered: Color32,
    pub paste: Color32,
    pub section: Color32,
    pub materials: [Color32; 3],
}

impl FacePalette {
    pub fn for_ui(ui: &egui::Ui) -> Self {
        Self::new(ui.visuals().dark_mode)
    }

    fn new(dark: bool) -> Self {
        if dark {
            Self {
                front: Color32::from_rgb(112, 128, 116),
                side: Color32::from_rgb(84, 101, 88),
                shaded_side: Color32::from_rgb(70, 88, 76),
                back: Color32::from_rgb(77, 100, 119),
                inactive: Color32::from_rgb(52, 62, 54),
                selected: Color32::from_rgb(136, 78, 57),
                hovered: Color32::from_rgb(78, 110, 76),
                paste: Color32::from_rgb(86, 117, 77),
                section: Color32::from_rgb(124, 79, 61),
                materials: [
                    Color32::from_rgb(162, 124, 68),
                    Color32::from_rgb(129, 71, 54),
                    Color32::from_rgb(63, 105, 91),
                ],
            }
        } else {
            Self {
                front: Color32::from_rgb(244, 247, 239),
                side: Color32::from_rgb(210, 218, 207),
                shaded_side: Color32::from_rgb(169, 183, 168),
                back: Color32::from_rgb(150, 172, 185),
                inactive: Color32::from_rgb(231, 234, 225),
                selected: Color32::from_rgb(244, 202, 181),
                hovered: Color32::from_rgb(208, 227, 202),
                paste: Color32::from_rgb(218, 232, 207),
                section: Color32::from_rgb(183, 133, 111),
                materials: [
                    Color32::from_rgb(233, 192, 128),
                    Color32::from_rgb(191, 108, 83),
                    Color32::from_rgb(106, 153, 139),
                ],
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
    style.interaction.tooltip_delay = 1.0;
    // Each newly hovered tool should wait, even after another tooltip was shown.
    style.interaction.tooltip_grace_time = 0.0;
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
    visuals.selection.stroke = Stroke::new(1., colors.accent);
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
    visuals.widgets.active.bg_stroke = Stroke::new(1., colors.accent);
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
