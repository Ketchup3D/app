use super::*;

#[test]
fn excluded_faces_do_not_hide_other_snap_targets() {
    use ketchup_core::inference::FaceQuery;

    let mut h = Harness::new();
    let target_face = modeling_rectangle(&mut h);
    let target = DVec3::new(1.1, 1.3, 0.);
    let shift = (h.app.doc.camera.eye() - target).normalize();
    let points = [
        DVec3::ZERO,
        DVec3::X * 4.,
        DVec3::new(4., 4., 0.),
        DVec3::Y * 4.,
    ];
    for i in 0..4 {
        h.app
            .doc
            .model
            .add_edge(points[i] + shift, points[(i + 1) % 4] + shift)
            .unwrap();
    }
    let front_face = *h
        .app
        .doc
        .model
        .faces
        .keys()
        .find(|id| **id != target_face)
        .unwrap();
    h.app.prepare_picking();
    let ray = h.app.doc.camera.ray(
        dv(h.at(target) - h.app.viewport.min),
        dv(h.app.viewport.size()),
    );
    let query = h
        .app
        .picking_cache
        .snap_query(&h.app.face_cache, &h.app.entity_cache);
    assert_eq!(query.nearest(ray, false).unwrap().0, front_face);
    let excluded = BTreeSet::from([front_face]);
    let query = query.excluding(Some(&excluded));
    assert!(query.is_hidden(&h.app.doc.model, front_face));
    let (id, _, point) = query.nearest(ray, false).unwrap();
    assert_eq!(id, target_face);
    assert!(point.distance(target) < 1e-6);
}

#[test]
fn move_and_scale_ignore_their_own_surfaces_but_keep_other_targets() {
    for tool in [Tool::Move, Tool::Scale] {
        for grouped in [false, true] {
            let mut h = Harness::new();
            let face = modeling_rectangle(&mut h);
            let selected = if grouped {
                let inner = h
                    .app
                    .doc
                    .model
                    .make_object(&h.app.doc.model.selectable_entities(), false, "Inner")
                    .unwrap();
                h.app
                    .doc
                    .model
                    .make_object(&BTreeSet::from([inner]), false, "Outer")
                    .unwrap()
            } else {
                face
            };
            h.app
                .doc
                .model
                .add_edge(DVec3::new(5., 0., 2.), DVec3::new(5., 4., 2.))
                .unwrap();
            let points = [
                DVec3::new(7., 0., 3.),
                DVec3::new(11., 0., 3.),
                DVec3::new(11., 4., 3.),
                DVec3::new(7., 4., 3.),
            ];
            for i in 0..4 {
                h.app
                    .doc
                    .model
                    .add_edge(points[i], points[(i + 1) % 4])
                    .unwrap();
            }
            h.app.selected = BTreeSet::from([selected]);
            h.app.set_tool(tool);
            h.app.doc.snap_to_axes = false;
            h.app.begin_modeling(h.at(DVec3::new(2., 2., 0.)), &h.ctx);
            assert!(h.app.modeling.is_some());
            for target in [DVec3::new(1.1, 0., 0.), DVec3::new(1.1, 1.3, 0.)] {
                h.app.update_modeling(h.at(target));
                assert_eq!(
                    h.app.modeling.as_ref().unwrap().snap.unwrap().kind,
                    Inference::Plane,
                    "{tool:?}, grouped={grouped}, {target:?}"
                );
            }
            for (target, kind) in [
                (DVec3::new(5., 1.1, 2.), Inference::OnEdge),
                (DVec3::new(8.1, 1.3, 3.), Inference::OnFace),
            ] {
                h.app.update_modeling(h.at(target));
                let snap = h.app.modeling.as_ref().unwrap().snap.unwrap();
                assert_eq!(snap.kind, kind, "{tool:?}, grouped={grouped}");
                assert!(snap.point.distance(target) < 1e-6);
            }
        }
    }
}

#[test]
fn moving_an_edge_ignores_connected_faces_that_change_with_it() {
    let mut h = Harness::new();
    modeling_rectangle(&mut h);
    let edge = h
        .app
        .doc
        .model
        .edges
        .values()
        .find(|edge| {
            let (a, b) = h.app.doc.model.points(edge);
            a.y == 0. && b.y == 0.
        })
        .unwrap()
        .id;
    h.app.selected = BTreeSet::from([edge]);
    h.app.set_tool(Tool::Move);
    h.app.doc.snap_to_axes = false;
    h.app.begin_modeling(h.at(DVec3::new(2., 0., 0.)), &h.ctx);
    for target in [DVec3::new(1.1, 0., 0.), DVec3::new(1.1, 1.3, 0.)] {
        h.app.update_modeling(h.at(target));
        assert_eq!(
            h.app.modeling.as_ref().unwrap().snap.unwrap().kind,
            Inference::Plane
        );
    }
}

#[test]
fn push_pull_does_not_acquire_its_starting_face_or_boundary_edges() {
    let mut h = Harness::new();
    modeling_rectangle(&mut h);
    h.app.set_tool(Tool::PushPull);
    h.app.doc.snap_to_axes = false;
    h.app.begin_modeling(h.at(DVec3::new(2., 2., 0.)), &h.ctx);
    assert!(h.app.modeling.is_some());
    for target in [DVec3::new(1.1, 0., 0.), DVec3::new(1.1, 1.3, 0.)] {
        h.app.update_modeling(h.at(target));
        assert!(h.app.modeling.as_ref().unwrap().snap.is_none());
    }
}
