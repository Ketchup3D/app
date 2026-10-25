use crate::*;
use std::collections::BTreeSet;

fn rectangle(model: &mut Model, origin: DVec3) {
    let points = [
        DVec3::ZERO,
        DVec3::X * 2.,
        DVec3::new(2., 2., 0.),
        DVec3::Y * 2.,
    ];
    for i in 0..4 {
        model
            .add_edge(origin + points[i], origin + points[(i + 1) % 4])
            .unwrap();
    }
}

#[test]
fn drawing_snapshot_preserves_group_and_surface_protection() {
    let mut base = Model::default();
    rectangle(&mut base, DVec3::ZERO);
    let group = base
        .make_object(&base.selectable_entities(), false, "Inner")
        .unwrap();
    let outer = base
        .make_object(&BTreeSet::from([group]), false, "Outer")
        .unwrap();
    rectangle(&mut base, DVec3::X * 5.);
    let face = *base.faces.keys().last().unwrap();
    let edge = *base.edges.keys().last().unwrap();
    let isolated = base.add_edge(DVec3::X * 10., DVec3::X * 11.).unwrap();
    for state in 0..9 {
        let mut model = base.clone();
        match state {
            1 => {
                model.hidden.insert(face);
            }
            2 => {
                model.locked.insert(face);
            }
            3 => {
                model.hidden.insert(edge);
            }
            4 => {
                model.locked.insert(edge);
            }
            5 => {
                model.hidden.insert(outer);
            }
            6 => {
                model.locked.insert(outer);
            }
            7 | 8 => {
                let tag = model.add_tag("Hidden layer").unwrap();
                model.tags.get_mut(&tag).unwrap().visible = false;
                model
                    .entity_tags
                    .insert(if state == 7 { face } else { outer }, tag);
            }
            _ => {}
        }
        let expected: BTreeSet<_> = model
            .edges
            .keys()
            .copied()
            .filter(|id| model.drawing_edge(*id))
            .collect();
        let drawing = model.drawing_edges();
        assert_eq!(drawing, expected, "state {state}");
        assert!(drawing.contains(&isolated));
        if matches!(state, 1 | 2 | 7) {
            assert!(
                !drawing.contains(&edge),
                "Protected faces protect their outlines"
            );
        }
    }
}

#[test]
fn drawing_on_a_hidden_grouped_import_preserves_geometry_and_undo() {
    // Construct import-like topology directly, keeping fixture setup out of the
    // interactive line path. Hidden nested groups triggered repeated scans of
    // every face and boundary for each loose edge.
    let mut model = Model::default();
    let mut groups = BTreeSet::new();
    for i in 0..128 {
        let origin = DVec3::new((i % 16) as f64 * 3., (i / 16) as f64 * 3., 0.);
        let mut vertices = Vec::new();
        for point in [DVec3::ZERO, DVec3::X, DVec3::X + DVec3::Y, DVec3::Y] {
            let id = model.id();
            model.vertices.insert(
                id,
                Vertex {
                    id,
                    position: origin + point,
                },
            );
            vertices.push(id);
        }
        let mut entities = BTreeSet::new();
        for j in 0..4 {
            let id = model.id();
            model.edges.insert(
                id,
                Edge {
                    id,
                    a: vertices[j],
                    b: vertices[(j + 1) % 4],
                },
            );
            entities.insert(id);
        }
        let face = model.id();
        model.faces.insert(
            face,
            Face {
                id: face,
                vertices,
                holes: vec![],
            },
        );
        entities.insert(face);
        let id = model.id();
        model.objects.insert(
            id,
            Object {
                id,
                name: "Imported part".into(),
                outliner_order: None,
                kind: ObjectKind::Group,
                entities,
                transform: None,
                definition_entities: Default::default(),
            },
        );
        groups.insert(id);
        if i % 2 == 0 {
            model.hidden.insert(id);
        }
    }
    let id = model.id();
    model.objects.insert(
        id,
        Object {
            id,
            name: "Import".into(),
            outliner_order: None,
            kind: ObjectKind::Group,
            entities: groups,
            transform: None,
            definition_entities: Default::default(),
        },
    );
    let before = model.clone();
    let origin = DVec3::new(-4., -4., 0.);
    rectangle(&mut model, origin);
    // Both endpoints split the same boundary. Face discovery must use the
    // replacement edge IDs, rather than the snapshot's removed original edge.
    model
        .add_edge(origin + DVec3::X * 0.5, origin + DVec3::X * 1.5)
        .unwrap();
    assert_eq!(model.edges.len(), before.edges.len() + 6);
    assert_eq!(model.faces.len(), before.faces.len() + 1);
    model
        .add_edge(origin + DVec3::Y, origin + DVec3::X * 2. + DVec3::Y)
        .unwrap();
    assert_eq!(model.faces.len(), before.faces.len() + 2);
    for (id, object) in &before.objects {
        assert_eq!(&model.objects[id], object);
    }
    for (id, face) in &before.faces {
        assert_eq!(&model.faces[id], face);
    }
    assert_eq!(model.hidden, before.hidden);
    model.validate().unwrap();
    let after = model.clone();
    let mut history = History::default();
    history.record("Draw on import", before.clone(), &model);
    assert!(history.undo(&mut model));
    assert_eq!(model, before);
    assert!(history.redo(&mut model));
    assert_eq!(model, after);
}

