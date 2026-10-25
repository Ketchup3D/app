//! Object hierarchy edits preserve world-space geometry and instance placement.
use crate::{Model, ObjectKind};
use std::collections::BTreeSet;

impl Model {
    /// Check a hierarchy drop without copying or changing the model.
    pub fn check_reparent_objects(
        &self,
        ids: &BTreeSet<u64>,
        parent: Option<u64>,
    ) -> Result<(), &'static str> {
        if ids.is_empty() || ids.iter().any(|id| !self.objects.contains_key(id)) {
            return Err("Drag groups or components to move them in the Outliner");
        }
        if self.selection_protected(ids) {
            return Err("Unlock the objects and their contents before moving them");
        }
        if let Some(parent) = parent {
            if !self.objects.get(&parent).is_some_and(|object| {
                object.kind == ObjectKind::Group
                    || ids.iter().all(|id| self.parent(*id) == Some(parent))
            }) {
                return Err("Drop inside a group or between sibling objects");
            }
            if self.is_locked(parent) || self.is_hidden(parent) {
                return Err("Show and unlock the destination group before moving objects into it");
            }
            let mut current = Some(parent);
            while let Some(id) = current {
                if ids.contains(&id) {
                    return Err("An object cannot be moved into itself or its descendants");
                }
                current = self.parent(id);
            }
        }
        if ids
            .iter()
            .filter_map(|id| self.parent(*id))
            .any(|parent| self.is_hidden(parent))
        {
            return Err("Show the parent object before moving its contents");
        }
        Ok(())
    }

    /// Move objects into a group, or to the model root (`None`), atomically.
    /// IDs, vertices, transforms, names, materials, and explicit visibility are retained.
    pub fn reparent_objects(
        &mut self,
        ids: &BTreeSet<u64>,
        parent: Option<u64>,
    ) -> Result<BTreeSet<u64>, &'static str> {
        self.check_reparent_objects(ids, parent)?;
        // If both an ancestor and descendant are supplied, move the ancestor once.
        let roots: BTreeSet<_> = ids
            .iter()
            .copied()
            .filter(|id| {
                let mut current = self.parent(*id);
                while let Some(parent) = current {
                    if ids.contains(&parent) {
                        return false;
                    }
                    current = self.parent(parent);
                }
                true
            })
            .collect();
        let moved: BTreeSet<_> = roots
            .iter()
            .copied()
            .filter(|id| self.parent(*id) != parent)
            .collect();
        if moved.is_empty() {
            return Ok(roots);
        }
        let contexts: BTreeSet<_> = moved
            .iter()
            .filter_map(|id| self.parent(*id))
            .chain(parent)
            .collect();
        let mut next = self.clone();
        for object in next.objects.values_mut() {
            object.entities.retain(|id| !moved.contains(id));
        }
        if let Some(parent) = parent {
            next.objects
                .get_mut(&parent)
                .unwrap()
                .entities
                .extend(&moved);
        }
        next.edit_context = parent;
        // Validate and update both sides before committing, including nested components.
        next.synchronize_object_contexts(contexts)?;
        *self = next;
        Ok(roots)
    }

    pub fn ordered_objects(&self, ids: &BTreeSet<u64>) -> Vec<u64> {
        let mut objects: Vec<_> = ids
            .iter()
            .copied()
            .filter(|id| self.objects.contains_key(id))
            .collect();
        objects.sort_by_key(|id| (self.objects[id].outliner_order.unwrap_or(*id), *id));
        objects
    }

    pub fn check_move_objects_before(
        &self,
        ids: &BTreeSet<u64>,
        parent: Option<u64>,
        before: Option<u64>,
    ) -> Result<(), &'static str> {
        self.check_reparent_objects(ids, parent)?;
        if let Some(before) = before {
            if !self.objects.contains_key(&before)
                || self.parent(before) != parent
                || ids.contains(&before)
            {
                return Err("Choose an insertion point outside the dragged objects");
            }
        }
        Ok(())
    }

    /// Move and reorder a block of objects. `None` appends to the destination.
    /// Ordering belongs to the document and follows undo, saving, and component edits.
    pub fn move_objects_before(
        &mut self,
        ids: &BTreeSet<u64>,
        parent: Option<u64>,
        before: Option<u64>,
    ) -> Result<BTreeSet<u64>, &'static str> {
        self.check_move_objects_before(ids, parent, before)?;
        let mut next = self.clone();
        let roots = next.reparent_objects(ids, parent)?;
        let siblings = parent
            .map(|id| next.objects[&id].entities.clone())
            .unwrap_or_else(|| next.root_entities());
        let previous = next.ordered_objects(&siblings);
        let moved = self.ordered_objects(&roots);
        let mut order: Vec<_> = previous
            .iter()
            .copied()
            .filter(|id| !roots.contains(id))
            .collect();
        let index = before
            .and_then(|id| order.iter().position(|other| *other == id))
            .unwrap_or(order.len());
        order.splice(index..index, moved);
        if order != previous {
            for (position, id) in order.into_iter().enumerate() {
                next.objects.get_mut(&id).unwrap().outliner_order = Some(position as u64);
            }
            next.synchronize_object_contexts(parent)?;
        }
        *self = next;
        Ok(roots)
    }
}
