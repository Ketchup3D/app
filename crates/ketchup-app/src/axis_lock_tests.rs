use super::*;

const PAIRS: [(Axis, Key, Axis, Key, Axis); 3] = [
    (Axis::X, Key::ArrowRight, Axis::Y, Key::ArrowLeft, Axis::Z),
    (Axis::X, Key::ArrowRight, Axis::Z, Key::ArrowUp, Axis::Y),
    (Axis::Y, Key::ArrowLeft, Axis::Z, Key::ArrowUp, Axis::X),
];

#[test]
fn completed_line_actions_reset_locks_only_when_enabled() {
    for enabled in [true, false] {
        let mut h = Harness::new();
        h.app.general_settings.reset_axis_locks_on_action = enabled;
        h.app.set_tool(Tool::Line);
        h.app.doc.snap_to_geometry = false;
        h.app.doc.snap_to_axes = false;
        h.click(h.at(DVec3::ZERO));
        h.key(Key::ArrowRight);
        h.key(Key::ArrowLeft);
        h.frame(
            vec![egui::Event::PointerMoved(h.at(DVec3::new(2., 1., 0.)))],
            egui::Modifiers::NONE,
        );
        let locks = (Some(Axis::X), Some(Axis::Y));
        assert_eq!((h.app.lock, h.app.second_lock), locks);
        // Invalid input leaves the operation and its constraints in progress.
        h.app.length_input = "invalid".into();
        h.key(Key::Enter);
        assert!(h.app.doc.model.edges.is_empty());
        assert_eq!((h.app.lock, h.app.second_lock), locks);
        h.app.length_input = "2m".into();
        h.key(Key::Enter);
        assert_eq!(h.app.doc.model.edges.len(), 1);
        assert!(h.app.line_start.is_some());
        assert_eq!(
            (h.app.lock, h.app.second_lock),
            if enabled { (None, None) } else { locks },
        );
    }
}

#[test]
fn completed_shapes_respect_the_axis_lock_reset_preference() {
    for enabled in [true, false] {
        let mut h = Harness::new();
        h.app.general_settings.reset_axis_locks_on_action = enabled;
        h.app.set_tool(Tool::Rectangle);
        h.key(Key::ArrowRight);
        h.key(Key::ArrowLeft);
        h.click(h.at(DVec3::ZERO));
        assert_eq!(h.app.plane_lock(), Some(Axis::Z));
        h.click(h.at(DVec3::new(2., 1., 0.)));
        assert_eq!(h.app.doc.model.faces.len(), 1);
        assert!(h.app.drawing.is_none());
        assert_eq!(
            h.app.plane_lock(),
            if enabled { None } else { Some(Axis::Z) },
        );
    }
}

#[test]
fn completed_measurements_reset_locks_only_when_enabled() {
    for enabled in [true, false] {
        for tool in [Tool::TapeMeasure, Tool::Protractor] {
            let mut h = Harness::new();
            h.app.general_settings.reset_axis_locks_on_action = enabled;
            h.app.set_tool(tool);
            h.app.create_guides = false;
            h.app.doc.snap_to_geometry = false;
            h.app.doc.snap_to_axes = false;
            h.key(Key::ArrowRight);
            h.key(Key::ArrowLeft);
            h.click(h.at(DVec3::ZERO));
            assert_eq!(h.app.plane_lock(), Some(Axis::Z));
            h.click(h.at(DVec3::X));
            if tool == Tool::Protractor {
                assert_eq!(h.app.plane_lock(), Some(Axis::Z));
                h.click(h.at(DVec3::Y));
            }
            assert!(h.app.measurement.as_ref().unwrap().complete);
            assert_eq!(
                h.app.plane_lock(),
                if enabled { None } else { Some(Axis::Z) },
            );
            assert!(!h.app.project.modified);
        }
    }
}

#[test]
fn tool_switches_and_cancellation_respect_the_axis_lock_reset_preference() {
    for enabled in [true, false] {
        let mut h = Harness::new();
        h.app.general_settings.reset_axis_locks_on_action = enabled;
        h.app.set_tool(Tool::Line);
        h.key(Key::ArrowRight);
        h.key(Key::ArrowLeft);
        h.app.set_tool(Tool::Move);
        assert_eq!(
            h.app.plane_lock(),
            if enabled { None } else { Some(Axis::Z) },
        );
        h.app.lock = Some(Axis::X);
        h.app.second_lock = Some(Axis::Y);
        h.key(Key::Escape);
        assert_eq!(
            h.app.plane_lock(),
            if enabled { None } else { Some(Axis::Z) },
        );
        h.key(Key::ArrowDown);
        assert_eq!((h.app.lock, h.app.second_lock), (None, None));
    }
}

