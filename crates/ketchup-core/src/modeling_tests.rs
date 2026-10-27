use crate::{DVec3, Document, History, Model};
use std::collections::BTreeSet;

fn rectangle(model: &mut Model, min: DVec3, max: DVec3) -> u64 {
    let points = [
        min,
        DVec3::new(max.x, min.y, min.z),
        max,
        DVec3::new(min.x, max.y, min.z),
    ];
    for i in 0..4 {
        model.add_edge(points[i], points[(i + 1) % 4]).unwrap();
    }
    *model.faces.keys().next_back().unwrap()
}

#[test]
fn push_pull_creates_closed_caps_and_walls_with_color_and_history() {
    for distance in [2., -2.] {
        let mut model = Model::default();
        let face = rectangle(&mut model, DVec3::ZERO, DVec3::new(4., 3., 0.));
        model.paint_face(face, [182, 53, 81]).unwrap();
        let before = model.clone();
        let cap = model.push_pull(face, distance).unwrap();
        assert_eq!(
            (model.vertices.len(), model.edges.len(), model.faces.len()),
            (8, 12, 6)
        );
        assert!((model.face_area(&model.faces[&cap]) - 12.).abs() < 1e-7);
        assert!(
            model.faces[&cap]
                .vertices
                .iter()
                .all(|id| (model.vertices[id].position.z.abs() - 2.).abs() < 1e-7)
        );
        assert_eq!(model.face_colors.len(), 6);
        model.validate().unwrap();
        let mut history = History::default();
        history.record("Push/Pull", before.clone(), &model);
        let after = model.clone();
        assert!(history.undo(&mut model));
        assert_eq!(model, before);
        assert!(history.redo(&mut model));
        assert_eq!(model, after);
    }
}

#[test]
fn extrusion_preserves_real_holes_and_their_walls() {
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO, DVec3::new(6., 6., 0.));
    let inner = rectangle(&mut model, DVec3::new(2., 2., 0.), DVec3::new(4., 4., 0.));
    model.erase(&BTreeSet::from([inner]));
    let cap = model.push_pull(face, 3.).unwrap();
    assert_eq!(model.faces[&cap].holes.len(), 1);
    assert_eq!(model.faces.len(), 10);
    assert!((model.face_area(&model.faces[&cap]) - 32.).abs() < 1e-7);
    model.validate().unwrap();
}

#[test]
fn move_scale_share_vertices_and_reject_invalid_geometry_atomically() {
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO, DVec3::new(4., 2., 0.));
    let selected = BTreeSet::from([face]);
    let center = model.selection_center(&selected).unwrap();
    model.scale_selection(&selected, center, 2.).unwrap();
    assert!((model.face_area(&model.faces[&face]) - 32.).abs() < 1e-7);
    model
        .move_selection(&selected, DVec3::new(0., 0., 3.))
        .unwrap();
    assert!(
        model
            .vertices
            .values()
            .all(|v| (v.position.z - 3.).abs() < 1e-7)
    );
    let before = model.clone();
    let edge = *model.edges.keys().next().unwrap();
    assert!(
        model
            .move_selection(&BTreeSet::from([edge]), DVec3::Z)
            .is_ok()
    ); // tilts a rectangle into a plane
    model = before.clone();
    assert!(model.scale_selection(&selected, center, 0.).is_err());
    assert_eq!(model, before);
    assert!(
        model
            .move_selection(&selected, DVec3::splat(f64::NAN))
            .is_err()
    );
    assert_eq!(model, before);
}

#[test]
fn inset_outset_and_overlarge_offsets() {
    for (distance, area) in [(0.5, 3.), (-0.5, 15.)] {
        let mut model = Model::default();
        let face = rectangle(&mut model, DVec3::ZERO, DVec3::new(4., 2., 0.));
        model.paint_face(face, [31, 66, 99]).unwrap();
        model.offset_face(face, distance).unwrap();
        assert_eq!(model.edges.len(), 8);
        if distance > 0. {
            assert!(
                model
                    .faces
                    .values()
                    .any(|f| (model.face_area(f) - area).abs() < 1e-7)
            );
        } else {
            assert!(
                (model
                    .faces
                    .values()
                    .map(|f| model.face_area(f))
                    .sum::<f64>()
                    - area)
                    .abs()
                    < 1e-7
            );
        }
        model.validate().unwrap();
    }
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO, DVec3::new(4., 2., 0.));
    let before = model.clone();
    for distance in [1., 2., f64::NAN] {
        assert!(model.offset_face(face, distance).is_err());
        assert_eq!(model, before);
    }
}

