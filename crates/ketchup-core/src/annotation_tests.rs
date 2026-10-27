use crate::{Camera, DVec2, DVec3, Document, History, Model, modeling::MovePreview, selection};
use std::collections::BTreeSet;

fn rectangle(model: &mut Model) {
    let corners = [
        DVec3::ZERO,
        DVec3::X * 4.,
        DVec3::new(4., 4., 0.),
        DVec3::Y * 4.,
    ];
    for i in 0..4 {
        model.add_edge(corners[i], corners[(i + 1) % 4]).unwrap();
    }
}

#[test]
fn text_tracks_vertex_edge_and_face_attachments_and_survives_removed_geometry() {
    let mut model = Model::default();
    rectangle(&mut model);
    let ids: Vec<_> = [DVec3::ZERO, DVec3::X * 2., DVec3::new(1., 1.5, 0.)]
        .into_iter()
        .map(|point| model.add_text(point, -DVec3::Y, "Attached\ntext").unwrap())
        .collect();
    for (id, count) in ids.iter().zip([1, 2, 3]) {
        assert_eq!(model.texts[id].anchor.weights.len(), count);
    }
    let geometry = model.edges.keys().copied().collect();
    model.move_selection(&geometry, DVec3::Z * 3.).unwrap();
    for id in &ids {
        assert!((model.texts[id].layout(&model)[0].z - 3.).abs() < 1e-7);
    }
    let positions: Vec<_> = ids
        .iter()
        .map(|id| model.texts[id].layout(&model))
        .collect();
    model.erase(&geometry);
    for (id, position) in ids.iter().zip(positions) {
        assert_eq!(model.texts[id].layout(&model), position);
        assert!(model.texts[id].anchor.weights.is_empty());
    }
    model.validate().unwrap();
}

#[test]
fn text_roundtrips_with_history_and_old_documents_default_to_no_text() {
    let mut doc = Document::default();
    let id = doc
        .model
        .add_text(DVec3::ZERO, DVec3::Y, "Roof – métal\nSecond line")
        .unwrap();
    let json = serde_json::to_string(&doc).unwrap();
    assert_eq!(Document::from_json(&json).unwrap().model, doc.model);
    let before = doc.model.clone();
    doc.model.edit_text(id, "Updated").unwrap();
    let mut history = History::default();
    history.record("Edit text", before.clone(), &doc.model);
    assert!(history.undo(&mut doc.model));
    assert_eq!(doc.model, before);
    assert!(history.redo(&mut doc.model));
    assert_eq!(doc.model.texts[&id].text, "Updated");
    let mut legacy: serde_json::Value = serde_json::from_str(&json).unwrap();
    legacy["model"].as_object_mut().unwrap().remove("texts");
    assert!(
        Document::from_json(&legacy.to_string())
            .unwrap()
            .model
            .texts
            .is_empty()
    );
}

#[test]
fn copied_text_remaps_included_geometry_and_detaches_when_copied_alone() {
    let mut model = Model::default();
    let edge = model.add_edge(DVec3::ZERO, DVec3::X * 4.).unwrap();
    let text = model.add_text(DVec3::X * 2., DVec3::Y, "Beam").unwrap();
    let alone = model.copy_entities(&BTreeSet::from([text]));
    assert!(alone.texts[&text].anchor.weights.is_empty());
    assert!(alone.edges.is_empty());
    alone.validate().unwrap();
    let copy = model.copy_entities(&BTreeSet::from([edge, text]));
    let pasted = model.paste_entities(&copy, DVec3::Z * 3.);
    let pasted_text = *pasted
        .iter()
        .find(|id| model.texts.contains_key(id))
        .unwrap();
    let pasted_edge = *pasted
        .iter()
        .find(|id| model.edges.contains_key(id))
        .unwrap();
    model
        .move_selection(&BTreeSet::from([pasted_edge]), DVec3::Y * 2.)
        .unwrap();
    assert_eq!(
        model.texts[&pasted_text].layout(&model)[0],
        DVec3::new(2., 2., 3.)
    );
    assert_eq!(model.texts[&text].layout(&model)[0], DVec3::X * 2.);
    let group = model.make_object(&pasted, false, "Annotated beam").unwrap();
    model
        .scale_selection(&BTreeSet::from([group]), DVec3::ZERO, 2.)
        .unwrap();
    let contents = model.object_contents(group).unwrap();
    let label = contents.texts.values().next().unwrap();
    assert_eq!(label.layout(&contents)[0], DVec3::new(4., 4., 6.));
    assert_eq!(label.offset, DVec3::Y * 2.);
    assert_eq!(label.anchor.weights.len(), 2);
    model.validate().unwrap();
    // Text alone is a valid object, including its bounds and clipboard state.
    let group = model
        .make_object(&BTreeSet::from([text]), false, "Notes")
        .unwrap();
    assert!(model.object_contents(group).unwrap().bounds().is_some());
    model.validate().unwrap();
}

