//! Construction references, deliberately separate from edges and faces.
use crate::{DVec3, Model};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConstructionGuide {
    pub origin: DVec3,
    /// A unit direction for an infinite line; None denotes a guide point.
    pub direction: Option<DVec3>,
}

impl ConstructionGuide {
    pub fn valid(&self) -> bool {
        self.origin.is_finite()
            && self.origin.abs().max_element() <= 1e10
            && self
                .direction
                .is_none_or(|d| d.is_finite() && (d.length() - 1.).abs() < 1e-7)
    }
}

impl Model {
    pub fn add_guide(
        &mut self,
        origin: DVec3,
        direction: Option<DVec3>,
    ) -> Result<u64, &'static str> {
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
