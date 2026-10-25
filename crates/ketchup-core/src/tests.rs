use super::*;
use std::collections::BTreeSet;

fn close(a: DVec3, b: DVec3) {
    assert!(a.distance(b) < 1e-6, "{a:?} != {b:?}");
}

#[test]
fn rejects_degenerate_and_duplicate_edges_without_mutating() {
    let mut m = Model::default();
    assert!(m.add_edge(DVec3::ZERO, DVec3::ZERO).is_err());
    assert!(m.vertices.is_empty());
    m.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    let before = m.clone();
    assert!(m.add_edge(DVec3::X, DVec3::ZERO).is_err());
    assert!(m.add_edge(DVec3::NAN, DVec3::X).is_err());
    assert_eq!(m, before);
}

#[test]
fn drawing_past_coordinate_limits_is_atomic_and_keeps_the_document_reloadable() {
    for context in [false, true] {
        let mut doc = Document::default();
        let limit = DVec3::X * 1e10;
        let edge = doc.model.add_edge(limit - DVec3::X, limit).unwrap();
        if context {
            let group = doc
                .model
                .make_object(&BTreeSet::from([edge]), false, "Limit")
                .unwrap();
            assert!(doc.model.enter_object(group));
        }
        let before = doc.model.clone();
        for (a, b) in [
            (limit, limit + DVec3::X),
            (-limit - DVec3::X, -limit),
            (DVec3::ZERO, DVec3::splat(f64::MAX)),
            (DVec3::INFINITY, DVec3::ZERO),
        ] {
            assert!(doc.model.add_edge(a, b).is_err(), "accepted {a:?} to {b:?}");
            assert_eq!(doc.model, before);
        }
        Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
    }
}

#[test]
fn midpoint_attachment_splits_edge_and_reuses_vertex() {
    let mut m = Model::default();
    m.add_edge(DVec3::ZERO, DVec3::X * 2.).unwrap();
    m.add_edge(DVec3::X, DVec3::X + DVec3::Z).unwrap();
    assert_eq!(m.edges.len(), 3);
    assert_eq!(m.vertices.len(), 4);
    let mid = m
        .vertices
        .values()
        .find(|v| v.position == DVec3::X)
        .unwrap()
        .id;
    assert_eq!(
        m.edges
            .values()
            .filter(|e| e.a == mid || e.b == mid)
            .count(),
        3
    );
    m.validate().unwrap();
}

#[test]
fn crossings_do_not_implicitly_split_edges() {
    let mut m = Model::default();
    m.add_edge(-DVec3::X, DVec3::X).unwrap();
    m.add_edge(-DVec3::Y, DVec3::Y).unwrap();
    assert_eq!(m.edges.len(), 2);
    assert_eq!(m.vertices.len(), 4);
}

#[test]
fn erase_cleans_orphans_and_batch_undo_restores_topology() {
    let mut m = Model::default();
    let a = m.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    let b = m.add_edge(DVec3::X, DVec3::Y).unwrap();
    let before = m.clone();
    let mut history = History::default();
    m.erase(&BTreeSet::from([a, b]));
    history.record("Erase edges", before.clone(), &m);
    assert!(m.vertices.is_empty());
    assert!(history.undo(&mut m));
    assert_eq!(m, before);
    assert!(history.redo(&mut m));
    assert!(m.edges.is_empty());
    assert!(!history.redo(&mut m));
}

#[test]
fn new_edit_clears_redo_and_noop_preserves_history() {
    let mut m = Model::default();
    let mut h = History::default();
    let before = m.clone();
    m.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    h.record("Line", before, &m);
    h.undo(&mut m);
    assert!(h.redo_label().is_some());
    h.record("Noop", m.clone(), &m);
    assert!(h.redo_label().is_some());
    let before = m.clone();
    m.add_edge(DVec3::ZERO, DVec3::Z).unwrap();
    h.record("Line", before, &m);
    assert!(h.redo_label().is_none());
}

#[test]
fn history_keeps_only_one_hundred_steps() {
    let mut m = Model::default();
    let mut h = History::default();
    for i in 0..105 {
        let before = m.clone();
        m.add_edge(DVec3::new(i as f64, 0., 0.), DVec3::new(i as f64, 1., 0.))
            .unwrap();
        h.record("Line", before, &m);
    }
    let mut count = 0;
    while h.undo(&mut m) {
        count += 1;
    }
    assert_eq!(count, 100);
    assert_eq!(m.edges.len(), 5);
}

#[test]
fn camera_ray_round_trip_and_vertical_constraint() {
    let camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let point = DVec3::new(2., 1., 0.);
    let pixel = camera.project(point, size).unwrap();
    close(camera.ray(pixel, size).plane_z(0.).unwrap(), point);
    let vertical = DVec3::new(2., 1., 4.);
    let ray = camera.ray(camera.project(vertical, size).unwrap(), size);
    close(ray.axis(point, DVec3::Z).unwrap(), vertical);
}

