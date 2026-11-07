//! Construction references, deliberately separate from edges and faces.
use crate::{Camera, DVec2, DVec3, Model};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConstructionGuide {
    pub origin: DVec3,
    /// A unit direction for an infinite line; None denotes a guide point.
    pub direction: Option<DVec3>,
}

impl ConstructionGuide {
    /// Clip the visible infinite line identically for drawing and picking.
    pub fn screen_segment(&self, camera: &Camera, size: DVec2) -> Option<(DVec2, DVec2)> {
        let direction = self.direction?;
        let center = self.origin + direction * (camera.target - self.origin).dot(direction);
        let extent = camera.distance.max(1.) * 100.;
        camera.segment(
            center - direction * extent,
            center + direction * extent,
            size,
        )
    }

    pub fn valid(&self) -> bool {
        self.origin.is_finite()
            && self.origin.abs().max_element() <= 1e10
            && self
                .direction
                .is_none_or(|d| d.is_finite() && (d.length() - 1.).abs() < 1e-7)
    }
}

impl Model {
    pub fn clear_guides(&mut self) {
        let removed: std::collections::BTreeSet<_> = self.guides.keys().copied().collect();
        self.guides.clear();
        for object in self.objects.values_mut() {
            object
                .definition_entities
                .retain(|_, id| !removed.contains(id));
        }
        self.clean_entity_state();
    }

