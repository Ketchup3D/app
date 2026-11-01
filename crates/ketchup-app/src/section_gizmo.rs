//! Small perimeter handles share the viewport's input routing, so planes never
//! cover the model with an interactive surface or steal a tool's existing drag.
use super::*;

#[derive(Clone)]
pub(super) struct SectionDrag {
    axis: usize,
    start: Pos2,
    offset: f64,
    anchor: DVec3,
    camera: Camera,
    viewport: Rect,
    ray_start: Option<f64>,
    screen_axis: Vec2,
    world_per_pixel: f64,
}

#[derive(Clone, Copy)]
struct Handle {
    axis: usize,
    anchor: DVec3,
    rect: Rect,
    direction: Vec2,
}

fn offset(display: &ketchup_core::DisplaySettings, axis: usize) -> f64 {
    if axis == 2 {
        display.section_height
    } else {
        display.section_offsets[axis]
    }
}
fn set_offset(display: &mut ketchup_core::DisplaySettings, axis: usize, value: f64) {
    if axis == 2 {
        display.section_height = value;
    } else {
        display.section_offsets[axis] = value;
    }
}
fn axis_vector(axis: usize) -> DVec3 {
    let mut vector = DVec3::ZERO;
    vector[axis] = 1.;
    vector
}
fn screen_direction(camera: &Camera, axis: usize) -> Vec2 {
    let vector = axis_vector(axis);
    let projected = egui::vec2(
        vector.dot(camera.right()) as f32,
        -vector.dot(camera.up()) as f32,
    );
    if projected.length() < 0.15 {
        -Vec2::Y
    } else {
        projected.normalized()
    }
}

impl SectionDrag {
    fn new(
        axis: usize,
        anchor: DVec3,
        start: Pos2,
        camera: &Camera,
        viewport: Rect,
        value: f64,
    ) -> Self {
        let vector = axis_vector(axis);
        let alignment = vector.dot((camera.eye() - camera.target).normalize()).abs();
        let ray_start = (alignment < 0.98)
            .then(|| {
                camera
                    .ray(dv(start - viewport.min), dv(viewport.size()))
                    .axis(anchor, vector)
            })
            .flatten()
            .map(|point| point[axis]);
        let distance = if camera.projection == ketchup_core::Projection::Parallel {
            camera.distance
        } else {
            camera.eye().distance(anchor)
        };
        Self {
            axis,
            start,
            offset: value,
            anchor,
            camera: camera.clone(),
            viewport,
            ray_start,
            screen_axis: screen_direction(camera, axis),
            world_per_pixel: 2. * distance.max(0.05) * (camera.fov * 0.5).tan()
                / f64::from(viewport.height().max(1.)),
        }
    }
    fn value(&self, pos: Pos2) -> f64 {
        let delta = self
            .ray_start
            .and_then(|start| {
                self.camera
                    .ray(dv(pos - self.viewport.min), dv(self.viewport.size()))
                    .axis(self.anchor, axis_vector(self.axis))
                    .map(|point| point[self.axis] - start)
            })
            .unwrap_or_else(|| {
                f64::from((pos - self.start).dot(self.screen_axis)) * self.world_per_pixel
            });
        let value = self.offset + delta;
        if value.is_finite() {
            value.clamp(-1e8, 1e8)
        } else {
            self.offset
        }
    }
}

impl KetchupApp {
    fn move_section_plane(&mut self, ctx: &egui::Context, axis: usize, value: f64) {
        if offset(&self.doc.display, axis) != value {
            set_offset(&mut self.doc.display, axis, value);
            self.changed(ctx);
        }
    }
    pub(super) fn cancel_section_drag(&mut self, ctx: &egui::Context) {
        if let Some(drag) = self.section_drag.take() {
            self.move_section_plane(ctx, drag.axis, drag.offset);
        }
    }
    fn section_handles(&self) -> Vec<Handle> {
        if !self.doc.display.section_planes
            || self.viewport.width() < 64.
            || self.viewport.height() < 64.
        {
            return Vec::new();
        }
        let (lo, hi) = self
            .doc
            .model
            .bounds()
            .unwrap_or((DVec3::splat(-2.), DVec3::splat(2.)));
        let pad = (hi - lo).max_element().max(1.) * 0.1;
        let mut handles: Vec<Handle> = Vec::new();
        for axis in 0..3 {
            if !self.doc.display.section_axes[axis] {
                continue;
            }
            let u = (axis + 1) % 3;
            let v = (axis + 2) % 3;
            // Attach to a plane corner outside the object's bounds.
            let mut anchor = lo - DVec3::splat(pad);
            anchor[axis] = offset(&self.doc.display, axis);
            anchor[u] = lo[u] - pad;
            anchor[v] = lo[v] - pad;
            let Some(projected) = self.doc.camera.project(anchor, dv(self.viewport.size())) else {
                continue;
            };
            let projected = self.viewport.min + egui::vec2(projected.x as f32, projected.y as f32);
            let safe = self.viewport.shrink2(egui::vec2(28., 20.));
            let base = safe.clamp(projected);
            let center = [
                Vec2::ZERO,
                egui::vec2(0., -32.),
                egui::vec2(0., 32.),
                egui::vec2(0., -64.),
                egui::vec2(0., 64.),
                egui::vec2(-52., 0.),
                egui::vec2(52., 0.),
            ]
            .into_iter()
            .map(|delta| safe.clamp(base + delta))
            .find(|center| {
                let candidate = Rect::from_center_size(*center, egui::vec2(44., 24.)).expand(3.);
                handles
                    .iter()
                    .all(|previous| !previous.rect.intersects(candidate))
            });
            let Some(center) = center else { continue };
            handles.push(Handle {
                axis,
                anchor,
                rect: Rect::from_center_size(center, egui::vec2(44., 24.)),
                direction: screen_direction(&self.doc.camera, axis),
            });
        }
        handles
    }