#[test]
fn endpoint_midpoint_and_axis_inference() {
    let camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let mut model = Model::default();
    model.add_edge(DVec3::ZERO, DVec3::X * 4.).unwrap();
    for (point, kind) in [
        (DVec3::X * 4., Inference::Endpoint),
        (DVec3::X * 2., Inference::Midpoint),
    ] {
        let snap = infer(
            &model,
            &camera,
            size,
            camera.project(point, size).unwrap() + DVec2::splat(2.),
            None,
            None,
        )
        .unwrap();
        close(snap.point, point);
        assert_eq!(snap.kind, kind);
    }
    let start = DVec3::new(1., 1., 0.);
    let point = start + DVec3::Z * 3.;
    let snap = infer(
        &model,
        &camera,
        size,
        camera.project(point, size).unwrap(),
        Some(start),
        Some(Axis::Z),
    )
    .unwrap();
    close(snap.point, point);
}

#[test]
fn explicit_axis_lock_ignores_off_axis_endpoint() {
    let camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let mut model = Model::default();
    model.add_edge(DVec3::Y, DVec3::Y + DVec3::X).unwrap();
    let snap = infer(
        &model,
        &camera,
        size,
        camera.project(DVec3::Y, size).unwrap(),
        Some(DVec3::ZERO),
        Some(Axis::X),
    )
    .unwrap();
    assert!(snap.point.y.abs() < 1e-8);
    assert!(snap.point.z.abs() < 1e-8);
}

#[test]
fn pan_translates_view_without_rotating_and_zoom_keeps_anchor() {
    let mut camera = Camera::default();
    let size = DVec2::new(1000., 700.);
    let eye_offset = camera.eye() - camera.target;
    let target = camera.target;
    camera.pan(DVec2::new(100., 50.), size.y);
    close(camera.eye() - camera.target, eye_offset);
    assert_ne!(camera.target, target);
    let cursor = DVec2::new(680., 470.);
    let anchor = camera.ray(cursor, size).plane_z(camera.target.z).unwrap();
    let screen_before = camera.project(anchor, size).unwrap();
    camera.zoom(100., cursor, size);
    assert!(
        camera
            .project(anchor, size)
            .unwrap()
            .distance(screen_before)
            < 1e-6
    );
}

#[test]
fn orbit_around_model_center_preserves_framing_and_radius() {
    let size = DVec2::new(1000., 700.);
    let pivot = DVec3::new(3., 4., 2.);
    for pitch in [-1.49, 0.62, 1.49] {
        let mut camera = Camera {
            target: pivot,
            pitch,
            ..Camera::default()
        };
        camera.pan(DVec2::new(140., -80.), size.y);
        camera.zoom(180., size * 0.7, size);
        let before = camera.clone();
        let screen = camera.project(pivot, size).unwrap();
        let radius = camera.eye().distance(pivot);
        assert!(screen.distance(size * 0.5) > 1.);
        camera.orbit_around(DVec2::ZERO, pivot);
        close(camera.eye(), before.eye());
        close(camera.target, before.target);
        for delta in [
            DVec2::new(0.01, 0.),
            DVec2::new(40., -100.),
            DVec2::new(-70., 500.),
        ] {
            camera.orbit_around(delta, pivot);
            assert!(camera.project(pivot, size).unwrap().distance(screen) < 1e-6);
            assert!((camera.eye().distance(pivot) - radius).abs() < 1e-6);
            assert_eq!(camera.distance, before.distance);
            assert!(camera.valid());
        }
    }
}

fn assert_box_selection(model: &Model, lo: DVec2, hi: DVec2, expected: BTreeSet<u64>) {
    for (start, end) in [
        (lo, hi),
        (hi, lo),
        (DVec2::new(hi.x, lo.y), DVec2::new(lo.x, hi.y)),
        (DVec2::new(lo.x, hi.y), DVec2::new(hi.x, lo.y)),
    ] {
        assert_eq!(
            selection::box_select(
                model,
                &Camera::default(),
                DVec2::new(1000., 700.),
                start,
                end,
            ),
            expected,
            "drag from {start:?} to {end:?}"
        );
    }
}

