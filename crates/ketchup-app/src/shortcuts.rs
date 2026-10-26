use super::*;

struct Section<'a> {
    title: &'a str,
    rows: Vec<(&'a str, String)>,
}

impl Section<'_> {
    fn matches(&self, action: &str, keys: &str, query: &str) -> bool {
        let text = format!("{} {action} {keys}", self.title).to_lowercase();
        query.split_whitespace().all(|word| text.contains(word))
    }

    fn show(&self, ui: &mut egui::Ui, query: &str) {
        let rows: Vec<_> = self
            .rows
            .iter()
            .filter(|(action, keys)| self.matches(action, keys, query))
            .collect();
        if rows.is_empty() {
            return;
        }
        let colors = Palette::for_ui(ui);
        egui::Frame::new()
            .fill(colors.surface)
            .stroke(Stroke::new(1., colors.border))
            .corner_radius(8)
            .inner_margin(14)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    RichText::new(self.title)
                        .size(15.)
                        .strong()
                        .color(colors.ink),
                );
                ui.add_space(8.);
                let column_width = (ui.available_width() - 12.) * 0.5;
                egui::Grid::new(self.title)
                    .num_columns(2)
                    .spacing(egui::vec2(12., 9.))
                    .min_col_width(column_width)
                    .max_col_width(column_width)
                    .show(ui, |ui| {
                        for (action, keys) in rows {
                            ui.add(egui::Label::new(*action).wrap());
                            // Keep badges compact and align them to the section's right edge.
                            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                                egui::Frame::new()
                                    .fill(colors.canvas)
                                    .stroke(Stroke::new(1., colors.border))
                                    .corner_radius(4)
                                    .inner_margin(egui::Margin::symmetric(7, 3))
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(keys)
                                                    .monospace()
                                                    .size(11.)
                                                    .color(colors.green),
                                            )
                                            .wrap(),
                                        );
                                    });
                            });
                            ui.end_row();
                        }
                    });
            });
        ui.add_space(6.);
    }
}

