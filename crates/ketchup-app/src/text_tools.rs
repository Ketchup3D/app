//! Attached leader text: pick geometry, place the label, then enter its contents.
use super::*;
use ketchup_core::{DimensionAnchor, TextAnnotation};

pub(super) struct TextEditor {
    pub(super) id: Option<u64>,
    point: DVec3,
    offset: DVec3,
    pub(super) text: String,
}

impl KetchupApp {
    pub(super) fn update_text_pointer(&mut self, pos: Pos2) {
        self.prepare_picking();
        if let Some(m) = self.measurement.as_mut().filter(|m| !m.points.is_empty()) {
            // Place on a camera-facing plane through the attachment. The stored
            // offset stays in model space while the glyphs always face the view.
            let point = m.points[0];
            let normal = (self.doc.camera.target - self.doc.camera.eye()).normalize();
            let ray = self
                .doc
                .camera
                .ray(dv(pos - self.viewport.min), dv(self.viewport.size()));
            let denominator = ray.direction.dot(normal);
            if denominator.abs() > 1e-8 {
                let t = (point - ray.origin).dot(normal) / denominator;
                if t > 0. {
                    m.offset = ray.origin + ray.direction * t - point;
                }
            }
            self.snap = None;
        } else {
            self.snap = ketchup_core::inference::infer_with_targets(
                &self.doc.model,
                &self.doc.camera,
                dv(self.viewport.size()),
                dv(pos - self.viewport.min),
                None,
                None,
                true,
                false,
                Some(
                    &self
                        .picking_cache
                        .snap_query(&self.face_cache, &self.entity_cache),
                ),
                self.snap_targets(),
            );
            // Leader text starts on geometry, not on the drawing plane.
            if self
                .snap
                .is_some_and(|s| self.doc.model.dimension_anchor(s.point).weights.is_empty())
            {
                self.snap = None;
            }
        }
    }

    pub(super) fn text_click(&mut self, pos: Pos2, ctx: &egui::Context) {
        if self.measurement.is_none() {
            if let Some(id) = selection::pick_text(
                &self.doc.model,
                &self.doc.camera,
                dv(self.viewport.size()),
                dv(pos - self.viewport.min),
                8.,
            )
            .filter(|id| self.doc.model.texts.contains_key(id))
            {
                self.selected = BTreeSet::from([id]);
                self.begin_text_edit(id, ctx);
                return;
            }
        }
        self.update_text_pointer(pos);
        if let Some(m) = self.measurement.take() {
            self.text_editor = Some(TextEditor {
                id: None,
                point: m.points[0],
                offset: m.offset,
                text: m.text,
            });
            Self::request_name_input_focus(ctx, egui::Id::unique("attached_text_input"));
        } else if let Some(snap) = self.snap {
            let ray = self
                .doc
                .camera
                .ray(dv(pos - self.viewport.min), dv(self.viewport.size()));
            let target = self
                .pick_at(dv(pos - self.viewport.min), dv(self.viewport.size()), false)
                .or_else(|| {
                    self.doc
                        .model
                        .nearest_context_face(ray)
                        .and_then(|(id, _, _)| self.doc.model.entity_in_context(id))
                });
            let text = target
                .and_then(|id| {
                    if let Some(object) = self.doc.model.objects.get(&id) {
                        Some(object.name.clone())
                    } else if let Some(edge) = self.doc.model.edges.get(&id) {
                        Some(
                            self.doc
                                .display
                                .units
                                .format_length(self.doc.model.edge_length(edge)),
                        )
                    } else {
                        self.doc.model.faces.get(&id).map(|face| {
                            self.doc
                                .display
                                .units
                                .format_area(self.doc.model.face_area(face))
                        })
                    }
                })
                .unwrap_or_else(|| "Text".into());
            self.measurement = Some(measurement_tools::Measurement {
                points: vec![snap.point],
                text,
                ..Default::default()
            });
        } else {
            self.say(
                ctx,
                "Click a visible edge, face, group, or component to attach text",
            );
        }
    }

