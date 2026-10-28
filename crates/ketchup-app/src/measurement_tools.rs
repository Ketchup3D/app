use super::*;
use ketchup_core::{Dimension, DimensionAnchor, angle_arc, angle_degrees};

#[derive(Default)]
pub(super) struct Measurement {
    pub(super) points: Vec<DVec3>,
    pub(super) complete: bool,
    pub(super) normal: DVec3,
    pub(super) offset: DVec3,
    pub(super) text: String,
}

impl KetchupApp {
    pub(super) fn measurement_plane(&self) -> Option<(DVec3, DVec3)> {
        let m = self.measurement.as_ref()?;
        (self.tool == Tool::Protractor && m.points.len() == 2).then(|| (m.points[0], m.normal))
    }

    pub(super) fn update_measurement(&mut self, pos: Pos2) {
        if self.tool == Tool::Text {
            self.update_text_pointer(pos);
            return;
        }
        self.prepare_picking();
        let points = self
            .measurement
            .as_ref()
            .filter(|m| !m.complete)
            .map(|m| m.points.as_slice())
            .unwrap_or(&[]);
        let size = dv(self.viewport.size());
        let cursor = dv(pos - self.viewport.min);
        self.snap = self.infer_snap(
            pos,
            points.first().copied(),
            // Dimensions align the annotation, not the geometry being measured.
            // Keep both picked endpoints and their attachments on the model.
            if self.tool == Tool::Dimensions {
                None
            } else {
                self.lock
            },
            self.doc.snap_to_axes,
        );
        if self.tool == Tool::Protractor && points.len() == 2 {
            if let Some(snap) = self.snap.as_mut().filter(|s| s.kind == Inference::Plane) {
                let ray = self.doc.camera.ray(cursor, size);
                let normal = self.measurement.as_ref().unwrap().normal;
                let denominator = ray.direction.dot(normal);
                if denominator.abs() > 1e-8 {
                    let t = (points[0] - ray.origin).dot(normal) / denominator;
                    if t > 0. {
                        snap.point = ray.origin + ray.direction * t;
                    }
                }
            }
        }
        if self.tool == Tool::Dimensions && points.len() == 2 {
            self.snap = None;
            let (a, b) = (points[0], points[1]);
            let direction = self
                .dimension_preview()
                .unwrap()
                .line_direction(&self.doc.model);
            let center = (a + b) * 0.5;
            let normal = (self.doc.camera.target - self.doc.camera.eye()).normalize();
            let ray = self.doc.camera.ray(cursor, size);
            let denominator = ray.direction.dot(normal);
            if let Some(axis) = self
                .lock
                .filter(|axis| (b - a).dot(axis.vector()).abs() < 1e-7)
            {
                // A perpendicular lock places the annotation along the axis;
                // projecting its measured span onto that axis would merge it.
                if let Some(point) = ray.axis(center, axis.vector()) {
                    self.measurement.as_mut().unwrap().offset = point - center;
                }
            } else if denominator.abs() > 1e-8 {
                let t = (center - ray.origin).dot(normal) / denominator;
                let delta = ray.origin + ray.direction * t - center;
                self.measurement.as_mut().unwrap().offset =
                    delta - direction * delta.dot(direction);
            }
        }
        self.update_drawing_inference(pos);
        self.apply_grid_snap();
    }