#[test]
fn groups_isolate_geometry_and_dissolve_one_level() {
    let mut model = Model::default();
    rectangle(&mut model, DVec3::ZERO);
    let original = model.clone();
    let group = model
        .make_object(&model.selectable_entities(), false, "Box")
        .unwrap();
    assert_eq!(model.selectable_entities(), BTreeSet::from([group]));
    let inner = model.objects[&group].entities.clone();
    model.add_edge(DVec3::X, DVec3::X + DVec3::Z).unwrap();
    assert_eq!(model.objects[&group].entities, inner);
    assert_eq!(model.faces.values().next().unwrap().vertices.len(), 4);
    model.validate().unwrap();
    let outer = model
        .make_object(&BTreeSet::from([group]), false, "Outer")
        .unwrap();
    let released = model.dissolve_objects(&BTreeSet::from([outer]), true);
    assert_eq!(released.len(), 1);
    assert!(model.objects.contains_key(released.first().unwrap()));
    let geometry = model.dissolve_objects(&released, false);
    assert_eq!(geometry.len(), original.edges.len() + original.faces.len());
    model.validate().unwrap();
}

#[test]
fn grouping_a_face_keeps_its_outline_and_unselected_neighbor() {
    let mut model = Model::default();
    rectangle(&mut model, DVec3::ZERO);
    model.add_edge(DVec3::X, DVec3::new(1., 2., 0.)).unwrap();
    assert_eq!(model.faces.len(), 2);
    let face = *model.faces.keys().next().unwrap();
    let group = model
        .make_object(&BTreeSet::from([face]), false, "Half")
        .unwrap();
    assert_eq!(model.faces.len(), 2);
    assert_eq!(model.objects[&group].entities.len(), 5);
    model.erase(&BTreeSet::from([group]));
    assert_eq!(model.faces.len(), 1);
    model.validate().unwrap();
}

#[test]
fn clipboard_preserves_faces_holes_missing_fills_colors_and_component_identity() {
    let mut model = Model::default();
    rectangle(&mut model, DVec3::ZERO);
    let face = *model.faces.keys().next().unwrap();
    model.face_colors.insert(face, [12, 34, 56]);
    let component = model
        .make_object(&model.selectable_entities(), true, "Tile")
        .unwrap();
    let kind = model.objects[&component].kind.clone();
    let copy = model.copy_entities(&BTreeSet::from([component]));
    let pasted = model.paste_entities(&copy, DVec3::new(5., 0., 0.));
    assert_eq!(model.objects[pasted.first().unwrap()].kind, kind);
    assert_eq!(model.face_colors.len(), 2);
    model.validate().unwrap();
    // Clipboard survives New Model and paste-in-place creates separate entity IDs.
    let mut fresh = Model::default();
    fresh.paste_entities(&copy, DVec3::ZERO);
    fresh.validate().unwrap();
    fresh.paste_entities(&copy, DVec3::ZERO);
    fresh.validate().unwrap();
    assert_eq!(fresh.faces.len(), 2);
    assert_eq!(fresh.face_colors.len(), 2);
    // An edge-only copy must never regenerate a deliberately deleted fill.
    let mut outline = Model::default();
    rectangle(&mut outline, DVec3::ZERO);
    outline.erase(&outline.faces.keys().copied().collect());
    let copy = outline.copy_entities(&outline.selectable_entities());
    outline.paste_entities(&copy, DVec3::Z);
    assert!(outline.faces.is_empty());
    outline.validate().unwrap();
}

