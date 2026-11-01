//! Conservative removal of redundant degree-two vertices on straight edges.
use crate::{Model, document::EPS};
use std::collections::{BTreeMap, BTreeSet};

impl Model {
    /// Merge selected straight edge runs, preserving branches, annotations,
    /// metadata boundaries, face IDs, and linked component definitions.
    pub fn clean_collinear_vertices(
        &mut self,
        selected: &BTreeSet<u64>,
    ) -> Result<usize, &'static str> {
        if selected.is_empty() {
            return Ok(0);
        }
        if self.selection_protected(selected) {
            return Err("Unlock the selection before cleaning it");
        }
        if self.edit_context.is_some() {
            return self.edit_in_context(|contents| contents.clean_collinear_vertices(selected));
        }
        let mut next = self.clone();
        let mut removed = 0;
        // Edit each selected object through the existing component synchronization path.
        let objects: Vec<_> = selected
            .iter()
            .copied()
            .filter(|id| next.objects.contains_key(id) && next.parent(*id).is_none())
            .collect();
        for id in objects {
            if !next.objects.contains_key(&id) {
                continue;
            }
            let contents = next.objects[&id].entities.clone();
            next.edit_context = Some(id);
            let result = next.edit_in_context(|model| model.clean_collinear_vertices(&contents));
            next.edit_context = None;
            removed += result?;
        }
        removed += next.clean_loose_collinear_vertices(selected);
        if removed > 0 {
            next.validate()?;
            *self = next;
        }
        Ok(removed)
    }

    fn clean_loose_collinear_vertices(&mut self, selected: &BTreeSet<u64>) -> usize {
        let owners: BTreeSet<_> = self
            .objects
            .values()
            .flat_map(|object| object.entities.iter().copied())
            .collect();
        let mut allowed: BTreeSet<_> = selected
            .iter()
            .copied()
            .filter(|id| self.edges.contains_key(id) && !owners.contains(id))
            .collect();
        let face_pairs: BTreeSet<_> = selected
            .iter()
            .filter(|id| !owners.contains(id))
            .filter_map(|id| self.faces.get(id))
            .flat_map(|face| std::iter::once(&face.vertices).chain(&face.holes))
            .flat_map(|ring| (0..ring.len()).map(|i| pair(ring[i], ring[(i + 1) % ring.len()])))
            .collect();
        for edge in self.edges.values() {
            if !owners.contains(&edge.id) && face_pairs.contains(&pair(edge.a, edge.b)) {
                allowed.insert(edge.id);
            }
        }
        let protected: BTreeSet<_> = self
            .dimensions
            .values()
            .flat_map(|d| d.start.weights.iter().chain(&d.end.weights))
            .chain(
                self.texts
                    .values()
                    .flat_map(|text| text.anchor.weights.iter()),
            )
            .map(|(id, _)| *id)
            .collect();
        let mut adjacency: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
        let mut pairs = BTreeSet::new();
        for edge in self.edges.values() {
            adjacency.entry(edge.a).or_default().insert(edge.id);
            adjacency.entry(edge.b).or_default().insert(edge.id);
            pairs.insert(pair(edge.a, edge.b));
        }
        let mut faces_at: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
        for face in self.faces.values() {
            for vertex in face.vertices.iter().chain(face.holes.iter().flatten()) {
                faces_at.entry(*vertex).or_default().insert(face.id);
            }
        }
        let mut candidates: BTreeSet<_> = adjacency
            .iter()
            .filter(|(_, edges)| edges.len() == 2)
            .map(|(id, _)| *id)
            .collect();
        let mut removed = 0;
        while let Some(vertex) = candidates.pop_first() {
            if protected.contains(&vertex) {
                continue;
            }
            let Some(edges) = adjacency.get(&vertex).filter(|edges| edges.len() == 2) else {
                continue;
            };
            let mut ids = edges.iter();
            let first = *ids.next().unwrap();
            let second = *ids.next().unwrap();
            if !allowed.contains(&first) || !allowed.contains(&second) {
                continue;
            }
            let e0 = self.edges[&first].clone();
            let e1 = self.edges[&second].clone();
            if self.is_locked(first)
                || self.is_locked(second)
                || self.is_hidden(first)
                || self.is_hidden(second)
                || self.entity_tags.get(&first) != self.entity_tags.get(&second)
            {
                continue;
            }
            let a = if e0.a == vertex { e0.b } else { e0.a };
            let b = if e1.a == vertex { e1.b } else { e1.a };
            if a == b || pairs.contains(&pair(a, b)) {
                continue;
            }
            let va = self.vertices[&a].position - self.vertices[&vertex].position;
            let vb = self.vertices[&b].position - self.vertices[&vertex].position;
            if va.length_squared() <= EPS * EPS
                || vb.length_squared() <= EPS * EPS
                || va.dot(vb) >= 0.
                || va.cross(vb).length_squared() > 1e-18 * va.length_squared() * vb.length_squared()
            {
                continue;
            }
            let faces = faces_at.get(&vertex).cloned().unwrap_or_default();
            if faces.iter().any(|id| {
                let face = &self.faces[id];
                self.is_locked(*id)
                    || self.is_hidden(*id)
                    || std::iter::once(&face.vertices)
                        .chain(&face.holes)
                        .any(|ring| {
                            ring.iter().position(|id| *id == vertex).is_some_and(|i| {
                                ring.len() <= 3
                                    || pair(
                                        ring[(i + ring.len() - 1) % ring.len()],
                                        ring[(i + 1) % ring.len()],
                                    ) != pair(a, b)
                            })
                        })
            }) {
                continue;
            }
            // Retain one edge identity and the existing face/material identities.
            let edge = self.edges.get_mut(&first).unwrap();
            edge.a = a;
            edge.b = b;
            self.edges.remove(&second);
            self.vertices.remove(&vertex);
            self.entity_tags.remove(&second);
            self.hidden.remove(&second);
            self.locked.remove(&second);
            for face in faces {
                let face = self.faces.get_mut(&face).unwrap();
                face.vertices.retain(|id| *id != vertex);
                for ring in &mut face.holes {
                    ring.retain(|id| *id != vertex);
                }
            }
            for object in self.objects.values_mut() {
                object.entities.remove(&second);
                object.definition_entities.retain(|_, id| *id != second);
            }
            pairs.remove(&pair(vertex, a));
            pairs.remove(&pair(vertex, b));
            pairs.insert(pair(a, b));
            adjacency.remove(&vertex);
            for endpoint in [a, b] {
                let edges = adjacency.get_mut(&endpoint).unwrap();
                edges.remove(&first);
                edges.remove(&second);
                edges.insert(first);
                candidates.insert(endpoint);
            }
            allowed.remove(&second);
            removed += 1;
        }
        removed
    }
}
fn pair(a: u64, b: u64) -> (u64, u64) {
    (a.min(b), a.max(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DVec3;

    fn chain() -> Model {
        let mut model = Model::default();
        for i in 0..4 {
            model
                .add_edge(DVec3::X * i as f64, DVec3::X * (i + 1) as f64)
                .unwrap();
        }
        model
    }
    #[test]
    fn merges_straight_runs_and_preserves_branches_corners_and_unselected_edges() {
        let mut model = chain();
        let roots = model.root_entities();
        assert_eq!(model.clean_collinear_vertices(&roots), Ok(3));
        assert_eq!(model.edges.len(), 1);
        model.validate().unwrap();
        let mut model = chain();
        model
            .add_edge(DVec3::X * 2., DVec3::X * 2. + DVec3::Y)
            .unwrap();
        let roots = model.root_entities();
        assert_eq!(model.clean_collinear_vertices(&roots), Ok(2));
        assert_eq!(model.edges.len(), 3);
        model.validate().unwrap();
        let mut model = chain();
        let first = *model.edges.keys().next().unwrap();
        assert_eq!(
            model.clean_collinear_vertices(&BTreeSet::from([first])),
            Ok(0)
        );
    }
    #[test]
    fn preserves_annotation_attachments_and_metadata_boundaries() {
        let mut model = chain();
        model.add_text(DVec3::X * 2., DVec3::Y, "Midpoint").unwrap();
        let roots = model.root_entities();
        assert_eq!(model.clean_collinear_vertices(&roots), Ok(2));
        model.validate().unwrap();
        assert_eq!(
            model.texts.values().next().unwrap().anchor.position(&model),
            DVec3::X * 2.
        );
        let mut model = chain();
        let first = *model.edges.keys().next().unwrap();
        let tag = model.add_tag("Keep seam").unwrap();
        model.entity_tags.insert(first, tag);
        let roots = model.root_entities();
        assert_eq!(model.clean_collinear_vertices(&roots), Ok(2));
        assert!(model.edges.contains_key(&first));
        model.validate().unwrap();
    }
    #[test]
    fn preserves_faces_and_cleans_groups_and_linked_components() {
        let mut model = Model::default();
        let points = [
            DVec3::ZERO,
            DVec3::X,
            DVec3::X * 2.,
            DVec3::new(2., 1., 0.),
            DVec3::Y,
        ];
        for i in 0..points.len() {
            model
                .add_edge(points[i], points[(i + 1) % points.len()])
                .unwrap();
        }
        let face = *model.faces.keys().next().unwrap();
        model.face_colors.insert(face, [100, 120, 140]);
        let roots = model.root_entities();
        assert_eq!(model.clean_collinear_vertices(&roots), Ok(1));
        assert_eq!(model.faces[&face].vertices.len(), 4);
        assert_eq!(model.face_colors[&face], [100, 120, 140]);
        model.validate().unwrap();
        for component in [false, true] {
            let mut model = chain();
            let object = model
                .make_object(&model.root_entities(), component, "Run")
                .unwrap();
            if component {
                let copy = model.copy_entities(&BTreeSet::from([object]));
                model.paste_entities(&copy, DVec3::Y * 2.);
            }
            assert_eq!(
                model.clean_collinear_vertices(&BTreeSet::from([object])),
                Ok(3)
            );
            assert_eq!(model.edges.len(), if component { 2 } else { 1 });
            model.validate().unwrap();
        }
    }
}
