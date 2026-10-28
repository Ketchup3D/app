use super::*;

const PLANE_LOCKS: [(Axis, Key); 3] = [
    (Axis::X, Key::ArrowRight),
    (Axis::Y, Key::ArrowLeft),
    (Axis::Z, Key::ArrowUp),
];

#[test]
fn shape_plane_locks_before_and_after_the_first_click_commit_the_preview() {
    for tool in [Tool::Rectangle, Tool::Circle, Tool::Polygon] {
        for (axis, key) in PLANE_LOCKS {
            for lock_first in [false, true] {
                for snapping in [false, true] {
                    let mut h = Harness::new();
                    h.app.doc.snap_to_geometry = snapping;
                    h.app.doc.snap_to_axes = snapping;
                    h.app.set_tool(tool);
                    if lock_first {
                        h.key(key);
                    }
                    h.click(h.at(DVec3::ZERO));
                    if !lock_first {
                        h.key(key);
                    }
                    let drawing = h.app.drawing.as_ref().unwrap();
                    let origin = drawing.origin;
                    let target = origin + drawing.u * 2. + drawing.v * 1.3;
                    assert_eq!(drawing.normal, axis.vector());
                    hover(&mut h, target);
                    assert!(h.app.snap.unwrap().point.distance(target) < 1e-5);
                    let preview = h.app.drawing_points();
                    assert!(preview.len() >= 5);
                    assert!(
                        preview
                            .iter()
                            .all(|p| { (*p - origin).dot(axis.vector()).abs() < 1e-7 })
                    );
                    let before = h.app.doc.model.clone();
                    h.click(h.at(target));
                    assert!(h.app.drawing.is_none(), "{tool:?} {axis:?}");
                    assert_eq!(h.app.doc.model.faces.len(), 1);
                    assert!(preview.iter().all(|p| {
                        h.app
                            .doc
                            .model
                            .vertices
                            .values()
                            .any(|v| v.position.distance(*p) < 1e-7)
                    }));
                    h.app.doc.model.validate().unwrap();
                    h.app.undo(&h.ctx, false);
                    assert_eq!(h.app.doc.model, before);
                }
            }
        }
    }
}

#[test]
fn locked_shape_planes_keep_typed_dimensions() {
    for tool in [Tool::Rectangle, Tool::Circle, Tool::Polygon] {
        for (axis, key) in PLANE_LOCKS {
            let mut h = Harness::new();
            h.app.set_tool(tool);
            h.click(h.at(DVec3::ZERO));
            h.key(key);
            let drawing = h.app.drawing.as_ref().unwrap();
            let origin = drawing.origin;
            let target = origin + drawing.u * 3. + drawing.v * 2.;
            hover(&mut h, target);
            h.frame(
                vec![egui::Event::Text(if tool == Tool::Rectangle {
                    "2m,50cm".into()
                } else {
                    "2m".into()
                })],
                egui::Modifiers::NONE,
            );
            let preview = h.app.drawing_points();
            assert!(
                preview
                    .iter()
                    .all(|p| (*p - origin).dot(axis.vector()).abs() < 1e-7)
            );
            if tool == Tool::Rectangle {
                assert!((preview[0].distance(preview[1]) - 2.).abs() < 1e-7);
                assert!((preview[1].distance(preview[2]) - 0.5).abs() < 1e-7);
            } else {
                assert!(
                    preview
                        .iter()
                        .all(|p| (p.distance(origin) - 2.).abs() < 1e-7)
                );
            }
            h.key(Key::Enter);
            assert!(h.app.drawing.is_none());
            assert_eq!(h.app.doc.model.faces.len(), 1);
            assert!(preview.iter().all(|p| {
                h.app
                    .doc
                    .model
                    .vertices
                    .values()
                    .any(|v| v.position.distance(*p) < 1e-7)
            }));
            h.app.doc.model.validate().unwrap();
        }
    }
}

