//! Object membership and geometry clipboard operations. Geometry uses independent
//! vertices inside each object, so drawing outside a group cannot alter its contents.
use crate::{DVec3, Model};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectKind {
    Group,
    /// Copies retain their definition identity. Exploding detaches an instance.
    Component {
        definition: uuid::Uuid,
    },
}

/// A purchasing material definition shared by ID within a document.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StockMaterial {
    pub name: String,
    /// Thickness, width and length in tenths of a millimetre.
    pub dimensions: [u64; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub surface_treatment: crate::surface_treatment::Settings,
    /// Distinguishes an explicit all-disabled override from an unconfigured object.
    #[serde(default)]
    pub surface_treatment_customized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock_material: Option<StockMaterial>,
    /// Stable purchasing-material reference. `stock_material` is legacy input
    /// migrated by the application when the reusable catalog becomes available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock_material_id: Option<crate::MaterialId>,
    /// Sibling display position in the Outliner; older models use entity IDs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outliner_order: Option<u64>,
    #[serde(default)]
    pub transform: Option<crate::InstanceTransform>,
    pub kind: ObjectKind,
    pub entities: BTreeSet<u64>,
    /// Stable definition entity -> this instance's entity, retained by copies.
    #[serde(default)]
    pub definition_entities: BTreeMap<u64, u64>,
}

impl Object {
    pub fn surface_treatment_override(&self) -> Option<&crate::surface_treatment::Settings> {
        // Preserve non-default configurations saved before the override flag existed.
        (self.surface_treatment_customized
            || self.surface_treatment != crate::surface_treatment::Settings::default())
        .then_some(&self.surface_treatment)
    }
}

impl Model {
    pub fn object_stock_material<'a>(&'a self, object: &'a Object) -> Option<&'a StockMaterial> {
        if let Some(id) = object.stock_material_id {
            self.stock_materials.get(&id)
        } else {
            object.stock_material.as_ref()
        }
    }

    /// Upgrade old inline assignments once and refresh referenced definitions.
    /// A missing catalog item retains its project definition for portability.
    pub fn sync_stock_materials(
        &mut self,
        catalog: &BTreeMap<crate::MaterialId, StockMaterial>,
    ) -> bool {
        let mut changed = false;
        for object in self.objects.values_mut() {
            if let Some(legacy) = object.stock_material.take() {
                if object.stock_material_id.is_none() {
                    let matches: Vec<_> = catalog
                        .iter()
                        .filter(|(_, value)| **value == legacy)
                        .map(|(id, _)| *id)
                        .collect();
                    let id = if matches.len() == 1 {
                        matches[0]
                    } else {
                        self.stock_materials
                            .iter()
                            .find(|(_, value)| **value == legacy)
                            .map(|(id, _)| *id)
                            .unwrap_or_else(crate::MaterialId::new_v4)
                    };
                    self.stock_materials.entry(id).or_insert(legacy);
                    object.stock_material_id = Some(id);
                }
                changed = true;
            }
            if let Some(id) = object.stock_material_id
                && let Some(material) = catalog.get(&id)
                && self.stock_materials.get(&id) != Some(material)
            {
                self.stock_materials.insert(id, material.clone());
                changed = true;
            }
        }
        changed
    }

