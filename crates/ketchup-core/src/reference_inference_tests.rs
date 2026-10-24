use crate::{Axis, Camera, DVec2, DVec3, Inference, inference::infer_reference_axis};

#[test]
fn references_project_onto_each_world_axis() {
    let camera = Camera::default();
    let size = DVec2::new(1100., 800.);
    let reference = DVec3::new(2., 3., 2.);
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        let target = reference + axis.vector() * 3.;
        let cursor = camera.project(target, size).unwrap() + DVec2::new(1., -1.);
        let (snap, guide) =
            infer_reference_axis(&camera, size, cursor, reference, None, None).unwrap();
        assert_eq!(snap.kind, Inference::Axis(axis));
        assert_eq!(guide.origin, reference);
        assert_eq!(guide.axis, axis);
        assert!((snap.point - reference).cross(axis.vector()).length() < 1e-7);
        assert!(camera.project(snap.point, size).unwrap().distance(cursor) <= 10.);
    }
}

#[test]
fn reference_guides_intersect_explicit_locks_in_world_space() {
    let camera = Camera::default();
    let size = DVec2::new(1100., 800.);
    let reference = DVec3::new(3., 0., 2.);
    let start = DVec3::new(0., 4., 2.);
    let target = DVec3::new(3., 4., 2.);
    let cursor = camera.project(target, size).unwrap() + DVec2::new(1., -1.);
    let (snap, guide) = infer_reference_axis(
        &camera,
        size,
        cursor,
        reference,
        Some((start, Axis::X)),
        None,
    )
    .unwrap();
    assert!(snap.point.distance(target) < 1e-7);
    assert_eq!(guide.axis, Axis::Y);
    assert!(
        infer_reference_axis(
            &camera,
            size,
            cursor,
            reference + DVec3::Z,
            Some((start, Axis::X)),
            None
        )
        .is_none()
    );
}

#[test]
fn reference_axes_keep_horizontal_and_vertical_drawing_planes() {
    let camera = Camera::default();
    let size = DVec2::new(1100., 800.);
    let reference = DVec3::new(3., 4., 2.);
    for (normal, target, axis) in [
        (DVec3::Z, DVec3::new(3., 4., 0.), Axis::Z),
        (DVec3::Y, DVec3::new(3., 0., 2.), Axis::Y),
    ] {
        let cursor = camera.project(target, size).unwrap() + DVec2::new(1., -1.);
        let (snap, guide) = infer_reference_axis(
            &camera,
            size,
            cursor,
            reference,
            None,
            Some((DVec3::ZERO, normal)),
        )
        .unwrap();
        assert!(snap.point.distance(target) < 1e-7);
        assert_eq!(guide.axis, axis);
        assert!(snap.point.dot(normal).abs() < 1e-7);
        assert!(
            infer_reference_axis(
                &camera,
                size,
                cursor,
                reference,
                Some((DVec3::new(0., 4., 1.), Axis::X)),
                Some((DVec3::ZERO, normal))
            )
            .is_none()
        );
    }
}