#[test]
fn shape_plane_locks_switch_and_restore_the_original_surface_plane() {
    for tool in [Tool::Rectangle, Tool::Circle, Tool::Polygon] {
        let mut h = Harness::new();
        let corners = [
            DVec3::new(-4., -4., -1.),
            DVec3::new(4., -4., -1.),
            DVec3::new(4., 4., 3.),
            DVec3::new(-4., 4., 3.),
        ];
        for i in 0..4 {
            h.app
                .doc
                .model
                .add_edge(corners[i], corners[(i + 1) % 4])
                .unwrap();
        }
        let before = h.app.doc.model.clone();
        h.app.set_tool(tool);
        h.click(h.at(DVec3::Z));
        let drawing = h.app.drawing.as_ref().unwrap();
        let (origin, normal) = (drawing.origin, drawing.normal);
        assert!(normal.cross(DVec3::Z).length() > 0.1);
        for (axis, key) in PLANE_LOCKS {
            h.key(key);
            assert_eq!(h.app.lock, Some(axis));
            assert_eq!(h.app.drawing.as_ref().unwrap().normal, axis.vector());
            // Off-plane geometry must not pull a locked shape off its plane.
            hover(&mut h, corners[2]);
            assert!(
                (h.app.snap.unwrap().point - origin)
                    .dot(axis.vector())
                    .abs()
                    < 1e-7
            );
            h.key(key);
            assert_eq!(h.app.lock, None);
            assert_eq!(h.app.drawing.as_ref().unwrap().normal, normal);
        }
        h.key(Key::ArrowRight);
        h.key(Key::ArrowLeft);
        assert_eq!(h.app.drawing.as_ref().unwrap().normal, DVec3::Y);
        h.key(Key::ArrowDown);
        assert_eq!(h.app.lock, None);
        assert_eq!(h.app.drawing.as_ref().unwrap().normal, normal);
        h.key(Key::ArrowUp);
        h.key(Key::Escape);
        assert!(h.app.drawing.is_none());
        assert_eq!(h.app.lock, None);
        assert_eq!(h.app.doc.model, before);
    }
}

#[test]
fn freehand_plane_locks_preserve_sampled_strokes_when_switching_planes() {
    for (axis, key) in PLANE_LOCKS {
        let mut h = Harness::new();
        h.app.doc.snap_to_geometry = false;
        h.app.doc.snap_to_axes = false;
        h.app.set_tool(Tool::Freehand);
        h.key(key);
        h.pointer(h.at(DVec3::ZERO), egui::PointerButton::Primary, true, false);
        let drawing = h.app.drawing.as_ref().unwrap();
        let (origin, u, v) = (drawing.origin, drawing.u, drawing.v);
        assert_eq!(drawing.normal, axis.vector());
        for offset in [u, u * 2. + v, u * 3. + v * 0.5] {
            hover(&mut h, origin + offset);
        }
        let samples = h.app.drawing.as_ref().unwrap().points.clone();
        assert!(samples.len() >= 4);
        let next = if axis == Axis::X {
            Key::ArrowLeft
        } else {
            Key::ArrowRight
        };
        h.key(next);
        let drawing = h.app.drawing.as_ref().unwrap();
        assert_eq!(drawing.origin, origin);
        assert_eq!(drawing.points.len(), samples.len());
        for (before, after) in samples.windows(2).zip(drawing.points.windows(2)) {
            assert!((before[0].distance(before[1]) - after[0].distance(after[1])).abs() < 1e-7);
        }
        assert!(
            drawing
                .points
                .iter()
                .all(|p| (*p - origin).dot(drawing.normal).abs() < 1e-7)
        );
        h.key(Key::ArrowDown);
        assert_eq!(h.app.drawing.as_ref().unwrap().normal, DVec3::Z);
        h.key(key);
        let drawing = h.app.drawing.as_ref().unwrap();
        assert!(
            drawing
                .points
                .iter()
                .zip(&samples)
                .all(|(a, b)| a.distance(*b) < 1e-7)
        );
        let end = h.at(origin + u * 3. + v * 0.5);
        let preview = h.app.drawing_points();
        h.pointer(end, egui::PointerButton::Primary, false, false);
        assert!(h.app.drawing.is_none());
        assert!(!h.app.doc.model.edges.is_empty());
        assert!(preview.iter().all(|p| {
            h.app
                .doc
                .model
                .vertices
                .values()
                .any(|v| v.position.distance(*p) < 1e-7)
        }));
        h.app.doc.model.validate().unwrap();
        h.app.undo(&h.ctx, false);
        assert!(h.app.doc.model.edges.is_empty());
    }
}

fn reference(h: &mut Harness, point: DVec3) -> u64 {
    h.app
        .doc
        .model
        .add_edge(point, point + DVec3::new(0.7, 0.4, 0.2))
        .unwrap()
}

fn hover(h: &mut Harness, point: DVec3) {
    h.frame(
        vec![egui::Event::PointerMoved(h.at(point))],
        egui::Modifiers::NONE,
    );
}