    pub(super) fn section_handle_at(&self, pos: Pos2) -> bool {
        self.section_handles()
            .iter()
            .any(|handle| handle.rect.contains(pos))
    }

    pub(super) fn section_plane_event(
        &mut self,
        ctx: &egui::Context,
        event: &egui::Event,
        response: &egui::Response,
    ) -> bool {
        if !self.doc.display.section_planes {
            self.section_drag = None;
            return false;
        }
        if let Some(drag) = self.section_drag.clone() {
            match event {
                egui::Event::PointerMoved(pos) => {
                    self.move_section_plane(ctx, drag.axis, drag.value(*pos));
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    self.move_section_plane(ctx, drag.axis, drag.value(*pos));
                    self.section_drag = None;
                }
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                }
                | egui::Event::PointerButton {
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    ..
                } => {
                    self.move_section_plane(ctx, drag.axis, drag.offset);
                    self.section_drag = None;
                }
                egui::Event::PointerGone => {
                    self.section_drag = None;
                }
                _ => {}
            }
            return true;
        }
        if self.gesture.is_some() {
            return false;
        }
        if let egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers,
            ..
        } = event
            && response.enabled()
            && response.contains_pointer()
            && self.viewport.contains(*pos)
            && !self.drawing_orbit_modifier(*modifiers)
            && !self.compass_contains(*pos)
            && let Some(handle) = self
                .section_handles()
                .into_iter()
                .find(|handle| handle.rect.contains(*pos))
        {
            self.section_drag = Some(SectionDrag::new(
                handle.axis,
                handle.anchor,
                *pos,
                &self.doc.camera,
                self.viewport,
                offset(&self.doc.display, handle.axis),
            ));
            self.hover = None;
            self.flip_hover = None;
            self.viewport_pointer = None;
            return true;
        }
        false
    }

    pub(super) fn paint_section_handles(
        &self,
        ui: &egui::Ui,
        response: &egui::Response,
        active: bool,
    ) {
        let painter = ui
            .painter()
            .with_clip_rect(self.viewport.intersect(ui.clip_rect()));
        let colors = Palette::for_ui(ui);
        for handle in self.section_handles() {
            let hovered = response.enabled()
                && response.contains_pointer()
                && ui
                    .ctx()
                    .input(|i| i.pointer.hover_pos())
                    .is_some_and(|pos| handle.rect.contains(pos));
            let dragging = active
                && self
                    .section_drag
                    .as_ref()
                    .is_some_and(|drag| drag.axis == handle.axis);
            let color = Color32::from_rgb(
                self.doc.display.axis_colors[handle.axis][0],
                self.doc.display.axis_colors[handle.axis][1],
                self.doc.display.axis_colors[handle.axis][2],
            );
            if let Some(point) = self
                .doc
                .camera
                .project(handle.anchor, dv(self.viewport.size()))
            {
                let anchor = self.viewport.min + egui::vec2(point.x as f32, point.y as f32);
                painter.line_segment([anchor, handle.rect.center()], Stroke::new(1., color));
            }
            painter.rect_filled(
                handle.rect,
                5,
                if hovered || dragging {
                    colors.paper
                } else {
                    colors.canvas
                },
            );
            painter.rect_stroke(
                handle.rect,
                5,
                Stroke::new(if hovered || dragging { 2. } else { 1. }, color),
                egui::StrokeKind::Inside,
            );
            painter.text(
                handle.rect.left_center() + egui::vec2(9., 0.),
                Align2::CENTER_CENTER,
                ["X", "Y", "Z"][handle.axis],
                FontId::proportional(12.),
                color,
            );
            let center = handle.rect.right_center() - egui::vec2(12., 0.);
            painter.arrow(
                center - handle.direction * 5.,
                handle.direction * 10.,
                Stroke::new(1.5, color),
            );
            if hovered || dragging {
                ui.ctx().set_cursor_icon(if dragging {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::Grab
                });
                if hovered {
                    response.clone().on_hover_text(format!("Drag section {} along its axis · Esc to cancel. When viewed along the axis, drag up/down.", ["X", "Y", "Z"][handle.axis]));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_ui::ContextExt as _;

    #[test]
    fn section_drag_is_constrained_and_finite_in_every_standard_projection() {
        let rect = Rect::from_min_size(egui::pos2(400., 30.), egui::vec2(600., 500.));
        for projection in [
            ketchup_core::Projection::Parallel,
            ketchup_core::Projection::Perspective,
        ] {
            for view in ketchup_core::StandardView::ALL {
                let mut camera = Camera::default();
                camera.set_standard_view(view);
                camera.projection = projection;
                for axis in 0..3 {
                    let drag =
                        SectionDrag::new(axis, DVec3::ZERO, rect.center(), &camera, rect, 1.);
                    let value = drag.value(rect.center() + drag.screen_axis * 40.);
                    assert!(
                        value.is_finite() && value > 1.,
                        "{projection:?} {view:?} {axis}: {value}"
                    );
                    let mut display = ketchup_core::DisplaySettings {
                        section_cuts: true,
                        section_axes: [true; 3],
                        ..Default::default()
                    };
                    let center = axis_vector(axis) * ((1. + value) * 0.5);
                    let u = axis_vector((axis + 1) % 3) * 0.01;
                    let v = axis_vector((axis + 2) % 3) * 0.01;
                    let triangle = [center, center + u, center + v];
                    assert_eq!(display.section_triangle(triangle).1, 0);
                    set_offset(&mut display, axis, value);
                    assert_eq!(display.section_triangle(triangle).1, 3);
                    assert_eq!(display.section_limits()[axis], Some(value));
                    assert!(display.section_contains(axis_vector(axis) * (value - 0.01)));
                    assert!(!display.section_contains(axis_vector(axis) * (value + 0.01)));
                    assert_eq!(offset(&display, (axis + 1) % 3), 1.);
                }
            }
        }
    }

    fn frame(
        app: &mut KetchupApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run_test_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000., 700.))),
                events,
                ..Default::default()
            },
            |ui| {
                app.keyboard(ctx);
                app.viewport_workspace(ui);
            },
        )
    }
    fn button(pos: Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    #[test]
    fn section_handle_routes_drag_to_pressed_view_and_keeps_model_tools_idle() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.doc.display.section_planes = true;
        app.doc.display.section_cuts = true;
        app.doc.display.section_axes = [true, false, false];
        let preferences = serde_json::from_value(serde_json::json!({
            "layout": {"Split": {"id":2,"horizontal":true,"ratio":0.5,
                "first":{"Pane":{"id":0,"tabs":[0],"selected":0}},
                "second":{"Pane":{"id":1,"tabs":[1],"selected":1}}}},
            "active":0,"next_id":3
        }))
        .unwrap();
        let mut other_camera = Camera::default();
        other_camera.set_standard_view(ketchup_core::StandardView::Right);
        let saved =
            std::collections::BTreeMap::from([(0, app.doc.camera.clone()), (1, other_camera)]);
        app.viewports = viewports::Workspace::restore(&preferences, &mut app.doc.camera, &saved);
        let output = frame(&mut app, &ctx, vec![]);
        let handle = output
            .shapes
            .iter()
            .rev()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    if text.galley.job.text == "X" && text.pos.x > 500. {
                        return Some(text.pos + text.galley.size() * 0.5);
                    }
                }
                None
            })
            .expect("right viewport section handle");
        frame(&mut app, &ctx, button(handle, true));
        assert!(app.section_drag.is_some());
        assert!(app.gesture.is_none());
        let selected = app.selected.clone();
        let camera = app.doc.camera.clone();
        let moved = handle - egui::vec2(0., 50.);
        frame(&mut app, &ctx, vec![egui::Event::PointerMoved(moved)]);
        assert!(app.doc.display.section_offsets[0] > 1.);
        assert_eq!(
            app.doc.display.section_limits()[0],
            Some(app.doc.display.section_offsets[0])
        );
        // A drag stays with its original viewport even across another pane.
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(egui::pos2(200., moved.y))],
        );
        assert_eq!(
            serde_json::to_value(app.viewports.capture(&app.doc.camera)).unwrap()["active"],
            1
        );
        assert_eq!(app.doc.camera, camera);
        assert_eq!(app.selected, selected);
        frame(&mut app, &ctx, button(moved, false));
        assert!(app.section_drag.is_none());
        assert!(app.gesture.is_none());
    }

    #[test]
    fn escape_restores_offset_and_plane_interior_does_not_claim_input() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.doc.display.section_planes = true;
        app.doc.display.section_axes = [false, false, true];
        frame(&mut app, &ctx, vec![]);
        let center = app.viewport.center();
        assert!(!app.section_handle_at(center));
        let handle = app.section_handles()[0];
        frame(&mut app, &ctx, button(handle.rect.center(), true));
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(
                handle.rect.center() + handle.direction * 50.,
            )],
        );
        assert_ne!(app.doc.display.section_height, 1.);
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(app.doc.display.section_height, 1.);
        assert!(app.section_drag.is_none());
        frame(&mut app, &ctx, button(center, false));
        frame(&mut app, &ctx, button(center, true));
        assert!(app.section_drag.is_none());
        assert!(app.gesture.is_some());
    }
}
