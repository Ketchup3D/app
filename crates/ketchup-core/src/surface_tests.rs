use super::*;
use std::collections::BTreeSet;

fn outline(model: &mut Model, points: &[DVec3]) {
    for i in 0..points.len() {
        model
            .add_edge(points[i], points[(i + 1) % points.len()])
            .unwrap();
    }
}
fn square(model: &mut Model, low: f64, high: f64) {
    outline(
        model,
        &[
            DVec3::new(low, low, 0.),
            DVec3::new(high, low, 0.),
            DVec3::new(high, high, 0.),
            DVec3::new(low, high, 0.),
        ],
    );
}
#[test]
fn closing_rectangle_creates_one_face_and_deleting_only_fill_keeps_edges() {
    let mut model = Model::default();
    square(&mut model, 0., 4.);
    assert_eq!(model.faces.len(), 1);
    assert_eq!(model.edges.len(), 4);
    let face = model.faces.values().next().unwrap().clone();
    assert!((model.face_area(&face) - 16.).abs() < 1e-7);
    model.validate().unwrap();
    model.erase(&BTreeSet::from([face.id]));
    assert!(model.faces.is_empty());
    assert_eq!(model.edges.len(), 4);
    model
        .add_edge(DVec3::new(10., 0., 0.), DVec3::new(10., 1., 0.))
        .unwrap();
    assert!(
        model.faces.is_empty(),
        "Unrelated edits must not regenerate a deleted fill"
    );
    model.add_edge(DVec3::ZERO, DVec3::X * 4.).unwrap();
    assert_eq!(
        model.faces.len(),
        1,
        "Retracing a boundary heals the surface"
    );
}
#[test]
fn concave_vertical_and_tilted_closed_outlines_fill() {
    let concave = [
        DVec3::ZERO,
        DVec3::new(3., 0., 0.),
        DVec3::new(3., 1., 0.),
        DVec3::new(1., 1., 0.),
        DVec3::new(1., 3., 0.),
        DVec3::new(0., 3., 0.),
    ];
    for points in [
        concave,
        concave.map(|p| DVec3::new(p.x, 0., p.y)),
        concave.map(|p| DVec3::new(p.x, p.y, p.y)),
    ] {
        let mut model = Model::default();
        outline(&mut model, &points);
        assert_eq!(model.faces.len(), 1);
        let face = model.faces.values().next().unwrap();
        assert!(!model.face_triangles(face).is_empty());
        model.validate().unwrap();
    }
}
#[test]
fn non_planar_and_self_crossing_loops_remain_wireframes() {
    for points in [
        [DVec3::ZERO, DVec3::X, DVec3::ONE, DVec3::Y],
        [
            DVec3::ZERO,
            DVec3::new(2., 2., 0.),
            DVec3::new(0., 2., 0.),
            DVec3::new(2., 0., 0.),
        ],
    ] {
        let mut model = Model::default();
        outline(&mut model, &points);
        assert!(model.faces.is_empty());
    }
}
#[test]
fn splitting_face_creates_two_regions_with_correct_area() {
    let mut model = Model::default();
    square(&mut model, 0., 4.);
    model
        .add_edge(DVec3::new(0., 2., 0.), DVec3::new(4., 2., 0.))
        .unwrap();
    assert_eq!(model.faces.len(), 2);
    assert_eq!(model.edges.len(), 7);
    assert!(
        (model
            .faces
            .values()
            .map(|f| model.face_area(f))
            .sum::<f64>()
            - 16.)
            .abs()
            < 1e-7
    );
    model.validate().unwrap();
}
#[test]
fn deleting_boundary_removes_dependent_fill_and_undo_restores_both() {
    let mut model = Model::default();
    square(&mut model, 0., 4.);
    let before = model.clone();
    let edge = *model.edges.keys().next().unwrap();
    model.erase(&BTreeSet::from([edge]));
    assert!(model.faces.is_empty());
    assert_eq!(model.edges.len(), 3);
    let mut history = History::default();
    history.record("Erase", before.clone(), &model);
    history.undo(&mut model);
    assert_eq!(model, before);
}
#[test]
fn nested_fill_can_be_deleted_to_leave_a_real_hole() {
    let mut model = Model::default();
    square(&mut model, 0., 6.);
    square(&mut model, 2., 4.);
    assert_eq!(model.faces.len(), 2);
    let outer = model
        .faces
        .values()
        .find(|f| {
            f.vertices
                .iter()
                .any(|id| model.vertices[id].position == DVec3::ZERO)
        })
        .unwrap()
        .clone();
    assert_eq!(outer.holes.len(), 1);
    assert!((model.face_area(&outer) - 32.).abs() < 1e-6);
    let inner = model.faces.values().find(|f| f.id != outer.id).unwrap().id;
    model.erase(&BTreeSet::from([inner]));
    assert!(!model.face_contains(&outer, DVec3::new(3., 3., 0.), true));
    let ray = Ray {
        origin: DVec3::new(3., 3., 5.),
        direction: -DVec3::Z,
    };
    assert!(model.nearest_face(ray).is_none());
    model.validate().unwrap();
}
#[test]
fn dangling_edge_and_midpoint_split_keep_existing_surface() {
    let mut model = Model::default();
    square(&mut model, 0., 4.);
    let id = *model.faces.keys().next().unwrap();
    model
        .add_edge(DVec3::new(0., 2., 0.), DVec3::new(1., 2., 0.))
        .unwrap();
    assert_eq!(model.faces.len(), 1);
    assert_eq!(model.faces[&id].vertices.len(), 5);
    model.validate().unwrap();
}
#[test]
fn geometry_snapping_can_be_disabled_and_includes_arbitrary_edge_points() {
    let camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let mut model = Model::default();
    model
        .add_edge(DVec3::new(0., 0., 2.), DVec3::new(6., 0., 2.))
        .unwrap();
    let point = DVec3::new(1.5, 0., 2.);
    let cursor = camera.project(point, size).unwrap() + DVec2::new(2., 2.);
    let snap = infer_with_snapping(&model, &camera, size, cursor, None, None, true, true).unwrap();
    assert_eq!(snap.kind, Inference::OnEdge);
    assert!((snap.point.z - 2.).abs() < 1e-7);
    let free =
        infer_with_snapping(&model, &camera, size, cursor, None, None, false, false).unwrap();
    assert_eq!(free.kind, Inference::Plane);
    assert!(free.point.z.abs() < 1e-7);
}