#[test]
fn axis_locks_toggle_independently_and_never_allow_three() {
    for (first, first_key, second, second_key, normal) in PAIRS {
        let mut h = Harness::new();
        h.app.set_tool(Tool::Line);
        h.key(first_key);
        h.key(second_key);
        assert_eq!(h.app.lock, Some(first));
        assert_eq!(h.app.second_lock, Some(second));
        assert_eq!(h.app.plane_lock(), Some(normal));
        let third_key = match normal {
            Axis::X => Key::ArrowRight,
            Axis::Y => Key::ArrowLeft,
            Axis::Z => Key::ArrowUp,
        };
        h.key(third_key);
        assert_eq!(h.app.plane_lock(), Some(normal));
        h.key(first_key);
        assert_eq!(h.app.single_axis_lock(), Some(second));
        h.key(first_key);
        assert_eq!(h.app.plane_lock(), Some(normal));
        h.key(first_key);
        assert_eq!(h.app.single_axis_lock(), Some(second));
        h.key(second_key);
        assert!(h.app.axis_constraint().is_none());
        h.key(first_key);
        h.key(second_key);
        h.key(Key::ArrowDown);
        assert_eq!((h.app.lock, h.app.second_lock), (None, None));
    }
}

#[test]
fn two_axis_locks_constrain_line_previews_grid_and_typed_commits() {
    for (_, first_key, _, second_key, normal) in PAIRS {
        for grid in [false, true] {
            let mut h = Harness::new();
            h.app.set_tool(Tool::Line);
            h.app.doc.snap_to_geometry = false;
            h.app.doc.snap_to_axes = false;
            let start = DVec3::new(0.23, 0.37, 0.41);
            h.app.line_start = Some(start);
            h.app.snap_to_grid = grid;
            h.key(first_key);
            h.key(second_key);
            let delta = AxisConstraint::Plane(normal).project(DVec3::new(2.1, 1.7, 1.3));
            let cursor = h.at(start + delta);
            h.frame(
                vec![egui::Event::PointerMoved(cursor)],
                egui::Modifiers::NONE,
            );
            let preview = h.app.snap.unwrap().point;
            assert!((preview - start).dot(normal.vector()).abs() < 1e-7);
            assert!(preview.distance(start) > 1.);
            h.app.length_input = "2m".into();
            h.key(Key::Enter);
            let edge = h.app.doc.model.edges.values().next().unwrap();
            let (a, b) = h.app.doc.model.points(edge);
            assert!((b - a).dot(normal.vector()).abs() < 1e-7);
            assert!((a.distance(b) - 2.).abs() < 1e-6);
        }
    }
}

#[test]
fn two_axis_locks_keep_shape_previews_and_commits_on_the_selected_plane() {
    for tool in [Tool::Rectangle, Tool::Circle, Tool::Polygon, Tool::Freehand] {
        for (_, first_key, _, second_key, normal) in PAIRS {
            let mut h = Harness::new();
            h.app.set_tool(tool);
            h.key(first_key);
            h.key(second_key);
            if tool == Tool::Freehand {
                h.pointer(h.at(DVec3::ZERO), egui::PointerButton::Primary, true, false);
            } else {
                h.click(h.at(DVec3::ZERO));
            }
            let drawing = h.app.drawing.as_ref().unwrap();
            assert_eq!(drawing.normal, normal.vector());
            let target = drawing.origin + drawing.u * 2. + drawing.v * 1.3;
            h.frame(
                vec![egui::Event::PointerMoved(h.at(target))],
                egui::Modifiers::NONE,
            );
            assert!(
                h.app
                    .drawing_points()
                    .iter()
                    .all(|p| p.dot(normal.vector()).abs() < 1e-6)
            );
            if tool != Tool::Freehand {
                h.click(h.at(target));
                assert_eq!(h.app.doc.model.faces.len(), 1);
                assert!(
                    h.app.doc.model.vertices.values().all(|v| v
                        .position
                        .dot(normal.vector())
                        .abs()
                        < 1e-6)
                );
            }
        }
    }
}

#[test]
fn two_axis_locks_constrain_move_previews_and_commits() {
    for (_, first_key, _, second_key, normal) in PAIRS {
        let mut h = Harness::new();
        let start = DVec3::new(0.23, 0.37, 0.41);
        let edge = h.app.doc.model.add_edge(start, start + DVec3::X).unwrap();
        h.app.set_tool(Tool::Move);
        h.app.selected.insert(edge);
        h.click(h.at(start));
        let anchor = h.app.modeling.as_ref().unwrap().anchor;
        h.app.doc.snap_to_geometry = false;
        h.app.doc.snap_to_axes = false;
        h.app.snap_to_grid = true;
        h.key(first_key);
        h.key(second_key);
        let delta = AxisConstraint::Plane(normal).project(DVec3::new(2.1, 1.7, 1.3));
        let cursor = h.at(anchor + delta);
        h.frame(
            vec![egui::Event::PointerMoved(cursor)],
            egui::Modifiers::NONE,
        );
        let edit = h.app.modeling.as_ref().unwrap();
        assert!(edit.direction.dot(normal.vector()).abs() < 1e-7);
        assert!(edit.direction.length() > 1.);
        h.app.length_input = "2m".into();
        h.key(Key::Enter);
        let (moved, _) = h.app.doc.model.points(&h.app.doc.model.edges[&edge]);
        assert!((moved - start).dot(normal.vector()).abs() < 1e-7);
        assert!((moved.distance(start) - 2.).abs() < 1e-6);
        assert_eq!((h.app.lock, h.app.second_lock), (None, None));
    }
}