    pub(super) fn measurement_click(&mut self, pos: Pos2, ctx: &egui::Context) {
        if self.tool == Tool::Text {
            self.text_click(pos, ctx);
            return;
        }
        if self.measurement.as_ref().is_some_and(|m| m.complete) {
            self.measurement = None;
            self.snap = None;
        }
        self.update_measurement(pos);
        if self.tool == Tool::Dimensions
            && self
                .measurement
                .as_ref()
                .is_some_and(|m| m.points.len() == 2)
        {
            let m = self.measurement.as_ref().unwrap();
            let before = self.doc.model.clone();
            match self.doc.model.add_dimension_with_axis(
                m.points[0],
                m.points[1],
                m.offset,
                self.lock,
            ) {
                Ok(id) => {
                    self.measurement = None;
                    self.snap = None;
                    self.selected = BTreeSet::from([id]);
                    self.finish_edit("Add dimension", before, ctx);
                }
                Err(message) => self.say(ctx, message),
            }
            return;
        }
        let Some(snap) = self.snap else {
            self.say(
                ctx,
                "Move over the model or drawing plane to choose a point",
            );
            return;
        };
        let size = dv(self.viewport.size());
        let surface_normal = self
            .doc
            .model
            .nearest_face(self.doc.camera.ray(dv(pos - self.viewport.min), size))
            .and_then(|(id, _, _)| {
                ketchup_core::faces::plane(&self.doc.model, &self.doc.model.faces[&id].vertices)
            })
            .map(|p| p.normal)
            .unwrap_or(DVec3::Z);
        let m = self.measurement.get_or_insert_with(|| Measurement {
            normal: surface_normal,
            ..Default::default()
        });
        if m.points
            .first()
            .is_some_and(|p| p.distance(snap.point) < 1e-7)
        {
            self.say(ctx, "Choose a point away from the measurement origin");
            return;
        }
        m.points.push(snap.point);
        if self.tool == Tool::Protractor && m.points.len() == 2 {
            let baseline = (m.points[1] - m.points[0]).normalize();
            if baseline.dot(m.normal).abs() > 1e-6 {
                let view = (self.doc.camera.eye() - m.points[0]).normalize();
                m.normal = (view - baseline * view.dot(baseline))
                    .try_normalize()
                    .unwrap_or_else(|| {
                        baseline.cross(DVec3::X).try_normalize().unwrap_or(DVec3::Y)
                    });
            }
        }
        m.complete = (self.tool == Tool::TapeMeasure && m.points.len() == 2)
            || (self.tool == Tool::Protractor && m.points.len() == 3);
    }