#[test]
fn short_following_segments_snap_from_each_current_endpoint() {
    let camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let mut model = Model::default();
    let mut start = DVec3::new(4., 0., 0.);
    model.add_edge(DVec3::ZERO, start).unwrap();
    for axis in [Axis::Y, Axis::Z, Axis::X] {
        let origin = camera.project(start, size).unwrap();
        let direction = (camera.project(start + axis.vector(), size).unwrap() - origin).normalize();
        let cursor = origin + direction * 9. + DVec2::new(-direction.y, direction.x) * 0.5;
        let snap =
            infer_with_snapping(&model, &camera, size, cursor, Some(start), None, true, true)
                .unwrap();
        assert_eq!(snap.kind, Inference::Axis(axis), "Failed after {start:?}");
        assert!(snap.point.distance(start) > 1e-3);
        assert!((snap.point - start).cross(axis.vector()).length() < 1e-7);
        model.add_edge(start, snap.point).unwrap();
        start = snap.point;
    }
}

#[test]
fn initial_and_subsequent_points_can_snap_to_visible_world_axes() {
    let camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let model = Model::default();
    let on_axis = DVec3::X * 3.;
    let pixel = camera.project(on_axis, size).unwrap();
    let direction = (pixel - camera.project(DVec3::ZERO, size).unwrap()).normalize();
    let cursor = pixel + DVec2::new(-direction.y, direction.x) * 2.;
    for start in [None, Some(DVec3::new(-2., 3., 1.))] {
        let snap =
            infer_with_snapping(&model, &camera, size, cursor, start, None, false, true).unwrap();
        assert_eq!(snap.kind, Inference::OnAxis(Axis::X));
        assert!(snap.point.y.abs() < 1e-7 && snap.point.z.abs() < 1e-7);
    }
}

#[test]
fn inferred_direction_can_also_snap_exactly_to_an_existing_edge() {
    let camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let mut model = Model::default();
    model
        .add_edge(DVec3::new(4., -1., 0.), DVec3::new(4., 4., 0.))
        .unwrap();
    let target = DVec3::X * 4.;
    let pixel = camera.project(target, size).unwrap() + DVec2::new(2., 2.);
    let snap = infer_with_snapping(
        &model,
        &camera,
        size,
        pixel,
        Some(DVec3::ZERO),
        None,
        true,
        true,
    )
    .unwrap();
    assert_eq!(snap.kind, Inference::OnEdge);
    assert!(snap.point.distance(target) < 1e-7);
}