#[test]
fn moving_text_repositions_the_label_without_detaching_and_preview_matches_commit() {
    let mut model = Model::default();
    let edge = model.add_edge(DVec3::ZERO, DVec3::X * 4.).unwrap();
    let text = model.add_text(DVec3::X * 2., DVec3::Y, "Beam").unwrap();
    for selection in [
        BTreeSet::from([text]),
        BTreeSet::from([edge, text]),
        BTreeSet::from([edge]),
    ] {
        let before = model.clone();
        let preview = MovePreview::new(&before, &selection).unwrap();
        let mut next = before.clone();
        preview.update(&mut next, DVec3::Z).unwrap();
        // Reused previews must be absolute rather than accumulate displacement.
        preview.update(&mut next, DVec3::Z * 3.).unwrap();
        model.move_selection(&selection, DVec3::Z * 3.).unwrap();
        assert_eq!(next, model);
        assert_eq!(model.texts[&text].anchor.weights.len(), 2);
    }
}

#[test]
fn scoped_text_is_shared_across_component_copies_and_tracks_their_geometry() {
    let mut model = Model::default();
    rectangle(&mut model);
    let group = model
        .make_object(&model.root_entities(), true, "Panel")
        .unwrap();
    let copy = model.copy_entities(&BTreeSet::from([group]));
    let peer = *model.paste_entities(&copy, DVec3::X * 6.).first().unwrap();
    model
        .scale_selection(&BTreeSet::from([peer]), DVec3::X * 6., 2.)
        .unwrap();
    assert!(model.enter_object(group));
    let text = model
        .add_text(DVec3::X * 2., -DVec3::Y, "Panel label")
        .unwrap();
    assert_eq!(model.parent(text), Some(group));
    model.edit_text(text, "Shared text").unwrap();
    let peer_contents = model.object_contents(peer).unwrap();
    let peer_text = peer_contents.texts.values().next().unwrap();
    assert_eq!(peer_text.text, "Shared text");
    assert_eq!(peer_text.layout(&peer_contents)[0], DVec3::X * 10.);
    assert_eq!(peer_text.offset, -DVec3::Y * 2.);
    assert_eq!(peer_text.anchor.weights.len(), 2);
    model.validate().unwrap();
    model.erase(&BTreeSet::from([text]));
    assert!(model.object_contents(peer).unwrap().texts.is_empty());
}

#[test]
fn text_is_pickable_on_the_label_and_leader_and_respects_visibility_and_scope() {
    let mut model = Model::default();
    let id = model
        .add_text(DVec3::ZERO, DVec3::Y * 2., "Leader label")
        .unwrap();
    let camera = Camera::default();
    let size = DVec2::new(1000., 800.);
    for point in [DVec3::Y, DVec3::Y * 2.] {
        let pixel = camera.project(point, size).unwrap();
        assert_eq!(
            selection::pick_entity(&model, &camera, size, pixel, 8.),
            Some(id)
        );
    }
    assert!(selection::box_select(&model, &camera, size, DVec2::ZERO, size).contains(&id));
    let tag = model.add_tag("Notes").unwrap();
    model.entity_tags.insert(id, tag);
    model.tags.get_mut(&tag).unwrap().visible = false;
    let pixel = camera.project(DVec3::Y * 2., size).unwrap();
    assert_eq!(
        selection::pick_entity(&model, &camera, size, pixel, 8.),
        None
    );
    model.tags.get_mut(&tag).unwrap().visible = true;
    let group = model
        .make_object(&BTreeSet::from([id]), false, "Notes")
        .unwrap();
    assert_eq!(
        selection::pick_entity(&model, &camera, size, pixel, 8.),
        Some(group)
    );
    model.hidden.insert(group);
    assert!(model.bounds().is_none());
    assert_eq!(
        selection::pick_entity(&model, &camera, size, pixel, 8.),
        None
    );
}

#[test]
fn invalid_text_is_atomic_and_corrupt_attachments_or_duplicate_ids_are_rejected() {
    let mut model = Model::default();
    let before = model.clone();
    for (point, offset, text) in [
        (DVec3::NAN, DVec3::ZERO, "Note"),
        (DVec3::ZERO, DVec3::INFINITY, "Note"),
        (DVec3::splat(1e10), DVec3::ONE, "Note"),
        (DVec3::ZERO, DVec3::Y, " \n "),
    ] {
        assert!(model.add_text(point, offset, text).is_err());
        assert_eq!(model, before);
    }
    assert!(
        model
            .add_text(DVec3::ZERO, DVec3::Y, &"a".repeat(4097))
            .is_err()
    );
    assert_eq!(model, before);
    let edge = model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    let text = model.add_text(DVec3::ZERO, DVec3::Y, "Note").unwrap();
    model.locked.insert(text);
    let valid = model.clone();
    assert!(model.edit_text(text, "Changed").is_err());
    assert_eq!(model, valid);
    model.texts.get_mut(&text).unwrap().anchor.weights = vec![(9999, 1.)];
    assert!(model.validate().is_err());
    model = valid;
    let mut duplicate = model.texts.remove(&text).unwrap();
    duplicate.id = edge;
    model.texts.insert(edge, duplicate);
    assert!(model.validate().is_err());
}
