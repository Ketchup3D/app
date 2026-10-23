use crate::{Axis, DVec3, History, Model, SolidOperation};
use std::collections::BTreeSet;

fn box_model(min: DVec3, max: DVec3) -> Model {
    let mut model = Model::default();
    let corners = [
        min,
        DVec3::new(max.x, min.y, min.z),
        DVec3::new(max.x, max.y, min.z),
        DVec3::new(min.x, max.y, min.z),
    ];
    for i in 0..4 {
        model.add_edge(corners[i], corners[(i + 1) % 4]).unwrap();
    }
    let face = *model.faces.keys().next().unwrap();
    let normal = crate::faces::plane(&model, &model.faces[&face].vertices)
        .unwrap()
        .normal;
    model.push_pull(face, (max.z - min.z) / normal.z).unwrap();
    model
}
fn add_box(model: &mut Model, min: DVec3, max: DVec3) -> u64 {
    let body = box_model(min, max);
    let ids = model.paste_entities(&body, DVec3::ZERO);
    model.make_object(&ids, false, "Box").unwrap()
}
fn pair() -> (Model, u64, u64) {
    let mut model = Model::default();
    let a = add_box(&mut model, DVec3::ZERO, DVec3::splat(2.));
    let b = add_box(&mut model, DVec3::new(1., 0., 0.), DVec3::new(3., 2., 2.));
    (model, a, b)
}
fn volume(model: &Model, ids: &BTreeSet<u64>) -> f64 {
    ids.iter().map(|id| model.solid_volume(*id).unwrap()).sum()
}
#[test]
fn overlapping_boxes_union_intersection_trim_and_split_have_correct_volumes() {
    for (operation, expected, pieces) in [
        (SolidOperation::Union, 12., 1),
        (SolidOperation::OuterShell, 12., 1),
        (SolidOperation::Intersect, 4., 1),
        (SolidOperation::Trim, 4., 1),
        (SolidOperation::Split, 12., 3),
    ] {
        let (mut model, a, b) = pair();
        assert!((model.solid_volume(a).unwrap() - 8.).abs() < 1e-6);
        let result = model.boolean_solids(a, b, operation).unwrap();
        assert_eq!(result.len(), pieces, "{operation:?}");
        assert!(
            (volume(&model, &result) - expected).abs() < 1e-6,
            "{operation:?}"
        );
        assert_eq!(
            model.objects.contains_key(&a),
            operation == SolidOperation::Trim
        );
        model.validate().unwrap();
        if operation == SolidOperation::Union {
            assert_eq!(
                model.faces.len(),
                6,
                "Coplanar fragments should merge into editable faces"
            );
        }
    }
}
#[test]
fn booleans_are_undoable_and_invalid_or_locked_operands_do_not_mutate() {
    let (mut model, a, b) = pair();
    let before = model.clone();
    let result = model.boolean_solids(a, b, SolidOperation::Union).unwrap();
    let mut history = History::default();
    history.record("Union", before.clone(), &model);
    assert!(history.undo(&mut model));
    assert_eq!(model, before);
    assert!(history.redo(&mut model));
    assert!((volume(&model, &result) - 12.).abs() < 1e-6);
    let (mut model, a, b) = pair();
    model.locked.insert(a);
    let before = model.clone();
    assert!(model.boolean_solids(a, b, SolidOperation::Union).is_err());
    assert_eq!(model, before);
    model.locked.clear();
    let face = *model
        .expand_entities(&BTreeSet::from([a]))
        .iter()
        .find(|id| model.faces.contains_key(id))
        .unwrap();
    // Delete inside the group so the object stays available as an open operand.
    assert!(model.enter_object(a));
    model.erase(&BTreeSet::from([face]));
    model.edit_context = None;
    let before = model.clone();
    assert!(model.boolean_solids(a, b, SolidOperation::Union).is_err());
    assert_eq!(model, before);
}
#[test]
fn disjoint_touching_and_nested_solids() {
    for (min, max, union) in [
        (DVec3::new(3., 0., 0.), DVec3::new(4., 1., 1.), 9.),
        (DVec3::new(2., 0., 0.), DVec3::new(3., 2., 2.), 12.),
        (DVec3::splat(0.5), DVec3::splat(1.5), 8.),
    ] {
        let mut model = Model::default();
        let a = add_box(&mut model, DVec3::ZERO, DVec3::splat(2.));
        let b = add_box(&mut model, min, max);
        let result = model.boolean_solids(a, b, SolidOperation::Union).unwrap();
        assert!((volume(&model, &result) - union).abs() < 1e-6);
        model.validate().unwrap();
    }
}
#[test]
fn flip_preserves_component_links_and_reflected_frames_when_editing_and_copying() {
    let mut model = box_model(DVec3::ZERO, DVec3::new(4., 2., 1.));
    let ids = model.root_entities();
    let original = model.make_object(&ids, true, "Component").unwrap();
    let copy = model.copy_entities(&BTreeSet::from([original]));
    let peer = *model.paste_entities(&copy, DVec3::X * 8.).first().unwrap();
    model
        .flip_selection(&BTreeSet::from([peer]), Axis::X)
        .unwrap();
    assert_eq!(
        model.objects[&peer].transform.unwrap().axes,
        DVec3::new(-1., 1., 1.)
    );
    assert!(model.enter_object(original));
    let face = *model
        .expand_entities(&model.objects[&original].entities)
        .iter()
        .find(|id| {
            model.faces.contains_key(id)
                && model.faces[id]
                    .vertices
                    .iter()
                    .all(|id| model.vertices[id].position.x.abs() < 1e-7)
        })
        .unwrap();
    model.push_pull(face, 0.5).unwrap();
    model.edit_context = None;
    model.validate().unwrap();
    assert!(
        (model.solid_volume(original).unwrap() - model.solid_volume(peer).unwrap()).abs() < 1e-6
    );
    let source_frame = model.objects[&original].transform.unwrap();
    let peer_frame = model.objects[&peer].transform.unwrap();
    for (key, source) in &model.objects[&original].definition_entities {
        if let Some(edge) = model.edges.get(source) {
            let other = &model.edges[&model.objects[&peer].definition_entities[key]];
            for (a, b) in [(edge.a, other.a), (edge.b, other.b)] {
                let a = (model.vertices[&a].position - source_frame.translation)
                    * source_frame.axes
                    / source_frame.scale;
                let b = (model.vertices[&b].position - peer_frame.translation) * peer_frame.axes
                    / peer_frame.scale;
                assert!(a.distance(b) < 1e-7);
            }
        }
    }
    let copy = model.copy_entities(&BTreeSet::from([peer]));
    let pasted = model.paste_entities(&copy, DVec3::Y * 5.);
    assert_eq!(pasted.len(), 1);
    model.validate().unwrap();
}

