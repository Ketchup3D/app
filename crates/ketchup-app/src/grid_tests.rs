use super::*;

fn grid_line_styles(camera: &Camera) -> std::collections::BTreeMap<(bool, i64), bool> {
    let spacing = grid::spacing(camera);
    grid::lines(camera)
        .map(|(a, b, major)| {
            assert_eq!(a.z, 0.);
            assert_eq!(b.z, 0.);
            let vertical = a.x == b.x;
            let coordinate = if vertical { a.x } else { a.y };
            ((vertical, (coordinate / spacing).round() as i64), major)
        })
        .collect()
}

#[test]
fn grid_lines_stay_anchored_while_orbiting_and_panning() {
    for projection in [
        ketchup_core::Projection::Perspective,
        ketchup_core::Projection::Parallel,
    ] {
        for distance in [6., 30., 300.] {
            let mut camera = Camera {
                distance,
                projection,
                ..Default::default()
            };
            let spacing = grid::spacing(&camera);
            camera.target = DVec3::new(2.3, -1.7, 4.) * spacing;
            let before = grid_line_styles(&camera);
            // The two axes must use their own absolute line indices.
            for vertical in [false, true] {
                for index in [-5, 0, 5] {
                    assert_eq!(before.get(&(vertical, index)), Some(&true));
                }
                for index in [-4, -1, 1, 4] {
                    assert_eq!(before.get(&(vertical, index)), Some(&false));
                }
            }
            let target = camera.target;
            let pivot = DVec3::new(-3., 2., 1.) * spacing;
            for delta in [DVec2::new(100., 50.), DVec2::new(-200., -100.)] {
                camera.orbit_around(delta, pivot);
                assert!(camera.target.distance(target) > spacing);
                assert_eq!(grid::spacing(&camera), spacing);
                let after = grid_line_styles(&camera);
                let mut shared = 0;
                for (line, major) in &before {
                    if let Some(after_major) = after.get(line) {
                        assert_eq!(after_major, major, "Grid line {line:?} moved during orbit");
                        shared += 1;
                    }
                }
                assert!(shared > 100);
            }
            camera.pan(DVec2::new(80., -50.), 900.);
            let after = grid_line_styles(&camera);
            for (line, major) in &before {
                if let Some(after_major) = after.get(line) {
                    assert_eq!(after_major, major, "Grid line {line:?} moved during pan");
                }
            }
        }
    }
}

#[test]
fn grid_spacing_can_readjust_when_zooming_in() {
    let mut camera = Camera {
        distance: 30.,
        target: DVec3::new(2.3, -1.7, 4.),
        ..Default::default()
    };
    let before = grid::spacing(&camera);
    let size = DVec2::new(1200., 900.);
    camera.zoom(800., size * 0.5, size);
    let after = grid::spacing(&camera);
    assert!(after < before);
    for (a, b, _) in grid::lines(&camera) {
        for point in [a, b] {
            for coordinate in [point.x, point.y] {
                let index = coordinate / after;
                assert!((index - index.round()).abs() < 1e-7);
            }
        }
    }
}

fn hover(h: &mut Harness, point: DVec3) {
    h.frame(
        vec![egui::Event::PointerMoved(h.at(point))],
        egui::Modifiers::NONE,
    );
}

#[test]
fn grid_snapping_menu_defaults_off_and_toggles_independently() {
    let mut h = Harness::new();
    assert!(!h.app.snap_to_grid);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, "▾"));
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let checkbox = text_position(&output, "Snap to grid");
    h.click(checkbox);
    assert!(h.app.snap_to_grid);
    assert!(h.app.doc.snap_to_geometry);
    assert!(h.app.doc.snap_to_axes);
    assert!(!h.app.project.modified);
    h.click(checkbox);
    assert!(!h.app.snap_to_grid);
}

#[test]
fn grid_snapping_line_preview_and_commit_use_the_displayed_spacing() {
    for distance in [6., 30., 300.] {
        let mut h = Harness::new();
        h.app.doc.camera.distance = distance;
        h.app.doc.snap_to_geometry = false;
        h.app.doc.snap_to_axes = false;
        h.app.set_tool(Tool::Line);
        let spacing = grid::spacing(&h.app.doc.camera);
        let target = DVec3::new(2.3, -1.7, 0.) * spacing;
        hover(&mut h, target);
        assert!(h.app.snap.unwrap().point.distance(target) < spacing * 1e-4);
        h.app.snap_to_grid = true;
        hover(&mut h, target);
        let expected = DVec3::new(2., -2., 0.) * spacing;
        assert!(h.app.snap.unwrap().point.distance(expected) < 1e-7);
        h.click(h.at(target));
        assert!(h.app.line_start.unwrap().distance(expected) < 1e-7);
        let end = DVec3::new(-2.2, 1.8, 0.) * spacing;
        hover(&mut h, end);
        let preview = h.app.line_preview_snap().unwrap().point;
        assert!(preview.distance(DVec3::new(-2., 2., 0.) * spacing) < 1e-7);
        h.click(h.at(end));
        let edge = h.app.doc.model.edges.values().next().unwrap();
        let (a, b) = h.app.doc.model.points(edge);
        assert!(a.distance(expected) < 1e-7);
        assert!(b.distance(preview) < 1e-7);
    }
}