#[test]
fn box_selection_requires_complete_edges_in_every_drag_direction() {
    let size = DVec2::new(1000., 700.);
    let c = Camera::default();
    let mut m = Model::default();
    let edge = m.add_edge(-DVec3::X * 3., DVec3::X * 3.).unwrap();
    let center = c.project(DVec3::ZERO, size).unwrap();
    let a = center - DVec2::splat(10.);
    let b = center + DVec2::splat(10.);
    assert_box_selection(&m, a, b, BTreeSet::new());
    let a = c.project(-DVec3::X * 3., size).unwrap();
    let b = c.project(DVec3::X * 3., size).unwrap();
    assert_box_selection(&m, a.min(b), a.max(b), BTreeSet::from([edge]));
    assert_box_selection(
        &m,
        center.min(a) - DVec2::splat(1.),
        center.max(a) + DVec2::splat(1.),
        BTreeSet::new(),
    );
    assert_eq!(
        selection::pick(&m, &c, size, center, 8.),
        m.edges.keys().next().copied()
    );
}

#[test]
fn box_selection_requires_complete_faces_in_every_drag_direction() {
    let mut model = Model::default();
    let points = [-DVec3::X, -DVec3::Y, DVec3::X, DVec3::Y];
    for i in 0..4 {
        model.add_edge(points[i], points[(i + 1) % 4]).unwrap();
    }
    assert_eq!(model.faces.len(), 1);
    let center = DVec2::new(500., 350.);
    assert_box_selection(
        &model,
        center - DVec2::splat(5.),
        center + DVec2::splat(5.),
        BTreeSet::new(),
    );
    assert_box_selection(
        &model,
        DVec2::ZERO,
        DVec2::new(1000., 700.),
        model.selectable_entities(),
    );
}

#[test]
fn box_selection_requires_all_visible_descendants_of_groups_and_components() {
    for component in [false, true] {
        let mut model = Model::default();
        let near = model.add_edge(-DVec3::X, DVec3::X).unwrap();
        let far = model
            .add_edge(DVec3::Y * 5., DVec3::Y * 5. + DVec3::X)
            .unwrap();
        let inner = model
            .make_object(&BTreeSet::from([near, far]), component, "Inner")
            .unwrap();
        let outer = model
            .make_object(&BTreeSet::from([inner]), false, "Outer")
            .unwrap();
        // Grouping copies geometry and assigns new IDs at each level.
        let inner = *model.objects[&outer].entities.first().unwrap();
        let far = model
            .edges
            .values()
            .find(|edge| model.points(edge).0.y > 4.)
            .unwrap()
            .id;
        let lo = DVec2::new(440., 320.);
        let hi = DVec2::new(560., 380.);
        assert_box_selection(&model, lo, hi, BTreeSet::new());
        assert_box_selection(
            &model,
            DVec2::ZERO,
            DVec2::new(1000., 700.),
            BTreeSet::from([outer]),
        );
        model.hidden.insert(far);
        assert_box_selection(&model, lo, hi, BTreeSet::from([outer]));
        assert!(model.enter_object(outer));
        assert_box_selection(&model, lo, hi, BTreeSet::from([inner]));
        model.hidden.remove(&far);
        assert_box_selection(&model, lo, hi, BTreeSet::new());
    }
}

#[test]
fn box_selection_requires_complete_dimensions_in_every_drag_direction() {
    let mut model = Model::default();
    let dimension = model
        .add_dimension(-DVec3::X * 3., DVec3::X * 3., DVec3::Y)
        .unwrap();
    let center = Camera::default()
        .project(DVec3::Y, DVec2::new(1000., 700.))
        .unwrap();
    assert_box_selection(
        &model,
        center - DVec2::splat(5.),
        center + DVec2::splat(5.),
        BTreeSet::new(),
    );
    assert_box_selection(
        &model,
        DVec2::ZERO,
        DVec2::new(1000., 700.),
        BTreeSet::from([dimension]),
    );
}

#[test]
fn fast_erase_stroke_detects_crossed_edge() {
    assert_eq!(
        selection::segment_distance(
            DVec2::new(-10., 0.),
            DVec2::new(10., 0.),
            DVec2::new(0., -20.),
            DVec2::new(0., 20.)
        ),
        0.
    );
    assert_eq!(
        selection::segment_distance(DVec2::ZERO, DVec2::X, DVec2::Y * 5., DVec2::new(1., 5.)),
        5.
    );
}

#[test]
fn document_round_trip_and_corruption_validation() {
    let mut doc = Document::default();
    doc.model.add_edge(DVec3::ZERO, DVec3::Z).unwrap();
    let json = serde_json::to_string(&doc).unwrap();
    let restored = Document::from_json(&json).unwrap();
    assert_eq!(restored.model, doc.model);
    assert!(Document::from_json("{}").is_err());
    doc.version = 99;
    assert!(Document::from_json(&serde_json::to_string(&doc).unwrap()).is_err());
    doc.version = 1;
    doc.model.vertices.clear();
    assert!(Document::from_json(&serde_json::to_string(&doc).unwrap()).is_err());
}
