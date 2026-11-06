//! Pivot/reference/angle rotation using the same targets and grid as Move.
use super::*;

pub(super) struct Rotation {
    pub normal: DVec3,
    surface_normal: DVec3,
    pub reference: Option<DVec3>,
}

impl KetchupApp {
    pub(super) fn rotation_at(&self, anchor: DVec3, ray: ketchup_core::Ray) -> Rotation {
        let surface_normal = self
            .doc
            .model
            .nearest_face(ray)
            .filter(|(_, _, point)| point.distance(anchor) < 1e-5)
            .and_then(|(id, _, _)| {
                ketchup_core::faces::plane(&self.doc.model, &self.doc.model.faces[&id].vertices)
            })
            .map_or(DVec3::Z, |plane| plane.normal);
        Rotation {
            normal: self.shape_lock().map_or(surface_normal, Axis::vector),
            surface_normal,
            reference: None,
        }
    }

    pub(super) fn rotation_status(&self) -> &'static str {
        match self
            .modeling
            .as_ref()
            .and_then(|edit| edit.rotation.as_ref())
        {
            None => {
                "Select geometry · Click a pivot (on or offset from the selection) · Arrow keys lock the rotation plane"
            }
            Some(rotation) if rotation.reference.is_none() => {
                "Click a reference point away from the pivot · Arrow keys lock the rotation plane · Esc to cancel"
            }
            Some(_) => {
                "Click to rotate · Type an angle or formula + Enter · Arrow keys lock the rotation plane · Esc to cancel"
            }
        }
    }

    pub(super) fn update_rotation(&mut self, pos: Pos2) {
        let lock = self.shape_lock();
        let size = dv(self.viewport.size());
        let cursor = dv(pos - self.viewport.min);
        let Some(edit) = self.modeling.as_mut() else {
            return;
        };
        let rotation = edit.rotation.as_mut().unwrap();
        rotation.normal = lock.map_or(rotation.surface_normal, Axis::vector);
        let normal = rotation.normal;
        edit.pointer = pos;
        edit.guides.clear();
        edit.snap = None;
        edit.direction = DVec3::ZERO;
        let plane_axis = [Axis::X, Axis::Y, Axis::Z]
            .into_iter()
            .find(|axis| normal.cross(axis.vector()).length() < 1e-7);
        let constraint = plane_axis.map(AxisConstraint::Plane);
        let inferred = edit.move_snap.as_mut().unwrap().infer_with_constraint(
            &self.doc.camera,
            size,
            cursor,
            edit.anchor,
            constraint,
            self.doc.snap_to_geometry,
            self.doc.snap_to_axes,
            ketchup_core::inference::SnapTargets {
                edges: self.snap_to_edge,
                faces: self.snap_to_face,
                face_centers: self.snap_to_face_center,
            },
        );
        let snap = inferred
            .filter(|inference| {
                inference.snap.kind != Inference::Plane
                    && (inference.snap.point - edit.anchor).dot(normal).abs() < 1e-6
            })
            .map(|inference| {
                edit.guides = inference.guides;
                inference.snap
            })
            .or_else(|| {
                let ray = self.doc.camera.ray(cursor, size);
                let denominator = ray.direction.dot(normal);
                if denominator.abs() < 1e-8 {
                    return None;
                }
                let t = (edit.anchor - ray.origin).dot(normal) / denominator;
                (0.0..1e7).contains(&t).then_some(Snap {
                    point: ray.origin + ray.direction * t,
                    kind: Inference::Plane,
                })
            });
        if let Some(snap) = snap {
            let snap = if self.snap_to_grid {
                grid::snap_with_geometry(
                    snap,
                    grid::configured_spacing(&self.doc.camera, &self.doc.display),
                    Some(normal),
                    grid::movement_axis(
                        snap.point,
                        Some(edit.anchor),
                        constraint,
                        self.doc.snap_to_axes,
                    ),
                    &edit.base,
                    rotation.reference.is_some().then_some(&edit.snap_excluded),
                )
            } else {
                snap
            };
            edit.direction = snap.point - edit.anchor;
            edit.snap = Some(snap);
        }
        edit.value = 0.;
        edit.error = None;
        edit.preview = None;
        let Some(reference) = rotation.reference else {
            return;
        };
        let reference = reference - normal * reference.dot(normal);
        let target = edit.direction;
        if reference.length_squared() < 1e-12 {
            edit.error = Some(
                "The reference lies on the rotation axis; clear the lock or restart with another reference",
            );
            return;
        }
        if target.length_squared() > 1e-12 {
            edit.value = normal
                .dot(reference.cross(target))
                .atan2(reference.dot(target))
                .to_degrees();
        }
        let value = precision::modeling_value_with_units(
            Tool::Rotate,
            &self.length_input,
            self.doc.display.units,
        )
        .unwrap_or(edit.value);
        match Self::modeling_result(Tool::Rotate, edit, value) {
            Ok(model) => edit.preview = Some(model),
            Err(message) => edit.error = Some(message),
        }
    }

    // Returns true while the reference phase handles the click/Enter itself.
    pub(super) fn choose_rotation_reference(&mut self, ctx: &egui::Context) -> bool {
        let Some(edit) = self.modeling.as_mut() else {
            return false;
        };
        let rotation = edit.rotation.as_mut().unwrap();
        if rotation.reference.is_some() {
            return false;
        }
        if edit.direction.length_squared() < 1e-12 {
            self.say(ctx, "Click a reference point away from the pivot first");
            return true;
        }
        rotation.reference = Some(edit.direction);
        edit.move_snap = Some(ketchup_core::inference::MoveSnapCache::excluding(
            &edit.base,
            &edit.snap_excluded,
        ));
        self.length_input.clear();
        let pointer = edit.pointer;
        self.update_rotation(pointer);
        true
    }

    pub(super) fn rotation_marker(edit: &Modeling, degrees: f64) -> Option<DVec3> {
        let rotation = edit.rotation.as_ref()?;
        let reference = rotation.reference?;
        let reference = reference - rotation.normal * reference.dot(rotation.normal);
        Some(
            edit.anchor
                + glam::DMat3::from_axis_angle(rotation.normal, degrees.to_radians()) * reference,
        )
    }

    pub(super) fn paint_rotation(&self, painter: &egui::Painter, rect: Rect) {
        let Some(edit) = self.modeling.as_ref() else {
            return;
        };
        let rotation = edit.rotation.as_ref().unwrap();
        let size = dv(rect.size());
        let screen = |point| {
            self.doc
                .camera
                .project(point, size)
                .map(|p| rect.min + pv(p))
        };
        let color = self
            .shape_lock()
            .map_or_else(|| self.inference_color(), |axis| self.axis_color(axis));
        let stroke = Stroke::new(1.5, color);
        let value = precision::modeling_value_with_units(
            Tool::Rotate,
            &self.length_input,
            self.doc.display.units,
        )
        .unwrap_or(edit.value);
        if let Some(reference) = rotation.reference {
            let reference = reference - rotation.normal * reference.dot(rotation.normal);
            let endpoint = Self::rotation_marker(edit, value).unwrap();
            if let (Some(center), Some(start), Some(end)) = (
                screen(edit.anchor),
                screen(edit.anchor + reference),
                screen(endpoint),
            ) {
                painter.line_segment([center, start], stroke);
                painter.line_segment([center, end], stroke);
                painter.circle_stroke(start, 3.5, stroke);
                painter.circle_stroke(end, 4., stroke);
            }
            let radius = reference.length() * 0.3;
            if let Some(unit) = reference.try_normalize() {
                // Bound the arc even for large typed angles while retaining full rotations.
                let degrees = value.clamp(-360., 360.);
                let steps = ((degrees.abs() / 5.).ceil() as usize).max(1);
                let points: Vec<_> = (0..=steps)
                    .filter_map(|i| {
                        let angle = (degrees * i as f64 / steps as f64).to_radians();
                        screen(
                            edit.anchor
                                + glam::DMat3::from_axis_angle(rotation.normal, angle)
                                    * unit
                                    * radius,
                        )
                    })
                    .collect();
                if points.len() > 1 {
                    painter.add(egui::Shape::line(points, stroke));
                }
            }
        } else if let (Some(center), Some(end)) =
            (screen(edit.anchor), screen(edit.anchor + edit.direction))
        {
            painter.line_segment([center, end], stroke);
        }
    }
}