#[test]
fn unlocked_shapes_follow_points_and_edges_on_vertical_and_tilted_planes() {
    for tool in [Tool::Rectangle, Tool::Circle, Tool::Polygon] {
        for a in [
            DVec3::new(0., 3., 2.),
            DVec3::new(3., 0., 2.),
            DVec3::new(3., 2., 0.),
            DVec3::new(2., 3., 4.),
        ] {
            for fraction in [0., 0.5, 0.25] {
                let mut h = Harness::new();
                // Geometry snapping can turn the plane independently of the
                // preference for automatic directional axis snapping.
                h.app.doc.snap_to_axes = fraction != 0.5;
                let direction = if a.x == 0. { DVec3::Y } else { DVec3::X };
                let b = a + direction * 3.;
                h.app.doc.model.add_edge(a, b).unwrap();
                let before = h.app.doc.model.clone();
                h.app.set_tool(tool);
                h.click(h.at(DVec3::ZERO));
                let target = a.lerp(b, fraction);
                hover(&mut h, target);
                let snap = h.app.snap.unwrap();
                assert!(snap.point.distance(target) < 1e-5, "{tool:?}: {snap:?}");
                assert_eq!(
                    snap.kind,
                    if fraction == 0. {
                        Inference::Endpoint
                    } else if fraction == 0.5 {
                        Inference::Midpoint
                    } else {
                        Inference::OnEdge
                    }
                );
                assert_eq!(h.app.lock, None);
                let drawing = h.app.drawing.as_ref().unwrap();
                let (origin, normal) = (drawing.origin, drawing.normal);
                assert!((snap.point - origin).dot(normal).abs() < 1e-7);
                for axis in [Axis::X, Axis::Y, Axis::Z] {
                    if a.dot(axis.vector()) == 0. {
                        assert!(normal.cross(axis.vector()).length() < 1e-7);
                    }
                }
                let preview = h.app.drawing_points();
                let picked = if tool == Tool::Rectangle {
                    preview[2]
                } else {
                    preview[0]
                };
                assert!(picked.distance(snap.point) < 1e-7);
                assert!(
                    preview
                        .iter()
                        .all(|p| (*p - origin).dot(normal).abs() < 1e-7)
                );
                if tool != Tool::Rectangle {
                    let radius = snap.point.distance(origin);
                    assert!(
                        preview
                            .iter()
                            .all(|p| (p.distance(origin) - radius).abs() < 1e-7)
                    );
                }
                h.click(h.at(target));
                assert!(h.app.drawing.is_none(), "{tool:?}: {target:?}");
                assert!(preview.iter().all(|p| {
                    h.app
                        .doc
                        .model
                        .vertices
                        .values()
                        .any(|v| v.position.distance(*p) < 1e-7)
                }));
                h.app.doc.model.validate().unwrap();
                h.app.undo(&h.ctx, false);
                assert_eq!(h.app.doc.model, before);
            }
        }
    }
}

#[test]
fn unlocked_shapes_acquire_all_local_axes_and_preserve_explicit_plane_locks() {
    for tool in [Tool::Rectangle, Tool::Circle, Tool::Polygon] {
        for (axis, _) in PLANE_LOCKS {
            let mut h = Harness::new();
            h.app.doc.snap_to_geometry = false;
            h.app.set_tool(tool);
            let start = DVec3::new(1., 2., 0.);
            h.click(h.at(start));
            let origin = h.app.drawing.as_ref().unwrap().origin;
            let target = origin + axis.vector() * 3.;
            hover(&mut h, target);
            assert_eq!(h.app.snap.unwrap().kind, Inference::Axis(axis));
            assert!(h.app.snap.unwrap().point.distance(target) < 1e-5);
            assert!(
                (h.app.snap.unwrap().point - origin)
                    .dot(h.app.drawing.as_ref().unwrap().normal)
                    .abs()
                    < 1e-7
            );
            h.key(Key::ArrowUp);
            assert_eq!(h.app.drawing.as_ref().unwrap().normal, DVec3::Z);
            hover(&mut h, origin + DVec3::new(0., 2., 3.));
            assert!(
                h.app
                    .drawing_points()
                    .iter()
                    .all(|p| (p.z - origin.z).abs() < 1e-7)
            );
            assert_eq!(h.app.lock, Some(Axis::Z));
        }
    }
}