    pub fn add_guide(
        &mut self,
        origin: DVec3,
        direction: Option<DVec3>,
    ) -> Result<u64, &'static str> {
        if self.edit_context.is_some() {
            return self.edit_in_context(|model| model.add_guide(origin, direction));
        }
        let direction = direction
            .map(|d| d.try_normalize().ok_or("Choose a direction for the guide"))
            .transpose()?;
        let guide = ConstructionGuide { origin, direction };
        if !guide.valid() {
            return Err("Invalid guide coordinates");
        }
        let id = self.id();
        self.guides.insert(id, guide);
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, History};
    use std::collections::BTreeSet;

    #[test]
    fn guides_are_entities_and_locked_guides_cannot_be_erased() {
        let mut model = Model::default();
        let id = model.add_guide(DVec3::Y, Some(DVec3::X)).unwrap();
        let selected = BTreeSet::from([id]);
        assert!(model.contains_entity(&id));
        assert_eq!(model.entity_in_context(id), Some(id));
        assert_eq!(model.selectable_entities(), selected);
        model.locked.insert(id);
        model.erase(&selected);
        assert!(model.guides.contains_key(&id));
        assert!(model.move_selection(&selected, DVec3::Z).is_err());
        model.locked.clear();
        model.hidden.insert(id);
        assert!(model.selectable_entities().is_empty());
        model.validate().unwrap();
        model.hidden.clear();
        model.erase(&selected);
        assert!(model.guides.is_empty());
        assert!(!model.contains_entity(&id));
        model.validate().unwrap();
    }

    #[test]
    fn guide_selection_supports_copy_paste_transform_and_move_previews() {
        let mut model = Model::default();
        let id = model.add_guide(DVec3::Y, Some(DVec3::X)).unwrap();
        let point = model.add_guide(DVec3::new(1., 2., 3.), None).unwrap();
        let selected = BTreeSet::from([id, point]);
        let copy = model.copy_entities(&selected);
        assert_eq!(copy.guides, model.guides);
        assert!(copy.bounds().is_some());
        let before = model.clone();
        let moving = crate::modeling::MovePreview::new(&model, &selected).unwrap();
        let mut preview = model.clone();
        moving.update(&mut preview, DVec3::Z).unwrap();
        model.move_selection(&selected, DVec3::Z).unwrap();
        assert_eq!(preview, model);
        assert_eq!(model.guides[&id].origin, DVec3::Y + DVec3::Z);
        assert_eq!(model.guides[&point].origin, DVec3::new(1., 2., 4.));
        model
            .rotate_selection(&selected, DVec3::ZERO, DVec3::Z, 90.)
            .unwrap();
        assert!(model.guides[&id].direction.unwrap().distance(DVec3::Y) < 1e-6);
        let pasted = model.paste_entities(&copy, DVec3::X * 2.);
        assert_eq!(pasted.len(), 2);
        assert!(pasted.iter().all(|id| !selected.contains(id)));
        for id in pasted {
            let guide = &model.guides[&id];
            assert!(
                before
                    .guides
                    .values()
                    .any(|source| source.direction == guide.direction
                        && source.origin + DVec3::X * 2. == guide.origin)
            );
        }
        model.validate().unwrap();
    }

    #[test]
    fn guides_inside_components_copy_and_sync_and_clear_without_removing_geometry() {
        let mut model = Model::default();
        let guide = model.add_guide(DVec3::Y, Some(DVec3::X)).unwrap();
        let edge = model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        let group = model
            .make_object(&BTreeSet::from([guide, edge]), true, "Guides")
            .unwrap();
        let guide = *model.objects[&group]
            .entities
            .iter()
            .find(|id| model.guides.contains_key(id))
            .unwrap();
        assert_eq!(model.entity_in_context(guide), Some(group));
        let copy = model.copy_entities(&BTreeSet::from([group]));
        let peer = *model.paste_entities(&copy, DVec3::Y * 3.).first().unwrap();
        assert!(model.enter_object(group));
        model
            .move_selection(&BTreeSet::from([guide]), DVec3::Z)
            .unwrap();
        let peer_guide = *model.objects[&peer]
            .entities
            .iter()
            .find(|id| model.guides.contains_key(id))
            .unwrap();
        assert_eq!(
            model.guides[&peer_guide].origin,
            model.guides[&guide].origin + DVec3::Y * 3.
        );
        let created = model.add_guide(DVec3::Z * 2., Some(DVec3::Y)).unwrap();
        assert_eq!(model.entity_in_context(created), Some(created));
        assert!(model.objects[&group].entities.contains(&created));
        model.validate().unwrap();
        model.erase(&BTreeSet::from([created]));
        assert!(!model.guides.contains_key(&created));
        model.validate().unwrap();
        let edges = model.edges.clone();
        model.clear_guides();
        assert!(model.guides.is_empty());
        assert_eq!(model.edges, edges);
        model.validate().unwrap();
    }

    #[test]
    fn picking_and_box_selection_use_the_visible_infinite_line_and_legacy_points() {
        let mut model = Model::default();
        let camera = Camera::default();
        let size = DVec2::new(900., 700.);
        let id = model.add_guide(DVec3::Y, Some(DVec3::X)).unwrap();
        let point = camera.project(DVec3::new(3., 1., 0.), size).unwrap();
        assert_eq!(
            crate::selection::pick_guide(&model, &camera, size, point, 8.),
            Some(id)
        );
        let start = point - DVec2::splat(15.);
        let end = point + DVec2::splat(15.);
        assert_eq!(
            crate::selection::box_select(&model, &camera, size, start, end),
            BTreeSet::from([id])
        );
        assert!(
            crate::selection::box_select_with_guides(&model, &camera, size, start, end, false)
                .is_empty()
        );
        model.hidden.insert(id);
        assert_eq!(
            crate::selection::pick_guide(&model, &camera, size, point, 8.),
            None
        );
        let point_id = model.add_guide(-DVec3::Y, None).unwrap();
        let point = camera.project(-DVec3::Y, size).unwrap();
        assert_eq!(
            crate::selection::pick_guide(&model, &camera, size, point, 8.),
            Some(point_id)
        );
    }

    #[test]
    fn guides_roundtrip_and_undo_without_creating_model_geometry() {
        let mut doc = Document::default();
        let before = doc.model.clone();
        doc.model.add_guide(DVec3::Y, Some(DVec3::X * 10.)).unwrap();
        doc.model.add_guide(DVec3::new(1., 2., 3.), None).unwrap();
        doc.model.validate().unwrap();
        doc.history.record("Add guides", before.clone(), &doc.model);
        let loaded = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(loaded.model, doc.model);
        let mut restored = loaded;
        assert!(restored.history.undo(&mut restored.model));
        assert_eq!(restored.model, before);
        assert!(restored.history.redo(&mut restored.model));
        assert_eq!(restored.model, doc.model);
        assert!(doc.model.vertices.is_empty());
        assert!(doc.model.edges.is_empty());
        assert!(doc.model.faces.is_empty());
        let mut history = History::default();
        history.record("No change", doc.model.clone(), &doc.model);
        assert!(history.undo_label().is_none());
    }

    #[test]
    fn old_documents_default_to_no_guides_and_invalid_guides_are_rejected() {
        let mut json = serde_json::to_value(Document::default()).unwrap();
        json["model"].as_object_mut().unwrap().remove("guides");
        let mut model = Document::from_json(&json.to_string()).unwrap().model;
        assert!(model.guides.is_empty());
        for (origin, direction) in [
            (DVec3::NAN, None),
            (DVec3::X * 1e11, None),
            (DVec3::ZERO, Some(DVec3::ZERO)),
            (DVec3::ZERO, Some(DVec3::NAN)),
        ] {
            assert!(model.add_guide(origin, direction).is_err());
            assert_eq!(model, Model::default());
        }
        let id = model.add_guide(DVec3::ZERO, Some(DVec3::X)).unwrap();
        model.guides.get_mut(&id).unwrap().direction = Some(DVec3::X * 2.);
        assert!(model.validate().is_err());
    }
}
