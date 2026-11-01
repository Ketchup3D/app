//! Reversible entity differences; unchanged geometry is never retained per step.
use crate::Model;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
const LIMIT: usize = 100;
fn history_version() -> u32 {
    1
}
type Changes<K, V> = Vec<(K, Option<V>, Option<V>)>;

fn difference<K: Ord + Clone, V: PartialEq + Clone>(
    before: &BTreeMap<K, V>,
    after: &BTreeMap<K, V>,
) -> Changes<K, V> {
    let mut old = before.iter().peekable();
    let mut new = after.iter().peekable();
    let mut changes = Vec::new();
    loop {
        match (old.peek(), new.peek()) {
            (Some((old_key, _)), Some((new_key, _))) if old_key == new_key => {
                let (key, before) = old.next().unwrap();
                let (_, after) = new.next().unwrap();
                if before != after {
                    changes.push((key.clone(), Some(before.clone()), Some(after.clone())));
                }
            }
            (Some((old_key, _)), Some((new_key, _))) if old_key < new_key => {
                let (key, value) = old.next().unwrap();
                changes.push((key.clone(), Some(value.clone()), None));
            }
            (_, Some(_)) => {
                let (key, value) = new.next().unwrap();
                changes.push((key.clone(), None, Some(value.clone())));
            }
            (Some(_), None) => {
                let (key, value) = old.next().unwrap();
                changes.push((key.clone(), Some(value.clone()), None));
            }
            (None, None) => break,
        }
    }
    changes
}
fn matches<K: Ord, V: PartialEq>(
    changes: &Changes<K, V>,
    target: &BTreeMap<K, V>,
    forward: bool,
) -> bool {
    changes.windows(2).all(|pair| pair[0].0 < pair[1].0)
        && changes.iter().all(|(key, before, after)| {
            target.get(key)
                == if forward {
                    before.as_ref()
                } else {
                    after.as_ref()
                }
        })
}
fn apply<K: Ord + Clone, V: Clone>(
    changes: &Changes<K, V>,
    target: &mut BTreeMap<K, V>,
    forward: bool,
) {
    for (key, before, after) in changes {
        if let Some(value) = if forward { after } else { before } {
            target.insert(key.clone(), value.clone());
        } else {
            target.remove(key);
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct SetChange {
    added: Vec<u64>,
    removed: Vec<u64>,
}
impl SetChange {
    fn between(before: &BTreeSet<u64>, after: &BTreeSet<u64>) -> Self {
        Self {
            added: after.difference(before).copied().collect(),
            removed: before.difference(after).copied().collect(),
        }
    }
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
    fn matches(&self, target: &BTreeSet<u64>, forward: bool) -> bool {
        self.added.windows(2).all(|p| p[0] < p[1])
            && self.removed.windows(2).all(|p| p[0] < p[1])
            && self.added.iter().all(|id| target.contains(id) != forward)
            && self.removed.iter().all(|id| target.contains(id) == forward)
    }
    fn apply(&self, target: &mut BTreeSet<u64>, forward: bool) {
        let (added, removed) = if forward {
            (&self.added, &self.removed)
        } else {
            (&self.removed, &self.added)
        };
        for id in removed {
            target.remove(id);
        }
        target.extend(added);
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct ModelChange {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    vertices: Changes<u64, crate::Vertex>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    edges: Changes<u64, crate::Edge>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    faces: Changes<u64, crate::Face>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    face_colors: Changes<u64, [u8; 3]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dimensions: Changes<u64, crate::Dimension>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    texts: Changes<u64, crate::TextAnnotation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    objects: Changes<u64, crate::Object>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stock_materials: Changes<crate::MaterialId, crate::StockMaterial>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Changes<u64, crate::Tag>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    entity_tags: Changes<u64, u64>,
    #[serde(skip_serializing_if = "SetChange::is_empty")]
    hidden: SetChange,
    #[serde(skip_serializing_if = "SetChange::is_empty")]
    locked: SetChange,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_id: Option<(u64, u64)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    edit_context: Option<(Option<u64>, Option<u64>)>,
}
impl ModelChange {
    fn between(before: &Model, after: &Model) -> Self {
        Self {
            vertices: difference(&before.vertices, &after.vertices),
            edges: difference(&before.edges, &after.edges),
            faces: difference(&before.faces, &after.faces),
            face_colors: difference(&before.face_colors, &after.face_colors),
            dimensions: difference(&before.dimensions, &after.dimensions),
            texts: difference(&before.texts, &after.texts),
            objects: difference(&before.objects, &after.objects),
            stock_materials: difference(&before.stock_materials, &after.stock_materials),
            tags: difference(&before.tags, &after.tags),
            entity_tags: difference(&before.entity_tags, &after.entity_tags),
            hidden: SetChange::between(&before.hidden, &after.hidden),
            locked: SetChange::between(&before.locked, &after.locked),
            next_id: (before.next_id != after.next_id).then_some((before.next_id, after.next_id)),
            edit_context: (before.edit_context != after.edit_context)
                .then_some((before.edit_context, after.edit_context)),
        }
    }
    fn is_empty(&self) -> bool {
        self.vertices.is_empty()
            && self.edges.is_empty()
            && self.faces.is_empty()
            && self.face_colors.is_empty()
            && self.dimensions.is_empty()
            && self.texts.is_empty()
            && self.objects.is_empty()
            && self.stock_materials.is_empty()
            && self.tags.is_empty()
            && self.entity_tags.is_empty()
            && self.hidden.is_empty()
            && self.locked.is_empty()
            && self.next_id.is_none()
            && self.edit_context.is_none()
    }
    fn apply_checked(&self, model: &mut Model, forward: bool) -> bool {
        if !(matches(&self.vertices, &model.vertices, forward)
            && matches(&self.edges, &model.edges, forward)
            && matches(&self.faces, &model.faces, forward)
            && matches(&self.face_colors, &model.face_colors, forward)
            && matches(&self.dimensions, &model.dimensions, forward)
            && matches(&self.texts, &model.texts, forward)
            && matches(&self.objects, &model.objects, forward)
            && matches(&self.stock_materials, &model.stock_materials, forward)
            && matches(&self.tags, &model.tags, forward)
            && matches(&self.entity_tags, &model.entity_tags, forward)
            && self.hidden.matches(&model.hidden, forward)
            && self.locked.matches(&model.locked, forward)
            && self.next_id.is_none_or(|(before, after)| {
                model.next_id == if forward { before } else { after }
            }))
        {
            return false;
        }
        apply(&self.vertices, &mut model.vertices, forward);
        apply(&self.edges, &mut model.edges, forward);
        apply(&self.faces, &mut model.faces, forward);
        apply(&self.face_colors, &mut model.face_colors, forward);
        apply(&self.dimensions, &mut model.dimensions, forward);
        apply(&self.texts, &mut model.texts, forward);
        apply(&self.objects, &mut model.objects, forward);
        apply(&self.stock_materials, &mut model.stock_materials, forward);
        apply(&self.tags, &mut model.tags, forward);
        apply(&self.entity_tags, &mut model.entity_tags, forward);
        self.hidden.apply(&mut model.hidden, forward);
        self.locked.apply(&mut model.locked, forward);
        if let Some((before, after)) = self.next_id {
            model.next_id = if forward { after } else { before };
        }
        // Editing-context navigation is not an undo operation, so do not use it
        // as a precondition. The app also restores its current valid edit path.
        if let Some((before, after)) = self.edit_context {
            model.edit_context = if forward { after } else { before };
        }
        true
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Entry {
    label: String,
    change: ModelChange,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    format_version: u32,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}
impl Default for History {
    fn default() -> Self {
        Self {
            format_version: history_version(),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}
impl History {
    pub fn is_empty(&self) -> bool {
        self.undo.is_empty() && self.redo.is_empty()
    }
    pub fn record(&mut self, label: impl Into<String>, before: Model, after: &Model) {
        let change = ModelChange::between(&before, after);
        if change.is_empty() {
            return;
        }
        self.undo.push(Entry {
            label: label.into(),
            change,
        });
        self.redo.clear();
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
    }
    /// Replace the committed result of a compound action, such as revising a
    /// copy array, while retaining its original label and single undo step.
    pub fn amend_last(&mut self, original: &Model, after: &Model) -> bool {
        let Some(entry) = self.undo.last_mut() else {
            return false;
        };
        entry.change = ModelChange::between(original, after);
        if entry.change.is_empty() {
            self.undo.pop();
        }
        self.redo.clear();
        true
    }
    fn transfer(
        from: &mut Vec<Entry>,
        to: &mut Vec<Entry>,
        model: &mut Model,
        forward: bool,
    ) -> bool {
        let Some(entry) = from.last() else {
            return false;
        };
        // Stage one result for atomic application, rather than retaining one
        // complete model for every operation in the history.
        let mut candidate = model.clone();
        if !entry.change.apply_checked(&mut candidate, forward) || candidate.validate().is_err() {
            return false;
        }
        *model = candidate;
        to.push(from.pop().unwrap());
        true
    }
    pub fn undo(&mut self, model: &mut Model) -> bool {
        Self::transfer(&mut self.undo, &mut self.redo, model, false)
    }
    pub fn redo(&mut self, model: &mut Model) -> bool {
        Self::transfer(&mut self.redo, &mut self.undo, model, true)
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|entry| entry.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|entry| entry.label.as_str())
    }
    pub(crate) fn restore(&mut self, model: &Model) {
        if self.format_version != history_version() {
            *self = Self::default();
            return;
        }
        if self.redo.len() > LIMIT {
            self.redo.drain(..self.redo.len() - LIMIT);
        }
        let keep_undo = LIMIT - self.redo.len();
        if self.undo.len() > keep_undo {
            self.undo.drain(..self.undo.len() - keep_undo);
        }
        let valid =
            [(&self.undo, false), (&self.redo, true)]
                .into_iter()
                .all(|(entries, forward)| {
                    if entries.is_empty() {
                        return true;
                    }
                    let mut candidate = model.clone();
                    entries.iter().rev().all(|entry| {
                        entry.change.apply_checked(&mut candidate, forward)
                            && candidate.validate().is_ok()
                    })
                });
        if !valid {
            *self = Self::default();
        }
    }
}
// A malformed optional history must not make otherwise valid geometry unreadable.
pub(crate) fn deserialize_history<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<History, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DVec3, Document};
    fn line(doc: &mut Document, y: f64, label: &str) {
        let before = doc.model.clone();
        doc.model
            .add_edge(DVec3::Y * y, DVec3::Y * y + DVec3::X)
            .unwrap();
        doc.history.record(label, before, &doc.model);
    }
    #[test]
    fn sparse_edits_store_only_changed_entities() {
        let mut doc = Document::default();
        for i in 0..100 {
            doc.model
                .add_edge(DVec3::Y * i as f64, DVec3::Y * i as f64 + DVec3::X)
                .unwrap();
        }
        let before = doc.model.clone();
        doc.model.vertices.values_mut().next().unwrap().position += DVec3::Z;
        doc.history.record("Move vertex", before, &doc.model);
        let change = &doc.history.undo[0].change;
        assert_eq!(change.vertices.len(), 1);
        assert!(change.edges.is_empty() && change.objects.is_empty());
        assert!(
            serde_json::to_vec(&doc.history).unwrap().len() * 5
                < serde_json::to_vec(&doc.model).unwrap().len()
        );
    }
    #[test]
    fn undo_and_redo_survive_project_round_trip_and_branching() {
        let mut doc = Document::default();
        line(&mut doc, 0., "First");
        let first = doc.model.clone();
        line(&mut doc, 1., "Second");
        let second = doc.model.clone();
        assert!(doc.history.undo(&mut doc.model));
        let mut restored = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(restored.history.undo_label(), Some("First"));
        assert_eq!(restored.history.redo_label(), Some("Second"));
        assert!(restored.history.undo(&mut restored.model));
        assert_eq!(restored.model, Model::default());
        assert!(restored.history.redo(&mut restored.model));
        assert_eq!(restored.model, first);
        assert!(restored.history.redo(&mut restored.model));
        assert_eq!(restored.model, second);
        assert!(restored.history.undo(&mut restored.model));
        line(&mut restored, 2., "Branch");
        assert!(restored.history.redo_label().is_none());
    }
    #[test]
    fn malformed_or_incompatible_history_does_not_discard_geometry() {
        let mut doc = Document::default();
        line(&mut doc, 0., "Line");
        let mut value = serde_json::to_value(&doc).unwrap();
        value["history"]["format_version"] = 99.into();
        let restored = Document::from_json(&value.to_string()).unwrap();
        assert_eq!(restored.model, doc.model);
        assert!(restored.history.is_empty());
        value["history"] = serde_json::json!({"undo":"invalid"});
        let restored = Document::from_json(&value.to_string()).unwrap();
        assert_eq!(restored.model, doc.model);
        assert!(restored.history.is_empty());
        value["history"] = serde_json::to_value(&doc.history).unwrap();
        value["history"]["undo"][0]["change"]["vertices"][0][2]["id"] = 9999.into();
        let restored = Document::from_json(&value.to_string()).unwrap();
        assert_eq!(restored.model, doc.model);
        assert!(restored.history.is_empty());
    }
    #[test]
    fn mismatched_changes_fail_atomically_and_leave_history_available() {
        let mut doc = Document::default();
        line(&mut doc, 0., "Line");
        doc.model.vertices.values_mut().next().unwrap().position += DVec3::Z;
        let before = doc.model.clone();
        let history = doc.history.clone();
        assert!(!doc.history.undo(&mut doc.model));
        assert_eq!(doc.model, before);
        assert_eq!(doc.history, history);
    }

    #[test]
    fn amended_compound_action_remains_one_persistent_undo_step() {
        let mut doc = Document::default();
        let original = doc.model.clone();
        line(&mut doc, 0., "Copy");
        let mut revised = doc.model.clone();
        revised.add_edge(DVec3::Y, DVec3::Y + DVec3::X).unwrap();
        assert!(doc.history.amend_last(&original, &revised));
        doc.model = revised.clone();
        let mut restored = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(restored.history.undo_label(), Some("Copy"));
        assert!(restored.history.undo(&mut restored.model));
        assert_eq!(restored.model, original);
        assert!(restored.history.undo_label().is_none());
        assert!(restored.history.redo(&mut restored.model));
        assert_eq!(restored.model, revised);
    }
}