#[test]
fn unlocked_shapes_can_follow_a_reference_axis_outside_the_current_plane() {
    for tool in [Tool::Rectangle, Tool::Circle, Tool::Polygon] {
        let mut h = Harness::new();
        let point = DVec3::new(0., 3., 2.);
        reference(&mut h, point);
        h.app.set_tool(tool);
        h.click(h.at(DVec3::ZERO));
        hover(&mut h, point);
        let target = DVec3::new(3., 3., 2.);
        hover(&mut h, target);
        let snap = h.app.snap.unwrap();
        assert!(snap.point.distance(target) < 1e-5);
        assert_eq!(h.app.active_drawing_guide().unwrap().axis, Axis::X);
        let drawing = h.app.drawing.as_ref().unwrap();
        assert!((snap.point - drawing.origin).dot(drawing.normal).abs() < 1e-7);
        assert_eq!(h.app.lock, None);
        let preview = h.app.drawing_points();
        h.click(h.at(target));
        assert!(
            h.app.drawing.is_none(),
            "tool={tool:?}, snap={:?}, notice={:?}",
            h.app.snap,
            h.app.notice
        );
        assert!(preview.iter().all(|p| {
            h.app
                .doc
                .model
                .vertices
                .values()
                .any(|v| v.position.distance(*p) < 1e-7)
        }));
        h.app.doc.model.validate().unwrap();
    }
}

#[test]
fn unlocked_freehand_snaps_in_three_dimensions_without_moving_prior_samples() {
    let mut h = Harness::new();
    let targets = [
        DVec3::new(0., 3., 2.),
        DVec3::new(3., 0., 2.),
        DVec3::new(2., 3., 4.),
    ];
    for point in targets {
        reference(&mut h, point);
    }
    let before = h.app.doc.model.clone();
    h.app.set_tool(Tool::Freehand);
    h.pointer(h.at(DVec3::ZERO), egui::PointerButton::Primary, true, false);
    for target in targets {
        let previous = h.app.drawing.as_ref().unwrap().points.clone();
        hover(&mut h, target);
        let drawing = h.app.drawing.as_ref().unwrap();
        assert_eq!(&drawing.points[..previous.len()], previous.as_slice());
        assert!(drawing.points.last().unwrap().distance(target) < 1e-5);
        assert_eq!(h.app.snap.unwrap().kind, Inference::Endpoint);
    }
    let preview = h.app.drawing_points();
    h.pointer(h.at(targets[2]), egui::PointerButton::Primary, false, false);
    assert!(h.app.drawing.is_none());
    assert!(preview.iter().all(|p| {
        h.app
            .doc
            .model
            .vertices
            .values()
            .any(|v| v.position.distance(*p) < 1e-7)
    }));
    h.app.doc.model.validate().unwrap();
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
}

#[test]
fn hovered_endpoints_midpoints_and_edges_supply_axis_references() {
    for fraction in [0., 0.5, 0.25] {
        let mut h = Harness::new();
        let a = DVec3::new(2., 3., 2.);
        let b = DVec3::new(6., 5., 3.);
        h.app.doc.model.add_edge(a, b).unwrap();
        h.key(Key::L);
        h.click(h.at(DVec3::new(-3., -2., 0.)));
        let point = a.lerp(b, fraction);
        hover(&mut h, point);
        let acquired = h.app.snap.unwrap().point;
        assert!(acquired.distance(point) < 1e-5);
        assert!(h.app.drawing_reference.is_some());
        let target = acquired - DVec3::X * 3.;
        hover(&mut h, target);
        let snap = h.app.snap.unwrap();
        let guide = h.app.drawing_guide.unwrap();
        assert!(guide.origin.distance(acquired) < 1e-7);
        assert_eq!(guide.axis, Axis::X);
        assert!((snap.point - acquired).cross(DVec3::X).length() < 1e-7);
        let click = h.at(target);
        h.click(click);
        assert!(h.app.line_start.unwrap().distance(snap.point) < 1e-7);
        assert_eq!(h.app.doc.model.edges.len(), 2);
        h.app.doc.model.validate().unwrap();
    }
}

#[test]
fn every_drawing_tool_can_align_the_first_point_from_hovered_geometry() {
    for tool in Tool::DRAWING {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let mut h = Harness::new();
            let point = DVec3::new(2., 3., 2.);
            reference(&mut h, point);
            h.app.set_tool(tool);
            hover(&mut h, point);
            hover(&mut h, point + axis.vector() * 3.);
            let snap = h.app.snap.unwrap();
            assert_eq!(h.app.drawing_guide.unwrap().axis, axis, "{tool:?}");
            assert!((snap.point - point).cross(axis.vector()).length() < 1e-7);
            assert_eq!(h.app.doc.model.edges.len(), 1);
        }
    }
}

