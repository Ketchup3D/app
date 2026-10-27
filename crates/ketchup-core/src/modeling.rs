//! Atomic, validated modeling operations shared by native and browser tools.
use crate::{DVec2, DVec3, Face, Model, Vertex, document::EPS, faces};
use std::collections::{BTreeMap, BTreeSet};

impl Model {
    pub fn selection_vertices(&self, selected: &BTreeSet<u64>) -> BTreeSet<u64> {
        let mut vertices = BTreeSet::new();
        for id in self.expand_entities(selected) {
            if let Some(edge) = self.edges.get(&id) {
                vertices.extend([edge.a, edge.b]);
            }
            if let Some(face) = self.faces.get(&id) {
                vertices.extend(face.vertices.iter().copied());
                vertices.extend(face.holes.iter().flatten().copied());
            }
        }
        vertices
    }

    pub fn selection_center(&self, selected: &BTreeSet<u64>) -> Option<DVec3> {
        let ids = self.selection_vertices(selected);
        let entities = self.expand_entities(selected);
        let points = ids.iter().map(|id| self.vertices[id].position).chain(
            entities
                .iter()
                .filter_map(|id| self.dimensions.get(id))
                .flat_map(|d| d.layout(self)),
        );
        let mut points = points.chain(
            entities
                .iter()
                .filter_map(|id| self.texts.get(id))
                .flat_map(|t| t.layout(self)),
        );
        let first = points.next()?;
        let (lo, hi) = points.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
        Some((lo + hi) * 0.5)
    }