#[test]
fn offset_with_holes_and_tilted_faces_remains_planar() {
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO, DVec3::new(8., 8., 0.));
    let inner = rectangle(&mut model, DVec3::new(3., 3., 0.), DVec3::new(5., 5., 0.));
    model.erase(&BTreeSet::from([inner]));
    // Rigidly tilt every vertex before applying the offset.
    for v in model.vertices.values_mut() {
        v.position = glam::DQuat::from_rotation_x(0.8) * v.position;
    }
    model.offset_face(face, 0.5).unwrap();
    assert_eq!(model.edges.len(), 16);
    model.validate().unwrap();
}

#[test]
fn paint_survives_serialization_clipboard_and_grouping_and_respects_locks() {
    let mut doc = Document::default();
    let face = rectangle(&mut doc.model, DVec3::ZERO, DVec3::new(4., 2., 0.));
    let color = [40, 90, 160];
    doc.model.paint_face(face, color).unwrap();
    let json = serde_json::to_string(&doc).unwrap();
    assert_eq!(Document::from_json(&json).unwrap().model, doc.model);
    let group = doc
        .model
        .make_object(&BTreeSet::from([face]), false, "Painted")
        .unwrap();
    doc.model.validate().unwrap();
    let selection = BTreeSet::from([group]);
    let center = doc.model.selection_center(&selection).unwrap();
    doc.model.scale_selection(&selection, center, 0.5).unwrap();
    doc.model.move_selection(&selection, DVec3::Z).unwrap();
    let copy = doc.model.copy_entities(&selection);
    copy.validate().unwrap();
    assert_eq!(
        copy.face_colors.values().copied().collect::<Vec<_>>(),
        vec![color]
    );
    let pasted = doc.model.paste_entities(&copy, DVec3::X * 6.);
    assert!(!pasted.is_empty());
    doc.model.validate().unwrap();
    assert_eq!(doc.model.face_colors.len(), 2);
    doc.model.locked.insert(group);
    let before = doc.model.clone();
    assert!(doc.model.move_selection(&selection, DVec3::X).is_err());
    assert!(doc.model.scale_selection(&selection, center, 2.).is_err());
    let child_face = *copy.faces.keys().next().unwrap();
    assert!(doc.model.paint_face(child_face, [0, 0, 0]).is_err());
    assert_eq!(doc.model, before);
}

