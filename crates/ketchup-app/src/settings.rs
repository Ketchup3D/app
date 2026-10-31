use super::shortcut_input::{shortcut_input, valid_tool_key};
use super::*;
use crate::inspector_table::{Column, InspectorTable};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum Section {
    #[default]
    Colors,
    Navigation,
    Shortcuts,
    Units,
    Grid,
    StockPricing,
    SurfaceTreatment,
}

impl Section {
    const ALL: [Self; 7] = [
        Self::Units,
        Self::Navigation,
        Self::Shortcuts,
        Self::Grid,
        Self::Colors,
        Self::StockPricing,
        Self::SurfaceTreatment,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Colors => "Axis and inference colors",
            Self::Navigation => "Navigation",
            Self::Shortcuts => "Shortcuts management",
            Self::Units => "Default units",
            Self::Grid => "Grid",
            Self::StockPricing => "Material",
            Self::SurfaceTreatment => "Sanding & Finishing",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Colors => "settings_colors",
            Self::Navigation => "settings_navigation",
            Self::Shortcuts => "settings_shortcuts",
            Self::Units => "units",
            Self::Grid => "settings_grid",
            Self::StockPricing => "settings_stock",
            Self::SurfaceTreatment => "time_estimates_sanding",
        }
    }

    fn button(self, ui: &mut egui::Ui, selected: bool) -> egui::Response {
        let colors = Palette::for_ui(ui);
        let color = if selected { colors.accent } else { colors.ink };
        let width = ui.available_width();
        let label = ui.painter().layout(
            self.label().to_owned(),
            egui::TextStyle::Button.resolve(ui.style()),
            color,
            (width - 44.).max(1.),
        );
        let height = (label.size().y + 16.).max(40.);
        let response = ui.add_sized([width, height], egui::Button::new("").selected(selected));
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::Role::RadioButton,
                ui.is_enabled(),
                selected,
                self.label(),
            )
        });
        if ui.is_rect_visible(response.rect) {
            icons::line_icon(
                ui.painter(),
                egui::pos2(response.rect.left() + 17., response.rect.center().y),
                self.icon(),
                color,
            );
            ui.painter().galley(
                egui::pos2(
                    response.rect.left() + 34.,
                    response.rect.center().y - label.size().y * 0.5,
                ),
                label,
                color,
            );
        }
        response
    }
}

pub(super) const TOOLS: [(Tool, &str); 16] = [
    (Tool::Select, "Space"),
    (Tool::Line, "L"),
    (Tool::Rectangle, "R"),
    (Tool::Circle, "C"),
    (Tool::Polygon, "G"),
    (Tool::Freehand, "D"),
    (Tool::Erase, "E"),
    (Tool::Orbit, "O"),
    (Tool::PushPull, "P"),
    (Tool::Scale, "S"),
    (Tool::Offset, "F"),
    (Tool::Move, "M"),
    (Tool::Paint, "B"),
    (Tool::TapeMeasure, "T"),
    (Tool::Protractor, "A"),
    (Tool::Dimensions, "I"),
];

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct GeneralSettings {
    pub section: Section,
    pub orbit_speed: f64,
    pub pan_speed: f64,
    pub zoom_speed: f64,
    pub reverse_zoom: bool,
    pub default_units: ketchup_core::UnitSettings,
    pub tool_keys: Vec<String>,
    pub stock: stock_settings::Settings,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            section: Section::default(),
            orbit_speed: 1.,
            pan_speed: 1.,
            zoom_speed: 1.,
            reverse_zoom: false,
            default_units: Default::default(),
            stock: Default::default(),
            tool_keys: TOOLS.iter().map(|(_, key)| key.to_string()).collect(),
        }
    }
}

impl GeneralSettings {
    pub(super) fn normalize(&mut self) {
        self.stock.normalize();
        for speed in [
            &mut self.orbit_speed,
            &mut self.pan_speed,
            &mut self.zoom_speed,
        ] {
            *speed = if speed.is_finite() {
                speed.clamp(0.1, 4.)
            } else {
                1.
            };
        }
        if self.tool_keys.len() != TOOLS.len()
            || self
                .tool_keys
                .iter()
                .enumerate()
                .any(|(i, key)| !valid_tool_key(key) || self.tool_keys[..i].contains(key))
        {
            self.tool_keys = Self::default().tool_keys;
        }
    }

    pub(super) fn tool_for_key(&self, key: Key) -> Option<Tool> {
        self.tool_keys
            .iter()
            .position(|name| Key::from_name(name) == Some(key))
            .map(|index| TOOLS[index].0)
    }

    pub(super) fn tool_shortcut(&self, tool: Tool) -> &str {
        TOOLS
            .iter()
            .position(|(candidate, _)| *candidate == tool)
            .map(|index| self.tool_keys[index].as_str())
            .unwrap_or(tool.shortcut())
    }

