use super::shortcut_input::{shortcut_input, valid_tool_key};
use super::*;
use crate::inspector_table::{Column, InspectorTable, inspector_field};
#[cfg(test)]
use crate::test_ui::ContextExt as _;
use ketchup_core::LengthUnit;

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
    Diagnostics,
    Rendering,
}

impl Section {
    const GROUPS: [(&'static str, &'static [Self]); 4] = [
        ("Workspace", &[Self::Units, Self::Grid, Self::Colors]),
        ("Controls", &[Self::Navigation, Self::Shortcuts]),
        (
            "Materials & estimates",
            &[Self::StockPricing, Self::SurfaceTreatment],
        ),
        ("System", &[Self::Diagnostics, Self::Rendering]),
    ];
    #[cfg(test)]
    const ALL: [Self; 9] = [
        Self::Units,
        Self::Navigation,
        Self::Shortcuts,
        Self::Grid,
        Self::Colors,
        Self::StockPricing,
        Self::SurfaceTreatment,
        Self::Diagnostics,
        Self::Rendering,
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
            Self::Diagnostics => "Diagnostics",
            Self::Rendering => "Rendering",
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
            Self::Diagnostics => "diagnostics",
            Self::Rendering => "settings_colors",
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

pub(super) const TOOLS: [(Tool, &str); 17] = [
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
    // Append to preserve the indices of saved custom shortcuts.
    (Tool::Rotate, "Q"),
];

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct GeneralSettings {
    pub section: Section,
    pub orbit_speed: f64,
    pub pan_speed: f64,
    pub zoom_speed: f64,
    pub reverse_zoom: bool,
    pub show_fps: bool,
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
            show_fps: false,
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
        // Migrate older shortcut lists without discarding custom assignments.
        if self.tool_keys.len() + 1 == TOOLS.len() {
            let key = std::iter::once("Q")
                .chain(["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "R", "S", "T", "U", "V", "W", "X", "Y", "Z"])
                .find(|key| !self.tool_keys.iter().any(|saved| saved.as_str() == *key))
                .unwrap();
            self.tool_keys.push(key.into());
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
            .resizable(true)
            .dismissible(self.material_removal.is_none())
            .show(ctx, |modal| {
                modal.header(|ui| {
                    ui.text_heading("Settings");
                    ui.text_weak("Changes are saved automatically on this device.");
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
                                        for (index, (title, sections)) in
                                            Section::GROUPS.iter().enumerate()
                                        {
                                            if index > 0 {
                                                ui.add_space(12.);
                                            }
                                            ui.text_label(
                                                RichText::new(*title)
                                                    .small()
                                                    .strong()
                                                    .color(Palette::for_ui(ui).muted),
                                            );
                                            for &section in *sections {
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
                                        }
                                    });
                            },
                        );
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), size.y),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                let key = match self.general_settings.section {
                                    Section::Colors => "settings/colors",
                                    Section::Navigation => "settings/navigation",
                                    Section::Shortcuts => "settings/shortcuts",
                                    Section::Units => "settings/units",
                                    Section::Grid => "settings/grid",
                                    Section::StockPricing => "settings/stock",
                                    Section::SurfaceTreatment => "settings/treatment",
                                    Section::Diagnostics => "settings/diagnostics",
                                    Section::Rendering => "settings/rendering",
                                };
                                let scroll = sidebar::restored_scroll_area(
                                    ui,
                                    key,
                                    egui::ScrollArea::vertical()
                                        .id_salt(key)
                                        .auto_shrink([false, false]),
                                );
                                let output = scroll.show(ui, |ui| {
                                    // Keep padding inside the viewport so the scrollbar
                                    // stays against the settings panel edge.
                                    egui::Frame::new().inner_margin(20).show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.spacing_mut().item_spacing = egui::vec2(8., 12.);
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
                                            ui.text_heading(self.general_settings.section.label());
                                        });
                                        ui.add_space(8.);
                                        self.settings_contents(ui);
                                    });
                                });
                                sidebar::remember_scroll(
                                    ui.ctx(),
                                    key,
                                    output.state.clamped_offset(),
                                );
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
            Section::Rendering => {
                ui.checkbox(&mut self.doc.display.shadow_antialiasing, "Shadow antialiasing")
                    .on_hover_text("Smooth shadow boundaries with 4× multisampling. Only shadows are affected. Changes apply immediately.");
            }
            Section::Diagnostics => {
                ui.text_weak("Tools for checking performance and troubleshooting.");
                ui.checkbox(&mut self.general_settings.show_fps, "Show frames per second")
                    .on_hover_text("Display the rendering frame rate in the lower-left corner of each viewport. Enables continuous rendering while visible.");
            }
            Section::SurfaceTreatment => {
                ui.text_weak("Defaults for objects in all projects. Objects use these values until customized in Entity Info.");
                time_estimates::treatment_controls(
                    ui,
                    &mut self.time_estimates,
                    true,
                    &self.general_settings.stock.finishes,
                    &mut self.finish_product_filters,
                );
            }
            Section::StockPricing => {
                if let Some(id) = self
                    .general_settings
                    .stock
                    .show(ui, &mut self.wood_type_filters)
                {
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
                ui.text_weak("Axis inferences use the corresponding axis color.");
                if ui.button("Restore default colors").clicked() {
                    let defaults = ketchup_core::DisplaySettings::default();
                    d.axis_colors = defaults.axis_colors;
                    d.inference_color = defaults.inference_color;
                    d.midpoint_color = defaults.midpoint_color;
                }
            }
            Section::Navigation => {
                ui.text_weak("Adjust mouse and trackpad navigation sensitivity.");
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
                ui.text_weak("Click a shortcut, then press a letter or Space. Escape cancels. Reusing a key swaps the assignments. Project commands and navigation gestures keep their standard shortcuts.");
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
                .centered()
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
                            ui.text_label(tool.label());
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
                    ui.text_weak("No tools match your search.");
                }
                if ui.button("Restore default shortcuts").clicked() {
                    self.general_settings.tool_keys = GeneralSettings::default().tool_keys;
                }
            }
            Section::Units => {
                ui.text_weak("Used for new projects. Change the current project's units in the Units inspector.");
                Self::unit_settings_contents(ui, &mut self.general_settings.default_units);
            }
            Section::Grid => {
                inspector_field(ui, "settings/grid/show", "Show grid", |ui| {
                    ui.checkbox(&mut self.doc.display.grid, "")
                });
                inspector_field(ui, "settings/grid/snap", "Snap to grid", |ui| {
                    ui.checkbox(&mut self.snap_to_grid, "")
                });
                inspector_field(ui, "settings/grid/infinite", "Infinite grid", |ui| {
                    ui.checkbox(&mut self.doc.display.grid_infinite, "")
                });
                let unit = self.doc.display.units.length;
                if !self.doc.display.grid_infinite {
                    grid_length_control(
                        ui,
                        "settings/grid/max_length",
                        "Maximum length",
                        &mut self.doc.display.grid_max_length,
                        unit,
                    );
                    ui.text_weak("Length of each side of the square, centered on the model origin. The boundary stays fixed when zooming or rotating.");
                }
                inspector_field(
                    ui,
                    "settings/grid/auto_size",
                    "Automatically size grid when zooming",
                    |ui| ui.checkbox(&mut self.doc.display.grid_auto_size, ""),
                );
                ui.add_enabled_ui(!self.doc.display.grid_auto_size, |ui| {
                    grid_length_control(
                        ui,
                        "settings/grid/spacing",
                        "Grid spacing",
                        &mut self.doc.display.grid_spacing,
                        unit,
                    );
                });
                ui.text_weak("Spacing controls both grid lines and grid snapping. Every fifth line is emphasized.");
            }
        }
    }
}