    pub(super) fn begin_text_edit(&mut self, id: u64, ctx: &egui::Context) {
        if self.doc.model.is_locked(id) {
            return;
        }
        if let Some(text) = self.doc.model.texts.get(&id) {
            self.text_editor = Some(TextEditor {
                id: Some(id),
                point: text.anchor.position(&self.doc.model),
                offset: text.offset,
                text: text.text.clone(),
            });
            Self::request_name_input_focus(ctx, egui::Id::unique("attached_text_input"));
        }
    }

    pub(super) fn commit_text_editor(&mut self, ctx: &egui::Context) {
        let Some(editor) = self.text_editor.take() else {
            return;
        };
        let before = self.doc.model.clone();
        let result = if let Some(id) = editor.id {
            self.doc.model.edit_text(id, &editor.text).map(|()| id)
        } else {
            self.doc
                .model
                .add_text(editor.point, editor.offset, &editor.text)
        };
        match result {
            Ok(id) => {
                self.selected = BTreeSet::from([id]);
                self.finish_edit(
                    if editor.id.is_some() {
                        "Edit text"
                    } else {
                        "Add text"
                    },
                    before,
                    ctx,
                );
            }
            Err(message) => {
                self.text_editor = Some(editor);
                self.say(ctx, message);
            }
        }
    }

    pub(super) fn text_editor_window(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.text_editor.take() else {
            return;
        };
        if editor
            .id
            .is_some_and(|id| !self.doc.model.texts.contains_key(&id))
        {
            return;
        }
        let mut save = false;
        let mut cancel = false;
        let response = Modal::new("attached_text_editor", 400.).show(ctx, |modal| {
            modal.header(|ui| {
                ui.heading(if editor.id.is_some() {
                    "Edit Text"
                } else {
                    "Attach Text"
                });
                ui.weak("Add a label to your model.");
            });
            modal.body(|ui| {
                let input_id = egui::Id::unique("attached_text_input");
                // Plain Enter accepts; Shift+Enter adds another line.
                save = ui.ctx().input_mut(|i| {
                    !i.modifiers.shift && i.consume_key(egui::Modifiers::NONE, Key::Enter)
                });
                ui.add(
                    egui::TextEdit::multiline(&mut editor.text)
                        .id(input_id)
                        .desired_width(320.)
                        .desired_rows(4)
                        .hint_text("Enter label text"),
                );
                Self::focus_name_input(ui, &editor.text, input_id);
                if editor.text.len() > 4096 {
                    editor.text.truncate(editor.text.floor_char_boundary(4096));
                }
                ui.weak("Enter to save · Shift+Enter for a new line · Esc to cancel");
            });
            modal.footer(|ui| {
                cancel = ui.button("Cancel").clicked();
                let valid = !editor.text.trim().is_empty();
                save = (primary_button(ui, "Save", valid).clicked() || save) && valid;
            });
        });
        if (!cancel && !response.should_close()) || save {
            self.text_editor = Some(editor);
            if save {
                self.commit_text_editor(ctx);
            }
        }
    }

    pub(super) fn text_entity_info(&mut self, ui: &mut egui::Ui) {
        if self.selected.len() != 1 {
            return;
        }
        let id = *self.selected.first().unwrap();
        if let Some(text) = self.doc.model.texts.get(&id) {
            ui.label(&text.text);
            if ui
                .add_enabled(
                    !self.doc.model.is_locked(id),
                    egui::Button::new("Edit Text…"),
                )
                .clicked()
            {
                self.begin_text_edit(id, ui.ctx());
            }
            ui.add_space(12.);
        }
    }

