use crate::{Camera, DVec2, DVec3, Document, History, Model, angle_arc, angle_degrees, selection};
use std::collections::BTreeSet;

#[test]
fn protractor_measures_true_spatial_angles_and_handles_collinear_rays() {
    let center = DVec3::new(2., -3., 5.);
    for (target, expected) in [
        (DVec3::Y, 90.),
        (DVec3::Z, 90.),
        (DVec3::X + DVec3::Y, 45.),
        (-DVec3::X, 180.),
        (DVec3::X, 0.),
    ] {
        let angle = angle_degrees(center, center + DVec3::X * 3., center + target * 4.).unwrap();
        assert!((angle - expected).abs() < 1e-9);
        let arc = angle_arc(center, center + DVec3::X * 3., center + target * 4.);
        assert_eq!(arc.len(), 33);
        assert!(arc.iter().all(|p| p.is_finite()));
    }
    assert!(angle_degrees(center, center, center + DVec3::Y).is_none());
    assert!(angle_degrees(DVec3::NAN, DVec3::X, DVec3::Y).is_none());
}

#[test]
fn dimensions_follow_vertex_edge_and_face_attachments_after_geometry_edits() {
    let mut model = Model::default();
    let edge = model.add_edge(DVec3::ZERO, DVec3::X * 4.).unwrap();
    let endpoints = model
        .add_dimension(DVec3::ZERO, DVec3::X * 4., DVec3::Y)
        .unwrap();
    let midpoints = model
        .add_dimension(DVec3::X, DVec3::X * 3., DVec3::Y * 2.)
        .unwrap();
    assert_eq!(model.dimensions[&endpoints].start.weights.len(), 1);
    assert_eq!(model.dimensions[&midpoints].start.weights.len(), 2);
    model
        .scale_selection(&BTreeSet::from([edge]), DVec3::ZERO, 2.)
        .unwrap();
    assert!((model.dimensions[&endpoints].length(&model) - 8.).abs() < 1e-7);
    assert!((model.dimensions[&midpoints].length(&model) - 4.).abs() < 1e-7);
    let last = model.dimensions[&midpoints].points(&model);
    model.erase(&BTreeSet::from([edge]));
    assert_eq!(model.dimensions[&midpoints].points(&model), last);
    assert!(model.dimensions[&midpoints].start.weights.is_empty());
    model.validate().unwrap();
    assert!(model.bounds().is_some());

    let mut model = Model::default();
    let corners = [
        DVec3::ZERO,
        DVec3::X * 4.,
        DVec3::new(4., 4., 0.),
        DVec3::Y * 4.,
    ];
    for i in 0..4 {
        model.add_edge(corners[i], corners[(i + 1) % 4]).unwrap();
    }
    let dimension = model
        .add_dimension(DVec3::new(1., 1.5, 0.), DVec3::new(3., 2.5, 0.), DVec3::Y)
        .unwrap();
    assert_eq!(model.dimensions[&dimension].start.weights.len(), 3);
    let face = *model.faces.keys().next().unwrap();
    model
        .move_selection(&BTreeSet::from([face]), DVec3::Z * 3.)
        .unwrap();
    let (a, b) = model.dimensions[&dimension].points(&model);
    assert!((a - DVec3::new(1., 1.5, 3.)).length() < 1e-7);
    assert!((b - DVec3::new(3., 2.5, 3.)).length() < 1e-7);
    model.validate().unwrap();
}

#[test]
fn dimensions_roundtrip_undo_copy_group_move_and_scale_without_extra_geometry() {
    let mut doc = Document::default();
    let id = doc
        .model
        .add_dimension(DVec3::ZERO, DVec3::X * 4., DVec3::Y)
        .unwrap();
    assert!(doc.model.edges.is_empty());
    let saved = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(saved.model, doc.model);
    let before = doc.model.clone();
    let mut history = History::default();
    doc.model.erase(&BTreeSet::from([id]));
    history.record("Delete dimension", before.clone(), &doc.model);
    assert!(history.undo(&mut doc.model));
    assert_eq!(doc.model, before);
    let copy = doc.model.copy_entities(&BTreeSet::from([id]));
    let pasted = doc.model.paste_entities(&copy, DVec3::Z);
    assert_eq!(pasted.len(), 1);
    assert_eq!(doc.model.dimensions.len(), 2);
    let group = doc.model.make_object(&pasted, false, "Dimensions").unwrap();
    doc.model
        .move_selection(&BTreeSet::from([group]), DVec3::Y)
        .unwrap();
    doc.model
        .scale_selection(&BTreeSet::from([group]), DVec3::ZERO, 0.5)
        .unwrap();
    let child = *doc.model.objects[&group].entities.first().unwrap();
    assert!((doc.model.dimensions[&child].length(&doc.model) - 2.).abs() < 1e-7);
    doc.model.validate().unwrap();
}

#[test]
fn dimension_copy_detaches_missing_geometry_and_remaps_included_attachments() {
    let mut model = Model::default();
    let edge = model.add_edge(DVec3::ZERO, DVec3::X * 4.).unwrap();
    let dim = model
        .add_dimension(DVec3::ZERO, DVec3::X * 4., DVec3::Y)
        .unwrap();
    let annotation = model.copy_entities(&BTreeSet::from([dim]));
    assert!(annotation.dimensions[&dim].start.weights.is_empty());
    annotation.validate().unwrap();
    let copy = model.copy_entities(&BTreeSet::from([edge, dim]));
    let selected = model.paste_entities(&copy, DVec3::Z * 3.);
    let pasted_dim = *selected
        .iter()
        .find(|id| model.dimensions.contains_key(id))
        .unwrap();
    let pasted_edge = *selected
        .iter()
        .find(|id| model.edges.contains_key(id))
        .unwrap();
    model
        .scale_selection(&BTreeSet::from([pasted_edge]), DVec3::ZERO, 2.)
        .unwrap();
    assert!((model.dimensions[&pasted_dim].length(&model) - 8.).abs() < 1e-7);
    assert!((model.dimensions[&dim].length(&model) - 4.).abs() < 1e-7);
    model.validate().unwrap();
}

