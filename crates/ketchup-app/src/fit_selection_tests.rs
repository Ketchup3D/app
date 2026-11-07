use super::*;

#[test]
fn fit_selection_frames_nested_geometry_and_annotations_without_unselected_entities() {
    let mut h = Harness::new();
    let model = &mut h.app.doc.model;
    let edge = model.add_edge(DVec3::ZERO, DVec3::X * 4.).unwrap();
    let inner = model
        .make_object(&BTreeSet::from([edge]), false, "Inner")
        .unwrap();
    let outer = model
        .make_object(&BTreeSet::from([inner]), false, "Outer")
        .unwrap();
    let dimension = model
        .add_dimension(DVec3::ZERO, DVec3::X * 4., DVec3::Y * 3.)
        .unwrap();
    let text = model
        .add_text(DVec3::ZERO, DVec3::Z * 2., "Selected note")
        .unwrap();
    model
        .add_edge(DVec3::splat(100.), DVec3::splat(110.))
        .unwrap();
    let before = model.clone();

    for projection in [Projection::Perspective, Projection::Parallel] {
        h.app.doc.camera.projection = projection;
        let yaw = h.app.doc.camera.yaw;
        let pitch = h.app.doc.camera.pitch;
        let fov = h.app.doc.camera.fov;
        for (selected, hi) in [
            (BTreeSet::from([outer]), DVec3::new(4., 0., 0.)),
            (BTreeSet::from([dimension]), DVec3::new(4., 3., 0.)),
            (BTreeSet::from([text]), DVec3::new(0., 0., 2.)),
            (
                BTreeSet::from([outer, dimension, text]),
                DVec3::new(4., 3., 2.),
            ),
        ] {
            h.app.selected = selected;
            h.app.fit(&h.ctx);
            let model_distance = h.app.doc.camera.distance;
            h.app.fit_selection(&h.ctx);
            assert_eq!(h.app.doc.camera.target, hi * 0.5);
            assert!(h.app.doc.camera.distance < model_distance);
            assert_eq!(h.app.doc.camera.projection, projection);
            assert_eq!(h.app.doc.camera.yaw, yaw);
            assert_eq!(h.app.doc.camera.pitch, pitch);
            assert_eq!(h.app.doc.camera.fov, fov);
            assert_eq!(h.app.doc.model, before);
            assert!(h.app.project.modified);
        }
    }
}

#[test]
fn fit_selection_without_geometry_leaves_the_camera_and_project_unchanged() {
    let mut h = Harness::new();
    let before = h.app.doc.camera.clone();
    for selected in [BTreeSet::new(), BTreeSet::from([u64::MAX])] {
        h.app.selected = selected;
        h.app.fit_selection(&h.ctx);
        assert_eq!(h.app.doc.camera, before);
        assert!(!h.app.project.modified);
    }
}