    pub(super) fn text_shapes(
        &self,
        mesh: &mut renderer::Mesh,
        p: &egui::Painter,
        rect: Rect,
        model: &Model,
    ) -> Vec<egui::Shape> {
        let colors = Palette::for_painter(p);
        let selected = model.expand_entities(&self.selected);
        let mut shapes = vec![];
        for text in model
            .texts
            .values()
            .filter(|t| self.entity_cache.get(t.id).displayed(&self.doc.display))
        {
            if self
                .text_editor
                .as_ref()
                .is_some_and(|editor| editor.id == Some(text.id))
            {
                continue;
            }
            let hovered = self.outliner_hover.contains(&text.id)
                || self.hover.is_some() && self.hover == model.entity_in_context(text.id);
            let color = if selected.contains(&text.id) {
                colors.accent
            } else if hovered {
                colors.green
            } else if model.entity_in_context(text.id).is_none() {
                colors.muted
            } else {
                colors.ink
            };
            shapes.extend(text_shapes(
                mesh,
                p,
                &self.doc.camera,
                rect,
                model,
                text,
                color,
            ));
        }
        if let Some(preview) = &self.paste_preview {
            let offset = preview.point - preview.clipboard.anchor;
            for source in preview
                .clipboard
                .model
                .texts
                .values()
                .filter(|t| !preview.clipboard.model.is_hidden(t.id))
            {
                let mut text = source.clone();
                text.anchor = DimensionAnchor {
                    point: source.anchor.position(&preview.clipboard.model) + offset,
                    weights: vec![],
                };
                shapes.extend(text_shapes(
                    mesh,
                    p,
                    &self.doc.camera,
                    rect,
                    model,
                    &text,
                    colors.green,
                ));
            }
        }
        if let Some(editor) = &self.text_editor {
            let text = TextAnnotation {
                id: 0,
                anchor: DimensionAnchor {
                    point: editor.point,
                    weights: vec![],
                },
                offset: editor.offset,
                text: editor.text.clone(),
            };
            shapes.extend(text_shapes(
                mesh,
                p,
                &self.doc.camera,
                rect,
                model,
                &text,
                colors.green,
            ));
        }
        if self.tool == Tool::Text {
            if let Some(m) = &self.measurement {
                let text = TextAnnotation {
                    id: 0,
                    anchor: DimensionAnchor {
                        point: m.points[0],
                        weights: vec![],
                    },
                    offset: m.offset,
                    text: m.text.clone(),
                };
                shapes.extend(text_shapes(
                    mesh,
                    p,
                    &self.doc.camera,
                    rect,
                    model,
                    &text,
                    colors.green,
                ));
            }
        }
        shapes
    }
}

fn text_shapes(
    mesh: &mut renderer::Mesh,
    p: &egui::Painter,
    camera: &Camera,
    rect: Rect,
    model: &Model,
    text: &TextAnnotation,
    color: Color32,
) -> Vec<egui::Shape> {
    let [a, b] = text.layout(model);
    let mut shapes = vec![];
    for (a, b) in mesh.annotation_line(a, b) {
        shapes.extend(mesh.annotation_stroke(p, rect, a, b, Stroke::new(1., color)));
    }
    if let Some((screen_a, screen_b)) = camera.segment(a, b, dv(rect.size())) {
        let direction = (screen_b - screen_a).normalize_or_zero();
        let normal = DVec2::new(-direction.y, direction.x);
        // Open arrowhead at the model attachment, with the same depth clipping
        // as dimension ticks so foreground geometry obscures it correctly.
        for delta in [direction * 8. + normal * 3., direction * 8. - normal * 3.] {
            for (start, end) in mesh.annotation_arrow(a, delta) {
                shapes.extend(mesh.annotation_stroke(p, rect, start, end, Stroke::new(1., color)));
            }
        }
    }
    if let Some(at) = camera.project(b, dv(rect.size())) {
        let galley = p.layout(text.text.clone(), FontId::monospace(12.), color, 320.);
        let bounds = Rect::from_center_size(rect.min + pv(at), galley.size() + egui::vec2(12., 8.));
        let label = egui::Shape::Vec(vec![
            egui::Shape::rect_filled(bounds, 4, Palette::for_painter(p).paper),
            egui::Shape::galley(bounds.min + egui::vec2(6., 4.), galley, color),
        ]);
        shapes.extend(mesh.annotation_label(p, rect, b, bounds, label));
    }
    shapes
}
