use crate::{Axis, Camera, DVec2, DVec3, Inference, Model, infer_move_with_snapping};

fn reference(model: &mut Model, point: DVec3) -> u64 {
    model
        .add_edge(point, point + DVec3::new(0.31, 0.27, 0.19))
        .unwrap()
}

#[test]
fn move_combines_each_pair_of_axes_and_all_three_guides() {
    let target = DVec3::new(3., 4., 2.);
    let camera = Camera::default();
    let size = DVec2::new(1100., 800.);
    for axes in [[Axis::X, Axis::Y], [Axis::X, Axis::Z], [Axis::Y, Axis::Z]] {
        let mut model = Model::default();
        for axis in axes {
            reference(&mut model, target - axis.vector() * 3.);
        }
        let cursor = camera.project(target, size).unwrap() + DVec2::new(2., -2.);
        let inference = infer_move_with_snapping(
            &model,
            &camera,
            size,
            cursor,
            DVec3::new(-4., -3., 2.),
            None,
            true,
            true,
        )
        .unwrap();
        assert_eq!(inference.snap.kind, Inference::Aligned, "{axes:?}");
        assert!(
            inference.snap.point.distance(target) < 1e-7,
            "{axes:?}: {:?}",
            inference.snap.point
        );
        assert!(
            axes.iter()
                .all(|axis| inference.guides.iter().any(|g| g.axis == *axis))
        );
    }
    let mut model = Model::default();
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        reference(&mut model, target - axis.vector() * 3.);
    }
    let inference = infer_move_with_snapping(
        &model,
        &camera,
        size,
        camera.project(target, size).unwrap() + DVec2::new(-2., 1.),
        DVec3::new(-4., -3., 2.),
        None,
        true,
        true,
    )
    .unwrap();
    assert!(inference.snap.point.distance(target) < 1e-7);
    assert_eq!(inference.guides.len(), 3);
}

#[test]
fn move_keeps_axis_locks_and_both_snap_settings() {
    let mut model = Model::default();
    let start = DVec3::new(0., 4., 2.);
    let target = DVec3::new(3., 4., 2.);
    reference(&mut model, DVec3::new(3., 0., 2.));
    let camera = Camera::default();
    let size = DVec2::new(1100., 800.);
    let cursor = camera.project(target, size).unwrap() + DVec2::new(3., 2.);
    let infer = |lock, geometry, axes| {
        infer_move_with_snapping(&model, &camera, size, cursor, start, lock, geometry, axes)
            .unwrap()
    };
    let snapped = infer(Some(Axis::X), true, true);
    assert!(snapped.snap.point.distance(target) < 1e-7);
    assert_eq!(snapped.guides.len(), 2);
    for lock in [Axis::Y, Axis::Z] {
        let snapped = infer(Some(lock), true, true);
        assert!((snapped.snap.point - start).cross(lock.vector()).length() < 1e-7);
    }
    for (geometry, axes) in [(true, false), (false, true), (false, false)] {
        let free = infer(None, geometry, axes);
        assert!(free.guides.is_empty());
        assert!(free.snap.point.distance(target) > 1e-4);
    }
}

#[test]
fn hidden_references_and_skew_projected_guides_do_not_combine() {
    let camera = Camera::default();
    let size = DVec2::new(1100., 800.);
    let target = DVec3::new(3., 4., 2.);
    let mut model = Model::default();
    let a = reference(&mut model, target - DVec3::X * 3.);
    reference(&mut model, target - DVec3::Y * 3.);
    model.hidden.insert(a);
    let snap = infer_move_with_snapping(
        &model,
        &camera,
        size,
        camera.project(target, size).unwrap() + DVec2::new(2., -2.),
        DVec3::new(-4., -3., 2.),
        None,
        true,
        true,
    )
    .unwrap();
    assert!(snap.guides.is_empty());

    let mut model = Model::default();
    let a = DVec3::new(1., 4., 2.);
    let b = DVec3::new(3., 1., 3.);
    reference(&mut model, a);
    reference(&mut model, b);
    let p = camera.project(a, size).unwrap();
    let q = camera.project(b, size).unwrap();
    let u = camera.project(a + DVec3::X, size).unwrap() - p;
    let v = camera.project(b + DVec3::Y, size).unwrap() - q;
    let cursor = p + u * (q - p).perp_dot(v) / u.perp_dot(v);
    let snap = infer_move_with_snapping(
        &model,
        &camera,
        size,
        cursor,
        DVec3::new(-4., -3., 7.),
        None,
        true,
        true,
    )
    .unwrap();
    assert!(
        snap.guides.is_empty(),
        "Screen crossings must also intersect in 3D"
    );
}