#[test]
fn rotated_solids_obey_volume_conservation_and_identical_solids_are_idempotent() {
    let (mut model, a, b) = pair();
    let rotation = glam::DQuat::from_rotation_z(0.43) * glam::DQuat::from_rotation_x(0.31);
    for vertex in model.vertices.values_mut() {
        vertex.position = rotation * vertex.position + DVec3::new(12., -7., 5.);
    }
    let result = model
        .boolean_solids(a, b, SolidOperation::Intersect)
        .unwrap();
    assert!((volume(&model, &result) - 4.).abs() < 1e-5);
    model.validate().unwrap();
    for operation in [
        SolidOperation::Union,
        SolidOperation::Intersect,
        SolidOperation::Split,
    ] {
        let mut model = Model::default();
        let a = add_box(&mut model, DVec3::ZERO, DVec3::splat(2.));
        let b = add_box(&mut model, DVec3::ZERO, DVec3::splat(2.));
        let result = model.boolean_solids(a, b, operation).unwrap();
        assert!((volume(&model, &result) - 8.).abs() < 1e-6);
        assert_eq!(result.len(), 1);
    }
    let (mut original, a, b) = pair();
    let vertices = original.selection_vertices(&BTreeSet::from([b]));
    let center = DVec3::new(2., 1., 1.);
    let rotation = glam::DQuat::from_rotation_z(0.41);
    for id in vertices {
        let v = original.vertices.get_mut(&id).unwrap();
        v.position = center + rotation * (v.position - center);
    }
    let mut union = original.clone();
    let u = union.boolean_solids(a, b, SolidOperation::Union).unwrap();
    let mut intersection = original.clone();
    let i = intersection
        .boolean_solids(a, b, SolidOperation::Intersect)
        .unwrap();
    assert!((volume(&union, &u) + volume(&intersection, &i) - 16.).abs() < 1e-5);
}