#[test]
fn automatic_perpendicular_alignment_does_not_reject_nearby_precise_targets() {
    let camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let start = DVec3::new(1., 1., 0.);
    let on_axis = camera.project(start + DVec3::Y * 3., size).unwrap();
    let direction = (on_axis - camera.project(start, size).unwrap()).normalize();
    let target = camera
        .ray(on_axis + DVec2::new(-direction.y, direction.x) * 6., size)
        .plane_z(0.)
        .unwrap();
    for (a, b, expected) in [
        (target, target + DVec3::X * 2., Inference::Endpoint),
        (
            target - DVec3::X * 2.,
            target + DVec3::X * 2.,
            Inference::Midpoint,
        ),
    ] {
        let mut model = Model::default();
        model.add_edge(a, b).unwrap();
        let snap = infer_with_snapping(
            &model,
            &camera,
            size,
            on_axis,
            Some(start),
            None,
            true,
            true,
        )
        .unwrap();
        assert_eq!(snap.kind, expected);
        assert!(snap.point.distance(target) < 1e-7);
    }
}

#[test]
fn automatic_perpendicular_alignment_can_snap_to_offset_parallel_edge() {
    let camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let start = DVec3::new(1., 1., 0.);
    let mut model = Model::default();
    model
        .add_edge(DVec3::new(1.15, 1., 0.), DVec3::new(1.15, 7., 0.))
        .unwrap();
    let point = DVec3::new(1.15, 3., 0.);
    let cursor = camera.project(point, size).unwrap();
    let automatic = infer_with_snapping(
        &model,
        &camera,
        size,
        cursor,
        Some(start),
        None,
        false,
        true,
    )
    .unwrap();
    assert_eq!(
        automatic.kind,
        Inference::Axis(Axis::Y),
        "The target must be in the automatic perpendicular alignment range"
    );
    let snap =
        infer_with_snapping(&model, &camera, size, cursor, Some(start), None, true, true).unwrap();
    assert_eq!(snap.kind, Inference::OnEdge);
    assert!(snap.point.distance(point) < 1e-7);
    let locked = infer_with_snapping(
        &model,
        &camera,
        size,
        cursor,
        Some(start),
        Some(Axis::Y),
        true,
        true,
    )
    .unwrap();
    assert_eq!(locked.kind, Inference::Axis(Axis::Y));
    assert!((locked.point.x - start.x).abs() < 1e-7);
}

#[test]
fn perpendicular_alignment_snaps_exactly_to_face_intersection() {
    let camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let start = DVec3::new(0.5, 0.5, 0.);
    let mut model = Model::default();
    outline(
        &mut model,
        &[
            DVec3::new(-3., -3., 3.),
            DVec3::new(3., -3., 3.),
            DVec3::new(3., 3., 3.),
            DVec3::new(-3., 3., 3.),
        ],
    );
    let target = start + DVec3::Z * 3.;
    let cursor = camera.project(target, size).unwrap() + DVec2::new(2., 1.);
    for lock in [None, Some(Axis::Z)] {
        let snap =
            infer_with_snapping(&model, &camera, size, cursor, Some(start), lock, true, true)
                .unwrap();
        assert_eq!(snap.kind, Inference::OnFace);
        assert!(snap.point.distance(target) < 1e-7);
    }
}

#[test]
fn perpendicular_preview_can_leave_its_starting_face() {
    let camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let mut model = Model::default();
    square(&mut model, -5., 5.);
    let start = DVec3::new(0.5, 0.5, 0.);
    let target = start + DVec3::Z;
    let cursor = camera.project(target, size).unwrap();
    assert!(model.nearest_face(camera.ray(cursor, size)).is_some());
    let snap =
        infer_with_snapping(&model, &camera, size, cursor, Some(start), None, true, true).unwrap();
    assert_eq!(snap.kind, Inference::Axis(Axis::Z));
    assert!(snap.point.distance(target) < 1e-7);
}

#[test]
fn automatic_axis_snapping_can_be_disabled_without_disabling_manual_locks() {
    let model = Model::default();
    let camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let start = DVec3::new(1., 1., 0.);
    let cursor = camera.project(start + DVec3::X * 3., size).unwrap() + DVec2::new(0., 2.);
    let snapped = infer_with_snapping(
        &model,
        &camera,
        size,
        cursor,
        Some(start),
        None,
        false,
        true,
    )
    .unwrap();
    assert_eq!(snapped.kind, Inference::Axis(Axis::X));
    assert!((snapped.point.y - start.y).abs() < 1e-7);
    let free = infer_with_snapping(
        &model,
        &camera,
        size,
        cursor,
        Some(start),
        None,
        false,
        false,
    )
    .unwrap();
    assert_eq!(free.kind, Inference::Plane);
    assert!((free.point.y - start.y).abs() > 1e-3);
    let locked = infer_with_snapping(
        &model,
        &camera,
        size,
        cursor,
        Some(start),
        Some(Axis::X),
        false,
        false,
    )
    .unwrap();
    assert_eq!(locked.kind, Inference::Axis(Axis::X));
    assert!((locked.point.y - start.y).abs() < 1e-7);
}