#[test]
fn exact_geometry_targets_keep_priority_over_combined_guides() {
    let mut model = Model::default();
    let target = DVec3::new(3., 4., 2.);
    reference(&mut model, target - DVec3::X * 3.);
    reference(&mut model, target - DVec3::Y * 3.);
    let endpoint = target + DVec3::new(0.01, 0.01, 0.);
    reference(&mut model, endpoint);
    let camera = Camera::default();
    let size = DVec2::new(1100., 800.);
    let result = infer_move_with_snapping(
        &model,
        &camera,
        size,
        camera.project(endpoint, size).unwrap(),
        DVec3::new(-4., -3., 2.),
        None,
        true,
        true,
    )
    .unwrap();
    assert_eq!(result.snap.kind, Inference::Endpoint);
    assert!(result.snap.point.distance(endpoint) < 1e-7);
}

#[test]
fn combined_alignment_refines_edges_and_faces_without_leaving_the_surface() {
    let camera = Camera::default();
    let size = DVec2::new(1100., 800.);
    let target = DVec3::new(3., 4., 2.);
    for surface in [false, true] {
        let mut model = Model::default();
        reference(&mut model, target - DVec3::X * 3.);
        reference(&mut model, target - DVec3::Y * 3.);
        if surface {
            let points = [
                DVec3::new(1., 2., 2.),
                DVec3::new(5., 2., 2.),
                DVec3::new(5., 6., 2.),
                DVec3::new(1., 6., 2.),
            ];
            for i in 0..4 {
                model.add_edge(points[i], points[(i + 1) % 4]).unwrap();
            }
        } else {
            model
                .add_edge(target - DVec3::X * 0.7, target + DVec3::X * 1.4)
                .unwrap();
        }
        let inference = infer_move_with_snapping(
            &model,
            &camera,
            size,
            camera.project(target, size).unwrap() + DVec2::new(2., -2.),
            DVec3::new(-4., -3., 2.),
            None,
            true,
            true,
        )
        .unwrap();
        assert_eq!(inference.snap.kind, Inference::Aligned);
        assert!(inference.snap.point.distance(target) < 1e-7);
    }
}

#[test]
fn prepared_move_snaps_match_uncached_queries_across_camera_settings_and_contexts() {
    use crate::inference::MoveSnapCache;
    let mut model = Model::default();
    let points = [
        DVec3::new(-2., -2., 0.),
        DVec3::new(2., -2., 0.),
        DVec3::new(2., 2., 0.),
        DVec3::new(-2., 2., 0.),
    ];
    for i in 0..4 {
        model.add_edge(points[i], points[(i + 1) % 4]).unwrap();
    }
    let hole = [
        DVec3::new(-1., -1., 0.),
        DVec3::new(1., -1., 0.),
        DVec3::new(1., 1., 0.),
        DVec3::new(-1., 1., 0.),
    ];
    for i in 0..4 {
        model.add_edge(hole[i], hole[(i + 1) % 4]).unwrap();
    }
    let inner = *model.faces.keys().next_back().unwrap();
    model.erase(&std::collections::BTreeSet::from([inner]));
    let group = model
        .make_object(&model.root_entities(), false, "Scoped face")
        .unwrap();
    reference(&mut model, DVec3::new(3., 4., 2.));
    let hidden = reference(&mut model, DVec3::new(0., 4., 2.));
    model.hidden.insert(hidden);
    let outside = model.clone();
    for scoped in [false, true] {
        model = outside.clone();
        if scoped {
            assert!(model.enter_object(group));
        }
        let mut cache = MoveSnapCache::new(&model);
        for size in [DVec2::new(1100., 800.), DVec2::new(800., 600.)] {
            for yaw in [-0.9, 0.3] {
                let camera = Camera {
                    yaw,
                    ..Camera::default()
                };
                for geometry in [true, false] {
                    for axes in [true, false] {
                        for lock in [None, Some(Axis::X), Some(Axis::Z)] {
                            for point in points
                                .into_iter()
                                .chain([DVec3::new(3., 4., 2.), DVec3::new(0.5, 0.5, 0.)])
                            {
                                let cursor =
                                    camera.project(point, size).unwrap() + DVec2::new(2., -1.);
                                let start = DVec3::new(-4., -3., 2.);
                                let expected = infer_move_with_snapping(
                                    &model, &camera, size, cursor, start, lock, geometry, axes,
                                );
                                let actual =
                                    cache.infer(&camera, size, cursor, start, lock, geometry, axes);
                                assert_eq!(actual.is_some(), expected.is_some());
                                if let (Some(actual), Some(expected)) = (actual, expected) {
                                    assert_eq!(actual.snap.kind, expected.snap.kind);
                                    assert!(actual.snap.point.distance(expected.snap.point) < 1e-7);
                                    assert_eq!(actual.guides.len(), expected.guides.len());
                                    for (a, b) in actual.guides.iter().zip(&expected.guides) {
                                        assert_eq!(a.axis, b.axis);
                                        assert!(a.origin.distance(b.origin) < 1e-7);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