#[test]
fn two_axis_locks_project_dimensions_without_moving_their_endpoints() {
    let mut h = Harness::new();
    let (a, b) = (DVec3::new(-1., -1., 1.), DVec3::new(2., 3., 3.));
    h.app.doc.model.add_edge(a, b).unwrap();
    h.app.set_tool(Tool::Dimensions);
    h.click(h.at(a));
    h.click(h.at(b));
    h.key(Key::ArrowRight);
    h.key(Key::ArrowLeft);
    assert_eq!(h.app.measurement_value().as_deref(), Some("5.000 m"));
    h.click(h.at(DVec3::new(3., -2., 3.)));
    let dimension = h.app.doc.model.dimensions.values().next().unwrap();
    assert_eq!(dimension.points(&h.app.doc.model), (a, b));
    assert_eq!(dimension.plane, Some(Axis::Z));
    assert!((dimension.length(&h.app.doc.model) - 5.).abs() < 1e-7);
    assert_eq!((h.app.lock, h.app.second_lock), (None, None));
    let saved = serde_json::to_string(&h.app.doc).unwrap();
    let restored: Document = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored.model.dimensions, h.app.doc.model.dimensions);
}

#[test]
fn two_axis_locks_constrain_push_pull_and_restore_the_original_normal() {
    for (_, first_key, _, second_key, normal) in PAIRS {
        let mut h = Harness::new();
        let points = [
            DVec3::new(1., 1., 3.),
            DVec3::new(4., 1., 0.),
            DVec3::new(4., 4., -3.),
            DVec3::new(1., 4., 0.),
        ];
        for i in 0..4 {
            h.app
                .doc
                .model
                .add_edge(points[i], points[(i + 1) % 4])
                .unwrap();
        }
        h.app.doc.snap_to_geometry = false;
        h.app.doc.snap_to_axes = false;
        h.app.set_tool(Tool::PushPull);
        h.click(h.at(DVec3::new(2.5, 2.5, 0.)));
        let original = h.app.modeling.as_ref().unwrap().direction;
        h.key(first_key);
        h.key(second_key);
        h.app.length_input = "2m".into();
        h.app.update_modeling(h.at(DVec3::new(2.5, 2.5, 2.)));
        let edit = h.app.modeling.as_ref().unwrap();
        assert!(edit.direction.dot(normal.vector()).abs() < 1e-7);
        assert!(
            edit.direction
                .distance(AxisConstraint::Plane(normal).project(original).normalize())
                < 1e-7
        );
        let preview = edit.preview.as_ref().unwrap().clone();
        h.key(Key::ArrowDown);
        assert_eq!(h.app.modeling.as_ref().unwrap().direction, original);
        h.key(first_key);
        h.key(second_key);
        h.key(Key::Enter);
        assert!(h.app.modeling.is_none());
        assert_eq!(h.app.doc.model, preview);
    }
}

#[test]
fn two_axis_locks_constrain_text_and_measurement_points() {
    for tool in [Tool::Text, Tool::TapeMeasure, Tool::Protractor] {
        for (_, first_key, _, second_key, normal) in PAIRS {
            let mut h = Harness::new();
            h.app.set_tool(tool);
            h.app.doc.snap_to_geometry = false;
            h.app.doc.snap_to_axes = false;
            h.app.snap_to_grid = true;
            let anchor = DVec3::new(0.23, 0.37, 0.41);
            h.app.measurement = Some(measurement_tools::Measurement {
                points: vec![anchor],
                ..Default::default()
            });
            h.key(first_key);
            h.key(second_key);
            let target = anchor + AxisConstraint::Plane(normal).project(DVec3::new(2., 3., 4.));
            h.frame(
                vec![egui::Event::PointerMoved(h.at(target))],
                egui::Modifiers::NONE,
            );
            assert!(
                (h.app.snap.unwrap().point - anchor)
                    .dot(normal.vector())
                    .abs()
                    < 1e-7
            );
            assert_eq!(h.app.measurement.as_ref().unwrap().points, vec![anchor]);
        }
    }
}