    pub fn parent(&self, id: u64) -> Option<u64> {
        self.objects
            .values()
            .find(|o| o.entities.contains(&id))
            .map(|o| o.id)
    }
    pub fn root_entity(&self, mut id: u64) -> u64 {
        while let Some(parent) = self.parent(id) {
            id = parent;
        }
        id
    }
    fn inherited(&self, mut id: u64, state: &BTreeSet<u64>) -> bool {
        loop {
            if state.contains(&id) {
                return true;
            }
            let Some(parent) = self.parent(id) else {
                return false;
            };
            id = parent;
        }
    }
    pub fn is_hidden(&self, id: u64) -> bool {
        self.is_entity_hidden(id) || self.is_tag_hidden(id)
    }
    pub fn is_entity_hidden(&self, id: u64) -> bool {
        !self.hidden.is_empty() && self.inherited(id, &self.hidden)
    }
    pub fn is_locked(&self, id: u64) -> bool {
        !self.locked.is_empty() && self.inherited(id, &self.locked)
    }
    pub fn root_entities(&self) -> BTreeSet<u64> {
        let children: BTreeSet<_> = self
            .objects
            .values()
            .flat_map(|object| object.entities.iter().copied())
            .collect();
        self.edges
            .keys()
            .chain(self.faces.keys())
            .chain(self.objects.keys())
            .chain(self.dimensions.keys())
            .chain(self.texts.keys())
            .copied()
            .filter(|id| !children.contains(id))
            .collect()
    }
    pub fn selectable_entities(&self) -> BTreeSet<u64> {
        self.edit_context
            .and_then(|id| self.objects.get(&id))
            .map(|o| o.entities.clone())
            .unwrap_or_else(|| self.root_entities())
            .into_iter()
            .filter(|id| !self.is_hidden(*id))
            .collect()
    }
    pub fn expand_entities(&self, roots: &BTreeSet<u64>) -> BTreeSet<u64> {
        let mut result = BTreeSet::new();
        let mut pending: Vec<_> = roots.iter().copied().collect();
        while let Some(id) = pending.pop() {
            if !self.contains_entity(&id) || !result.insert(id) {
                continue;
            }
            if let Some(object) = self.objects.get(&id) {
                pending.extend(&object.entities);
            }
        }
        result
    }
    fn boundary_edges(&self, faces: &BTreeSet<u64>) -> BTreeSet<u64> {
        let pairs: BTreeSet<_> = faces
            .iter()
            .filter_map(|id| self.faces.get(id))
            .flat_map(|face| std::iter::once(&face.vertices).chain(face.holes.iter()))
            .flat_map(|ring| {
                (0..ring.len()).map(|i| {
                    let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                    (a.min(b), a.max(b))
                })
            })
            .collect();
        self.edges
            .values()
            .filter(|e| pairs.contains(&(e.a.min(e.b), e.a.max(e.b))))
            .map(|e| e.id)
            .collect()
    }
    pub fn drawing_edge(&self, id: u64) -> bool {
        self.parent(id).is_none()
            && !self.is_hidden(id)
            && !self.is_locked(id)
            && !self
                .faces
                .values()
                .filter(|f| self.is_locked(f.id) || self.is_hidden(f.id))
                .any(|f| self.boundary_edges(&BTreeSet::from([f.id])).contains(&id))
    }
    /// Snapshot drawing eligibility once before changing topology. Testing each
    /// edge separately repeatedly scans every protected face and all its edges
    /// on imports containing hidden groups.
    pub(crate) fn drawing_edges(&self) -> BTreeSet<u64> {
        let children: BTreeSet<_> = self
            .objects
            .values()
            .flat_map(|object| object.entities.iter().copied())
            .collect();
        let mut hidden_roots = self.hidden.clone();
        hidden_roots.extend(self.entity_tags.iter().filter_map(|(entity, tag)| {
            self.tags
                .get(tag)
                .is_some_and(|tag| !tag.visible)
                .then_some(*entity)
        }));
        let hidden = self.expand_entities(&hidden_roots);
        let locked = self.expand_entities(&self.locked);
        let protected_faces = self
            .faces
            .keys()
            .copied()
            .filter(|id| hidden.contains(id) || locked.contains(id))
            .collect();
        let protected_edges = self.boundary_edges(&protected_faces);
        self.edges
            .keys()
            .copied()
            .filter(|id| {
                !children.contains(id)
                    && !hidden.contains(id)
                    && !locked.contains(id)
                    && !protected_edges.contains(id)
            })
            .collect()
    }
    pub(crate) fn erasable_entities(&self, roots: &BTreeSet<u64>) -> BTreeSet<u64> {
        let roots = roots
            .iter()
            .copied()
            .map(|id| self.root_entity(id))
            .filter(|id| {
                !self
                    .expand_entities(&BTreeSet::from([*id]))
                    .iter()
                    .any(|id| self.is_locked(*id))
            })
            .collect();
        let mut ids = self.expand_entities(&roots);
        // A locked surface also protects its supporting boundary edges.
        let protected: BTreeSet<_> = self
            .faces
            .keys()
            .copied()
            .filter(|id| self.is_locked(*id))
            .collect();
        let protected_edges = self.boundary_edges(&protected);
        ids.retain(|id| !protected_edges.contains(id));
        ids
    }
    pub fn selection_locked(&self, ids: &BTreeSet<u64>) -> bool {
        if self.locked.is_empty() {
            return false;
        }
        !self
            .expand_entities(&self.locked)
            .is_disjoint(&self.expand_entities(ids))
    }
    pub fn selection_protected(&self, ids: &BTreeSet<u64>) -> bool {
        if self.locked.is_empty() {
            return false;
        }
        let expanded = self.expand_entities(ids);
        let locked = self.expand_entities(&self.locked);
        !locked.is_disjoint(&expanded)
            || self
                .faces
                .keys()
                .filter(|id| locked.contains(*id))
                .any(|id| {
                    !self
                        .boundary_edges(&BTreeSet::from([*id]))
                        .is_disjoint(&expanded)
                })
    }
    pub(crate) fn clean_entity_state(&mut self) {
        self.sync_dimensions();
        self.face_colors.retain(|id, _| self.faces.contains_key(id));
        let entities: BTreeSet<_> = self
            .edges
            .keys()
            .chain(self.faces.keys())
            .chain(self.objects.keys())
            .chain(self.dimensions.keys())
            .chain(self.texts.keys())
            .copied()
            .collect();
        self.entity_tags.retain(|id, _| entities.contains(id));
        self.hidden.retain(|id| entities.contains(id));
        self.locked.retain(|id| entities.contains(id));
        for object in self.objects.values_mut() {
            object.entities.retain(|id| entities.contains(id));
        }
    }
    /// Include face outlines without inventing fills that the user has erased.
    pub fn copy_entities(&self, roots: &BTreeSet<u64>) -> Self {
        let mut ids = self.expand_entities(roots);
        ids.extend(self.boundary_edges(&ids));
        let mut copy = self.clone();
        copy.sync_dimensions();
        copy.dimensions.retain(|id, _| ids.contains(id));
        copy.texts.retain(|id, _| ids.contains(id));
        copy.edges.retain(|id, _| ids.contains(id));
        copy.faces.retain(|id, _| ids.contains(id));
        copy.objects.retain(|id, _| ids.contains(id));
        let used: BTreeSet<_> = copy.edges.values().flat_map(|e| [e.a, e.b]).collect();
        copy.vertices.retain(|id, _| used.contains(id));
        copy.sync_dimensions();
        copy.face_colors.retain(|id, _| copy.faces.contains_key(id));
        copy.clean_entity_state();
        let used_tags: BTreeSet<_> = copy.entity_tags.values().copied().collect();
        copy.tags.retain(|id, _| used_tags.contains(id));
        let used_materials: BTreeSet<_> = copy
            .objects
            .values()
            .filter_map(|object| object.stock_material_id)
            .collect();
        copy.stock_materials
            .retain(|id, _| used_materials.contains(id));
        copy.edit_context = None;
        copy
    }
    /// Fresh IDs and vertices preserve overlapping pasted geometry and object isolation.
    pub fn paste_entities(&mut self, source: &Self, offset: DVec3) -> BTreeSet<u64> {
        if self.edit_context.is_some() {
            return self
                .edit_in_context(|model| Ok(model.paste_entities(source, offset)))
                .unwrap_or_default();
        }
        let Ok(source) = self.prepare_clipboard(source) else {
            return BTreeSet::new();
        };
        self.paste_mapped(&source, offset, true).0
    }
    pub(crate) fn paste_mapped(
        &mut self,
        source: &Self,
        offset: DVec3,
        existing_materials: bool,
    ) -> (BTreeSet<u64>, BTreeMap<u64, u64>) {
        if !offset.is_finite()
            || source
                .dimensions
                .values()
                .flat_map(|d| d.layout(source))
                .chain(source.texts.values().flat_map(|t| t.layout(source)))
                .any(|p| !(p + offset).is_finite() || (p + offset).abs().max_element() > 1e10)
            || source.vertices.values().any(|v| {
                !(v.position + offset).is_finite()
                    || (v.position + offset).abs().max_element() > 1e10
            })
        {
            return (BTreeSet::new(), BTreeMap::new());
        }
        self.next_id = self.next_id.max(source.next_id);
        let ids: BTreeMap<_, _> = source
            .vertices
            .keys()
            .chain(source.edges.keys())
            .chain(source.faces.keys())
            .chain(source.objects.keys())
            .chain(source.dimensions.keys())
            .chain(source.texts.keys())
            .map(|old| (*old, self.id()))
            .collect();
        let roots = source.root_entities().iter().map(|id| ids[id]).collect();
        for vertex in source.vertices.values() {
            let mut v = vertex.clone();
            v.id = ids[&v.id];
            v.position += offset;
            self.vertices.insert(v.id, v);
        }
        for edge in source.edges.values() {
            let mut e = edge.clone();
            e.id = ids[&e.id];
            e.a = ids[&e.a];
            e.b = ids[&e.b];
            self.edges.insert(e.id, e);
        }
        for face in source.faces.values() {
            let mut f = face.clone();
            f.id = ids[&f.id];
            for ring in std::iter::once(&mut f.vertices).chain(f.holes.iter_mut()) {
                for id in ring {
                    *id = ids[id];
                }
            }
            self.faces.insert(f.id, f);
        }
        for dimension in source.dimensions.values() {
            let mut dimension = dimension.clone();
            dimension.id = ids[&dimension.id];
            for anchor in [&mut dimension.start, &mut dimension.end] {
                anchor.point += offset;
                if anchor.weights.iter().all(|(id, _)| ids.contains_key(id)) {
                    for (id, _) in &mut anchor.weights {
                        *id = ids[id];
                    }
                } else {
                    anchor.weights.clear();
                }
            }
            self.dimensions.insert(dimension.id, dimension);
        }
        for text in source.texts.values() {
            let mut text = text.clone();
            text.id = ids[&text.id];
            text.anchor.point += offset;
            if text
                .anchor
                .weights
                .iter()
                .all(|(id, _)| ids.contains_key(id))
            {
                for (id, _) in &mut text.anchor.weights {
                    *id = ids[id];
                }
            } else {
                text.anchor.weights.clear();
            }
            self.texts.insert(text.id, text);
        }
        for (id, color) in &source.face_colors {
            self.face_colors.insert(ids[id], *color);
        }
        // When the definition already exists, its current materials take
        // precedence over an older clipboard snapshot of the same component.
        for object in source.objects.values().filter(|_| existing_materials) {
            let ObjectKind::Component { definition } = object.kind else {
                continue;
            };
            if let Some(existing) = self
                .objects
                .values()
                .find(|o| o.kind == (ObjectKind::Component { definition }))
            {
                for (key, source_id) in &object.definition_entities {
                    if !source.faces.contains_key(source_id) {
                        continue;
                    }
                    if let Some(existing_id) = existing.definition_entities.get(key) {
                        let target_id = ids[source_id];
                        if let Some(color) = self.face_colors.get(existing_id).copied() {
                            self.face_colors.insert(target_id, color);
                        } else {
                            self.face_colors.remove(&target_id);
                        }
                    }
                }
            }
        }
        for (id, material) in &source.stock_materials {
            self.stock_materials
                .entry(*id)
                .or_insert_with(|| material.clone());
        }
        for object in source.objects.values() {
            let mut o = object.clone();
            o.id = ids[&o.id];
            o.transform = Some(o.transform.unwrap_or_default().translated(offset));
            o.entities = o.entities.iter().map(|id| ids[id]).collect();
            o.definition_entities = o
                .definition_entities
                .iter()
                .map(|(key, id)| (*key, ids[id]))
                .collect();
            self.objects.insert(o.id, o);
        }
        let mut tags = BTreeMap::new();
        for tag in source.tags.values() {
            let folded = tag.name.to_lowercase();
            let target = self
                .tags
                .values()
                .find(|t| t.name.to_lowercase() == folded)
                .map(|t| t.id)
                .unwrap_or_else(|| {
                    let id = self.id();
                    let mut tag = tag.clone();
                    tag.id = id;
                    self.tags.insert(id, tag);
                    id
                });
            tags.insert(tag.id, target);
        }
        self.entity_tags.extend(
            source
                .entity_tags
                .iter()
                .filter_map(|(entity, tag)| Some((*ids.get(entity)?, *tags.get(tag)?))),
        );
        self.hidden.extend(source.hidden.iter().map(|id| ids[id]));
        self.locked.extend(source.locked.iter().map(|id| ids[id]));
        (roots, ids)
    }
    pub fn make_object(
        &mut self,
        selected: &BTreeSet<u64>,
        component: bool,
        name: &str,
    ) -> Option<u64> {
        if self.edit_context.is_some() {
            return self
                .edit_in_context(|model| Ok(model.make_object(selected, component, name)))
                .ok()
                .flatten();
        }
        if selected.is_empty() || self.selection_protected(selected) {
            return None;
        }
        let ids = self.expand_entities(selected);
        let copy = self.copy_entities(selected);
        if copy.edges.is_empty() && copy.dimensions.is_empty() && copy.texts.is_empty() {
            return None;
        }
        // Keep a duplicate of any boundary still needed by an unselected face.
        let outside_faces = self
            .faces
            .keys()
            .copied()
            .filter(|id| !ids.contains(id))
            .collect();
        let outside_edges = self.boundary_edges(&outside_faces);
        if ids
            .iter()
            .any(|id| outside_edges.contains(id) && self.is_locked(*id))
        {
            return None;
        }
        self.faces.retain(|id, _| !ids.contains(id));
        self.dimensions.retain(|id, _| !ids.contains(id));
        self.texts.retain(|id, _| !ids.contains(id));
        self.face_colors.retain(|id, _| self.faces.contains_key(id));
        self.edges
            .retain(|id, _| !ids.contains(id) || outside_edges.contains(id));
        self.objects.retain(|id, _| !ids.contains(id));
        self.clean_entity_state();
        let entities = self.paste_entities(&copy, DVec3::ZERO);
        let id = self.id();
        let definition_entities = if component {
            self.expand_entities(&entities)
                .into_iter()
                .filter(|id| !self.objects.contains_key(id))
                .map(|id| (id, id))
                .collect()
        } else {
            BTreeMap::new()
        };
        self.objects.insert(
            id,
            Object {
                id,
                surface_treatment: Default::default(),
                surface_treatment_customized: false,
                stock_material: None,
                stock_material_id: None,
                outliner_order: None,
                transform: Some(crate::InstanceTransform::default()),
                name: if name.trim().is_empty() {
                    if component { "Component" } else { "Group" }.into()
                } else {
                    name.trim().into()
                },
                kind: if component {
                    ObjectKind::Component {
                        definition: uuid::Uuid::new_v4(),
                    }
                } else {
                    ObjectKind::Group
                },
                entities,
                definition_entities,
            },
        );
        let used: BTreeSet<_> = self.edges.values().flat_map(|e| [e.a, e.b]).collect();
        self.vertices.retain(|id, _| used.contains(id));
        self.sync_dimensions();
        Some(id)
    }
    /// Release one level, preserving nested groups and component instances.
    pub fn dissolve_objects(
        &mut self,
        selected: &BTreeSet<u64>,
        groups_only: bool,
    ) -> BTreeSet<u64> {
        if self.edit_context.is_some() {
            return self
                .edit_in_context(|model| Ok(model.dissolve_objects(selected, groups_only)))
                .unwrap_or_else(|_| selected.clone());
        }
        let mut result = BTreeSet::new();
        for id in selected {
            if self.is_locked(*id) {
                result.insert(*id);
                continue;
            }
            if self
                .objects
                .get(id)
                .is_some_and(|o| !groups_only || o.kind == ObjectKind::Group)
            {
                let object = self.objects.remove(id).unwrap();
                result.extend(&object.entities);
                for parent in self.objects.values_mut() {
                    if parent.entities.remove(id) {
                        parent.entities.extend(&object.entities);
                    }
                }
                self.entity_tags.remove(id);
                self.hidden.remove(id);
                self.locked.remove(id);
            } else {
                result.insert(*id);
            }
        }
        result
    }
    pub(crate) fn validate_objects(&self) -> Result<(), &'static str> {
        let mut all_ids = BTreeSet::new();
        if self
            .vertices
            .keys()
            .chain(self.edges.keys())
            .chain(self.faces.keys())
            .chain(self.objects.keys())
            .chain(self.dimensions.keys())
            .chain(self.texts.keys())
            .any(|id| !all_ids.insert(*id))
        {
            return Err("Duplicate entity ID");
        }
        let mut parents = BTreeMap::new();
        for (id, object) in &self.objects {
            if *id != object.id
                || object.name.len() > 1024
                || object.transform.is_some_and(|t| !t.valid())
            {
                return Err("Invalid object");
            }
            if let ObjectKind::Component { definition } = object.kind {
                if definition.is_nil() {
                    return Err("Invalid component definition");
                }
            }
            for child in &object.entities {
                if !self.contains_entity(child) || parents.insert(*child, *id).is_some() {
                    return Err("Invalid object membership");
                }
            }
        }
        for id in self.objects.keys() {
            let mut seen = BTreeSet::new();
            let mut current = *id;
            while let Some(parent) = parents.get(&current) {
                if !seen.insert(current) {
                    return Err("Cyclic object membership");
                }
                current = *parent;
            }
        }
        let mut definitions: BTreeMap<uuid::Uuid, BTreeMap<u64, bool>> = BTreeMap::new();
        for object in self.objects.values() {
            if object.kind == ObjectKind::Group {
                if !object.definition_entities.is_empty() {
                    return Err("A group cannot have a component mapping");
                }
                continue;
            }
            let contents = self.expand_entities(&object.entities);
            if object
                .definition_entities
                .values()
                .any(|id| !contents.contains(id) || self.objects.contains_key(id))
            {
                return Err("Invalid component instance mapping");
            }
            if let ObjectKind::Component { definition } = object.kind {
                let mapped: BTreeSet<_> = object.definition_entities.values().copied().collect();
                let geometry: BTreeSet<_> = contents
                    .into_iter()
                    .filter(|id| !self.objects.contains_key(id))
                    .collect();
                if mapped != geometry || mapped.len() != object.definition_entities.len() {
                    return Err("Incomplete component instance mapping");
                }
                let types: BTreeMap<_, _> = object
                    .definition_entities
                    .iter()
                    .map(|(key, id)| (*key, self.faces.contains_key(id)))
                    .collect();
                if definitions
                    .get(&definition)
                    .is_some_and(|previous| previous != &types)
                {
                    return Err("Mismatched component instances");
                }
                definitions.insert(definition, types);
            } else if !object.definition_entities.is_empty() {
                return Err("A group cannot have a component mapping");
            }
        }
        self.validate_definition_nesting()?;
        if self
            .hidden
            .iter()
            .chain(self.locked.iter())
            .any(|id| !self.contains_entity(id))
        {
            return Err("Invalid entity state");
        }
        Ok(())
    }
}