#[test]
fn hidden_and_locked_objects_affect_picking_bounds_erasing_and_snapping() {
    let mut model = Model::default();
    let edge = model.add_edge(DVec3::ZERO, DVec3::X * 2.).unwrap();
    let group = model
        .make_object(&BTreeSet::from([edge]), false, "Line")
        .unwrap();
    let camera = Camera::default();
    let size = DVec2::new(1000., 800.);
    let pixel = camera.project(DVec3::X, size).unwrap();
    assert_eq!(
        selection::pick_entity(&model, &camera, size, pixel, 8.),
        Some(group)
    );
    model.locked.insert(group);
    let before = model.clone();
    model.erase(&BTreeSet::from([group]));
    assert_eq!(model, before);
    assert!(
        model
            .make_object(&BTreeSet::from([group]), false, "No")
            .is_none()
    );
    model.hidden.insert(group);
    assert!(selection::pick_entity(&model, &camera, size, pixel, 8.).is_none());
    assert!(model.bounds().is_none());
    assert!(model.selectable_entities().is_empty());
    let snap = infer_with_snapping(&model, &camera, size, pixel, None, None, true, false).unwrap();
    assert_eq!(snap.kind, Inference::Plane);
}

#[test]
fn object_history_and_document_roundtrip_preserve_state_and_reject_cycles() {
    let mut doc = Document::default();
    rectangle(&mut doc.model, DVec3::ZERO);
    let before = doc.model.clone();
    let group = doc
        .model
        .make_object(&doc.model.selectable_entities(), true, "Panel")
        .unwrap();
    doc.model.hidden.insert(group);
    doc.model.locked.insert(group);
    let after = doc.model.clone();
    let mut history = History::default();
    history.record("Component", before.clone(), &doc.model);
    assert!(history.undo(&mut doc.model));
    assert_eq!(doc.model, before);
    assert!(history.redo(&mut doc.model));
    assert_eq!(doc.model, after);
    let json = serde_json::to_string(&doc).unwrap();
    assert_eq!(Document::from_json(&json).unwrap().model, after);
    doc.model
        .objects
        .get_mut(&group)
        .unwrap()
        .entities
        .insert(group);
    assert!(doc.model.validate().is_err());
}

#[test]
fn component_materials_link_copies_and_explode_detaches_just_one_instance() {
    let mut model = Model::default();
    rectangle(&mut model, DVec3::ZERO);
    let component = model
        .make_object(&model.selectable_entities(), true, "Tile")
        .unwrap();
    let source = model.copy_entities(&BTreeSet::from([component]));
    let pasted = model.paste_entities(&source, DVec3::X * 5.);
    let original_face = *model.objects[&component]
        .entities
        .iter()
        .find(|id| model.faces.contains_key(id))
        .unwrap();
    model.paint_face(original_face, [25, 50, 75]).unwrap();
    assert_eq!(
        model
            .face_colors
            .values()
            .filter(|color| **color == [25, 50, 75])
            .count(),
        2
    );
    model.move_selection(&pasted, DVec3::Z * 3.).unwrap();
    assert!(
        model.vertices[&model.faces[&original_face].vertices[0]]
            .position
            .z
            .abs()
            < 1e-7
    );
    let detached = model.dissolve_objects(&pasted, false);
    let detached_face = *detached
        .iter()
        .find(|id| model.faces.contains_key(id))
        .unwrap();
    model.paint_face(detached_face, [100, 120, 140]).unwrap();
    assert_eq!(model.face_colors[&original_face], [25, 50, 75]);
    assert_eq!(model.face_colors[&detached_face], [100, 120, 140]);
    // Identical, independently created components must have distinct definitions,
    // including when created in a fresh document whose integer IDs restart.
    let mut fresh = Model::default();
    rectangle(&mut fresh, DVec3::ZERO);
    let unrelated = fresh
        .make_object(&fresh.selectable_entities(), true, "Tile")
        .unwrap();
    assert_ne!(
        fresh.objects[&unrelated].kind,
        model.objects[&component].kind
    );
    fresh.paste_entities(&source, DVec3::X * 5.);
    let face = *fresh.objects[&unrelated]
        .entities
        .iter()
        .find(|id| fresh.faces.contains_key(id))
        .unwrap();
    fresh.paint_face(face, [1, 2, 3]).unwrap();
    assert_eq!(fresh.face_colors.len(), 1);
    model.validate().unwrap();
    fresh.validate().unwrap();
}