#[test]
fn painting_a_group_includes_nested_faces_and_rejects_locks_atomically() {
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO, DVec3::new(4., 2., 0.));
    model.push_pull(face, 2.).unwrap();
    model
        .make_object(&model.selectable_entities(), false, "Inner")
        .unwrap();
    let loose = rectangle(&mut model, DVec3::X * 6., DVec3::new(8., 2., 0.));
    let group = model
        .make_object(&model.selectable_entities(), false, "Outer")
        .unwrap();
    let outside = rectangle(&mut model, DVec3::X * 10., DVec3::new(12., 2., 0.));
    let inside: Vec<_> = model
        .expand_entities(&model.objects[&group].entities)
        .into_iter()
        .filter(|id| model.faces.contains_key(id))
        .collect();
    assert_eq!(inside.len(), 7);
    assert!(!model.faces.contains_key(&loose)); // Grouping remaps source IDs.
    let color = [40, 90, 160];
    model.paint_object(group, color).unwrap();
    assert!(
        inside
            .iter()
            .all(|id| model.face_colors.get(id) == Some(&color))
    );
    assert!(!model.face_colors.contains_key(&outside));
    let saved = Document::from_json(
        &serde_json::to_string(&Document {
            model: model.clone(),
            ..Document::default()
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(saved.model, model);
    for locked in [group, *inside.last().unwrap()] {
        model.locked.insert(locked);
        let before = model.clone();
        assert!(model.paint_object(group, [1, 2, 3]).is_err());
        assert_eq!(model, before);
        model.locked.remove(&locked);
    }
    let before = model.clone();
    assert!(model.paint_object(outside, color).is_err());
    assert_eq!(model, before);
    model.validate().unwrap();
}

#[test]
fn painting_a_nested_group_in_component_context_updates_instances_only_in_that_group() {
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO, DVec3::new(4., 2., 0.));
    model.push_pull(face, 2.).unwrap();
    model
        .make_object(&model.selectable_entities(), false, "Nested")
        .unwrap();
    rectangle(&mut model, DVec3::X * 6., DVec3::new(8., 2., 0.));
    let component = model
        .make_object(&model.selectable_entities(), true, "Component")
        .unwrap();
    let copy = model.copy_entities(&BTreeSet::from([component]));
    let peer = *model.paste_entities(&copy, DVec3::X * 12.).first().unwrap();
    assert!(model.enter_object(component));
    let group = *model.objects[&component]
        .entities
        .iter()
        .find(|id| model.objects.contains_key(id))
        .unwrap();
    let color = [40, 90, 160];
    model.paint_object(group, color).unwrap();
    assert_eq!(model.edit_context, Some(component));
    for id in [component, peer] {
        let contents = model.object_contents(id).unwrap();
        assert_eq!(contents.faces.len(), 7);
        assert_eq!(contents.face_colors.len(), 6);
        assert!(contents.face_colors.values().all(|rgb| *rgb == color));
    }
    model.edit_context = None;
    model.paint_object(component, [1, 2, 3]).unwrap();
    for id in [component, peer] {
        let contents = model.object_contents(id).unwrap();
        assert_eq!(contents.face_colors.len(), 7);
        assert!(contents.face_colors.values().all(|rgb| *rgb == [1, 2, 3]));
    }
    model.validate().unwrap();
}

#[test]
fn pulling_an_existing_cap_stretches_walls_without_internal_faces() {
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO, DVec3::new(4., 2., 0.));
    let cap = model.push_pull(face, 2.).unwrap();
    let normal = crate::faces::plane(&model, &model.faces[&cap].vertices)
        .unwrap()
        .normal;
    let old = model.vertices[&model.faces[&cap].vertices[0]].position;
    assert_eq!(model.push_pull(cap, 1.).unwrap(), cap);
    assert_eq!(
        (model.vertices.len(), model.edges.len(), model.faces.len()),
        (8, 12, 6)
    );
    let point = model.vertices[&model.faces[&cap].vertices[0]].position;
    assert!((point - old - normal).length() < 1e-7);
    let before = model.clone();
    assert!(model.push_pull(cap, -3.).is_err());
    assert_eq!(model, before);
    model.validate().unwrap();
}

#[test]
fn pulling_an_offset_inner_face_opens_the_surface_and_preserves_the_rim() {
    for angle in [0., 0.8] {
        for depth in [-2., 2.] {
            let rotation = glam::DQuat::from_rotation_x(angle);
            let axis = rotation * DVec3::Z;
            let mut model = Model::default();
            let rim = rectangle(&mut model, DVec3::ZERO, DVec3::new(6., 6., 0.));
            model.paint_face(rim, [31, 66, 99]).unwrap();
            for vertex in model.vertices.values_mut() {
                vertex.position = rotation * vertex.position;
            }
            model.offset_face(rim, 1.).unwrap();
            let center = rotation * DVec3::new(3., 3., 0.);
            let inner = model
                .faces
                .values()
                .find(|face| model.face_contains(face, center, false))
                .unwrap()
                .id;
            let before = model.clone();
            let normal = crate::faces::plane(&model, &model.faces[&inner].vertices)
                .unwrap()
                .normal;
            let cap = model.push_pull(inner, depth * normal.dot(axis)).unwrap();

            assert!(!model.faces.contains_key(&inner));
            assert_eq!(model.faces[&rim], before.faces[&rim]);
            for id in model.faces[&rim]
                .vertices
                .iter()
                .chain(model.faces[&rim].holes.iter().flatten())
            {
                assert_eq!(model.vertices[id], before.vertices[id]);
            }
            assert_eq!(
                (model.vertices.len(), model.edges.len(), model.faces.len()),
                (12, 16, 6)
            );
            assert!((model.face_area(&model.faces[&cap]) - 16.).abs() < 1e-7);
            assert_eq!(model.face_colors.len(), 6);
            assert!(model.face_colors.values().all(|c| *c == [31, 66, 99]));
            let (hit, _, point) = model
                .nearest_face(crate::Ray {
                    origin: center + axis * 10.,
                    direction: -axis,
                })
                .unwrap();
            assert_eq!(hit, cap, "The original face must not cover the moved floor");
            assert!(point.distance(center + axis * depth) < 1e-7);
            model.validate().unwrap();

            let mut history = History::default();
            history.record("Push/Pull", before.clone(), &model);
            let after = model.clone();
            assert!(history.undo(&mut model));
            assert_eq!(model, before);
            assert!(history.redo(&mut model));
            assert_eq!(model, after);
            let doc = Document {
                model: model.clone(),
                ..Document::default()
            };
            assert_eq!(
                Document::from_json(&serde_json::to_string(&doc).unwrap())
                    .unwrap()
                    .model,
                model
            );

            assert_eq!(model.push_pull(cap, normal.dot(axis) * depth).unwrap(), cap);
            assert_eq!(model.faces.len(), 6);
            assert!(model.face_contains(&model.faces[&cap], center + axis * depth * 2., false));
            model.validate().unwrap();
        }
    }
}

