use super::*;
use ketchup_core::StandardView;

impl KetchupApp {
    pub(super) fn view_contents(&mut self, ui: &mut egui::Ui) {
        let before = self.doc.camera.clone();
        inspector_section(ui, "Camera", "view_camera", false, |ui| {
            ui.label("Projection");
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(
                    &mut self.doc.camera.projection,
                    Projection::Perspective,
                    "Perspective",
                );
                ui.selectable_value(
                    &mut self.doc.camera.projection,
                    Projection::Parallel,
                    "Parallel",
                )
                .on_hover_text("Orthographic projection: parallel lines stay parallel");
            });
            ui.add_enabled_ui(
                self.doc.camera.projection == Projection::Perspective,
                |ui| {
                    ui.label("Field of view");
                    let mut degrees = self.doc.camera.fov.to_degrees();
                    if ui
                        .add(egui::Slider::new(&mut degrees, 15. ..=100.).suffix("°"))
                        .changed()
                    {
                        self.doc.camera.fov = degrees.to_radians();
                    }
                },
            );
            ui.horizontal_wrapped(|ui| {
                if ui.button("Fit model").clicked() {
                    self.doc.camera.fit(
                        self.doc.model.bounds(),
                        dv(self.viewport.size().max(Vec2::splat(1.))),
                    );
                }
                if ui.button("Reset camera").clicked() {
                    self.doc.camera = Camera::default();
                    self.doc.camera.fit(
                        self.doc.model.bounds(),
                        dv(self.viewport.size().max(Vec2::splat(1.))),
                    );
                }
            });
        });
        inspector_section(ui, "Standard views", "view_standard_views", false, |ui| {
            standard_view_picker(ui, &mut self.doc.camera);
        });
        if before != self.doc.camera {
            self.changed(ui.ctx());
        }
    }
}

// Match the face-style cards: responsive columns, cube preview, caption,
// selected background, and a radio-button accessibility description.
fn standard_view_picker(ui: &mut egui::Ui, camera: &mut Camera) {
    let colors = Palette::for_ui(ui);
    let faces = FacePalette::for_ui(ui);
    let spacing = ui.spacing().item_spacing.x;
    let columns = ((ui.available_width() + spacing) / (96. + spacing))
        .floor()
        .clamp(1., StandardView::ALL.len() as f32) as usize;
    let width = (ui.available_width() - spacing * (columns - 1) as f32) / columns as f32;
    ui.push_id("standard_views", |ui| {
        for row in StandardView::ALL.chunks(columns) {
            ui.horizontal(|ui| {
                for &view in row {
                    let selected = camera.standard_view() == Some(view);
                    let mut response = ui
                        .add_sized(
                            egui::vec2(width, 100.),
                            egui::Button::new("").selected(selected).corner_radius(5),
                        )
                        .on_hover_text(format!("{} view", view.label()));
                    if response.clicked() && !selected {
                        camera.set_standard_view(view);
                        response.mark_changed();
                    }
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::Role::RadioButton,
                            ui.is_enabled(),
                            camera.standard_view() == Some(view),
                            view.label(),
                        )
                    });
                    if ui.is_rect_visible(response.rect) {
                        let rect = response.rect;
                        standard_view_preview(
                            ui.painter(),
                            Rect::from_center_size(
                                egui::pos2(rect.center().x, rect.top() + 33.),
                                egui::vec2(width - 16., 50.),
                            ),
                            view,
                            colors,
                            faces,
                        );
                        ui.painter().text(
                            egui::pos2(rect.center().x, rect.bottom() - 21.),
                            egui::Align2::CENTER_CENTER,
                            view.label(),
                            FontId::proportional(11.),
                            colors.ink,
                        );
                    }
                }
            });
        }
    });
}

fn standard_view_preview(
    painter: &egui::Painter,
    rect: Rect,
    view: StandardView,
    colors: Palette,
    faces: FacePalette,
) {
    // Directional icons share one orientation, including the three faces on
    // the far side. Only Iso uses its own shaded, isometric preview.
    let mut camera = Camera::default();
    if view == StandardView::Iso {
        camera.set_standard_view(view);
    }
    let toward_eye = (camera.eye() - camera.target).normalize();
    let scale = rect.width().min(rect.height()) * 0.30;
    let project = |p: DVec3| {
        rect.center() + egui::vec2(p.dot(camera.right()) as f32, -p.dot(camera.up()) as f32) * scale
    };
    let axes = [
        (DVec3::X, DVec3::Y, DVec3::Z),
        (DVec3::Y, DVec3::X, DVec3::Z),
        (DVec3::Z, DVec3::X, DVec3::Y),
    ];
    let face_points = |normal: DVec3, u: DVec3, v: DVec3| {
        [
            normal - u - v,
            normal + u - v,
            normal + u + v,
            normal - u + v,
        ]
        .map(project)
        .to_vec()
    };
    if view == StandardView::Iso {
        for (axis, u, v) in axes {
            for sign in [-1., 1.] {
                let normal = axis * sign;
                if normal.dot(toward_eye) <= 0. {
                    continue;
                }
                let fill = if axis == DVec3::Z {
                    faces.front
                } else if axis == DVec3::X {
                    faces.side
                } else {
                    faces.shaded_side
                };
                painter.add(egui::Shape::convex_polygon(
                    face_points(normal, u, v),
                    fill,
                    Stroke::new(1.2, colors.ink),
                ));
            }
        }
        return;
    }
    let (normal, u, v) = match view {
        StandardView::Top => (DVec3::Z, DVec3::X, DVec3::Y),
        StandardView::Front => (-DVec3::Y, DVec3::X, DVec3::Z),
        StandardView::Right => (DVec3::X, DVec3::Y, DVec3::Z),
        StandardView::Back => (DVec3::Y, DVec3::X, DVec3::Z),
        StandardView::Left => (-DVec3::X, DVec3::Y, DVec3::Z),
        StandardView::Bottom => (-DVec3::Z, DVec3::X, DVec3::Y),
        StandardView::Iso => unreachable!(),
    };
    let highlighted = face_points(normal, u, v);
    painter.add(egui::Shape::convex_polygon(
        highlighted.clone(),
        Color32::from_rgba_unmultiplied(colors.green.r(), colors.green.g(), colors.green.b(), 80),
        Stroke::NONE,
    ));
    // Draw all twelve edges so back, left, and bottom remain readable through
    // the empty cube. Recede the hidden edges without hiding the chosen face.
    for hidden in [true, false] {
        for (axis, u, v) in axes {
            for a in [-1., 1.] {
                for b in [-1., 1.] {
                    let is_hidden = (u * a).dot(toward_eye) <= 0. && (v * b).dot(toward_eye) <= 0.;
                    if is_hidden != hidden {
                        continue;
                    }
                    let center = u * a + v * b;
                    painter.line_segment(
                        [project(center - axis), project(center + axis)],
                        if hidden {
                            Stroke::new(1., colors.muted)
                        } else {
                            Stroke::new(1.2, colors.ink)
                        },
                    );
                }
            }
        }
    }
    painter.add(egui::Shape::closed_line(
        highlighted,
        Stroke::new(1.8, colors.green),
    ));
}