#[test]
fn reference_axis_and_explicit_line_lock_intersect_without_breaking_the_lock() {
    let mut h = Harness::new();
    let point = DVec3::new(3., 0., 2.);
    reference(&mut h, point);
    h.key(Key::L);
    let start = DVec3::new(0., 4., 2.);
    // Start above the default drawing plane using existing geometry.
    reference(&mut h, start);
    h.click(h.at(start));
    h.key(Key::ArrowRight);
    hover(&mut h, point);
    let target = DVec3::new(3., 4., 2.);
    hover(&mut h, target);
    assert!(h.app.snap.unwrap().point.distance(target) < 1e-7);
    assert_eq!(h.app.lock, Some(Axis::X));
    let (source, projected) = h.app.active_drawing_projection().unwrap();
    assert!(source.distance(point) < 1e-7);
    assert!(projected.distance(target) < 1e-7);
}

#[test]
fn rectangle_reference_alignment_can_return_to_the_horizontal_plane() {
    let mut h = Harness::new();
    let point = DVec3::new(3., 4., 2.);
    reference(&mut h, point);
    h.key(Key::R);
    h.click(h.at(DVec3::ZERO));
    hover(&mut h, point);
    hover(&mut h, DVec3::new(3., 4., 0.));
    let snap = h.app.snap.unwrap();
    assert!(
        snap.point.distance(DVec3::new(3., 4., 0.)) < 1e-6,
        "snap={snap:?}"
    );
    assert_eq!(h.app.drawing_guide.unwrap().axis, Axis::Z);
    assert!(h.app.drawing_points().iter().all(|p| p.z.abs() < 1e-7));
}

#[test]
fn disabled_snapping_hidden_and_deleted_geometry_release_references() {
    for change in [0, 1, 2, 3] {
        let mut h = Harness::new();
        let point = DVec3::new(2., 3., 2.);
        let edge = reference(&mut h, point);
        h.key(Key::L);
        hover(&mut h, point);
        assert!(h.app.drawing_reference.is_some());
        match change {
            0 => h.app.doc.snap_to_geometry = false,
            1 => h.app.doc.snap_to_axes = false,
            2 => {
                h.app.doc.model.hidden.insert(edge);
            }
            _ => {
                h.app.doc.model.erase(&BTreeSet::from([edge]));
            }
        }
        hover(&mut h, point + DVec3::X * 3.);
        assert!(h.app.drawing_reference.is_none());
        assert!(h.app.drawing_guide.is_none());
        assert!(h.app.drawing_projection.is_none());
    }
}

#[test]
fn y_locked_lines_snap_immediately_to_hovered_group_and_component_geometry() {
    for component in [false, true] {
        for nested in [false, true] {
            for editing_another_group in [false, true] {
                for fraction in [0., 0.5, 0.25] {
                    let mut h = Harness::new();
                    let a = DVec3::new(4., 3., 2.);
                    let b = DVec3::new(6., 7., 3.);
                    let edge = h.app.doc.model.add_edge(a, b).unwrap();
                    let object = h
                        .app
                        .doc
                        .model
                        .make_object(&BTreeSet::from([edge]), component, "Reference")
                        .unwrap();
                    let object = if nested {
                        h.app
                            .doc
                            .model
                            .make_object(&BTreeSet::from([object]), !component, "Outer")
                            .unwrap()
                    } else {
                        object
                    };
                    let offset = DVec3::new(0.5, 0.5, 0.5);
                    h.app
                        .doc
                        .model
                        .move_selection(&BTreeSet::from([object]), offset)
                        .unwrap();
                    let reference_edges: Vec<_> = h
                        .app
                        .doc
                        .model
                        .expand_entities(&BTreeSet::from([object]))
                        .into_iter()
                        .filter(|id| h.app.doc.model.edges.contains_key(id))
                        .map(|id| (id, h.app.doc.model.points(&h.app.doc.model.edges[&id])))
                        .collect();
                    let start = DVec3::new(-2., -2., 0.);
                    let editing = if editing_another_group {
                        let edge = h
                            .app
                            .doc
                            .model
                            .add_edge(start, start + DVec3::X * 0.5)
                            .unwrap();
                        let group = h
                            .app
                            .doc
                            .model
                            .make_object(&BTreeSet::from([edge]), false, "Editing")
                            .unwrap();
                        assert!(h.app.doc.model.enter_object(group));
                        Some(group)
                    } else {
                        None
                    };
                    h.key(Key::L);
                    h.click(h.at(start));
                    assert!(
                        h.app.line_start.unwrap().distance(start) < 1e-6,
                        "start={start:?}, actual={:?}, offset={offset:?}",
                        h.app.line_start
                    );
                    h.key(Key::ArrowLeft);
                    let reference = a.lerp(b, fraction) + offset;
                    let expected = DVec3::new(start.x, reference.y, start.z);
                    assert!(h.at(reference).distance(h.at(expected)) > 10.);

                    // Keep the cursor on the reference edge, not the target axis.
                    hover(&mut h, reference);
                    if editing_another_group {
                        assert!(h.app.hover.is_none());
                    }
                    let snap = h.app.snap.unwrap();
                    assert!(
                        snap.point.distance(expected) < 1e-5,
                        "component={component}, nested={nested}, editing={editing_another_group}, fraction={fraction}: {:?}",
                        snap.point
                    );
                    assert_eq!(h.app.lock, Some(Axis::Y));
                    assert!(h.app.active_drawing_projection().is_some());
                    assert!((snap.point - start).cross(DVec3::Y).length() < 1e-6);

                    h.click(h.at(reference));
                    assert!(h.app.line_start.unwrap().distance(expected) < 1e-5);
                    assert_eq!(h.app.doc.model.edit_context, editing);
                    for (id, (a, b)) in reference_edges {
                        let (actual_a, actual_b) =
                            h.app.doc.model.points(&h.app.doc.model.edges[&id]);
                        assert_eq!((actual_a, actual_b), (a, b));
                    }
                    h.app.doc.model.validate().unwrap();
                }
            }
        }
    }
}