#[test]
fn grid_snapping_preserves_object_targets_and_off_grid_axis_anchors() {
    let mut h = Harness::new();
    h.app.snap_to_grid = true;
    h.app.set_tool(Tool::Line);
    let target = DVec3::new(2.3, 1.7, 0.);
    h.app
        .doc
        .model
        .add_edge(target, target + DVec3::X * 2.)
        .unwrap();
    hover(&mut h, target);
    assert_eq!(h.app.snap.unwrap().kind, Inference::Endpoint);
    assert!(h.app.snap.unwrap().point.distance(target) < 1e-7);
    h.click(h.at(target));
    h.app.doc.snap_to_geometry = false;
    h.key(Key::ArrowRight);
    hover(&mut h, target + DVec3::X * 3.1);
    let point = h.app.snap.unwrap().point;
    assert!((point - target).cross(DVec3::X).length() < 1e-7);
    assert!((point.x - 5.).abs() < 1e-7);
}

#[test]
fn grid_snapping_shapes_stay_on_each_locked_plane() {
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        let mut h = Harness::new();
        h.app.doc.snap_to_geometry = false;
        h.app.doc.snap_to_axes = false;
        h.app.snap_to_grid = true;
        h.app.set_tool(Tool::Rectangle);
        h.click(h.at(DVec3::ZERO));
        h.app.set_axis_lock(Some(axis), &h.ctx);
        let drawing = h.app.drawing.as_ref().unwrap();
        let (origin, u, v) = (drawing.origin, drawing.u, drawing.v);
        hover(&mut h, origin + u * 2.3 + v * 1.7);
        let point = h.app.snap.unwrap().point;
        assert!(point.distance(origin + u * 2. + v * 2.) < 1e-7);
        assert!((point - origin).dot(axis.vector()).abs() < 1e-7);
    }
}

#[test]
fn grid_snapping_move_and_measurement_use_grid_points() {
    let mut h = Harness::new();
    h.app.doc.snap_to_geometry = false;
    h.app.doc.snap_to_axes = false;
    h.app.snap_to_grid = true;
    let edge = h.app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    h.app.selected.insert(edge);
    h.app.set_tool(Tool::Move);
    h.app.begin_modeling(h.at(DVec3::ZERO), &h.ctx);
    hover(&mut h, DVec3::new(2.3, -1.7, 0.));
    let edit = h.app.modeling.as_ref().unwrap();
    assert!(edit.direction.distance(DVec3::new(2., -2., 0.)) < 1e-7);
    h.key(Key::Escape);
    h.app.set_tool(Tool::TapeMeasure);
    hover(&mut h, DVec3::new(2.3, -1.7, 0.));
    assert!(h.app.snap.unwrap().point.distance(DVec3::new(2., -2., 0.)) < 1e-7);
}

#[test]
fn edge_and_face_snapping_menu_toggles_are_independent_preferences() {
    let mut h = Harness::new();
    assert!(h.app.snap_to_edge);
    assert!(h.app.snap_to_face);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, "▾"));
    for label in ["Snap to edge", "Snap to face"] {
        let output = h.frame(vec![], egui::Modifiers::NONE);
        let checkbox = text_position(&output, label);
        h.click(checkbox);
        assert_eq!(h.app.snap_to_edge, label != "Snap to edge");
        assert_eq!(h.app.snap_to_face, label != "Snap to face");
        assert!(h.app.doc.snap_to_geometry);
        assert!(h.app.doc.snap_to_axes);
        assert!(!h.app.project.modified);
        h.click(checkbox);
        assert!(h.app.snap_to_edge);
        assert!(h.app.snap_to_face);
    }
}

#[test]
fn drawing_uses_edge_and_face_preferences_with_cached_picking() {
    let mut h = Harness::new();
    h.app.set_tool(Tool::Line);
    h.app.doc.snap_to_axes = false;
    h.app
        .doc
        .model
        .add_edge(DVec3::new(0., 0., 2.), DVec3::new(6., 0., 2.))
        .unwrap();
    let edge_target = DVec3::new(1.5, 0., 2.);
    hover(&mut h, edge_target);
    assert_eq!(h.app.snap.unwrap().kind, Inference::OnEdge);
    h.app.snap_to_edge = false;
    hover(&mut h, edge_target);
    assert_eq!(h.app.snap.unwrap().kind, Inference::Plane);
    for (a, b) in [
        (DVec3::new(-3., -3., 4.), DVec3::new(3., -3., 4.)),
        (DVec3::new(3., -3., 4.), DVec3::new(3., 3., 4.)),
        (DVec3::new(3., 3., 4.), DVec3::new(-3., 3., 4.)),
        (DVec3::new(-3., 3., 4.), DVec3::new(-3., -3., 4.)),
    ] {
        h.app.doc.model.add_edge(a, b).unwrap();
    }
    let face_target = DVec3::new(0.5, 0.5, 4.);
    hover(&mut h, face_target);
    assert_eq!(h.app.snap.unwrap().kind, Inference::OnFace);
    h.app.snap_to_face = false;
    hover(&mut h, face_target);
    assert_eq!(h.app.snap.unwrap().kind, Inference::Plane);
}
