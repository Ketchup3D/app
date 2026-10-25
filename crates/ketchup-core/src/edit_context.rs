//! Scoped editing and propagation of component definitions between instances.
use crate::{DVec3, Model, ObjectKind, Ray};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Instance geometry is stored in world coordinates. This frame preserves the
/// translation and uniform scale when its shared definition changes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstanceTransform {
    pub translation: DVec3,
    pub scale: f64,
    #[serde(default = "identity_axes")]
    pub axes: DVec3,
}
fn identity_axes() -> DVec3 {
    DVec3::ONE
}
impl Default for InstanceTransform {
    fn default() -> Self {
        Self {
            translation: DVec3::ZERO,
            scale: 1.,
            axes: DVec3::ONE,
        }
    }
}
impl InstanceTransform {
    pub fn valid(self) -> bool {
        self.translation.is_finite()
            && self.scale.is_finite()
            && self.scale > 0.
            && self.axes.is_finite()
            && (self.axes.abs() - DVec3::ONE).abs().max_element() < 1e-12
    }
    pub fn translated(self, delta: DVec3) -> Self {
        Self {
            translation: self.translation + delta,
            ..self
        }
    }
    pub fn scaled(self, center: DVec3, factor: f64) -> Self {
        Self {
            translation: center + (self.translation - center) * factor,
            scale: self.scale * factor,
            ..self
        }
    }
}