#[test]
fn axis_snap_setting_defaults_on_for_older_documents_and_persists_off() {
    let mut doc = Document::default();
    assert!(doc.snap_to_axes);
    doc.snap_to_axes = false;
    let restored = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert!(!restored.snap_to_axes);
    assert!(restored.snap_to_geometry);
    let mut json = serde_json::to_value(&doc).unwrap();
    json.as_object_mut().unwrap().remove("snap_to_axes");
    assert!(Document::from_json(&json.to_string()).unwrap().snap_to_axes);
}
#[test]
fn face_pick_and_surface_snap_target_visible_surface_not_hidden_edges() {
    let camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let mut model = Model::default();
    let points = [
        DVec3::new(-3., -3., 2.),
        DVec3::new(3., -3., 2.),
        DVec3::new(3., 3., 2.),
        DVec3::new(-3., 3., 2.),
    ];
    outline(&mut model, &points);
    let face = *model.faces.keys().next().unwrap();
    let cursor = camera.project(DVec3::Z * 2., size).unwrap();
    assert_eq!(
        selection::pick_entity(&model, &camera, size, cursor, 8.),
        Some(face)
    );
    let snap = infer(&model, &camera, size, cursor, None, None).unwrap();
    assert_eq!(snap.kind, Inference::OnFace);
    assert!(snap.point.distance(DVec3::Z * 2.) < 1e-6);
    let ray = camera.ray(cursor, size);
    let hidden = ray.plane_z(0.).unwrap();
    model
        .add_edge(hidden - DVec3::X, hidden + DVec3::X)
        .unwrap();
    assert_eq!(
        selection::pick_entity(&model, &camera, size, cursor, 8.),
        Some(face)
    );
}
#[test]
fn legacy_documents_migrate_and_face_deletion_survives_reload() {
    let mut doc = Document::default();
    square(&mut doc.model, 0., 4.);
    doc.snap_to_geometry = false;
    let id = *doc.model.faces.keys().next().unwrap();
    doc.model.erase(&BTreeSet::from([id]));
    let restored = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert!(restored.model.faces.is_empty());
    assert!(!restored.snap_to_geometry);
    let mut json = serde_json::to_value(&doc).unwrap();
    json["version"] = 1.into();
    json.as_object_mut().unwrap().remove("snap_to_geometry");
    json["model"].as_object_mut().unwrap().remove("faces");
    let migrated = Document::from_json(&json.to_string()).unwrap();
    assert_eq!(migrated.version, 2);
    assert!(migrated.snap_to_geometry);
    assert_eq!(migrated.model.faces.len(), 1);
}
#[test]
fn existing_internal_spur_does_not_prevent_closure() {
    let mut model = Model::default();
    model.add_edge(DVec3::ZERO, DVec3::new(1., 1., 0.)).unwrap();
    square(&mut model, 0., 4.);
    assert_eq!(model.faces.len(), 1);
    model.validate().unwrap();
}
#[test]
fn adjacent_coplanar_faces_and_box_faces_remain_distinct() {
    let mut model = Model::default();
    square(&mut model, 0., 4.);
    let adjacent = [
        DVec3::new(4., 0., 0.),
        DVec3::new(8., 0., 0.),
        DVec3::new(8., 4., 0.),
        DVec3::new(4., 4., 0.),
    ];
    for pair in adjacent.windows(2) {
        model.add_edge(pair[0], pair[1]).unwrap();
    }
    assert_eq!(model.faces.len(), 2);
    assert!(model.faces.values().all(|f| f.holes.is_empty()));
    let mut cube = Model::default();
    square(&mut cube, 0., 4.);
    let points = [
        DVec3::new(0., 0., 4.),
        DVec3::new(4., 0., 4.),
        DVec3::new(4., 4., 4.),
        DVec3::new(0., 4., 4.),
    ];
    outline(&mut cube, &points);
    for point in points {
        cube.add_edge(point, point - DVec3::Z * 4.).unwrap();
    }
    assert_eq!(cube.faces.len(), 6);
    cube.validate().unwrap();
}
