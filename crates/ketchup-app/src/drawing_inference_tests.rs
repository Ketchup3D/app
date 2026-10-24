use super::*;

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
fn rectangle_reference_alignment_stays_on_its_plane() {
    let mut h = Harness::new();
    let point = DVec3::new(3., 4., 2.);
    reference(&mut h, point);
    h.key(Key::R);
    h.click(h.at(DVec3::ZERO));
    hover(&mut h, point);
    hover(&mut h, DVec3::new(3., 4., 0.));
    let snap = h.app.snap.unwrap();
    assert!(snap.point.distance(DVec3::new(3., 4., 0.)) < 1e-7);
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
                    assert!(h.app.line_start.unwrap().distance(start) < 1e-7);
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
                    assert!((snap.point - start).cross(DVec3::Y).length() < 1e-7);

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
            h.app.snap.unwrap().point.distance(expected) < 1e-7,
            "{axis:?}"
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
    assert!(h.app.snap.unwrap().point.distance(expected) < 1e-7);
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