impl KetchupApp {
    pub(super) fn help_window(&mut self, ctx: &egui::Context) {
        let mac = ctx.os() == egui::os::OperatingSystem::Mac;
        let command = if mac { "⌘" } else { "Ctrl" };
        let section = |title: &'static str, rows: &[(&'static str, &str)]| Section {
            title,
            rows: rows
                .iter()
                .map(|(action, keys)| (*action, (*keys).to_owned()))
                .collect(),
        };
        let sections = [
            section(
                "Drawing & measuring",
                &[
                    ("Select", "Space"),
                    ("Connected lines", "L"),
                    ("Rectangle", "R"),
                    ("Circle", "C"),
                    ("Polygon", "G"),
                    ("Freehand", "D"),
                    ("Paint faces", "B"),
                    ("Erase edges or faces", "E"),
                    ("Tape Measure", "T"),
                    ("Protractor", "A"),
                    ("Dimensions", "I"),
                ],
            ),
            Section {
                title: "Project & clipboard",
                rows: [
                    ("New project", format!("{command} + N")),
                    ("Open project", format!("{command} + O")),
                    ("Save", format!("{command} + S")),
                    ("Save as", format!("{command} + Shift + S")),
                    ("Undo", format!("{command} + Z")),
                    ("Redo", format!("{command} + Shift + Z")),
                    ("Cut", format!("{command} + X")),
                    ("Copy", format!("{command} + C")),
                    ("Paste", format!("{command} + V")),
                    ("Paste in place", format!("{command} + Shift + V")),
                ]
                .into(),
            },
            section(
                "Modeling",
                &[
                    ("Push/Pull a face", "P"),
                    ("Scale geometry", "S"),
                    ("Offset a face", "Q"),
                    ("Move geometry", "M"),
                    ("Toggle Move / Copy", Self::move_copy_key(ctx)),
                    ("Divide a copy span", "/N + Enter"),
                    ("Repeat copies", "Nx + Enter"),
                    ("Flip & solid operations", "Union flyout"),
                ],
            ),
            section(
                "Navigation",
                &[
                    ("Orbit", "O"),
                    ("Pan while orbiting", "Shift + drag"),
                    (
                        "Orbit while drawing",
                        if mac {
                            "⌘ + move pointer"
                        } else {
                            "Ctrl + move pointer"
                        },
                    ),
                    ("Temporary orbit", "Middle drag"),
                    ("Temporary pan", "Shift + middle drag"),
                    ("Zoom to pointer", "Scroll"),
                    ("Fit model to view", "F"),
                    ("Toggle emphasis on current group/component", "Tab"),
                ],
            ),
            section(
                "Precision & inference",
                &[
                    ("Lock X / Y / Z axis", "→ / ← / ↑"),
                    ("Lock shape plane normal X / Y / Z", "→ / ← / ↑"),
                    ("Clear axis lock", "↓"),
                    ("Set length or radius", "Value + Enter"),
                    ("Set rectangle size", "Width,height + Enter"),
                    ("Acquire reference axes", "Hover geometry"),
                    ("Match reference coordinate", "Axis lock + hover"),
                    ("Cancel current operation", "Esc"),
                ],
            ),
            section(
                "Selection & objects",
                &[
                    ("Toggle selection", "Shift + click"),
                    ("Select all", if mac { "⌘ + A" } else { "Ctrl + A" }),
                    ("Object & clipboard menu", "Right click"),
                    ("Edit group or component", "Double-click"),
                    ("Close editing level", "Esc"),
                    ("Delete selection", "Delete / Backspace"),
                ],
            ),
        ];
        let width = (ctx.content_rect().width() - 64.).clamp(240., 900.);
        let response = egui::Modal::new(egui::Id::unique("keyboard_shortcuts"))
            .backdrop_color(Color32::TRANSPARENT)
            .frame(
                egui::Frame::popup(&ctx.global_style())
                    .inner_margin(20)
                    .corner_radius(12),
            )
            .show(ctx, |ui| {
                let colors = Palette::for_ui(ui);
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.heading("Keyboard shortcuts");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if Self::icon_button(
                            ui,
                            "close",
                            "Close keyboard shortcuts",
                            true,
                            colors.muted,
                        ) {
                            ui.close();
                        }
                    });
                });
                ui.label(RichText::new("Find a tool, command, or gesture.").color(colors.muted));
                ui.add_space(8.);
                let mut filter_changed = false;
                ui.horizontal(|ui| {
                    let clear_width = if self.shortcut_filter.is_empty() {
                        0.
                    } else {
                        60.
                    };
                    filter_changed = ui
                        .add(
                            egui::TextEdit::singleline(&mut self.shortcut_filter)
                                .hint_text("Search shortcuts…")
                                .desired_width(ui.available_width() - clear_width)
                                .margin(egui::vec2(10., 8.)),
                        )
                        .changed();
                    if !self.shortcut_filter.is_empty() && ui.button("Clear").clicked() {
                        self.shortcut_filter.clear();
                        filter_changed = true;
                    }
                });
                ui.add_space(10.);
                let query = self.shortcut_filter.trim().to_lowercase();
                let any_matches = sections.iter().any(|section| {
                    section
                        .rows
                        .iter()
                        .any(|(action, keys)| section.matches(action, keys, &query))
                });
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt("shortcut_results")
                    .auto_shrink([false, false])
                    .max_height((ctx.content_rect().height() - 260.).max(80.));
                if filter_changed {
                    scroll = scroll.vertical_scroll_offset(0.);
                }
                scroll.show(ui, |ui| {
                    if !any_matches {
                        ui.add_space(24.);
                        ui.label(RichText::new("No shortcuts found").strong());
                        ui.label(
                            RichText::new(format!(
                                "Try a tool name like “orbit” or a key like “{command}”."
                            ))
                            .color(colors.muted),
                        );
                    } else if width >= 740. {
                        ui.columns(2, |columns| {
                            for (index, section) in sections.iter().enumerate() {
                                section.show(&mut columns[index % 2], &query);
                            }
                        });
                    } else {
                        for section in &sections {
                            section.show(ui, &query);
                        }
                    }
                });
                ui.add_space(8.);
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Esc to close").small().color(colors.muted));
                    ui.label(
                        RichText::new("·  Lengths accept units and formulas: (1m + 20cm) / 2")
                            .small()
                            .color(colors.muted),
                    );
                });
            });
        if response.should_close() {
            self.help = false;
            self.shortcut_filter.clear();
        }
    }
}
