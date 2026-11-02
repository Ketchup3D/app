use crate::{
    Camera, DVec2, DVec3, Model,
    inference::{Axis, Inference, SnapTargets, infer_with_targets},
};

fn outline(model: &mut Model, points: &[DVec3]) -> u64 {
    let before: std::collections::BTreeSet<_> = model.faces.keys().copied().collect();
    for i in 0..points.len() {
        model
            .add_edge(points[i], points[(i + 1) % points.len()])
            .unwrap();
    }
    *model.faces.keys().find(|id| !before.contains(id)).unwrap()
}
#[test]
fn face_center_target_respects_its_toggle_master_visibility_and_axis_lock() {
    let mut model = Model::default();
    let id = outline(
        &mut model,
        &[
            DVec3::ZERO,
            DVec3::new(4., 0., 0.),
            DVec3::new(4., 4., 0.),
            DVec3::new(0., 4., 0.),
        ],
    );
    let center = DVec3::new(2., 2., 0.);
    let camera = Camera::default();
    let size = DVec2::new(1000., 800.);
    let cursor = camera.project(center, size).unwrap();
    let query = |model: &Model, enabled, geometry, start, lock| {
        infer_with_targets(
            model,
            &camera,
            size,
            cursor,
            start,
            lock,
            geometry,
            false,
            None,
            SnapTargets {
                edges: false,
                faces: false,
                face_centers: enabled,
            },
        )
    };
    assert_eq!(model.face_center(&model.faces[&id]), Some(center));
    let snap = query(&model, true, true, None, None).unwrap();
    assert_eq!(snap.kind, Inference::FaceCenter);
    assert_eq!(snap.point, center);
    assert_ne!(
        query(&model, false, true, None, None).unwrap().kind,
        Inference::FaceCenter
    );
    assert_ne!(
        query(&model, true, false, None, None).unwrap().kind,
        Inference::FaceCenter
    );
    assert_ne!(
        query(&model, true, true, Some(DVec3::ZERO), Some(Axis::X))
            .unwrap()
            .kind,
        Inference::FaceCenter
    );
    model.hidden.insert(id);
    assert_ne!(
        query(&model, true, true, None, None).unwrap().kind,
        Inference::FaceCenter
    );
}
#[test]
fn triangle_center_is_area_based_and_hole_centers_are_not_snap_targets() {
    let mut model = Model::default();
    let triangle = outline(
        &mut model,
        &[
            DVec3::new(1., 1., 0.),
            DVec3::new(7., 1., 0.),
            DVec3::new(1., 4., 0.),
        ],
    );
    assert!(
        (model.face_center(&model.faces[&triangle]).unwrap() - DVec3::new(3., 2., 0.)).length()
            < 1e-7
    );
    let mut hole = Model::default();
    let outer = outline(
        &mut hole,
        &[
            DVec3::ZERO,
            DVec3::new(4., 0., 0.),
            DVec3::new(4., 4., 0.),
            DVec3::new(0., 4., 0.),
        ],
    );
    outline(
        &mut hole,
        &[
            DVec3::new(1., 1., 0.),
            DVec3::new(3., 1., 0.),
            DVec3::new(3., 3., 0.),
            DVec3::new(1., 3., 0.),
        ],
    );
    assert!(!hole.faces[&outer].holes.is_empty());
    assert!(hole.face_center(&hole.faces[&outer]).is_none());
}