    fn measurement_points(&self) -> Vec<DVec3> {
        let Some(m) = &self.measurement else {
            return vec![];
        };
        let mut points = m.points.clone();
        if !m.complete && points.len() < if self.tool == Tool::Protractor { 3 } else { 2 } {
            if let Some(snap) = self.snap {
                points.push(snap.point);
            }
        }
        points
    }
    fn dimension_preview(&self) -> Option<Dimension> {
        let points = self.measurement_points();
        if points.len() != 2 {
            return None;
        }
        let anchor = |point| DimensionAnchor {
            point,
            weights: vec![],
        };
        Some(Dimension {
            id: 0,
            start: anchor(points[0]),
            end: anchor(points[1]),
            offset: self.measurement.as_ref()?.offset,
            axis: self.lock,
        })
    }
    pub(super) fn measurement_value(&self) -> Option<String> {
        if self.tool == Tool::Text {
            return None;
        }
        let points = self.measurement_points();
        if self.tool == Tool::Dimensions {
            self.dimension_preview().map(|d| {
                self.doc
                    .display
                    .units
                    .format_length(d.length(&self.doc.model))
            })
        } else if self.tool == Tool::Protractor {
            (points.len() == 3)
                .then(|| angle_degrees(points[0], points[1], points[2]))
                .flatten()
                .map(|angle| self.doc.display.units.format_angle(angle))
        } else {
            (points.len() >= 2).then(|| {
                self.doc
                    .display
                    .units
                    .format_length(points[0].distance(points[1]))
            })
        }
    }
    pub(super) fn measurement_status(&self) -> &'static str {
        let Some(m) = &self.measurement else {
            return self.tool.instructions();
        };
        if m.complete {
            return "Measurement complete · Click to start another · Esc to clear";
        }
        match (self.tool, m.points.len()) {
            (Tool::Text, 1) => "Move to position the text · Click to place · Esc to cancel",
            (Tool::TapeMeasure, 1) => {
                "Click the second point · Arrow keys lock axes · Esc to cancel"
            }
            (Tool::Protractor, 1) => "Click a baseline point · Esc to cancel",
            (Tool::Protractor, 2) => "Click the angle point · Esc to cancel",
            (Tool::Dimensions, 1) => {
                "Click the second endpoint · Arrow keys align X / Y / Z · Esc to cancel"
            }
            (Tool::Dimensions, 2) => {
                "Move to place · Arrow keys align X / Y / Z · Down clears the lock"
            }
            _ => self.tool.instructions(),
        }
    }
    pub(super) fn measurement_inspector(&mut self, ui: &mut egui::Ui) {
        if !self.tool.is_measuring() {
            return;
        }
        ui.heading(self.tool.label());
        if let Some(value) = self.measurement_value() {
            ui.label(
                RichText::new(value)
                    .monospace()
                    .size(22.)
                    .color(Palette::for_ui(ui).green),
            );
        }
        ui.label(
            RichText::new(self.measurement_status())
                .small()
                .color(Palette::for_ui(ui).muted),
        );
        ui.add_space(12.);
    }
    pub(super) fn dimension_shapes(
        &self,
        mesh: &mut renderer::Mesh,
        p: &egui::Painter,
        rect: Rect,
        model: &Model,
    ) -> Vec<egui::Shape> {
        if !self.doc.display.dimensions {
            return vec![];
        }
        let colors = Palette::for_painter(p);
        let camera = &self.doc.camera;
        let mut shapes = vec![];
        let selected = model.expand_entities(&self.selected);
        for dimension in model
            .dimensions
            .values()
            .filter(|d| self.entity_cache.get(d.id).displayed(&self.doc.display))
        {
            let outliner_hover = self.outliner_hover.contains(&dimension.id);
            let color = if model.entity_in_context(dimension.id).is_none() && !outliner_hover {
                colors.muted
            } else if selected.contains(&dimension.id) {
                colors.accent
            } else if outliner_hover
                || self.hover.is_some() && self.hover == model.entity_in_context(dimension.id)
            {
                colors.green
            } else {
                colors.ink
            };
            shapes.extend(dimension_shapes(
                mesh,
                p,
                camera,
                rect,
                model,
                dimension,
                color,
                self.doc.display.units,
            ));
        }
        if let Some(preview) = &self.paste_preview {
            let offset = preview.point - preview.clipboard.anchor;
            for source in preview
                .clipboard
                .model
                .dimensions
                .values()
                .filter(|d| !preview.clipboard.model.is_hidden(d.id))
            {
                let (a, b) = source.points(&preview.clipboard.model);
                let mut dimension = source.clone();
                dimension.start = DimensionAnchor {
                    point: a + offset,
                    weights: vec![],
                };
                dimension.end = DimensionAnchor {
                    point: b + offset,
                    weights: vec![],
                };
                shapes.extend(dimension_shapes(
                    mesh,
                    p,
                    camera,
                    rect,
                    model,
                    &dimension,
                    colors.green,
                    self.doc.display.units,
                ));
            }
        }
        if self.tool == Tool::Dimensions {
            if let Some(dimension) = self.dimension_preview() {
                shapes.extend(dimension_shapes(
                    mesh,
                    p,
                    camera,
                    rect,
                    model,
                    &dimension,
                    self.lock.map(axis_color).unwrap_or(colors.green),
                    self.doc.display.units,
                ));
            }
        }
        shapes
    }
    pub(super) fn paint_measurements(&self, p: &egui::Painter, rect: Rect) {
        if !self.doc.display.dimensions {
            return;
        }
        let colors = Palette::for_painter(p);
        let camera = &self.doc.camera;
        let size = dv(rect.size());
        if !self.tool.is_measuring() || self.tool == Tool::Text {
            return;
        }
        let points = self.measurement_points();
        let screen = |pt| camera.project(pt, size).map(|p| rect.min + pv(p));
        let segment = |a, b| {
            if let Some((a, b)) = camera.segment(a, b, size) {
                p.line_segment(
                    [rect.min + pv(a), rect.min + pv(b)],
                    Stroke::new(1.5, colors.green),
                );
            }
        };
        if self.tool == Tool::Protractor {
            if points.len() >= 2 {
                segment(points[0], points[1]);
            }
            if points.len() == 3 {
                segment(points[0], points[2]);
                let arc = angle_arc(points[0], points[1], points[2]);
                for pts in arc.windows(2) {
                    segment(pts[0], pts[1]);
                }
                if let Some(mid) = arc.get(arc.len() / 2).and_then(|pt| screen(*pt)) {
                    if let Some(angle) = angle_degrees(points[0], points[1], points[2]) {
                        measurement_label(
                            p,
                            mid + egui::vec2(0., -14.),
                            self.doc.display.units.format_angle(angle),
                            colors.green,
                        );
                    }
                }
            }
        } else if self.tool == Tool::TapeMeasure && points.len() == 2 {
            segment(points[0], points[1]);
            if let Some(mid) = screen((points[0] + points[1]) * 0.5) {
                measurement_label(
                    p,
                    mid + egui::vec2(0., -14.),
                    self.doc
                        .display
                        .units
                        .format_length(points[0].distance(points[1])),
                    colors.green,
                );
            }
        }
        for point in points {
            if let Some(point) = screen(point) {
                p.circle_filled(point, 3., colors.paper);
                p.circle_stroke(point, 3., Stroke::new(1.5, colors.green));
            }
        }
        if !self.measurement.as_ref().is_some_and(|m| m.complete) {
            if let Some(snap) = self.snap {
                if let Some(point) = screen(snap.point) {
                    p.circle_stroke(point, 4., Stroke::new(1.5, colors.green));
                    if snap.kind != Inference::Plane {
                        measurement_label(
                            p,
                            point + egui::vec2(45., 18.),
                            snap.kind.label().into(),
                            colors.green,
                        );
                    }
                }
            }
        }
    }
}

