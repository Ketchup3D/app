use super::*;
use ketchup_core::ConstructionGuide;

impl KetchupApp {
    pub(super) fn guide_modifier(
        &mut self,
        ctx: &egui::Context,
        modifiers: egui::Modifiers,
        shortcuts: bool,
    ) {
        let down = modifiers.ctrl || modifiers.command || modifiers.mac_cmd;
        let pressed = down && !self.guide_modifier_down;
        let released = !down && self.guide_modifier_down;
        self.guide_modifier_down = down;
        let eligible =
            ctx.input(|i| i.focused) && matches!(self.tool, Tool::TapeMeasure | Tool::Protractor);
        if pressed {
            self.guide_toggle_pending = eligible;
        }
        if shortcuts || !eligible {
            self.guide_toggle_pending = false;
        }
        // Commit a standalone modifier tap on release, so Ctrl/Cmd shortcuts
        // (including ones whose key arrives in a later frame) never change mode.
        let toggle = released && self.guide_toggle_pending;
        if released {
            self.guide_toggle_pending = false;
        }
        if toggle {
            self.create_guides = !self.create_guides;
            self.length_input.clear();
            if self.measurement.as_ref().is_some_and(|m| m.complete) {
                self.measurement = None;
            }
            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                self.update_measurement(pos);
            }
            ctx.request_repaint();
        }
    }

    pub(super) fn construction_snap(
        &self,
        pos: Pos2,
        start: Option<DVec3>,
        lock: Option<Axis>,
    ) -> Option<Snap> {
        if !self.doc.display.guides || !self.doc.snap_to_geometry {
            return None;
        }
        let size = dv(self.viewport.size());
        let cursor = dv(pos - self.viewport.min);
        let ray = self.doc.camera.ray(cursor, size);
        let constraint = lock.map(|axis| {
            self.plane_lock()
                .map_or(AxisConstraint::Axis(axis), AxisConstraint::Plane)
        });
        let mut nearby = vec![];
        for guide in self.doc.model.guides.values() {
            let point = match guide.direction {
                None => guide.origin,
                Some(direction) => match start.zip(constraint) {
                    Some((start, AxisConstraint::Axis(axis))) => {
                        let Some(point) = (ketchup_core::Ray {
                            origin: start,
                            direction: axis.vector(),
                        })
                        .axis(guide.origin, direction) else {
                            continue;
                        };
                        if !AxisConstraint::Axis(axis).contains(point - start) {
                            continue;
                        }
                        point
                    }
                    Some((start, AxisConstraint::Plane(normal)))
                        if direction.dot(normal.vector()).abs() > 1e-9 =>
                    {
                        guide.origin
                            + direction
                                * ((start - guide.origin).dot(normal.vector())
                                    / direction.dot(normal.vector()))
                    }
                    _ => {
                        let Some(point) = ray.axis(guide.origin, direction) else {
                            continue;
                        };
                        point
                    }
                },
            };
            if start
                .zip(constraint)
                .is_some_and(|(start, constraint)| !constraint.contains(point - start))
            {
                continue;
            }
            let Some(screen) = self.doc.camera.project(point, size) else {
                continue;
            };
            let distance = screen.distance(cursor);
            if distance < 8. {
                nearby.push((guide, point, distance));
            }
        }
        // Intersections are exact targets even when the cursor is a few pixels away.
        for (i, (a, _, _)) in nearby.iter().enumerate() {
            let Some(ad) = a.direction else {
                continue;
            };
            for (b, _, _) in &nearby[i + 1..] {
                let Some(bd) = b.direction else {
                    continue;
                };
                let Some(point) = (ketchup_core::Ray {
                    origin: a.origin,
                    direction: ad,
                })
                .axis(b.origin, bd) else {
                    continue;
                };
                if (point - a.origin).cross(ad).length() < 1e-6
                    && start
                        .zip(constraint)
                        .is_none_or(|(start, c)| c.contains(point - start))
                    && self
                        .doc
                        .camera
                        .project(point, size)
                        .is_some_and(|p| p.distance(cursor) < 11.)
                {
                    return Some(Snap {
                        point,
                        kind: Inference::GuidePoint,
                    });
                }
            }
        }
        nearby
            .into_iter()
            .min_by(|a, b| {
                a.0.direction
                    .is_some()
                    .cmp(&b.0.direction.is_some())
                    .then(a.2.total_cmp(&b.2))
            })
            .map(|(guide, point, _)| Snap {
                point,
                kind: if guide.direction.is_some() {
                    Inference::OnGuide
                } else {
                    Inference::GuidePoint
                },
            })
    }

    pub(super) fn tape_reference_direction(&self, snap: Snap) -> Option<DVec3> {
        if snap.kind == Inference::OnGuide {
            return self.doc.model.guides.values().find_map(|g| {
                let direction = g.direction?;
                ((snap.point - g.origin).cross(direction).length() < 1e-6).then_some(direction)
            });
        }
        if !matches!(
            snap.kind,
            Inference::OnEdge | Inference::Midpoint | Inference::Origin
        ) {
            return None;
        }
        self.doc
            .model
            .edges
            .values()
            .filter(|e| !self.doc.model.is_hidden(e.id))
            .find_map(|e| {
                let (a, b) = self.doc.model.points(e);
                let direction = (b - a).normalize();
                let t = (snap.point - a).dot(direction);
                (t > 1e-7
                    && t < a.distance(b) - 1e-7
                    && (snap.point - a).cross(direction).length() < 1e-6)
                    .then_some(direction)
            })
    }

    pub(super) fn delete_construction_guides(&mut self, ctx: &egui::Context) {
        let before = self.doc.model.clone();
        self.doc.model.guides.clear();
        self.finish_edit("Delete guides", before, ctx);
    }

    pub(super) fn paint_construction_guides(&self, p: &egui::Painter, rect: Rect) {
        if !self.doc.display.guides {
            return;
        }
        let color = Palette::for_painter(p).muted;
        let model = self.history_preview.as_ref().unwrap_or(&self.doc.model);
        for guide in model.guides.values() {
            self.paint_construction_guide(p, rect, guide, color);
        }
        if self.history_preview.is_none()
            && self.create_guides
            && matches!(self.tool, Tool::TapeMeasure | Tool::Protractor)
            && let Some(m) = self.measurement.as_ref()
        {
            let points = self.measurement_points();
            let preview = if self.tool == Tool::TapeMeasure && points.len() == 2 {
                Some(ConstructionGuide {
                    origin: points[1],
                    direction: m.guide_direction,
                })
            } else if self.tool == Tool::Protractor && points.len() == 3 {
                (points[2] - points[0])
                    .try_normalize()
                    .map(|direction| ConstructionGuide {
                        origin: points[0],
                        direction: Some(direction),
                    })
            } else {
                None
            };
            if let Some(guide) = preview {
                self.paint_construction_guide(p, rect, &guide, Palette::for_painter(p).green);
            }
        }
    }

    fn paint_construction_guide(
        &self,
        p: &egui::Painter,
        rect: Rect,
        guide: &ConstructionGuide,
        color: Color32,
    ) {
        let camera = &self.doc.camera;
        let size = dv(rect.size());
        if let Some(direction) = guide.direction {
            // Center the drawn span near the view, so distant guide origins do not clip it.
            let center = guide.origin + direction * (camera.target - guide.origin).dot(direction);
            let extent = camera.distance.max(1.) * 100.;
            if let Some((a, b)) = camera.segment(
                center - direction * extent,
                center + direction * extent,
                size,
            ) {
                p.extend(egui::Shape::dashed_line(
                    &[rect.min + pv(a), rect.min + pv(b)],
                    Stroke::new(1., color),
                    6.,
                    4.,
                ));
            }
        } else if let Some(point) = camera.project(guide.origin, size) {
            let point = rect.min + pv(point);
            for delta in [egui::vec2(4., 4.), egui::vec2(4., -4.)] {
                p.line_segment([point - delta, point + delta], Stroke::new(1.5, color));
            }
        }
    }
}
