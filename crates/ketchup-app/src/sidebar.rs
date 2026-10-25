use super::*;
use ketchup_core::{DisplaySettings, FaceStyle};

#[path = "outliner.rs"]
mod outliner;

fn face_style_preview(
    painter: &egui::Painter,
    rect: Rect,
    style: FaceStyle,
    colors: Palette,
    faces: FacePalette,
) {
    let center = rect.center();
    let scale = rect.width().min(rect.height()) * 0.46;
    let point = |x, y| center + egui::vec2(x * scale, y * scale);
    let top = point(0., -1.);
    let left = point(-0.9, -0.55);
    let front = point(0., -0.1);
    let right = point(0.9, -0.55);
    let bottom_left = point(-0.9, 0.55);
    let bottom = point(0., 1.);
    let bottom_right = point(0.9, 0.55);
    let outline = Stroke::new(1.2, colors.ink);
    if style == FaceStyle::Wireframe {
        let back = point(0., 0.1);
        for end in [top, bottom_left, bottom_right] {
            painter.line_segment([back, end], Stroke::new(1., colors.muted));
        }
    } else {
        let fills = match style {
            FaceStyle::HiddenLine => [colors.paper; 3],
            FaceStyle::Monochrome => [faces.front, faces.side, faces.back],
            FaceStyle::Materials => faces.materials,
            FaceStyle::Shaded => [faces.front, faces.side, faces.shaded_side],
            FaceStyle::Wireframe => unreachable!(),
        };
        for (points, fill) in [
            ([top, right, front, left], fills[0]),
            ([left, front, bottom, bottom_left], fills[1]),
            ([front, right, bottom_right, bottom], fills[2]),
        ] {
            painter.add(egui::Shape::convex_polygon(
                points.to_vec(),
                fill,
                Stroke::NONE,
            ));
        }
    }
    painter.add(egui::Shape::closed_line(
        vec![top, right, bottom_right, bottom, bottom_left, left],
        outline,
    ));
    for end in [left, right, bottom] {
        painter.line_segment([front, end], outline);
    }
}

fn face_style_picker(ui: &mut egui::Ui, current: &mut FaceStyle) {
    let colors = Palette::for_ui(ui);
    let faces = FacePalette::for_ui(ui);
    let spacing = ui.spacing().item_spacing.x;
    let columns = ((ui.available_width() + spacing) / (96. + spacing))
        .floor()
        .clamp(1., FaceStyle::ALL.len() as f32) as usize;
    let width = (ui.available_width() - spacing * (columns - 1) as f32) / columns as f32;
    ui.push_id("face_style", |ui| {
        for row in FaceStyle::ALL.chunks(columns) {
            ui.horizontal(|ui| {
                for &style in row {
                    let selected = *current == style;
                    let mut response = ui
                        .add_sized(
                            egui::vec2(width, 100.),
                            egui::Button::new("").selected(selected).corner_radius(5),
                        )
                        .on_hover_text(style.label());
                    if response.clicked() && !selected {
                        *current = style;
                        response.mark_changed();
                    }
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::Role::RadioButton,
                            ui.is_enabled(),
                            *current == style,
                            style.label(),
                        )
                    });
                    if ui.is_rect_visible(response.rect) {
                        let rect = response.rect;
                        face_style_preview(
                            ui.painter(),
                            Rect::from_center_size(
                                egui::pos2(rect.center().x, rect.top() + 33.),
                                egui::vec2(width - 16., 50.),
                            ),
                            style,
                            colors,
                            faces,
                        );
                        let mut label = egui::text::LayoutJob::simple(
                            style.label().to_owned(),
                            FontId::proportional(11.),
                            colors.ink,
                            width - 12.,
                        );
                        label.halign = egui::Align::Center;
                        let galley = ui.fonts_mut(|fonts| fonts.layout_job(label));
                        let pos = egui::pos2(
                            rect.center().x,
                            rect.bottom() - 21. - galley.size().y * 0.5,
                        );
                        ui.painter().galley(pos, galley, colors.ink);
                    }
                }
            });
        }
    });
}