#[test]
fn clipboard_keeps_a_face_hole_and_locked_surfaces_protect_boundary_edits() {
    let mut model = Model::default();
    rectangle(&mut model, DVec3::ZERO);
    let outer = *model.faces.keys().next().unwrap();
    let points = [
        DVec3::new(0.5, 0.5, 0.),
        DVec3::new(1.5, 0.5, 0.),
        DVec3::new(1.5, 1.5, 0.),
        DVec3::new(0.5, 1.5, 0.),
    ];
    for i in 0..4 {
        model.add_edge(points[i], points[(i + 1) % 4]).unwrap();
    }
    let inner = *model.faces.keys().find(|id| **id != outer).unwrap();
    model.erase(&BTreeSet::from([inner]));
    let copy = model.copy_entities(&BTreeSet::from([outer]));
    let mut pasted = Model::default();
    pasted.paste_entities(&copy, DVec3::Z * 2.);
    assert_eq!(pasted.faces.len(), 1);
    let face = pasted.faces.values().next().unwrap();
    assert_eq!(face.holes.len(), 1);
    assert!((pasted.face_area(face) - 3.).abs() < 1e-7);
    pasted.validate().unwrap();
    model.locked.insert(outer);
    let before = model.clone();
    model.erase(&model.edges.keys().copied().collect());
    assert_eq!(model, before);
    let edge = *model.edges.keys().next().unwrap();
    assert!(
        model
            .move_selection(&BTreeSet::from([edge]), DVec3::Z)
            .is_err()
    );
    assert!(model.push_pull(outer, 1.).is_err());
    assert!(model.offset_face(outer, 0.1).is_err());
}

#[test]
fn component_paste_uses_current_definition_materials_instead_of_stale_clipboard() {
    let mut model = Model::default();
    rectangle(&mut model, DVec3::ZERO);
    let component = model
        .make_object(&model.selectable_entities(), true, "Panel")
        .unwrap();
    let stale_copy = model.copy_entities(&BTreeSet::from([component]));
    let face = *model.faces.keys().next().unwrap();
    model.paint_face(face, [123, 45, 67]).unwrap();
    model.paste_entities(&stale_copy, DVec3::X * 5.);
    assert_eq!(model.face_colors.len(), 2);
    assert!(
        model
            .face_colors
            .values()
            .all(|color| *color == [123, 45, 67])
    );
    model.validate().unwrap();
}

#[test]
fn grouped_boundary_edge_can_be_picked_over_the_outline_retained_for_its_neighbor() {
    let mut model = Model::default();
    rectangle(&mut model, DVec3::ZERO);
    let edge = *model.edges.keys().next().unwrap();
    let (a, b) = model.points(&model.edges[&edge]);
    let group = model
        .make_object(&BTreeSet::from([edge]), false, "Boundary")
        .unwrap();
    assert!(model.edges.contains_key(&edge)); // The unselected face still needs it.
    assert_eq!(model.edges.len(), 5);
    let camera = Camera::default();
    let size = DVec2::new(1000., 800.);
    let pixel = camera.project((a + b) * 0.5, size).unwrap();
    assert_eq!(
        selection::pick_entity(&model, &camera, size, pixel, 8.),
        Some(group)
    );
    model.validate().unwrap();
}

#[test]
fn bulk_root_bounds_and_lock_queries_preserve_inherited_state() {
    let mut model = Model::default();
    let edge = model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    let inner = model
        .make_object(&BTreeSet::from([edge]), false, "Inner")
        .unwrap();
    let outer = model
        .make_object(&BTreeSet::from([inner]), false, "Outer")
        .unwrap();
    // Grouping copies the selected contents into isolated geometry with fresh IDs.
    let inner = *model.objects[&outer].entities.first().unwrap();
    let loose = model.add_edge(DVec3::Y * 10., DVec3::Y * 11.).unwrap();
    assert_eq!(model.root_entities(), BTreeSet::from([outer, loose]));
    let nested = model.expand_entities(&BTreeSet::from([outer]));
    assert!(!model.selection_locked(&nested));
    assert!(!model.selection_protected(&nested));
    model.locked.insert(outer);
    assert!(model.selection_locked(&BTreeSet::from([inner])));
    assert!(model.selection_protected(&BTreeSet::from([inner])));
    assert!(!model.selection_protected(&BTreeSet::from([loose])));
    model.hidden.insert(outer);
    assert_eq!(model.bounds(), Some((DVec3::Y * 10., DVec3::Y * 11.)));
    model.hidden.clear();
    let tag = model.add_tag("Hidden assembly").unwrap();
    model.entity_tags.insert(outer, tag);
    model.tags.get_mut(&tag).unwrap().visible = false;
    assert_eq!(model.bounds(), Some((DVec3::Y * 10., DVec3::Y * 11.)));
    model.validate().unwrap();
}