#[test]
fn pushing_an_inset_into_a_solid_creates_a_closed_recess() {
    let mut model = Model::default();
    let base = rectangle(&mut model, DVec3::ZERO, DVec3::new(8., 6., 0.));
    let normal = crate::faces::plane(&model, &model.faces[&base].vertices)
        .unwrap()
        .normal;
    let top = model.push_pull(base, 4. / normal.z).unwrap();
    model.offset_face(top, 1.).unwrap();
    let inner = model
        .faces
        .values()
        .find(|face| model.face_contains(face, DVec3::new(4., 3., 4.), false))
        .unwrap()
        .id;
    let normal = crate::faces::plane(&model, &model.faces[&inner].vertices)
        .unwrap()
        .normal;
    let floor = model.push_pull(inner, -1.5 / normal.z).unwrap();

    assert!(!model.faces.contains_key(&inner));
    assert_eq!(model.faces.len(), 11);
    assert_eq!(model.faces[&top].holes.len(), 1);
    let (hit, _, point) = model
        .nearest_face(crate::Ray {
            origin: DVec3::new(4., 3., 10.),
            direction: -DVec3::Z,
        })
        .unwrap();
    assert_eq!(hit, floor);
    assert!(point.distance(DVec3::new(4., 3., 2.5)) < 1e-7);
    assert!((model.solid_volume(base).unwrap() - (8. * 6. * 4. - 6. * 4. * 1.5)).abs() < 1e-6);
    model.validate().unwrap();
}

#[test]
fn transforms_cannot_change_vertices_of_a_locked_neighbor() {
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO, DVec3::new(4., 2., 0.));
    let edge = *model.edges.keys().next().unwrap();
    model.locked.insert(face);
    let before = model.clone();
    assert!(
        model
            .move_selection(&BTreeSet::from([edge]), DVec3::X)
            .is_err()
    );
    assert!(
        model
            .scale_selection(&BTreeSet::from([edge]), DVec3::ZERO, 2.)
            .is_err()
    );
    assert_eq!(model, before);
}

#[test]
fn move_preview_reuses_source_positions_and_matches_validated_commits() {
    use crate::modeling::MovePreview;
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO, DVec3::new(4., 3., 0.));
    model
        .add_dimension(DVec3::ZERO, DVec3::X * 4., DVec3::Y)
        .unwrap();
    let group = model
        .make_object(&model.root_entities(), true, "Moving component")
        .unwrap();
    let selected = BTreeSet::from([group]);
    let plan = MovePreview::new(&model, &selected).unwrap();
    let mut preview = model.clone();
    for delta in [
        DVec3::X,
        DVec3::new(-2., 3., 5.),
        DVec3::ZERO,
        DVec3::X * 4.,
    ] {
        plan.update(&mut preview, delta).unwrap();
        let mut committed = model.clone();
        committed.move_selection(&selected, delta).unwrap();
        assert_eq!(preview, committed);
    }
    assert!(!model.faces.contains_key(&face)); // grouping replaces the source IDs
    let before = model.clone();
    assert!(plan.update(&mut preview, DVec3::splat(1e11)).is_err());
    assert_eq!(model, before);
}

#[test]
fn move_preview_preserves_connected_face_constraints_and_locks() {
    use crate::modeling::MovePreview;
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO, DVec3::new(4., 3., 0.));
    let edge = *model.edges.keys().next().unwrap();
    let selected = BTreeSet::from([edge]);
    let plan = MovePreview::new(&model, &selected).unwrap();
    for delta in [DVec3::Y * 0.25, DVec3::Z, DVec3::Y * 3.] {
        let mut preview = model.clone();
        let mut committed = model.clone();
        assert_eq!(
            plan.update(&mut preview, delta).is_ok(),
            committed.move_selection(&selected, delta).is_ok()
        );
    }
    model.locked.insert(face);
    assert!(MovePreview::new(&model, &selected).is_err());
}
