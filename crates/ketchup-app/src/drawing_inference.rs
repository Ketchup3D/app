//! Hovering geometry acquires a reference for subsequent axis alignment.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct DrawingReference {
    edge: u64,
    point: DVec3,
}

impl KetchupApp {
    pub(super) fn active_drawing_projection(&self) -> Option<(DVec3, DVec3)> {
        let projection = self.drawing_projection?;
        let point = if self.tool == Tool::Line {
            self.line_preview_snap()?.point
        } else {
            self.snap?.point
        };
        (point.distance(projection.1) < 1e-6).then_some(projection)
    }

    pub(super) fn active_drawing_guide(&self) -> Option<AxisGuide> {
        let guide = self.drawing_guide?;
        let point = if self.tool == Tool::Line {
            self.line_preview_snap()?.point
        } else {
            self.snap?.point
        };
        ((point - guide.origin).cross(guide.axis.vector()).length() < 1e-6).then_some(guide)
    }

    fn reference_edge_contains(&self, edge: u64, point: DVec3) -> bool {
        let Some(edge) = self.doc.model.edges.get(&edge) else {
            return false;
        };
        if self.entity_cache.get(edge.id).hidden {
            return false;
        }
        let (a, b) = self.doc.model.points(edge);
        let t = (point - a).dot(b - a) / a.distance_squared(b);
        (-1e-7..=1. + 1e-7).contains(&t) && (a + (b - a) * t).distance(point) < 1e-6
    }

    pub(super) fn update_drawing_inference(&mut self, pos: Pos2) {
        self.drawing_guide = None;
        self.drawing_projection = None;
        if !(self.tool.is_drawing() || self.tool.is_measuring())
            || self.paste_preview.is_some()
            || !self.doc.snap_to_geometry
            || !self.doc.snap_to_axes
        {
            self.drawing_reference = None;
            return;
        }
        let Some(base) = self.snap else {
            return;
        };
        let geometry = |snap: Snap| {
            matches!(
                snap.kind,
                Inference::Endpoint | Inference::Midpoint | Inference::Origin | Inference::OnEdge
            )
        };
        // A lock or drawing plane may reject the hovered point as a target;
        // it can still supply a reference for a later compatible intersection.
        let source = if geometry(base) {
            Some(base)
        } else if self.lock.is_some() || self.hover.is_some() {
            // Reference geometry is not an editable selection. Query its world
            // position even when it belongs to a different group/component.
            self.infer_snap(pos, None, None, false)
                .filter(|snap| geometry(*snap))
        } else {
            None
        };
        if let Some(source) = source {
            let existing = self.drawing_reference.filter(|r| {
                r.point.distance(source.point) < 1e-7
                    && self.reference_edge_contains(r.edge, source.point)
            });
            self.drawing_reference = existing.or_else(|| {
                self.doc
                    .model
                    .edges
                    .keys()
                    .find(|&&id| self.reference_edge_contains(id, source.point))
                    .map(|&edge| DrawingReference {
                        edge,
                        point: source.point,
                    })
            });
        }
        let Some(reference) = self.drawing_reference else {
            return;
        };
        let size = dv(self.viewport.size());
        if !self.reference_edge_contains(reference.edge, reference.point)
            || !self.picking_cache.visible(
                &self.face_cache,
                &self.entity_cache,
                &self.doc.camera,
                size,
                reference.point,
            )
        {
            self.drawing_reference = None;
            return;
        }
        // Exact geometry wins over projected guides.
        if geometry(base) {
            return;
        }
        let start = self.line_start.or_else(|| {
            self.measurement
                .as_ref()
                .filter(|m| !m.complete)?
                .points
                .first()
                .copied()
        });
        let surface = (base.kind == Inference::OnFace)
            .then(|| {
                self.picking_cache.nearest(
                    &self.face_cache,
                    &self.entity_cache,
                    self.doc.camera.ray(dv(pos - self.viewport.min), size),
                    false,
                )
            })
            .flatten()
            .filter(|(id, _, _)| {
                self.doc
                    .model
                    .face_contains(&self.doc.model.faces[id], base.point, true)
            });
        let plane = self
            .drawing
            .as_ref()
            .map(|d| (d.origin, d.normal))
            .or_else(|| self.measurement_plane())
            .or_else(|| surface.map(|(id, _, _)| (base.point, self.face_cache.get(id).normal)));
        if let Some((start, axis)) = start.zip(self.lock) {
            // The cursor selects the reference, not the inferred endpoint: the
            // endpoint can be far away on screen and still match its coordinate.
            let point = start + axis.vector() * (reference.point - start).dot(axis.vector());
            let nearby = point.distance(start) > 1e-7
                && self
                    .doc
                    .camera
                    .project(point, size)
                    .is_some_and(|p| p.distance(dv(pos - self.viewport.min)) <= 10.);
            if (source.is_some() || nearby)
                && plane.is_none_or(|(origin, normal)| (point - origin).dot(normal).abs() < 1e-6)
                && surface.is_none_or(|(id, _, _)| {
                    self.doc
                        .model
                        .face_contains(&self.doc.model.faces[&id], point, true)
                })
                && self.doc.camera.project(point, size).is_some()
            {
                self.snap = Some(Snap {
                    point,
                    kind: Inference::Aligned,
                });
                self.drawing_projection = Some((reference.point, point));
                return;
            }
        }
        let constraint = start.zip(self.lock.or_else(|| {
            let delta = base.point - start?;
            (delta.length() > 1e-7)
                .then(|| {
                    [Axis::X, Axis::Y, Axis::Z]
                        .into_iter()
                        .find(|axis| delta.cross(axis.vector()).length() < 1e-7)
                })
                .flatten()
        }));
        let infer = |constraint| {
            ketchup_core::inference::infer_reference_axis(
                &self.doc.camera,
                size,
                dv(pos - self.viewport.min),
                reference.point,
                constraint,
                plane,
            )
        };
        let result =
            infer(constraint).or_else(|| (self.lock.is_none()).then(|| infer(None)).flatten());
        if let Some((snap, guide)) = result {
            if surface.is_some_and(|(id, _, _)| {
                !self
                    .doc
                    .model
                    .face_contains(&self.doc.model.faces[&id], snap.point, true)
            }) {
                return;
            }
            if self.picking_cache.visible(
                &self.face_cache,
                &self.entity_cache,
                &self.doc.camera,
                size,
                snap.point,
            ) {
                self.snap = Some(snap);
                self.drawing_guide = Some(guide);
            }
        }
    }
}
