use crate::Model;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub color: [u8; 3],
}
impl Model {
    pub fn add_tag(&mut self, name: &str) -> Result<u64, &'static str> {
        let name = name.trim();
        if name.is_empty() || name.len() > 128 || name.eq_ignore_ascii_case("Untagged") {
            return Err("Choose a tag name of 1–128 characters other than Untagged");
        }
        let folded = name.to_lowercase();
        if self
            .tags
            .values()
            .any(|tag| tag.name.to_lowercase() == folded)
        {
            return Err("A tag with that name already exists");
        }
        let id = self.id();
        let colors = [
            [108, 148, 102],
            [95, 143, 177],
            [195, 96, 67],
            [163, 121, 167],
            [225, 179, 87],
        ];
        let color = colors[self.tags.len() % colors.len()];
        self.tags.insert(
            id,
            Tag {
                id,
                name: name.into(),
                visible: true,
                color,
            },
        );
        Ok(id)
    }
    pub fn rename_tag(&mut self, id: u64, name: &str) -> Result<(), &'static str> {
        let name = name.trim();
        if name.is_empty() || name.len() > 128 || name.eq_ignore_ascii_case("Untagged") {
            return Err("Choose a tag name of 1–128 characters other than Untagged");
        }
        let folded = name.to_lowercase();
        if self
            .tags
            .values()
            .any(|tag| tag.id != id && tag.name.to_lowercase() == folded)
        {
            return Err("A tag with that name already exists");
        }
        self.tags.get_mut(&id).ok_or("Missing tag")?.name = name.into();
        Ok(())
    }
    pub fn remove_tag(&mut self, id: u64) {
        self.entity_tags.retain(|_, tag| *tag != id);
        self.tags.remove(&id);
    }
    pub fn assign_tag(
        &mut self,
        entities: &BTreeSet<u64>,
        tag: Option<u64>,
    ) -> Result<(), &'static str> {
        if tag.is_some_and(|id| !self.tags.contains_key(&id)) {
            return Err("Missing tag");
        }
        if self.selection_protected(entities) {
            return Err("Unlock the selection before assigning a tag");
        }
        for id in entities
            .iter()
            .filter(|id| self.contains_entity(id))
            .copied()
            .collect::<Vec<_>>()
        {
            if let Some(tag) = tag {
                self.entity_tags.insert(id, tag);
            } else {
                self.entity_tags.remove(&id);
            }
        }
        Ok(())
    }
    pub fn inherited_tag(&self, mut id: u64) -> Option<&Tag> {
        loop {
            if let Some(tag) = self.entity_tags.get(&id).and_then(|tag| self.tags.get(tag)) {
                return Some(tag);
            }
            id = self.parent(id)?;
        }
    }
    pub fn is_tag_hidden(&self, mut id: u64) -> bool {
        if self.entity_tags.is_empty() {
            return false;
        }
        loop {
            if self
                .entity_tags
                .get(&id)
                .and_then(|tag| self.tags.get(tag))
                .is_some_and(|tag| !tag.visible)
            {
                return true;
            }
            let Some(parent) = self.parent(id) else {
                return false;
            };
            id = parent;
        }
    }
    pub(crate) fn validate_tags(&self) -> Result<(), &'static str> {
        let mut names = BTreeSet::new();
        if self.tags.len() > 10000
            || self.tags.iter().any(|(id, tag)| {
                *id != tag.id
                    || *id == 0
                    || *id > self.next_id
                    || self.contains_entity(id)
                    || self.vertices.contains_key(id)
                    || tag.name.trim().is_empty()
                    || tag.name.len() > 128
                    || tag.name.eq_ignore_ascii_case("Untagged")
                    || !names.insert(tag.name.to_lowercase())
            })
            || self
                .entity_tags
                .iter()
                .any(|(id, tag)| !self.contains_entity(id) || !self.tags.contains_key(tag))
        {
            return Err("Invalid tags");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DVec3, Document, History};

    #[test]
    fn tag_visibility_inherits_through_objects_and_preserves_explicit_hiding() {
        let mut model = Model::default();
        let edge = model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        let group = model
            .make_object(&BTreeSet::from([edge]), false, "Group")
            .unwrap();
        let edge = *model.edges.keys().next().unwrap();
        let tag = model.add_tag("Structure").unwrap();
        model
            .assign_tag(&BTreeSet::from([group]), Some(tag))
            .unwrap();
        model.hidden.insert(edge);
        model.tags.get_mut(&tag).unwrap().visible = false;
        assert!(model.is_hidden(edge));
        assert!(model.is_tag_hidden(edge));
        assert!(model.selectable_entities().is_empty());
        model.tags.get_mut(&tag).unwrap().visible = true;
        assert!(model.is_hidden(edge));
        assert!(!model.is_hidden(group));
        assert_eq!(model.inherited_tag(edge).unwrap().id, tag);
        model.remove_tag(tag);
        assert!(model.entity_tags.is_empty());
        assert!(model.hidden.contains(&edge));
        model.validate().unwrap();
    }

    #[test]
    fn tagged_clipboard_merges_names_and_survives_context_edits_undo_and_files() {
        let mut source = Model::default();
        let edge = source.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        let tag = source.add_tag("Structure").unwrap();
        source
            .assign_tag(&BTreeSet::from([edge]), Some(tag))
            .unwrap();
        source.add_tag("Unused").unwrap();
        let clipboard = source.copy_entities(&BTreeSet::from([edge]));
        assert_eq!(clipboard.tags.len(), 1);
        let mut model = Model::default();
        let existing = model.add_tag("structure").unwrap();
        let pasted = model.paste_entities(&clipboard, DVec3::Y);
        assert_eq!(model.tags.len(), 1);
        assert_eq!(model.entity_tags[&*pasted.first().unwrap()], existing);
        let group = model.make_object(&pasted, false, "Group").unwrap();
        assert!(model.enter_object(group));
        let before = model.clone();
        model.add_edge(DVec3::ZERO, DVec3::Z).unwrap();
        assert_eq!(model.entity_tags.len(), 1);
        let mut history = History::default();
        history.record("Draw", before.clone(), &model);
        assert!(history.undo(&mut model));
        assert_eq!(model, before);
        assert!(history.redo(&mut model));
        model.validate().unwrap();
        let doc = Document {
            model,
            ..Default::default()
        };
        let restored = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(doc.model.tags, restored.model.tags);
        assert_eq!(doc.model.entity_tags, restored.model.entity_tags);
    }

    #[test]
    fn tag_validation_rejects_invalid_references_and_duplicate_names() {
        let mut model = Model::default();
        let tag = model.add_tag("Structure").unwrap();
        assert!(model.add_tag("structure").is_err());
        assert!(model.add_tag("Untagged").is_err());
        assert!(model.rename_tag(tag, " ").is_err());
        model.entity_tags.insert(999, tag);
        assert!(model.validate().is_err());
    }

    #[test]
    fn unicode_tag_names_cannot_create_invalid_case_duplicates() {
        let mut model = Model::default();
        let first = model.add_tag("Étage").unwrap();
        let second = model.add_tag("Structure").unwrap();
        let before = model.clone();
        assert!(model.add_tag("étage").is_err());
        assert!(model.rename_tag(second, "ÉTAGE").is_err());
        assert_eq!(model, before);
        assert!(model.rename_tag(first, "ÉTAGE").is_ok());
        model.validate().unwrap();
        let edge = model.add_edge(crate::DVec3::ZERO, crate::DVec3::X).unwrap();
        model
            .assign_tag(&BTreeSet::from([edge]), Some(first))
            .unwrap();
        let mut destination = Model::default();
        let existing = destination.add_tag("étage").unwrap();
        let pasted = destination.paste_entities(
            &model.copy_entities(&BTreeSet::from([edge])),
            crate::DVec3::ZERO,
        );
        assert!(!pasted.is_empty());
        assert_eq!(destination.tags.len(), 1);
        assert_eq!(destination.entity_tags[pasted.first().unwrap()], existing);
        destination.validate().unwrap();
    }
}