    fn assign_key(&mut self, index: usize, key: String) {
        if let Some(other) = self.tool_keys.iter().position(|value| *value == key) {
            self.tool_keys.swap(index, other);
        } else {
            self.tool_keys[index] = key;
        }
    }
}

impl KetchupApp {
    pub(super) fn axis_color(&self, axis: Axis) -> Color32 {
        let index = match axis {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        };
        let [r, g, b] = self.doc.display.axis_colors[index];
        Color32::from_rgb(r, g, b)
    }

    pub(super) fn inference_color(&self) -> Color32 {
        let [r, g, b] = self.doc.display.inference_color;
        Color32::from_rgb(r, g, b)
    }

    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        let response = Modal::new("general_settings", 950.)
            .dismissible(self.material_removal.is_none())
            .show(ctx, |modal| {
                modal.header(|ui| {
                    ui.heading("Settings");
                    ui.weak("Changes are saved automatically on this device.");
                });
                modal.body_unpadded(650., |ui| {
                    let size = ui.available_size();
                    let sidebar_width = (size.x * 0.29).clamp(100., 220.);
                    ui.horizontal_top(|ui| {
                        // Frames inherit their parent's layout. Give both columns an
                        // explicit vertical layout instead of inheriting this row.
                        ui.allocate_ui_with_layout(
                            egui::vec2(sidebar_width, size.y),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                egui::Frame::new()
                                    .fill(Palette::for_ui(ui).canvas)
                                    .inner_margin(14)
                                    .show(ui, |ui| {
                                        ui.set_width((sidebar_width - 28.).max(1.));
                                        ui.set_min_height((size.y - 28.).max(24.));
                                        ui.spacing_mut().item_spacing.y = 8.;
                                        for section in Section::ALL {
                                            if section
                                                .button(
                                                    ui,
                                                    self.general_settings.section == section,
                                                )
                                                .clicked()
                                            {
                                                self.general_settings.section = section;
                                            }
                                        }
                                    });
                            },
                        );
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), size.y),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                egui::Frame::new().inner_margin(20).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.spacing_mut().item_spacing = egui::vec2(8., 12.);
                                    let key = match self.general_settings.section {
                                        Section::Colors => "settings/colors",
                                        Section::Navigation => "settings/navigation",
                                        Section::Shortcuts => "settings/shortcuts",
                                        Section::Units => "settings/units",
                                        Section::Grid => "settings/grid",
                                        Section::StockPricing => "settings/stock",
                                        Section::SurfaceTreatment => "settings/treatment",
                                    };
                                    let scroll = sidebar::restored_scroll_area(
                                        ui,
                                        key,
                                        egui::ScrollArea::vertical()
                                            .id_salt(key)
                                            .auto_shrink([false, false]),
                                    );
                                    let output = scroll.show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            let (rect, _) = ui.allocate_exact_size(
                                                egui::vec2(22., 22.),
                                                Sense::hover(),
                                            );
                                            icons::line_icon(
                                                ui.painter(),
                                                rect.center(),
                                                self.general_settings.section.icon(),
                                                Palette::for_ui(ui).ink,
                                            );
                                            ui.heading(self.general_settings.section.label());
                                        });
                                        ui.add_space(8.);
                                        self.settings_contents(ui);
                                    });
                                    sidebar::remember_scroll(
                                        ui.ctx(),
                                        key,
                                        output.state.clamped_offset(),
                                    );
                                });
                            },
                        );
                    });
                });
                modal.footer(|ui| {
                    if primary_button(ui, "Close", true).clicked() {
                        ui.close();
                    }
                });
            });
        if response.should_close() {
            self.settings_open = false;
        }
    }

    fn settings_contents(&mut self, ui: &mut egui::Ui) {
        match self.general_settings.section {
            Section::SurfaceTreatment => {
                ui.weak("Defaults for objects in all projects. Objects use these values until customized in Entity Info.");
                time_estimates::treatment_controls(
                    ui,
                    &mut self.time_estimates,
                    true,
                    &self.general_settings.stock.finishes,
                    &mut self.finish_product_filters,
                );
            }
            Section::StockPricing => {
                if let Some(id) = self.general_settings.stock.show(ui) {
                    self.material_removal = Some(id);
                }
            }
            Section::Colors => {
                let d = &mut self.doc.display;
                color_row(ui, "X axis", &mut d.axis_colors[0]);
                color_row(ui, "Y axis", &mut d.axis_colors[1]);
                color_row(ui, "Z axis", &mut d.axis_colors[2]);
                color_row(ui, "Inference points", &mut d.inference_color);
                color_row(ui, "Midpoints", &mut d.midpoint_color);
                ui.weak("Axis inferences use the corresponding axis color.");
                if ui.button("Restore default colors").clicked() {
                    let defaults = ketchup_core::DisplaySettings::default();
                    d.axis_colors = defaults.axis_colors;
                    d.inference_color = defaults.inference_color;
                    d.midpoint_color = defaults.midpoint_color;
                }
            }
            Section::Navigation => {
                ui.weak("Adjust mouse and trackpad navigation sensitivity.");
                for (label, value) in [
                    ("Orbit speed", &mut self.general_settings.orbit_speed),
                    ("Pan speed", &mut self.general_settings.pan_speed),
                    ("Zoom speed", &mut self.general_settings.zoom_speed),
                ] {
                    ui.add(egui::Slider::new(value, 0.1..=4.).text(label).suffix("×"));
                }
                ui.checkbox(
                    &mut self.general_settings.reverse_zoom,
                    "Reverse scroll zoom",
                );
            }
            Section::Shortcuts => {
                ui.weak("Click a shortcut, then press a letter or Space. Escape cancels. Reusing a key swaps the assignments. Project commands and navigation gestures keep their standard shortcuts.");
                ui.add(
                    egui::TextEdit::singleline(&mut self.shortcut_filter)
                        .desired_width(ui.available_width())
                        .hint_text("Search tools…"),
                );
                let query = self.shortcut_filter.to_lowercase();
                let mut table = InspectorTable::new(
                    ui.available_width(),
                    [Column::flex(0.55), Column::flex(0.45)],
                )
                .striped();
                table.header(ui, ["Tool", "Shortcut"]);
                let mut matches = 0;
                for (index, (tool, _)) in TOOLS.iter().enumerate() {
                    if !tool.label().to_lowercase().contains(&query) {
                        continue;
                    }
                    matches += 1;
                    table.row(ui, ("settings_tool", index), |column, ui| {
                        if column == 0 {
                            ui.label(tool.label());
                        } else if let Some(key) = shortcut_input(
                            ui,
                            ("settings_tool_key", index),
                            &self.general_settings.tool_keys[index],
                            ui.available_width(),
                        ) {
                            self.general_settings.assign_key(index, key);
                        }
                    });
                }
                if matches == 0 {
                    ui.weak("No tools match your search.");
                }
                if ui.button("Restore default shortcuts").clicked() {
                    self.general_settings.tool_keys = GeneralSettings::default().tool_keys;
                }
            }
            Section::Units => {
                ui.weak("Used for new projects. Change the current project's units in the Units inspector.");
                Self::unit_settings_contents(ui, &mut self.general_settings.default_units);
            }
            Section::Grid => {
                ui.checkbox(&mut self.doc.display.grid, "Show grid");
                ui.checkbox(&mut self.snap_to_grid, "Snap to grid");
                ui.checkbox(
                    &mut self.doc.display.grid_auto_size,
                    "Automatically size grid when zooming",
                );
                let unit = self.doc.display.units.length;
                ui.add_enabled_ui(!self.doc.display.grid_auto_size, |ui| {
                    let mut spacing = self.doc.display.grid_spacing / unit.factor();
                    ui.horizontal(|ui| {
                        ui.label("Grid spacing");
                        if ui
                            .add(
                                egui::DragValue::new(&mut spacing)
                                    .range((0.000001 / unit.factor())..=(1e6 / unit.factor()))
                                    .speed(0.1)
                                    .suffix(format!(" {}", unit.symbol())),
                            )
                            .changed()
                        {
                            self.doc.display.grid_spacing = spacing * unit.factor();
                        }
                    });
                });
                ui.weak("Spacing controls both grid lines and grid snapping. Every fifth line is emphasized.");
            }
        }
    }
}