impl Model {
    /// Resolve a leaf to the immediate selectable child of the current context.
    pub fn entity_in_context(&self, mut id: u64) -> Option<u64> {
        if !self.contains_entity(&id) || self.edit_context == Some(id) {
            return None;
        }
        loop {
            let parent = self.parent(id);
            if parent == self.edit_context {
                return Some(id);
            }
            id = parent?;
        }
    }
    pub fn edit_path(&self) -> Vec<u64> {
        let mut result = Vec::new();
        let mut current = self.edit_context;
        while let Some(id) = current {
            if !self.objects.contains_key(&id) {
                break;
            }
            result.push(id);
            current = self.parent(id);
        }
        result.reverse();
        result
    }
    pub fn enter_object(&mut self, id: u64) -> bool {
        if self.entity_in_context(id) != Some(id)
            || !self.objects.contains_key(&id)
            || self.is_locked(id)
            || self.is_hidden(id)
        {
            return false;
        }
        self.edit_context = Some(id);
        true
    }
    pub fn nearest_context_face(&self, ray: Ray) -> Option<(u64, f64, DVec3)> {
        self.faces
            .values()
            .filter(|face| !self.is_hidden(face.id) && self.entity_in_context(face.id).is_some())
            .filter_map(|face| self.face_hit(face, ray).map(|(t, p)| (face.id, t, p)))
            .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
    }
    pub fn nearest_editable_face(&self, ray: Ray) -> Option<(u64, f64, DVec3)> {
        self.faces
            .values()
            .filter(|face| !self.is_hidden(face.id) && self.parent(face.id) == self.edit_context)
            .filter_map(|face| self.face_hit(face, ray).map(|(t, p)| (face.id, t, p)))
            .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
    }
    pub fn object_contents(&self, id: u64) -> Option<Self> {
        Some(self.copy_entities(&self.objects.get(&id)?.entities))
    }
    pub(crate) fn edit_in_context<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, &'static str>,
    ) -> Result<T, &'static str> {
        let context = self.edit_context.ok_or("No object is open for editing")?;
        if self.is_locked(context) {
            return Err("Unlock the object before editing it");
        }
        let mut contents = self
            .object_contents(context)
            .ok_or("The editing context no longer exists")?;
        let before = contents.clone();
        let result = operation(&mut contents)?;
        if contents != before {
            let mut next = self.clone();
            next.replace_contents(context, &contents)?;
            next.synchronize_edit_context()?;
            *self = next;
        }
        Ok(result)
    }
    fn remove_contents(&mut self, id: u64) -> Result<(), &'static str> {
        let object = self.objects.get(&id).ok_or("Missing object")?;
        let ids = self.expand_entities(&object.entities);
        self.sync_dimensions();
        self.dimensions.retain(|id, _| !ids.contains(id));
        self.edges.retain(|id, _| !ids.contains(id));
        self.faces.retain(|id, _| !ids.contains(id));
        self.objects.retain(|id, _| !ids.contains(id));
        self.entity_tags.retain(|id, _| !ids.contains(id));
        self.hidden.retain(|id| !ids.contains(id));
        self.locked.retain(|id| !ids.contains(id));
        self.objects.get_mut(&id).unwrap().entities.clear();
        Ok(())
    }
    fn replace_contents(&mut self, id: u64, contents: &Self) -> Result<(), &'static str> {
        self.remove_contents(id)?;
        self.vertices.extend(contents.vertices.clone());
        self.edges.extend(contents.edges.clone());
        self.faces.extend(contents.faces.clone());
        self.dimensions.extend(contents.dimensions.clone());
        self.face_colors.extend(contents.face_colors.clone());
        self.objects.extend(contents.objects.clone());
        self.tags.extend(contents.tags.clone());
        self.entity_tags.extend(contents.entity_tags.clone());
        self.hidden.extend(&contents.hidden);
        self.locked.extend(&contents.locked);
        self.next_id = self.next_id.max(contents.next_id);
        self.objects.get_mut(&id).unwrap().entities = contents.root_entities();
        self.clean_unused_vertices();
        self.clean_entity_state();
        Ok(())
    }
    fn clean_unused_vertices(&mut self) {
        self.sync_dimensions();
        let used: BTreeSet<_> = self.edges.values().flat_map(|e| [e.a, e.b]).collect();
        self.vertices.retain(|id, _| used.contains(id));
        self.sync_dimensions();
    }
    fn refresh_definition_mapping(&mut self, id: u64) {
        if !matches!(self.objects[&id].kind, ObjectKind::Component { .. }) {
            return;
        }
        let geometry: BTreeSet<_> = self
            .expand_entities(&self.objects[&id].entities)
            .into_iter()
            .filter(|id| !self.objects.contains_key(id))
            .collect();
        let mut mapping = self.objects[&id].definition_entities.clone();
        mapping.retain(|_, id| geometry.contains(id));
        let mapped: BTreeSet<_> = mapping.values().copied().collect();
        for id in geometry.difference(&mapped) {
            let mut key = *id;
            while mapping.contains_key(&key) {
                key = self.id();
            }
            mapping.insert(key, *id);
        }
        self.objects.get_mut(&id).unwrap().definition_entities = mapping;
    }
    fn transform_contents(&mut self, scale: DVec3, translation: DVec3) {
        self.sync_dimensions();
        for d in self.dimensions.values_mut() {
            d.start.point = d.start.point * scale + translation;
            d.end.point = d.end.point * scale + translation;
            d.offset *= scale;
        }
        for vertex in self.vertices.values_mut() {
            vertex.position = vertex.position * scale + translation;
        }
        for object in self.objects.values_mut() {
            let transform = object.transform.unwrap_or_default();
            object.transform = Some(InstanceTransform {
                translation: transform.translation * scale + translation,
                scale: transform.scale * scale.abs().x,
                axes: transform.axes * scale.signum(),
            });
        }
    }
    /// Component dependency edges run from nested definitions to their containers.
    fn definition_dependencies(&self) -> Result<BTreeMap<Uuid, BTreeMap<Uuid, u64>>, &'static str> {
        let mut graph: BTreeMap<_, BTreeMap<_, _>> = BTreeMap::new();
        for object in self.objects.values() {
            let ObjectKind::Component { definition } = object.kind else {
                continue;
            };
            graph.entry(definition).or_default();
            let mut parent = self.parent(object.id);
            while let Some(id) = parent {
                if let ObjectKind::Component {
                    definition: container,
                } = self.objects[&id].kind
                {
                    if container == definition {
                        return Err("A component cannot contain its own definition");
                    }
                    graph.entry(definition).or_default().insert(container, id);
                }
                parent = self.parent(id);
            }
        }
        Ok(graph)
    }
    pub(crate) fn validate_definition_nesting(&self) -> Result<(), &'static str> {
        let mut graph = self.definition_dependencies()?;
        while !graph.is_empty() {
            let leaves: BTreeSet<_> = graph
                .keys()
                .copied()
                .filter(|key| !graph.values().any(|parents| parents.contains_key(key)))
                .collect();
            if leaves.is_empty() {
                return Err("Cyclic component definitions");
            }
            graph.retain(|id, _| !leaves.contains(id));
        }
        Ok(())
    }
    /// Call after direct state changes (hide/lock), as well as scoped geometry edits.
    pub fn synchronize_edit_context(&mut self) -> Result<(), &'static str> {
        self.synchronize_object_contexts(self.edit_context)
    }

    /// Reparenting can change both the source and destination component definitions.
    pub(crate) fn synchronize_object_contexts(
        &mut self,
        contexts: impl IntoIterator<Item = u64>,
    ) -> Result<(), &'static str> {
        let mut sources = BTreeMap::new();
        for context in contexts {
            let mut current = Some(context);
            while let Some(id) = current {
                let object = self.objects.get(&id).ok_or("Missing editing context")?;
                if let ObjectKind::Component { definition } = object.kind {
                    if sources
                        .insert(definition, id)
                        .is_some_and(|source| source != id)
                    {
                        return Err("Cannot move objects between instances of the same component");
                    }
                }
                current = self.parent(id);
            }
        }
        let graph = self.definition_dependencies()?;
        // A definition edited through one instance may also be nested in other
        // definitions elsewhere. Update all affected containers, inside out.
        loop {
            let mut additions = BTreeMap::new();
            for definition in sources.keys() {
                if let Some(parents) = graph.get(definition) {
                    for (parent, id) in parents {
                        if !sources.contains_key(parent) {
                            additions.insert(*parent, *id);
                        }
                    }
                }
            }
            if additions.is_empty() {
                break;
            }
            sources.extend(additions);
        }
        while !sources.is_empty() {
            let ready = sources
                .keys()
                .copied()
                .find(|definition| {
                    !sources.keys().any(|child| {
                        graph
                            .get(child)
                            .is_some_and(|parents| parents.contains_key(definition))
                    })
                })
                .ok_or("Cyclic component definitions")?;
            let source = sources.remove(&ready).unwrap();
            self.refresh_definition_mapping(source);
            let instance = self.objects[&source].clone();
            let transform = instance.transform.unwrap_or_default();
            let mut canonical = self.object_contents(source).ok_or("Missing component")?;
            canonical.transform_contents(
                transform.axes / transform.scale,
                -transform.translation * transform.axes / transform.scale,
            );
            let peers: Vec<_> = self
                .objects
                .values()
                .filter(|o| o.id != source && o.kind == instance.kind)
                .map(|o| (o.id, o.transform.unwrap_or_default()))
                .collect();
            for (peer, frame) in peers {
                let mut contents = canonical.clone();
                contents.transform_contents(frame.axes * frame.scale, frame.translation);
                // Keep surviving peer vertices stable so dimensions attached
                // from outside the component continue to track its geometry.
                let mut stable_vertices = BTreeMap::new();
                let peer_instance = self.objects[&peer].clone();
                for (key, source_id) in &instance.definition_entities {
                    let Some(old_id) = peer_instance.definition_entities.get(key) else {
                        continue;
                    };
                    if let (Some(source_edge), Some(old_edge)) =
                        (contents.edges.get(source_id), self.edges.get(old_id))
                    {
                        stable_vertices.insert(source_edge.a, old_edge.a);
                        stable_vertices.insert(source_edge.b, old_edge.b);
                    }
                }
                self.remove_contents(peer)?;
                let (entities, remap) = self.paste_mapped(&contents, DVec3::ZERO, false);
                let vertex_remap: BTreeMap<_, _> = stable_vertices
                    .iter()
                    .map(|(source, old)| (remap[source], *old))
                    .collect();
                for (new, old) in &vertex_remap {
                    let position = self.vertices[new].position;
                    self.vertices
                        .insert(*old, crate::Vertex { id: *old, position });
                }
                let inserted = self.expand_entities(&entities);
                for id in &inserted {
                    if let Some(edge) = self.edges.get_mut(id) {
                        edge.a = vertex_remap.get(&edge.a).copied().unwrap_or(edge.a);
                        edge.b = vertex_remap.get(&edge.b).copied().unwrap_or(edge.b);
                    }
                    if let Some(face) = self.faces.get_mut(id) {
                        for vertex in face
                            .vertices
                            .iter_mut()
                            .chain(face.holes.iter_mut().flatten())
                        {
                            *vertex = vertex_remap.get(vertex).copied().unwrap_or(*vertex);
                        }
                    }
                    if let Some(dimension) = self.dimensions.get_mut(id) {
                        for anchor in [&mut dimension.start, &mut dimension.end] {
                            for (vertex, _) in &mut anchor.weights {
                                *vertex = vertex_remap.get(vertex).copied().unwrap_or(*vertex);
                            }
                        }
                    }
                }
                let object = self.objects.get_mut(&peer).unwrap();
                object.entities = entities;
                object.definition_entities = instance
                    .definition_entities
                    .iter()
                    .map(|(key, id)| (*key, remap[id]))
                    .collect();
            }
        }
        self.clean_unused_vertices();
        self.clean_entity_state();
        self.validate()
    }
    /// A clipboard snapshot may predate a definition edit. Reuse the current
    /// definition while preserving the copied instance's placement.
    pub(crate) fn prepare_clipboard(&self, source: &Self) -> Result<Self, &'static str> {
        let mut copy = source.clone();
        copy.edit_context = None;
        copy.next_id = copy.next_id.max(self.next_id);
        copy.initialize_instance_transforms();
        let mut objects: Vec<_> = copy.objects.keys().copied().collect();
        objects.sort_by_key(|id| {
            let mut depth = 0;
            let mut parent = copy.parent(*id);
            while let Some(id) = parent {
                depth += 1;
                parent = copy.parent(id);
            }
            depth
        });
        for id in objects {
            let Some(instance) = copy.objects.get(&id).cloned() else {
                continue;
            };
            if !matches!(instance.kind, ObjectKind::Component { .. }) {
                continue;
            }
            let Some(reference) = self
                .objects
                .values()
                .find(|object| object.kind == instance.kind)
            else {
                continue;
            };
            let from = reference.transform.unwrap_or_default();
            let to = instance.transform.unwrap_or_default();
            let mut contents = self
                .object_contents(reference.id)
                .ok_or("Missing component definition")?;
            contents.transform_contents(
                to.axes * from.axes * (to.scale / from.scale),
                to.translation - from.translation * to.axes * from.axes * (to.scale / from.scale),
            );
            copy.remove_contents(id)?;
            let (entities, remap) = copy.paste_mapped(&contents, DVec3::ZERO, false);
            let object = copy.objects.get_mut(&id).unwrap();
            object.entities = entities;
            object.definition_entities = reference
                .definition_entities
                .iter()
                .map(|(key, id)| (*key, remap[id]))
                .collect();
            copy.edit_context = Some(id);
            copy.synchronize_edit_context()?;
            copy.edit_context = None;
        }
        Ok(copy)
    }

    /// Older documents stored world geometry without instance frames. Recover
    /// their translation and uniform scale from matching definition edges.
    pub fn initialize_instance_transforms(&mut self) {
        let ids: Vec<_> = self.objects.keys().copied().collect();
        for id in ids {
            if self.objects[&id].transform.is_some() {
                continue;
            }
            let object = &self.objects[&id];
            let mut frame = InstanceTransform::default();
            if let ObjectKind::Component { .. } = object.kind {
                if let Some(reference) = self
                    .objects
                    .values()
                    .find(|o| o.kind == object.kind && o.id != id)
                {
                    for (key, edge) in &object.definition_entities {
                        let Some(other) = reference
                            .definition_entities
                            .get(key)
                            .and_then(|id| self.edges.get(id))
                        else {
                            continue;
                        };
                        let Some(edge) = self.edges.get(edge) else {
                            continue;
                        };
                        let (a, b) = self.points(edge);
                        let (c, d) = self.points(other);
                        let reference_frame = reference.transform.unwrap_or_default();
                        let scale = a.distance(b) / c.distance(d) * reference_frame.scale;
                        let canonical_start = (c - reference_frame.translation)
                            * reference_frame.axes
                            / reference_frame.scale;
                        frame = InstanceTransform {
                            scale,
                            translation: a - canonical_start * scale,
                            ..Default::default()
                        };
                        break;
                    }
                }
            }
            self.objects.get_mut(&id).unwrap().transform = Some(frame);
        }
    }
}