#[test]
fn outer_shell_fills_an_enclosed_void_while_union_preserves_it() {
    let mut cavity = Model::default();
    let outer = add_box(&mut cavity, DVec3::ZERO, DVec3::splat(4.));
    let cutter = add_box(&mut cavity, DVec3::ONE, DVec3::splat(3.));
    let shell = *cavity
        .boolean_solids(cutter, outer, SolidOperation::Trim)
        .unwrap()
        .first()
        .unwrap();
    cavity.erase(&BTreeSet::from([cutter]));
    assert!((cavity.solid_volume(shell).unwrap() - 56.).abs() < 1e-6);
    let extra = add_box(&mut cavity, DVec3::new(5., 0., 0.), DVec3::new(6., 1., 1.));
    let mut union = cavity.clone();
    let result = union
        .boolean_solids(shell, extra, SolidOperation::Union)
        .unwrap();
    assert!((volume(&union, &result) - 57.).abs() < 1e-6);
    let result = cavity
        .boolean_solids(shell, extra, SolidOperation::OuterShell)
        .unwrap();
    assert!((volume(&cavity, &result) - 65.).abs() < 1e-6);
}

#[test]
fn non_overlapping_intersection_trim_and_split_are_atomic_and_complete_trim_keeps_cutter() {
    for operation in [
        SolidOperation::Intersect,
        SolidOperation::Trim,
        SolidOperation::Split,
    ] {
        let mut model = Model::default();
        let a = add_box(&mut model, DVec3::ZERO, DVec3::ONE);
        let b = add_box(&mut model, DVec3::splat(3.), DVec3::splat(4.));
        let before = model.clone();
        assert!(model.boolean_solids(a, b, operation).is_err());
        assert_eq!(model, before);
    }
    let mut model = Model::default();
    let cutter = add_box(&mut model, DVec3::ZERO, DVec3::splat(4.));
    let target = add_box(&mut model, DVec3::ONE, DVec3::splat(2.));
    let result = model
        .boolean_solids(cutter, target, SolidOperation::Trim)
        .unwrap();
    assert!(result.is_empty());
    assert!(model.objects.contains_key(&cutter));
    assert!(!model.objects.contains_key(&target));
    model.validate().unwrap();
}

#[test]
fn boolean_results_preserve_materials_dimensions_elsewhere_and_scoped_component_edits() {
    let (mut model, a, b) = pair();
    let face_ids: Vec<_> = model
        .expand_entities(&BTreeSet::from([a]))
        .into_iter()
        .filter(|id| model.faces.contains_key(id))
        .collect();
    for id in face_ids {
        model.paint_face(id, [180, 60, 40]).unwrap();
    }
    let dimension = model
        .add_dimension(DVec3::ZERO, DVec3::new(2., 0., 0.), DVec3::NEG_Y)
        .unwrap();
    let result = model.boolean_solids(a, b, SolidOperation::Union).unwrap();
    assert!(model.face_colors.values().any(|c| *c == [180, 60, 40]));
    assert!(model.dimensions.contains_key(&dimension));
    assert!((model.dimensions[&dimension].length(&model) - 2.).abs() < 1e-7);
    let container = model
        .make_object(&result, true, "Boolean container")
        .unwrap();
    let copy = model.copy_entities(&BTreeSet::from([container]));
    let peer = *model.paste_entities(&copy, DVec3::Y * 6.).first().unwrap();
    assert!(model.enter_object(container));
    let first = *model.objects[&container].entities.first().unwrap();
    let second = add_box(&mut model, DVec3::new(2., 0., 0.), DVec3::new(4., 2., 2.));
    let results = model
        .boolean_solids(first, second, SolidOperation::Union)
        .unwrap();
    assert_eq!(results.len(), 1);
    model.edit_context = None;
    assert!((model.solid_volume(container).unwrap() - 16.).abs() < 1e-6);
    assert!((model.solid_volume(peer).unwrap() - 16.).abs() < 1e-6);
    model.validate().unwrap();
}