#[test]
fn every_explicit_lock_uses_the_hovered_reference_coordinate() {
    for (axis, key) in [
        (Axis::X, Key::ArrowRight),
        (Axis::Y, Key::ArrowLeft),
        (Axis::Z, Key::ArrowUp),
    ] {
        let mut h = Harness::new();
        let point = DVec3::new(4., 3., 2.);
        reference(&mut h, point);
        h.key(Key::L);
        let start = DVec3::new(-2., -2., 0.);
        h.click(h.at(start));
        h.key(key);
        hover(&mut h, point);
        let expected = start + axis.vector() * (point - start).dot(axis.vector());
        assert!(
            h.app.snap.unwrap().point.distance(expected) < 1e-6,
            "{axis:?}, snap={:?}, start={start:?}",
            h.app.snap
        );
        assert_eq!(h.app.lock, Some(axis));
    }
}

#[test]
fn y_lock_can_reference_a_solid_component_outside_the_current_editing_group() {
    let mut h = Harness::new();
    let solid = ui_solid_box(
        &mut h.app.doc.model,
        DVec3::new(3., 3., 0.),
        DVec3::new(5., 5., 2.),
    );
    h.app
        .doc
        .model
        .make_object(&BTreeSet::from([solid]), true, "Solid component")
        .unwrap();
    let start = DVec3::new(-2., -2., 0.);
    let edge = h
        .app
        .doc
        .model
        .add_edge(start, start + DVec3::X * 0.5)
        .unwrap();
    let editing = h
        .app
        .doc
        .model
        .make_object(&BTreeSet::from([edge]), false, "Editing")
        .unwrap();
    assert!(h.app.doc.model.enter_object(editing));
    h.key(Key::L);
    h.click(h.at(start));
    h.key(Key::ArrowLeft);
    let reference = DVec3::new(4., 3., 2.);
    hover(&mut h, reference);
    assert!(h.app.hover.is_none());
    let expected = DVec3::new(-2., 3., 0.);
    assert!(h.app.snap.unwrap().point.distance(expected) < 1e-6);
    assert!(h.app.active_drawing_projection().is_some());
    h.click(h.at(reference));
    assert!(h.app.line_start.unwrap().distance(expected) < 1e-7);
    assert_eq!(h.app.doc.model.edit_context, Some(editing));
    h.app.doc.model.validate().unwrap();
}

#[test]
fn typed_length_overrides_the_hovered_locked_coordinate_and_hides_its_projection() {
    let mut h = Harness::new();
    let point = DVec3::new(4., 3., 2.);
    reference(&mut h, point);
    h.key(Key::L);
    h.click(h.at(DVec3::ZERO));
    h.key(Key::ArrowLeft);
    hover(&mut h, point);
    assert!(h.app.active_drawing_projection().is_some());
    h.frame(
        vec![egui::Event::Text("60cm - 89mm".into())],
        egui::Modifiers::NONE,
    );
    let preview = h.app.line_preview_snap().unwrap().point;
    assert!(preview.distance(DVec3::Y * 0.511) < 1e-7);
    assert!(h.app.active_drawing_projection().is_none());
    h.key(Key::Enter);
    assert!(h.app.line_start.unwrap().distance(preview) < 1e-7);
}

