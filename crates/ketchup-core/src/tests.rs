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
fn crossing_and_window_selection_differ() {
    let size = DVec2::new(1000., 700.);
    let c = Camera::default();
    let mut m = Model::default();
    m.add_edge(-DVec3::X * 3., DVec3::X * 3.).unwrap();
    let center = c.project(DVec3::ZERO, size).unwrap();
    let a = center - DVec2::splat(10.);
    let b = center + DVec2::splat(10.);
    assert!(selection::box_select(&m, &c, size, a, b).is_empty());
    assert_eq!(
        selection::box_select(&m, &c, size, DVec2::new(b.x, a.y), DVec2::new(a.x, b.y)).len(),
        1
    );
    assert_eq!(
        selection::pick(&m, &c, size, center, 8.),
        m.edges.keys().next().copied()
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
    doc.version = 2;
    assert!(Document::from_json(&serde_json::to_string(&doc).unwrap()).is_err());
    doc.version = 1;
    doc.model.vertices.clear();
    assert!(Document::from_json(&serde_json::to_string(&doc).unwrap()).is_err());
}