#[test]
fn dimension_labels_are_pickable_and_box_selectable_and_old_models_still_load() {
    let mut doc = Document::default();
    let id = doc
        .model
        .add_dimension(DVec3::ZERO, DVec3::X * 4., DVec3::Y * 2.)
        .unwrap();
    let size = DVec2::new(1000., 800.);
    let camera = Camera::default();
    let [_, _, a, b] = doc.model.dimensions[&id].layout(&doc.model);
    let cursor = camera.project((a + b) * 0.5, size).unwrap();
    assert_eq!(
        selection::pick_entity(&doc.model, &camera, size, cursor, 8.),
        Some(id)
    );
    assert!(selection::box_select(&doc.model, &camera, size, DVec2::ZERO, size).contains(&id));
    doc.model.hidden.insert(id);
    assert_eq!(
        selection::pick_entity(&doc.model, &camera, size, cursor, 8.),
        None
    );
    doc.model.hidden.clear();
    let mut json = serde_json::to_value(Document::default()).unwrap();
    json["model"].as_object_mut().unwrap().remove("dimensions");
    assert!(
        Document::from_json(&json.to_string())
            .unwrap()
            .model
            .dimensions
            .is_empty()
    );
}

#[test]
fn invalid_dimension_creation_is_atomic_and_bad_attachments_are_rejected() {
    let mut model = Model::default();
    let before = model.clone();
    for (a, b, offset) in [
        (DVec3::ZERO, DVec3::ZERO, DVec3::Y),
        (DVec3::NAN, DVec3::X, DVec3::Y),
        (DVec3::new(0., 1e10, 0.), DVec3::new(1., 1e10, 0.), DVec3::Y),
    ] {
        assert!(model.add_dimension(a, b, offset).is_err());
        assert_eq!(model, before);
    }
    let id = model
        .add_dimension(DVec3::ZERO, DVec3::X, DVec3::Y)
        .unwrap();
    model.dimensions.get_mut(&id).unwrap().start.weights = vec![(999, 1.)];
    assert!(model.validate().is_err());
}

#[test]
fn axis_dimensions_project_layout_and_measure_attached_endpoints() {
    use crate::Axis;
    let (a, b) = (DVec3::new(1., 2., 3.), DVec3::new(4., 6., 15.));
    for (axis, expected) in [(Axis::X, 3.), (Axis::Y, 4.), (Axis::Z, 12.)] {
        let mut model = Model::default();
        let edge = model.add_edge(a, b).unwrap();
        let id = model
            .add_dimension_with_axis(a, b, DVec3::new(2., 3., 4.), Some(axis))
            .unwrap();
        let dimension = &model.dimensions[&id];
        assert_eq!(dimension.points(&model), (a, b));
        assert_eq!(dimension.length(&model), expected);
        let [start, end, line_start, line_end] = dimension.layout(&model);
        assert!((line_end - line_start - axis.vector() * expected).length() < 1e-9);
        assert!((line_start - start).dot(axis.vector()).abs() < 1e-9);
        assert!((line_end - end).dot(axis.vector()).abs() < 1e-9);
        assert!(dimension.start.weights.len() == 1 && dimension.end.weights.len() == 1);
        model
            .scale_selection(&BTreeSet::from([edge]), a, 2.)
            .unwrap();
        assert_eq!(model.dimensions[&id].length(&model), expected * 2.);
        assert_eq!(model.dimensions[&id].axis, Some(axis));
        model.validate().unwrap();
    }
}

#[test]
fn axis_dimensions_preserve_alignment_in_history_clipboard_and_documents() {
    use crate::Axis;
    let mut doc = Document::default();
    let (a, b) = (DVec3::ZERO, DVec3::new(3., 4., 0.));
    doc.model.add_edge(a, b).unwrap();
    let before = doc.model.clone();
    let id = doc
        .model
        .add_dimension_with_axis(a, b, DVec3::Z, Some(Axis::Y))
        .unwrap();
    let mut history = History::default();
    history.record("Dimension", before.clone(), &doc.model);
    let saved = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(saved.model, doc.model);
    assert!(history.undo(&mut doc.model));
    assert_eq!(doc.model, before);
    assert!(history.redo(&mut doc.model));
    assert_eq!(doc.model.dimensions[&id].axis, Some(Axis::Y));
    let source = doc.model.copy_entities(&BTreeSet::from([id]));
    let copy = *doc
        .model
        .paste_entities(&source, DVec3::X * 6.)
        .first()
        .unwrap();
    assert_eq!(doc.model.dimensions[&copy].axis, Some(Axis::Y));
    assert_eq!(doc.model.dimensions[&copy].length(&doc.model), 4.);
    doc.model.validate().unwrap();
    let mut legacy = serde_json::to_value(&saved).unwrap();
    legacy["model"]["dimensions"][id.to_string()]
        .as_object_mut()
        .unwrap()
        .remove("axis");
    let legacy = Document::from_json(&legacy.to_string()).unwrap();
    assert_eq!(legacy.model.dimensions[&id].axis, None);
    assert_eq!(legacy.model.dimensions[&id].length(&legacy.model), 5.);
}