#[test]
fn measurement_click_uses_the_same_hover_axis_as_its_preview() {
    let mut h = Harness::new();
    let point = DVec3::new(2., 3., 2.);
    reference(&mut h, point);
    h.key(Key::T);
    h.click(h.at(DVec3::new(-3., -2., 0.)));
    hover(&mut h, point);
    let target = point + DVec3::X * 3.;
    hover(&mut h, target);
    let preview = h.app.snap.unwrap().point;
    assert_eq!(h.app.drawing_guide.unwrap().axis, Axis::X);
    h.click(h.at(target));
    let measurement = h.app.measurement.as_ref().unwrap();
    assert!(measurement.complete);
    assert!(measurement.points[1].distance(preview) < 1e-7);
    assert_eq!(h.app.doc.model.edges.len(), 1);
}

#[test]
fn a_typed_formula_keeps_its_length_without_claiming_an_incompatible_reference_axis() {
    let mut h = Harness::new();
    let point = DVec3::new(2., 3., 2.);
    reference(&mut h, point);
    h.key(Key::L);
    h.click(h.at(DVec3::new(-3., -2., 0.)));
    let start = h.app.line_start.unwrap();
    hover(&mut h, point);
    hover(&mut h, point + DVec3::X * 3.);
    assert!(h.app.active_drawing_guide().is_some());
    h.frame(
        vec![egui::Event::Text("60cm - 89mm".into())],
        egui::Modifiers::NONE,
    );
    let preview = h.app.line_preview_snap().unwrap();
    assert!((preview.point.distance(start) - 0.511).abs() < 1e-7);
    assert!(h.app.active_drawing_guide().is_none());
    h.key(Key::Enter);
    assert!(h.app.line_start.unwrap().distance(preview.point) < 1e-7);
}

#[test]
fn lines_snap_to_each_axis_pair_and_three_way_intersections_and_commit_the_preview() {
    let target = DVec3::new(3., 4., 2.);
    let start = DVec3::new(-4., -3., 2.);
    for axes in [
        vec![Axis::X, Axis::Y],
        vec![Axis::X, Axis::Z],
        vec![Axis::Y, Axis::Z],
        vec![Axis::X, Axis::Y, Axis::Z],
    ] {
        let mut h = Harness::new();
        for axis in &axes {
            reference(&mut h, target - axis.vector() * 3.);
        }
        reference(&mut h, start);
        h.key(Key::L);
        h.click(h.at(start));
        let cursor = h.at(target) + egui::vec2(2., -2.);
        h.frame(
            vec![egui::Event::PointerMoved(cursor)],
            egui::Modifiers::NONE,
        );
        let preview = h.app.line_preview_snap().unwrap();
        assert_eq!(preview.kind, Inference::Aligned, "{axes:?}");
        assert!(
            preview.point.distance(target) < 1e-7,
            "{axes:?}: {preview:?}"
        );
        let guides: Vec<_> = h.app.active_drawing_axis_guides().collect();
        assert_eq!(guides.len(), axes.len(), "{axes:?}");
        for axis in axes {
            assert!(guides.iter().any(|guide| guide.axis == axis));
        }
        let before = h.app.doc.model.clone();
        h.click(cursor);
        assert!(h.app.line_start.unwrap().distance(preview.point) < 1e-7);
        assert!(h.app.doc.model.edges.values().any(|edge| {
            let (a, b) = h.app.doc.model.points(edge);
            (a.distance(start) < 1e-7 && b.distance(target) < 1e-7)
                || (b.distance(start) < 1e-7 && a.distance(target) < 1e-7)
        }));
        h.app.doc.model.validate().unwrap();
        h.app.undo(&h.ctx, false);
        assert_eq!(h.app.doc.model, before);
        assert!(h.app.drawing_axis_guides.is_empty());
    }
}

#[test]
fn combined_line_guides_respect_every_explicit_axis_lock() {
    let target = DVec3::new(3., 4., 2.);
    for (axis, key) in PLANE_LOCKS {
        let mut h = Harness::new();
        let start = target - axis.vector() * 3.;
        for other in [Axis::X, Axis::Y, Axis::Z] {
            reference(&mut h, target - other.vector() * 3.);
        }
        h.key(Key::L);
        h.click(h.at(start));
        h.key(key);
        let cursor = h.at(target) + egui::vec2(2., -2.);
        h.frame(
            vec![egui::Event::PointerMoved(cursor)],
            egui::Modifiers::NONE,
        );
        let snap = h.app.line_preview_snap().unwrap();
        assert_eq!(h.app.lock, Some(axis));
        assert!(snap.point.distance(target) < 1e-7, "{axis:?}: {snap:?}");
        assert_eq!(h.app.active_drawing_axis_guides().count(), 3);

        // Moving to an incompatible guide crossing must never break the lock.
        hover(&mut h, target + DVec3::ONE);
        let snap = h.app.line_preview_snap().unwrap();
        assert!((snap.point - start).cross(axis.vector()).length() < 1e-7);
    }
}