fn color_row(ui: &mut egui::Ui, label: &str, color: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.color_edit_button_srgb(color);
        ui.label(label);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reassigning_shortcuts_swaps_conflicts_and_repairs_invalid_saved_values() {
        let mut settings = GeneralSettings::default();
        settings.assign_key(1, "R".into());
        assert_eq!(settings.tool_for_key(Key::R), Some(Tool::Line));
        assert_eq!(settings.tool_for_key(Key::L), Some(Tool::Rectangle));
        settings.normalize();
        assert_eq!(settings.tool_for_key(Key::R), Some(Tool::Line));
        settings.tool_keys[0] = "Escape".into();
        settings.orbit_speed = f64::NAN;
        settings.normalize();
        assert_eq!(settings.tool_keys, GeneralSettings::default().tool_keys);
        assert_eq!(settings.orbit_speed, 1.);
        let old: GeneralSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old, GeneralSettings::default());
    }

    #[test]
    fn every_settings_section_renders_in_the_shared_modal() {
        for section in Section::ALL {
            let ctx = egui::Context::default();
            let mut app = KetchupApp::with_document(Document::default(), None, true);
            app.settings_open = true;
            app.general_settings.section = section;
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1100., 800.))),
                    ..Default::default()
                },
                |_| app.settings_window(&ctx),
            );
            assert!(app.settings_open);
        }
    }
}