fn grid_length_control(
    ui: &mut egui::Ui,
    key: &'static str,
    label: &str,
    meters: &mut f64,
    unit: LengthUnit,
) -> egui::Response {
    let mut value = *meters / unit.factor();
    let response = inspector_field(ui, key, label, |ui| {
        ui.add_sized(
            [ui.available_width(), 0.],
            egui::DragValue::new(&mut value)
                .range((0.000001 / unit.factor())..=(1e6 / unit.factor()))
                .speed(0.1)
                .suffix(format!(" {}", unit.symbol())),
        )
    });
    if response.changed() && value.is_finite() {
        *meters = (value * unit.factor()).clamp(0.000001, 1e6);
    }
    response
}

fn color_row(ui: &mut egui::Ui, label: &str, color: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.color_edit_button_srgb(color);
        ui.text_label(label);
    });
}

#[cfg(test)]
mod tests {
    fn rendered_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        fn collect(shape: &egui::Shape, text: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(value) => text.push(value.galley.job.text.clone()),
                egui::Shape::Vec(values) => {
                    for value in values {
                        collect(value, text);
                    }
                }
                _ => {}
            }
        }
        let mut text = Vec::new();
        for shape in shapes {
            collect(&shape.shape, &mut text);
        }
        text
    }

    #[test]
    fn grid_length_field_converts_project_units_on_display_and_input() {
        fn frame(
            ctx: &egui::Context,
            time: &mut f64,
            length: &mut f64,
            unit: LengthUnit,
            events: Vec<egui::Event>,
        ) -> (Rect, Vec<String>) {
            *time += 0.02;
            let mut rect = Rect::NOTHING;
            let mut output = ctx.run_test_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800., 300.))),
                    time: Some(*time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    rect =
                        grid_length_control(ui, "test_grid_length", "Maximum length", length, unit)
                            .rect;
                },
            );
            let text = rendered_text(&output.shapes);
            output.textures_delta.clear();
            (rect, text)
        }
        for unit in [
            LengthUnit::Meters,
            LengthUnit::Feet,
            LengthUnit::Inches,
            LengthUnit::Millimeters,
        ] {
            let ctx = egui::Context::default();
            let mut time = 0.;
            let mut length = 3.048;
            let (rect, text) = frame(&ctx, &mut time, &mut length, unit, vec![]);
            assert!(
                text.iter()
                    .any(|s| s.ends_with(&format!(" {}", unit.symbol())))
            );
            assert_eq!(
                length, 3.048,
                "Changing display units changed the physical length"
            );
            let pos = rect.center();
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut time,
                    &mut length,
                    unit,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            let command = egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            };
            frame(
                &ctx,
                &mut time,
                &mut length,
                unit,
                vec![
                    egui::Event::ModifiersChanged(command),
                    egui::Event::Key {
                        key: Key::A,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: command,
                    },
                    egui::Event::Text("12".into()),
                ],
            );
            frame(
                &ctx,
                &mut time,
                &mut length,
                unit,
                vec![egui::Event::Key {
                    key: Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert!(
                (length - 12. * unit.factor()).abs() < 1e-9,
                "Input did not convert {:?}: {length}",
                unit
            );
            let physical_length = length;
            for changed_unit in [
                LengthUnit::Meters,
                LengthUnit::Feet,
                LengthUnit::Millimeters,
            ] {
                frame(&ctx, &mut time, &mut length, changed_unit, vec![]);
                assert_eq!(length, physical_length);
            }
        }
    }

    #[test]
    fn finite_grid_length_control_only_appears_when_infinite_is_disabled() {
        for infinite in [true, false] {
            let ctx = egui::Context::default();
            let mut app = KetchupApp::with_document(Document::default(), None, true);
            app.general_settings.section = Section::Grid;
            app.doc.display.grid_infinite = infinite;
            let mut output =
                ctx.run_test_ui(egui::RawInput::default(), |ui| app.settings_contents(ui));
            let text = rendered_text(&output.shapes);
            output.textures_delta.clear();
            assert!(text.iter().any(|s| s == "Infinite grid"));
            assert_eq!(text.iter().any(|s| s == "Maximum length"), !infinite);
        }
    }

    #[test]
    fn settings_groups_include_every_section_once() {
        let grouped: Vec<_> = super::Section::GROUPS
            .iter()
            .flat_map(|(_, sections)| sections.iter().copied())
            .collect();
        assert_eq!(grouped.len(), super::Section::ALL.len());
        for section in super::Section::ALL {
            assert_eq!(grouped.iter().filter(|&&value| value == section).count(), 1);
        }
    }
    use super::*;

    #[test]
    fn adding_rotate_migrates_existing_custom_keys_and_resolves_conflicts() {
        for custom in ["H", "Q"] {
            let mut settings = GeneralSettings::default();
            settings.tool_keys.pop();
            settings.tool_keys[1] = custom.into();
            let old = settings.tool_keys.clone();
            settings.normalize();
            assert_eq!(&settings.tool_keys[..old.len()], old.as_slice());
            assert_eq!(settings.tool_keys.len(), TOOLS.len());
            assert_eq!(settings.tool_for_key(Key::from_name(custom).unwrap()), Some(Tool::Line));
            let rotation = settings.tool_shortcut(Tool::Rotate);
            assert_ne!(rotation, custom);
            assert_eq!(settings.tool_for_key(Key::from_name(rotation).unwrap()), Some(Tool::Rotate));
        }
    }

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
            let mut output = ctx.run_test_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1100., 800.))),
                    ..Default::default()
                },
                |_| app.settings_window(&ctx),
            );
            output.textures_delta.clear();
            assert!(app.settings_open);
        }
    }
}