    fn transform_selection(
        &mut self,
        selected: &BTreeSet<u64>,
        f: impl Fn(DVec3) -> DVec3,
    ) -> Result<(), &'static str> {
        if self.selection_locked(selected) {
            return Err("Unlock the selection before editing it");
        }
        let vertices = self.selection_vertices(selected);
        let dimensions: Vec<_> = self
            .expand_entities(selected)
            .into_iter()
            .filter(|id| self.dimensions.contains_key(id))
            .collect();
        let texts: Vec<_> = self
            .expand_entities(selected)
            .into_iter()
            .filter(|id| self.texts.contains_key(id))
            .collect();
        if vertices.is_empty() && dimensions.is_empty() && texts.is_empty() {
            return Err("Select geometry, a dimension, or text first");
        }
        let protected: BTreeSet<_> = self
            .edges
            .keys()
            .chain(self.faces.keys())
            .copied()
            .filter(|id| self.is_locked(*id))
            .collect();
        if !vertices.is_disjoint(&self.selection_vertices(&protected)) {
            return Err("Unlock the connected geometry before editing it");
        }
        let mut next = self.clone();
        for id in dimensions {
            let d = &self.dimensions[&id];
            let (a, b) = d.points(self);
            let changed = next.dimensions.get_mut(&id).unwrap();
            changed.start.point = f(a);
            changed.end.point = f(b);
            changed.offset = f(a + d.offset) - f(a);
            for anchor in [&mut changed.start, &mut changed.end] {
                if anchor.weights.iter().any(|(id, _)| !vertices.contains(id)) {
                    anchor.weights.clear();
                }
            }
        }
        for id in texts {
            let source = &self.texts[&id];
            let [a, b] = source.layout(self);
            let changed = next.texts.get_mut(&id).unwrap();
            let attached_geometry_moves = !source.anchor.weights.is_empty()
                && source
                    .anchor
                    .weights
                    .iter()
                    .all(|(id, _)| vertices.contains(id));
            // Moving a label alone repositions the text without losing its attachment.
            let anchor = if attached_geometry_moves || source.anchor.weights.is_empty() {
                f(a)
            } else {
                a
            };
            changed.anchor.point = anchor;
            changed.offset = f(b) - anchor;
        }
        for id in vertices {
            next.vertices.get_mut(&id).unwrap().position = f(self.vertices[&id].position);
        }
        next.sync_dimensions();
        next.validate().map_err(
            |_| "This change would collapse an edge or bend a face; select its connected geometry",
        )?;
        *self = next;
        Ok(())
    }

    pub fn move_selection(
        &mut self,
        selected: &BTreeSet<u64>,
        delta: DVec3,
    ) -> Result<(), &'static str> {
        if !delta.is_finite() {
            return Err("Enter a finite displacement");
        }
        if self.edit_context.is_some() {
            return self.edit_in_context(|model| model.move_selection(selected, delta));
        }
        self.transform_selection(selected, |p| p + delta)?;
        for id in self.expand_entities(selected) {
            if let Some(object) = self.objects.get_mut(&id) {
                object.transform = Some(object.transform.unwrap_or_default().translated(delta));
            }
        }
        Ok(())
    }

    /// Mirror the selection about its bounds center without detaching component instances.
    pub fn flip_selection(
        &mut self,
        selected: &BTreeSet<u64>,
        axis: crate::Axis,
    ) -> Result<(), &'static str> {
        if self.edit_context.is_some() {
            return self.edit_in_context(|model| model.flip_selection(selected, axis));
        }
        let center = self
            .selection_center(selected)
            .ok_or("Select geometry to flip first")?;
        let normal = axis.vector();
        let mirror = |p: DVec3| p - normal * 2. * (p - center).dot(normal);
        self.transform_selection(selected, mirror)?;
        let signs = DVec3::ONE - normal * 2.;
        for id in self.expand_entities(selected) {
            if let Some(object) = self.objects.get_mut(&id) {
                let mut frame = object.transform.unwrap_or_default();
                frame.translation = mirror(frame.translation);
                frame.axes *= signs;
                object.transform = Some(frame);
            }
        }
        Ok(())
    }

    pub fn scale_selection(
        &mut self,
        selected: &BTreeSet<u64>,
        center: DVec3,
        factor: f64,
    ) -> Result<(), &'static str> {
        if !factor.is_finite() || factor <= EPS || factor > 1e7 {
            return Err("Enter a positive scale factor, such as 2 or 0.5");
        }
        if self.edit_context.is_some() {
            return self.edit_in_context(|model| model.scale_selection(selected, center, factor));
        }
        self.transform_selection(selected, |p| center + (p - center) * factor)?;
        for id in self.expand_entities(selected) {
            if let Some(object) = self.objects.get_mut(&id) {
                object.transform =
                    Some(object.transform.unwrap_or_default().scaled(center, factor));
            }
        }
        Ok(())
    }

    pub fn paint_face(&mut self, face: u64, color: [u8; 3]) -> Result<(), &'static str> {
        if self.edit_context.is_some() {
            return self.edit_in_context(|model| model.paint_face(face, color));
        }
        if self.is_locked(face) {
            return Err("Unlock the face before painting it");
        }
        if !self.faces.contains_key(&face) {
            return Err("Click a filled face to paint it");
        }
        // A component's material belongs to its definition; update matching faces
        // in all instances while retaining each instance's position and scale.
        let definition = self
            .objects
            .values()
            .filter_map(|object| {
                let crate::ObjectKind::Component { definition } = object.kind else {
                    return None;
                };
                object
                    .definition_entities
                    .iter()
                    .find(|(_, id)| **id == face)
                    .map(|(key, _)| (definition, *key))
            })
            .next();
        let mut faces = BTreeSet::from([face]);
        if let Some((definition, key)) = definition {
            faces.extend(self.objects.values().filter_map(|object| {
                (object.kind == crate::ObjectKind::Component { definition })
                    .then(|| object.definition_entities.get(&key).copied())
                    .flatten()
            }));
        }
        for face in faces {
            self.face_colors.insert(face, color);
        }
        Ok(())
    }

    /// Paint all descendant faces as one atomic edit, preserving component materials.
    pub fn paint_object(&mut self, object: u64, color: [u8; 3]) -> Result<(), &'static str> {
        if self.edit_context.is_some() {
            return self.edit_in_context(|model| model.paint_object(object, color));
        }
        let object = self
            .objects
            .get(&object)
            .ok_or("Click a face in a group or component to paint it")?;
        if self.is_locked(object.id) {
            return Err("Unlock the group or component before painting it");
        }
        let faces: Vec<_> = self
            .expand_entities(&object.entities)
            .into_iter()
            .filter(|id| self.faces.contains_key(id))
            .collect();
        if faces.is_empty() {
            return Err("The group or component has no faces to paint");
        }
        if faces.iter().any(|id| self.is_locked(*id)) {
            return Err("Unlock all faces in the group or component before painting it");
        }
        let mut next = self.clone();
        for face in faces {
            next.paint_face(face, color)?;
        }
        *self = next;
        Ok(())
    }

    fn editable_surface(&self, face: u64) -> Result<(), &'static str> {
        if self.is_locked(face) {
            return Err("Unlock the object before editing it");
        }
        if self.parent(face).is_some() {
            return Err("Double-click the object to edit its faces");
        }
        Ok(())
    }

    fn raw_vertex(&mut self, position: DVec3) -> u64 {
        let id = self.id();
        self.vertices.insert(id, Vertex { id, position });
        id
    }
    fn raw_face(
        &mut self,
        vertices: Vec<u64>,
        holes: Vec<Vec<u64>>,
        color: Option<[u8; 3]>,
    ) -> u64 {
        for ring in std::iter::once(&vertices).chain(holes.iter()) {
            for i in 0..ring.len() {
                self.insert_edge(ring[i], ring[(i + 1) % ring.len()]);
            }
        }
        let id = self.id();
        self.faces.insert(
            id,
            Face {
                id,
                vertices,
                holes,
            },
        );
        if let Some(color) = color {
            self.face_colors.insert(id, color);
        }
        id
    }

    /// Extrude a planar face, including its hole walls. Keep a base only when the
    /// face is not joined to a coplanar surface, so inset regions open into recesses.
    pub fn push_pull(&mut self, face_id: u64, distance: f64) -> Result<u64, &'static str> {
        if self.edit_context.is_some() {
            return self.edit_in_context(|model| model.push_pull(face_id, distance));
        }
        self.editable_surface(face_id)?;
        if !distance.is_finite() || distance.abs() < EPS || distance.abs() > 1e7 {
            return Err("Enter a nonzero distance in meters");
        }
        let face = self
            .faces
            .get(&face_id)
            .ok_or("Click a filled face first")?
            .clone();
        let normal = faces::plane(self, &face.vertices)
            .ok_or("The face must be planar")?
            .normal;
        let color = self.face_colors.get(&face_id).copied();
        // On an existing solid, moving the cap stretches the adjoining walls.
        let boundary: Vec<_> = std::iter::once(&face.vertices)
            .chain(face.holes.iter())
            .flat_map(|ring| (0..ring.len()).map(move |i| (ring[i], ring[(i + 1) % ring.len()])))
            .collect();
        let shares_edge = |other: &Face, a, b| {
            std::iter::once(&other.vertices)
                .chain(other.holes.iter())
                .any(|ring| {
                    (0..ring.len()).any(|i| {
                        let (c, d) = (ring[i], ring[(i + 1) % ring.len()]);
                        (a == c && b == d) || (a == d && b == c)
                    })
                })
        };
        if boundary.iter().all(|&(a, b)| {
            self.faces.values().any(|other| {
                other.id != face_id
                    && self.parent(other.id).is_none()
                    && faces::plane(self, &other.vertices)
                        .is_some_and(|p| p.normal.cross(normal).length() > EPS)
                    && shares_edge(other, a, b)
            })
        }) {
            self.move_selection(&BTreeSet::from([face_id]), normal * distance)?;
            return Ok(face_id);
        }
        let mut next = self.clone();
        let joined_surface = self.faces.values().any(|other| {
            other.id != face_id
                && self.parent(other.id).is_none()
                && faces::plane(self, &other.vertices)
                    .is_some_and(|p| p.normal.cross(normal).length() <= EPS)
                && boundary.iter().any(|&(a, b)| shares_edge(other, a, b))
        });
        if joined_surface {
            // The surrounding surface already supplies the opening's boundary.
            // Leaving this face behind would cover the recess and add an internal cap.
            next.erase(&BTreeSet::from([face_id]));
        }
        let mut top = BTreeMap::new();
        for id in std::iter::once(&face.vertices)
            .chain(face.holes.iter())
            .flatten()
        {
            top.entry(*id)
                .or_insert_with(|| next.raw_vertex(self.vertices[id].position + normal * distance));
        }
        for ring in std::iter::once(&face.vertices).chain(face.holes.iter()) {
            for i in 0..ring.len() {
                let a = ring[i];
                let b = ring[(i + 1) % ring.len()];
                next.raw_face(vec![a, b, top[&b], top[&a]], vec![], color);
            }
        }
        let cap = next.raw_face(
            face.vertices.iter().map(|id| top[id]).collect(),
            face.holes
                .iter()
                .map(|r| r.iter().map(|id| top[id]).collect())
                .collect(),
            color,
        );
        next.validate()?;
        *self = next;
        Ok(cap)
    }

    /// Positive offsets inset the surface; negative offsets extend it outward.
    pub fn offset_face(&mut self, face_id: u64, distance: f64) -> Result<(), &'static str> {
        if self.edit_context.is_some() {
            return self.edit_in_context(|model| model.offset_face(face_id, distance));
        }
        self.editable_surface(face_id)?;
        if !distance.is_finite() || distance.abs() < EPS || distance.abs() > 1e7 {
            return Err("Enter a nonzero offset in meters");
        }
        let face = self
            .faces
            .get(&face_id)
            .ok_or("Click a filled face first")?;
        let plane = faces::plane(self, &face.vertices).ok_or("The face must be planar")?;
        let rings: Vec<_> = std::iter::once(&face.vertices)
            .chain(face.holes.iter())
            .enumerate()
            .map(|(i, r)| {
                let points: Vec<_> = r
                    .iter()
                    .map(|id| plane.project(self.vertices[id].position))
                    .collect();
                offset_ring(&points, if i == 0 { distance } else { -distance })
            })
            .collect::<Result<_, _>>()?;
        // Reject colliding boundaries before touching the document.
        for i in 1..rings.len() {
            if !rings[i]
                .iter()
                .all(|p| faces::in_polygon(*p, &rings[0], false))
            {
                return Err("Offset is too large for this face");
            }
            for j in 0..i {
                if rings_intersect(&rings[i], &rings[j])
                    || (j > 0
                        && (faces::in_polygon(rings[i][0], &rings[j], true)
                            || faces::in_polygon(rings[j][0], &rings[i], true)))
                {
                    return Err("Offset boundaries would overlap");
                }
            }
        }
        let mut next = self.clone();
        let color = self.face_colors.get(&face_id).copied();
        let old_faces: BTreeSet<_> = self.faces.keys().copied().collect();
        for ring in rings {
            for i in 0..ring.len() {
                let world = |p: DVec2| plane.origin + plane.u * p.x + plane.v * p.y;
                next.add_edge(world(ring[i]), world(ring[(i + 1) % ring.len()]))?;
            }
        }
        next.face_colors.retain(|id, _| next.faces.contains_key(id));
        if let Some(color) = color {
            for id in next.faces.keys().filter(|id| !old_faces.contains(id)) {
                next.face_colors.insert(*id, color);
            }
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
}