// Keep egui's disclosure behavior, animation, and accessibility while giving
// inspector groups a shared surface and a full-width click target.
fn inspector_section<R>(
    ui: &mut egui::Ui,
    title: &str,
    id: &str,
    primary: bool,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) {
    let colors = Palette::for_ui(ui);
    let body_style = ui.style().as_ref().clone();
    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1., colors.border))
        .corner_radius(if primary { 8 } else { 5 })
        .inner_margin(1)
        .shadow(if primary {
            egui::epaint::Shadow {
                offset: [0, 2],
                blur: 5,
                spread: 0,
                color: Color32::from_black_alpha(12),
            }
        } else {
            egui::epaint::Shadow::NONE
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.scope(|ui| {
                ui.spacing_mut().button_padding = egui::vec2(10., if primary { 10. } else { 7. });
                ui.spacing_mut().indent = 28.;
                let visuals = ui.visuals_mut();
                visuals.collapsing_header_frame = true;
                for widget in [
                    &mut visuals.widgets.inactive,
                    &mut visuals.widgets.hovered,
                    &mut visuals.widgets.active,
                ] {
                    widget.corner_radius = egui::CornerRadius::same(if primary { 7 } else { 4 });
                    widget.bg_stroke = Stroke::NONE;
                    widget.expansion = 0.;
                }
                visuals.widgets.inactive.weak_bg_fill =
                    if primary { colors.canvas } else { colors.paper };
                visuals.widgets.hovered.weak_bg_fill = colors.hovered;
                visuals.widgets.active.weak_bg_fill = colors.selected;
                egui::CollapsingHeader::new(RichText::new(title).strong())
                    .id_salt(id)
                    .default_open(primary)
                    .icon(move |ui, openness, response| {
                        let center = response.rect.center();
                        let rotation =
                            egui::emath::Rot2::from_angle(openness * std::f32::consts::FRAC_PI_2);
                        let points = [
                            egui::vec2(-2., -4.),
                            egui::vec2(2., 0.),
                            egui::vec2(-2., 4.),
                        ]
                        .map(|point| center + rotation * point);
                        ui.painter().add(egui::Shape::line(
                            points.to_vec(),
                            Stroke::new(
                                1.6,
                                if openness > 0. {
                                    colors.green
                                } else {
                                    colors.muted
                                },
                            ),
                        ));
                    })
                    .show_unindented(ui, |ui| {
                        *ui.style_mut() = body_style;
                        egui::Frame::new().inner_margin(10).show(ui, contents).inner
                    });
            });
        });
    ui.add_space(if primary { 4. } else { 1. });
}

impl KetchupApp {
    pub(super) fn inspector_contents(&mut self, ui: &mut egui::Ui) {
        inspector_section(ui, "Outliner", "outliner", true, |ui| {
            self.outliner_contents(ui)
        });
        inspector_section(ui, "Entity Info", "entity_info", true, |ui| {
            self.entity_info_contents(ui)
        });
        inspector_section(ui, "Components", "components", true, |ui| {
            self.components_contents(ui)
        });
        inspector_section(ui, "Tags", "tags", true, |ui| self.tags_contents(ui));
        inspector_section(ui, "Display", "display", true, |ui| {
            self.display_contents(ui)
        });
    }

    pub(super) fn begin_component_placement(&mut self, id: u64, ctx: &egui::Context) {
        if !self
            .doc
            .model
            .objects
            .get(&id)
            .is_some_and(|o| matches!(o.kind, ObjectKind::Component { .. }))
        {
            return;
        }
        let mut model = self.doc.model.copy_entities(&BTreeSet::from([id]));
        // An instance placed from the library starts visible and unlocked.
        model.hidden.remove(&id);
        model.locked.remove(&id);
        model.entity_tags.remove(&id);
        let mut points = model.vertices.values().map(|v| v.position).chain(
            model
                .dimensions
                .values()
                .flat_map(|dimension| dimension.layout(&model)),
        );
        let Some(first) = points.next() else {
            self.say(ctx, "This component is empty");
            return;
        };
        let anchor = points.fold(first, |lo, point| lo.min(point));
        let clipboard = GeometryClipboard { model, anchor };
        self.set_tool(Tool::Select);
        self.paste_preview = Some(PastePreview {
            clipboard,
            point: self.doc.camera.target,
        });
        self.say(ctx, "Click to place the component · Escape to cancel");
    }