fn dimension_shapes(
    mesh: &mut renderer::Mesh,
    p: &egui::Painter,
    camera: &Camera,
    rect: Rect,
    model: &Model,
    d: &Dimension,
    color: Color32,
    units: ketchup_core::UnitSettings,
) -> Vec<egui::Shape> {
    let points = d.layout(model);
    let size = dv(rect.size());
    let mut shapes = vec![];
    for (a, b) in [(0, 2), (1, 3), (2, 3)] {
        for (a, b) in mesh.annotation_line(points[a], points[b]) {
            shapes.extend(mesh.annotation_stroke(p, rect, a, b, Stroke::new(1., color)));
        }
    }
    let Some(a) = camera.project(points[2], size) else {
        return shapes;
    };
    let Some(b) = camera.project(points[3], size) else {
        return shapes;
    };
    let direction = (b - a).normalize_or_zero();
    let normal = DVec2::new(-direction.y, direction.x);
    for point in [points[2], points[3]] {
        for (a, b) in mesh.annotation_tick(point, normal * 4. + direction * 3.) {
            shapes.extend(mesh.annotation_stroke(p, rect, a, b, Stroke::new(1.5, color)));
        }
    }
    let midpoint = (a + b) * 0.5;
    // Keep the label centered on the projected span, and use the corresponding
    // point on the 3D line for its depth (also in perspective views).
    let center = camera
        .ray(midpoint, size)
        .segment_point(points[2], points[3]);
    let (bounds, label) = measurement_label_shape(
        p,
        rect.min + pv(midpoint),
        units.format_length(d.length(model)),
        color,
    );
    shapes.extend(mesh.annotation_label(p, rect, center, bounds, label));
    shapes
}
fn measurement_label_shape(
    p: &egui::Painter,
    at: Pos2,
    text: String,
    color: Color32,
) -> (Rect, egui::Shape) {
    let galley = p.layout_no_wrap(text, FontId::monospace(12.), color);
    let rect = Rect::from_center_size(at, galley.size() + egui::vec2(12., 8.));
    (
        rect,
        egui::Shape::Vec(vec![
            egui::Shape::rect_filled(rect, 4, Palette::for_painter(p).paper),
            egui::Shape::galley(rect.min + egui::vec2(6., 4.), galley, color),
        ]),
    )
}
fn measurement_label(p: &egui::Painter, at: Pos2, text: String, color: Color32) {
    p.add(measurement_label_shape(p, at, text, color).1);
}