fn rings_intersect(a: &[DVec2], b: &[DVec2]) -> bool {
    (0..a.len()).any(|i| {
        (0..b.len()).any(|j| {
            crate::selection::segment_distance(
                a[i],
                a[(i + 1) % a.len()],
                b[j],
                b[(j + 1) % b.len()],
            ) < EPS
        })
    })
}
fn offset_ring(points: &[DVec2], distance: f64) -> Result<Vec<DVec2>, &'static str> {
    let area = |p: &[DVec2]| {
        (0..p.len())
            .map(|i| p[i].perp_dot(p[(i + 1) % p.len()]))
            .sum::<f64>()
            * 0.5
    };
    let sign = area(points).signum();
    let mut result = Vec::new();
    for i in 0..points.len() {
        let a = points[(i + points.len() - 1) % points.len()];
        let b = points[i];
        let c = points[(i + 1) % points.len()];
        let u = (b - a).normalize();
        let v = (c - b).normalize();
        let n = |d: DVec2| DVec2::new(-d.y, d.x) * sign * distance;
        let (p, q) = (b + n(u), b + n(v));
        let cross = u.perp_dot(v);
        let point = if cross.abs() < 1e-9 {
            if u.dot(v) < 0. {
                return Err("Cannot offset a folded boundary");
            }
            p
        } else {
            p + u * (q - p).perp_dot(v) / cross
        };
        if !point.is_finite() {
            return Err("Cannot offset this corner");
        }
        result.push(point);
    }
    let invalid = "Offset is too large or crosses itself";
    if area(&result) * area(points) <= EPS {
        return Err(invalid);
    }
    for i in 0..result.len() {
        if (result[(i + 1) % result.len()] - result[i])
            .dot(points[(i + 1) % points.len()] - points[i])
            <= EPS
        {
            return Err(invalid);
        }
        for j in i + 1..result.len() {
            if j == i + 1 || (i == 0 && j == result.len() - 1) {
                continue;
            }
            if crate::selection::segment_distance(
                result[i],
                result[(i + 1) % result.len()],
                result[j],
                result[(j + 1) % result.len()],
            ) < EPS
            {
                return Err(invalid);
            }
        }
        if distance > 0.
            && (!faces::in_polygon(result[i], points, false)
                || (0..points.len()).any(|j| {
                    crate::selection::distance_to_segment(
                        result[i],
                        points[j],
                        points[(j + 1) % points.len()],
                    ) < distance - 1e-6
                }))
        {
            return Err(invalid);
        }
        if distance < 0. && !faces::in_polygon(points[i], &result, false) {
            return Err(invalid);
        }
    }
    Ok(result)
}