#[test]
fn combined_line_guides_release_when_snapping_is_disabled_or_a_reference_is_removed() {
    let target = DVec3::new(3., 4., 2.);
    for change in 0..4 {
        let mut h = Harness::new();
        let edge = reference(&mut h, target - DVec3::X * 3.);
        reference(&mut h, target - DVec3::Y * 3.);
        let start = DVec3::new(-4., -3., 2.);
        reference(&mut h, start);
        h.key(Key::L);
        h.click(h.at(start));
        let cursor = h.at(target) + egui::vec2(2., -2.);
        h.frame(
            vec![egui::Event::PointerMoved(cursor)],
            egui::Modifiers::NONE,
        );
        assert_eq!(h.app.active_drawing_axis_guides().count(), 2);
        match change {
            0 => h.app.doc.snap_to_geometry = false,
            1 => h.app.doc.snap_to_axes = false,
            2 => {
                h.app.doc.model.hidden.insert(edge);
            }
            _ => h.app.doc.model.erase(&BTreeSet::from([edge])),
        }
        h.app.update_pointer(cursor);
        assert!(h.app.drawing_axis_guides.is_empty());
        assert!(h.app.snap.unwrap().point.distance(target) > 1e-4);
    }
}

#[test]
fn exact_line_geometry_wins_over_combined_axis_guides() {
    let mut h = Harness::new();
    let target = DVec3::new(3., 4., 2.);
    reference(&mut h, target - DVec3::X * 3.);
    reference(&mut h, target - DVec3::Y * 3.);
    let endpoint = target + DVec3::new(0.01, 0.01, 0.);
    reference(&mut h, endpoint);
    let start = DVec3::new(-4., -3., 2.);
    reference(&mut h, start);
    h.key(Key::L);
    h.click(h.at(start));
    hover(&mut h, endpoint);
    assert_eq!(h.app.snap.unwrap().kind, Inference::Endpoint);
    assert!(h.app.snap.unwrap().point.distance(endpoint) < 1e-5);
    assert!(h.app.drawing_axis_guides.is_empty());
}

#[test]
fn typed_line_length_releases_incompatible_combined_guides_and_commits_the_preview() {
    let mut h = Harness::new();
    let target = DVec3::new(3., 4., 2.);
    reference(&mut h, target - DVec3::X * 3.);
    reference(&mut h, target - DVec3::Y * 3.);
    let start = DVec3::new(-4., -3., 2.);
    reference(&mut h, start);
    h.key(Key::L);
    h.click(h.at(start));
    let cursor = h.at(target) + egui::vec2(2., -2.);
    h.frame(
        vec![egui::Event::PointerMoved(cursor)],
        egui::Modifiers::NONE,
    );
    assert_eq!(h.app.active_drawing_axis_guides().count(), 2);
    h.frame(vec![egui::Event::Text("1m".into())], egui::Modifiers::NONE);
    let preview = h.app.line_preview_snap().unwrap();
    assert!((preview.point.distance(start) - 1.).abs() < 1e-7);
    assert_ne!(preview.kind, Inference::Aligned);
    assert_eq!(h.app.active_drawing_axis_guides().count(), 0);
    h.key(Key::Enter);
    assert!(h.app.line_start.unwrap().distance(preview.point) < 1e-7);
}

#[test]
fn line_start_and_world_axis_guides_combine_with_object_snapping_disabled() {
    let mut h = Harness::new();
    h.app.doc.snap_to_geometry = false;
    h.key(Key::L);
    h.click(h.at(DVec3::new(2., 3., 0.)));
    let start = h.app.line_start.unwrap();
    let target = DVec3::new(0., start.y, 0.);
    let cursor = h.at(target) + egui::vec2(2., -2.);
    h.frame(
        vec![egui::Event::PointerMoved(cursor)],
        egui::Modifiers::NONE,
    );
    let snap = h.app.line_preview_snap().unwrap();
    assert_eq!(snap.kind, Inference::Aligned);
    assert!(snap.point.distance(target) < 1e-7);
    let guides: Vec<_> = h.app.active_drawing_axis_guides().collect();
    assert_eq!(guides.len(), 2);
    assert!(
        guides
            .iter()
            .any(|g| g.axis == Axis::X && g.origin == start)
    );
    assert!(
        guides
            .iter()
            .any(|g| g.axis == Axis::Y && g.origin == DVec3::ZERO)
    );
}