    fn components_contents(&mut self, ui: &mut egui::Ui) {
        let mut definitions = std::collections::BTreeMap::new();
        for object in self.doc.model.objects.values() {
            if let ObjectKind::Component { definition } = object.kind {
                let entry =
                    definitions
                        .entry(definition)
                        .or_insert((object.id, object.name.clone(), 0));
                entry.2 += 1;
            }
        }
        if definitions.is_empty() {
            ui.label(RichText::new("No components yet").color(Palette::for_ui(ui).muted));
            ui.label("Select geometry and choose Make Component from the context menu.");
        } else {
            ui.add(
                egui::TextEdit::singleline(&mut self.component_filter)
                    .hint_text("Search components")
                    .desired_width(f32::INFINITY),
            );
            let query = self.component_filter.to_lowercase();
            let mut rows: Vec<_> = definitions
                .into_values()
                .filter(|(_, name, _)| name.to_lowercase().contains(&query))
                .collect();
            rows.sort_by(|a, b| {
                a.1.to_lowercase()
                    .cmp(&b.1.to_lowercase())
                    .then(a.0.cmp(&b.0))
            });
            if rows.is_empty() {
                ui.label(RichText::new("No matching components").color(Palette::for_ui(ui).muted));
            }
            for (id, name, count) in rows {
                ui.push_id(id, |ui| {
                    egui::Frame::new()
                        .fill(Palette::for_ui(ui).canvas)
                        .inner_margin(8)
                        .corner_radius(4)
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(RichText::new(name).strong());
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{count} instance{}",
                                        if count == 1 { "" } else { "s" }
                                    ))
                                    .small()
                                    .color(Palette::for_ui(ui).muted),
                                );
                                if ui.small_button("Place instance").clicked() {
                                    self.begin_component_placement(id, ui.ctx());
                                }
                            });
                        });
                });
            }
        }
        if ui
            .add_enabled(
                !self.selected.is_empty() && !self.doc.model.selection_protected(&self.selected),
                egui::Button::new("Make Component…"),
            )
            .clicked()
        {
            self.component_name = Some("Component".into());
        }
    }

    fn tags_contents(&mut self, ui: &mut egui::Ui) {
        let mut before = None;
        let mut label = "Edit tags";
        ui.horizontal(|ui| {
            let input = ui.add(
                egui::TextEdit::singleline(&mut self.new_tag_name)
                    .hint_text("New tag name")
                    .desired_width((ui.available_width() - 40.).max(80.)),
            );
            let enter = input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if ui
                .add_enabled(!self.new_tag_name.trim().is_empty(), egui::Button::new("+"))
                .on_hover_text("Add tag")
                .clicked()
                || enter
            {
                before.get_or_insert_with(|| self.doc.model.clone());
                match self.doc.model.add_tag(&self.new_tag_name) {
                    Ok(_) => {
                        self.new_tag_name.clear();
                        label = "Add tag";
                    }
                    Err(error) => self.say(ui.ctx(), error),
                }
            }
        });
        ui.label(
            RichText::new("Visibility · Color · Name")
                .small()
                .color(Palette::for_ui(ui).muted),
        );
        ui.label("Untagged");
        let tags: Vec<_> = self.doc.model.tags.values().cloned().collect();
        for mut tag in tags {
            ui.push_id(tag.id, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut tag.visible, "")
                        .on_hover_text("Show or hide this tag and its contents")
                        .changed()
                    {
                        before.get_or_insert_with(|| self.doc.model.clone());
                        self.doc.model.tags.get_mut(&tag.id).unwrap().visible = tag.visible;
                    }
                    if ui.color_edit_button_srgb(&mut tag.color).changed() {
                        before.get_or_insert_with(|| self.doc.model.clone());
                        self.doc.model.tags.get_mut(&tag.id).unwrap().color = tag.color;
                    }
                    let name = ui
                        .add_sized(
                            egui::vec2((ui.available_width() - 80.).max(30.), 24.),
                            egui::Label::new(&tag.name).truncate(),
                        )
                        .on_hover_text("Double-click to rename");
                    if name.double_clicked() {
                        self.tag_rename = Some((tag.id, tag.name.clone()));
                    }
                    if ui
                        .small_button("Edit")
                        .on_hover_text("Rename tag")
                        .clicked()
                    {
                        self.tag_rename = Some((tag.id, tag.name.clone()));
                    }
                    if ui
                        .small_button("×")
                        .on_hover_text("Delete tag; its entities become Untagged")
                        .clicked()
                    {
                        before.get_or_insert_with(|| self.doc.model.clone());
                        self.doc.model.remove_tag(tag.id);
                        label = "Delete tag";
                    }
                });
            });
        }
        if let Some((id, mut name)) = self.tag_rename.take() {
            if self.doc.model.tags.contains_key(&id) {
                let mut keep = true;
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut name).desired_width(115.));
                    if ui.small_button("Save").clicked() {
                        before.get_or_insert_with(|| self.doc.model.clone());
                        match self.doc.model.rename_tag(id, &name) {
                            Ok(()) => {
                                keep = false;
                                label = "Rename tag";
                            }
                            Err(error) => self.say(ui.ctx(), error),
                        }
                    }
                    if ui.small_button("Cancel").clicked() {
                        keep = false;
                    }
                });
                if keep {
                    self.tag_rename = Some((id, name));
                }
            }
        }
        ui.separator();
        if self.selected.is_empty() {
            ui.label(
                RichText::new("Select entities to assign a tag.")
                    .small()
                    .color(Palette::for_ui(ui).muted),
            );
        } else {
            let first = self
                .selected
                .first()
                .and_then(|id| self.doc.model.entity_tags.get(id))
                .copied();
            let mixed = self
                .selected
                .iter()
                .any(|id| self.doc.model.entity_tags.get(id).copied() != first);
            let current = if mixed {
                "Mixed tags"
            } else {
                first
                    .and_then(|id| self.doc.model.tags.get(&id))
                    .map(|t| t.name.as_str())
                    .unwrap_or("Untagged")
            };
            let mut assign = None;
            ui.add_enabled_ui(!self.doc.model.selection_protected(&self.selected), |ui| {
                egui::ComboBox::from_id_salt("selection_tag")
                    .selected_text(current)
                    .width(ui.available_width() - 10.)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(!mixed && first.is_none(), "Untagged")
                            .clicked()
                        {
                            assign = Some(None);
                        }
                        for tag in self.doc.model.tags.values() {
                            if ui
                                .selectable_label(!mixed && first == Some(tag.id), &tag.name)
                                .clicked()
                            {
                                assign = Some(Some(tag.id));
                            }
                        }
                    });
            });
            if let Some(tag) = assign {
                before.get_or_insert_with(|| self.doc.model.clone());
                if let Err(error) = self.doc.model.assign_tag(&self.selected, tag) {
                    self.say(ui.ctx(), error);
                }
                label = "Assign tag";
            }
        }
        if let Some(before) = before.filter(|before| *before != self.doc.model) {
            // Tag visibility is global, while assignments inside definitions are shared.
            if label == "Assign tag" {
                self.finish_context_state_edit(label, before, ui.ctx());
            } else {
                self.finish_edit(label, before, ui.ctx());
            }
            if self
                .doc
                .model
                .edit_context
                .is_some_and(|id| self.doc.model.is_hidden(id))
            {
                self.navigate_context(None, ui.ctx());
            }
            self.hover = None;
            self.snap = None;
        }
    }

    fn display_contents(&mut self, ui: &mut egui::Ui) {
        let before = self.doc.display.clone();
        let d = &mut self.doc.display;
        ui.label(RichText::new("Visibility").strong());
        for (value, label) in [
            (&mut d.axes, "Axes"),
            (&mut d.guides, "Guides"),
            (&mut d.grid, "Grid"),
            (&mut d.dimensions, "Dimensions"),
            (&mut d.hidden_geometry, "Hidden geometry"),
            (&mut d.hidden_objects, "Hidden objects"),
        ] {
            ui.checkbox(value, label);
        }
        ui.separator();
        ui.label(RichText::new("Face style").strong());
        face_style_picker(ui, &mut d.face_style);
        ui.checkbox(&mut d.x_ray, "X-Ray");
        ui.checkbox(&mut d.color_by_tag, "Color by tag");
        inspector_section(ui, "Edge style", "edge_style", false, |ui| {
            for (value, label) in [
                (&mut d.edges, "Edges"),
                (&mut d.back_edges, "Back edges"),
                (&mut d.profiles, "Profiles"),
                (&mut d.depth_cue, "Depth cue"),
                (&mut d.endpoints, "Endpoints"),
                (&mut d.extensions, "Extensions"),
            ] {
                ui.checkbox(value, label);
            }
            ui.add_enabled_ui(d.profiles, |ui| {
                // Keep the label above the slider so this nested section fits
                // even at the inspector's minimum width.
                let label = ui.label("Profile width");
                ui.add(egui::Slider::new(&mut d.profile_width, 1. ..=8.))
                    .labelled_by(label.id);
            });
        });
        inspector_section(ui, "Sections", "sections", false, |ui| {
            ui.checkbox(&mut d.section_planes, "Section planes");
            ui.checkbox(&mut d.section_cuts, "Section cuts");
            ui.checkbox(&mut d.section_fill, "Section fill");
            ui.horizontal(|ui| {
                ui.label("Height");
                ui.add(
                    egui::DragValue::new(&mut d.section_height)
                        .speed(0.05)
                        .range(-1e8..=1e8)
                        .suffix(" m"),
                );
            });
            ui.label(
                RichText::new("Horizontal section: keeps geometry below the plane.")
                    .small()
                    .color(Palette::for_ui(ui).muted),
            );
        });
        inspector_section(ui, "Shadows", "shadows", false, |ui| {
            ui.checkbox(&mut d.shadows, "Display shadows");
            ui.add_enabled_ui(d.shadows, |ui| {
                ui.add(
                    egui::Slider::new(&mut d.sun_angle, 0. ..=360.)
                        .text("Direction")
                        .suffix("°"),
                );
                ui.add(
                    egui::Slider::new(&mut d.sun_elevation, 5. ..=85.)
                        .text("Elevation")
                        .suffix("°"),
                );
                ui.add(egui::Slider::new(&mut d.shadow_darkness, 0. ..=1.).text("Darkness"));
            });
        });
        inspector_section(ui, "Fog", "fog", false, |ui| {
            ui.checkbox(&mut d.fog, "Display fog");
            ui.add_enabled_ui(d.fog, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Start");
                    ui.add(
                        egui::DragValue::new(&mut d.fog_start)
                            .speed(0.5)
                            .range(0. ..=1e8)
                            .suffix(" m"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("End");
                    ui.add(
                        egui::DragValue::new(&mut d.fog_end)
                            .speed(0.5)
                            .range(0.1..=1e9)
                            .suffix(" m"),
                    );
                });
                d.fog_end = d.fog_end.max(d.fog_start + 0.1);
            });
            ui.horizontal(|ui| {
                crate::environment::background_control(ui, d);
                ui.label("Background / fog color");
            });
        });
        inspector_section(ui, "Environment", "display_environment", false, |ui| {
            crate::environment::controls(ui, d);
        });
        if ui.button("Reset display").clicked() {
            *d = DisplaySettings::default();
        }
        if before != self.doc.display {
            self.changed(ui.ctx());
        }
    }
}