#[test]
fn faceted_curves_and_through_holes_remain_closed_after_clipping() {
    let mut cylinder = Model::default();
    let ring: Vec<_> = (0..12)
        .map(|i| {
            let angle = i as f64 * std::f64::consts::TAU / 12.;
            DVec3::new(angle.cos() * 2., angle.sin() * 2., 0.)
        })
        .collect();
    for i in 0..ring.len() {
        cylinder
            .add_edge(ring[i], ring[(i + 1) % ring.len()])
            .unwrap();
    }
    let face = *cylinder.faces.keys().next().unwrap();
    let normal = crate::faces::plane(&cylinder, &cylinder.faces[&face].vertices)
        .unwrap()
        .normal;
    cylinder.push_pull(face, 2. / normal.z).unwrap();
    let roots = cylinder.root_entities();
    let a = cylinder.make_object(&roots, false, "Cylinder").unwrap();
    assert!((cylinder.solid_volume(a).unwrap() - 24.).abs() < 1e-6);
    let b = add_box(
        &mut cylinder,
        DVec3::new(-1., -3., -1.),
        DVec3::new(1., 3., 3.),
    );
    let mut intersection = cylinder.clone();
    let overlap = intersection
        .boolean_solids(a, b, SolidOperation::Intersect)
        .unwrap();
    let joined = cylinder
        .boolean_solids(a, b, SolidOperation::Union)
        .unwrap();
    assert!((volume(&cylinder, &joined) + volume(&intersection, &overlap) - 72.).abs() < 1e-5);

    let mut ring = Model::default();
    for points in [
        [
            DVec3::new(-3., -3., 0.),
            DVec3::new(3., -3., 0.),
            DVec3::new(3., 3., 0.),
            DVec3::new(-3., 3., 0.),
        ],
        [
            DVec3::new(-1., -1., 0.),
            DVec3::new(1., -1., 0.),
            DVec3::new(1., 1., 0.),
            DVec3::new(-1., 1., 0.),
        ],
    ] {
        for i in 0..4 {
            ring.add_edge(points[i], points[(i + 1) % 4]).unwrap();
        }
    }
    let base = *ring.faces.keys().next().unwrap();
    let inner = *ring.faces.keys().next_back().unwrap();
    ring.erase(&BTreeSet::from([inner]));
    let normal = crate::faces::plane(&ring, &ring.faces[&base].vertices)
        .unwrap()
        .normal;
    ring.push_pull(base, 2. / normal.z).unwrap();
    let a = ring
        .make_object(&ring.root_entities(), false, "Through hole")
        .unwrap();
    assert!((ring.solid_volume(a).unwrap() - 64.).abs() < 1e-6);
    let b = add_box(&mut ring, DVec3::new(2., -1., 0.), DVec3::new(4., 1., 2.));
    let result = ring
        .boolean_solids(a, b, SolidOperation::OuterShell)
        .unwrap();
    assert!((volume(&ring, &result) - 68.).abs() < 1e-6);
    assert!(ring.faces.values().any(|f| !f.holes.is_empty()));
}

#[test]
fn outer_shell_removes_solid_islands_inside_an_enclosed_void() {
    let mut model = Model::default();
    let outer = add_box(&mut model, DVec3::ZERO, DVec3::splat(4.));
    let cutter = add_box(&mut model, DVec3::ONE, DVec3::splat(3.));
    let shell = *model
        .boolean_solids(cutter, outer, SolidOperation::Trim)
        .unwrap()
        .first()
        .unwrap();
    model.erase(&BTreeSet::from([cutter]));
    let island = add_box(&mut model, DVec3::splat(1.5), DVec3::splat(2.5));
    let mut union = model.clone();
    let result = union
        .boolean_solids(shell, island, SolidOperation::Union)
        .unwrap();
    assert!((volume(&union, &result) - 57.).abs() < 1e-6);
    let result = model
        .boolean_solids(shell, island, SolidOperation::OuterShell)
        .unwrap();
    assert!((volume(&model, &result) - 64.).abs() < 1e-6);
    assert_eq!(model.faces.len(), 6);
}