/// A reusable translation of a fixed, valid model for interactive previews.
/// Only affected geometry is checked here; commits still use `move_selection`.
pub struct MovePreview {
    vertices: Vec<(u64, DVec3)>,
    dimensions: Vec<crate::Dimension>,
    texts: Vec<crate::TextAnnotation>,
    objects: Vec<(u64, crate::InstanceTransform)>,
    edges: Vec<u64>,
    faces: Vec<(u64, bool)>,
}
impl MovePreview {
    pub fn new(model: &Model, selected: &BTreeSet<u64>) -> Result<Self, &'static str> {
        if model.edit_context.is_some() {
            return Err("Scoped moves require component synchronization");
        }
        if model.selection_locked(selected) {
            return Err("Unlock the selection before editing it");
        }
        let vertices = model.selection_vertices(selected);
        let entities = model.expand_entities(selected);
        let dimensions: Vec<_> = entities
            .iter()
            .filter_map(|id| model.dimensions.get(id))
            .map(|d| {
                let mut d = d.clone();
                let (a, b) = d.points(model);
                d.start.point = a;
                d.end.point = b;
                for anchor in [&mut d.start, &mut d.end] {
                    if anchor.weights.iter().any(|(id, _)| !vertices.contains(id)) {
                        anchor.weights.clear();
                    }
                }
                d
            })
            .collect();
        let texts: Vec<_> = entities
            .iter()
            .filter_map(|id| model.texts.get(id))
            .cloned()
            .collect();
        if vertices.is_empty() && dimensions.is_empty() && texts.is_empty() {
            return Err("Select geometry, a dimension, or text first");
        }
        let protected = model
            .edges
            .keys()
            .chain(model.faces.keys())
            .copied()
            .filter(|id| model.is_locked(*id))
            .collect();
        if !vertices.is_disjoint(&model.selection_vertices(&protected)) {
            return Err("Unlock the connected geometry before editing it");
        }
        let edges = model
            .edges
            .values()
            .filter(|e| vertices.contains(&e.a) || vertices.contains(&e.b))
            .map(|e| e.id)
            .collect();
        let faces = model
            .faces
            .values()
            .filter_map(|face| {
                let mut ids = face.vertices.iter().chain(face.holes.iter().flatten());
                let affected = ids.clone().any(|id| vertices.contains(id));
                affected.then(|| (face.id, !ids.all(|id| vertices.contains(id))))
            })
            .collect();
        let objects = entities
            .iter()
            .filter_map(|id| model.objects.get(id))
            .map(|o| (o.id, o.transform.unwrap_or_default()))
            .collect();
        Ok(Self {
            vertices: vertices
                .into_iter()
                .map(|id| (id, model.vertices[&id].position))
                .collect(),
            dimensions,
            texts,
            objects,
            edges,
            faces,
        })
    }
    /// `preview` is a clone of the source passed to `new`, reused between frames.
    pub fn update(&self, preview: &mut Model, delta: DVec3) -> Result<(), &'static str> {
        const INVALID: &str =
            "This change would collapse an edge or bend a face; select its connected geometry";
        if !delta.is_finite() {
            return Err("Enter a finite displacement");
        }
        for &(id, point) in &self.vertices {
            let point = point + delta;
            if !point.is_finite() || point.abs().max_element() > 1e10 {
                return Err(INVALID);
            }
            preview.vertices.get_mut(&id).unwrap().position = point;
        }
        for source in &self.dimensions {
            let d = preview.dimensions.get_mut(&source.id).unwrap();
            d.clone_from(source);
            d.start.point += delta;
            d.end.point += delta;
            d.offset = (source.start.point + source.offset + delta) - (source.start.point + delta);
        }
        for source in &self.texts {
            let text = preview.texts.get_mut(&source.id).unwrap();
            text.clone_from(source);
            if source.anchor.weights.is_empty() {
                text.anchor.point += delta;
            } else if !source
                .anchor
                .weights
                .iter()
                .all(|(id, _)| self.vertices.iter().any(|(v, _)| v == id))
            {
                text.offset += delta;
            }
        }
        for &(id, frame) in &self.objects {
            preview.objects.get_mut(&id).unwrap().transform = Some(frame.translated(delta));
        }
        preview.sync_dimensions();
        if self
            .edges
            .iter()
            .any(|id| preview.edge_length(&preview.edges[id]) < EPS)
        {
            return Err(INVALID);
        }
        for &(id, partial) in &self.faces {
            let face = &preview.faces[&id];
            if std::iter::once(&face.vertices)
                .chain(face.holes.iter())
                .any(|ring| faces::plane(preview, ring).is_none())
                || (partial && preview.face_area(face) <= EPS)
            {
                return Err(INVALID);
            }
        }
        preview.validate_dimensions().map_err(|_| INVALID)?;
        preview.validate_texts().map_err(|_| INVALID)?;
        Ok(())
    }
}
