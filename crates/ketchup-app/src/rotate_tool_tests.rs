use super::*;

fn start(h: &mut Harness, pivot: DVec3, u: DVec3, keys: &[Key]) -> (u64, u64) {
    let edge = h
        .app
        .doc
        .model
        .add_edge(pivot + u * 2., pivot + u * 3.)
        .unwrap();
    // A separate target proves the pivot can be offset from the selection in 3D.
    h.app.doc.model.add_edge(pivot, pivot - DVec3::ONE).unwrap();
    h.app.set_tool(Tool::Rotate);
    h.app.doc.snap_to_axes = false;
    h.app.selected = BTreeSet::from([edge]);
    for &key in keys {
        h.key(key);
    }
    h.click(h.at(pivot));
    assert!(h.app.modeling.as_ref().unwrap().anchor.distance(pivot) < 1e-6);
    assert!(
        h.app
            .modeling
            .as_ref()
            .unwrap()
            .rotation
            .as_ref()
            .unwrap()
            .reference
            .is_none()
    );
    h.click(h.at(pivot + u * 2.));
    let edit = h.app.modeling.as_ref().unwrap();
    assert!(
        edit.rotation
            .as_ref()
            .unwrap()
            .reference
            .unwrap()
            .distance(u * 2.)
            < 1e-6
    );
    (edge, edit.base.edges[&edge].a)
}

#[test]
fn rotate_is_directly_under_move_and_has_a_keyboard_shortcut() {
    let index = Tool::RAIL
        .iter()
        .position(|tool| *tool == Tool::Move)
        .unwrap();
    assert_eq!(Tool::RAIL[index + 1], Tool::Rotate);
    let mut h = Harness::new();
    h.key(Key::Q);
    assert_eq!(h.app.tool, Tool::Rotate);
}

#[test]
fn offset_pivot_snapped_reference_and_destination_commit_one_undo_step() {
    let mut h = Harness::new();
    let pivot = DVec3::new(-0.7, -0.5, 0.4);
    let (edge, vertex) = start(&mut h, pivot, DVec3::X, &[Key::ArrowUp]);
    let target = pivot + DVec3::Y * 2.;
    h.app.doc.model.add_edge(target, target + DVec3::Z).unwrap();
    // Restart so the gesture snapshot contains the destination target.
    h.key(Key::Escape);
    h.app.selected = BTreeSet::from([edge]);
    h.click(h.at(pivot));
    h.click(h.at(pivot + DVec3::X * 2.));
    let before = h.app.doc.model.clone();
    h.frame(
        vec![egui::Event::PointerMoved(h.at(target))],
        egui::Modifiers::NONE,
    );
    let edit = h.app.modeling.as_ref().unwrap();
    assert_eq!(edit.snap.unwrap().kind, Inference::Endpoint);
    assert!((edit.value - 90.).abs() < 1e-5);
    let preview = edit.preview.clone().unwrap();
    assert!(preview.vertices[&vertex].position.distance(target) < 1e-5);
    h.click(h.at(target));
    assert!(h.app.modeling.is_none());
    assert_eq!(h.app.doc.model, preview);
    assert_eq!(h.app.doc.history.undo_label(), Some("Rotate"));
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
    h.app.undo(&h.ctx, true);
    assert_eq!(h.app.doc.model, preview);
}

#[test]
fn every_single_axis_and_two_axis_plane_lock_rotates_typed_values_and_markers() {
    for (keys, normal, u) in [
        (vec![Key::ArrowRight], DVec3::X, DVec3::Y),
        (vec![Key::ArrowLeft], DVec3::Y, DVec3::Z),
        (vec![Key::ArrowUp], DVec3::Z, DVec3::X),
        (vec![Key::ArrowRight, Key::ArrowLeft], DVec3::Z, DVec3::X),
        (vec![Key::ArrowRight, Key::ArrowUp], DVec3::Y, DVec3::Z),
        (vec![Key::ArrowLeft, Key::ArrowUp], DVec3::X, DVec3::Y),
    ] {
        let mut h = Harness::new();
        let pivot = DVec3::new(0.23, 0.37, 0.41);
        let (_, vertex) = start(&mut h, pivot, u, &keys);
        h.app.length_input = "-90deg".into();
        h.app.update_modeling(h.at(pivot + u * 2.));
        let edit = h.app.modeling.as_ref().unwrap();
        assert_eq!(edit.rotation.as_ref().unwrap().normal, normal);
        let expected = pivot - normal.cross(u) * 2.;
        let preview = edit.preview.clone().unwrap();
        assert!(
            preview.vertices[&vertex].position.distance(expected) < 1e-6,
            "{keys:?}"
        );
        assert!(
            h.app
                .modeling_preview_snap()
                .unwrap()
                .point
                .distance(expected)
                < 1e-6
        );
        h.key(Key::Enter);
        assert_eq!(h.app.doc.model, preview);
        assert!(h.app.modeling.is_none());
    }
}

#[test]
fn changing_and_clearing_locks_recomputes_rotation_without_moving_the_pivot() {
    let mut h = Harness::new();
    let pivot = DVec3::new(0.23, 0.37, 0.41);
    let (_, vertex) = start(&mut h, pivot, DVec3::X, &[]);
    h.app.length_input = "90".into();
    h.key(Key::ArrowLeft);
    let edit = h.app.modeling.as_ref().unwrap();
    assert_eq!(edit.rotation.as_ref().unwrap().normal, DVec3::Y);
    assert!(
        edit.preview.as_ref().unwrap().vertices[&vertex]
            .position
            .distance(pivot - DVec3::Z * 2.)
            < 1e-6
    );
    h.key(Key::ArrowDown);
    let edit = h.app.modeling.as_ref().unwrap();
    assert_eq!(edit.rotation.as_ref().unwrap().normal, DVec3::Z);
    assert!(edit.anchor.distance(pivot) < 1e-6);
    assert!(
        edit.preview.as_ref().unwrap().vertices[&vertex]
            .position
            .distance(pivot + DVec3::Y * 2.)
            < 1e-6
    );
}

#[test]
fn rotation_grid_and_geometry_preferences_share_moves_target_rules() {
    let mut h = Harness::new();
    let (_, _) = start(&mut h, DVec3::ZERO, DVec3::X, &[Key::ArrowUp]);
    h.app.doc.snap_to_geometry = false;
    h.app.doc.snap_to_axes = false;
    h.app.snap_to_grid = true;
    h.app.doc.display.grid_auto_size = false;
    h.app.doc.display.grid_spacing = 0.5;
    h.app.update_modeling(h.at(DVec3::new(1.13, 1.37, 0.)));
    let edit = h.app.modeling.as_ref().unwrap();
    assert!(edit.snap.unwrap().point.distance(DVec3::new(1., 1.5, 0.)) < 1e-6);
    assert!((edit.value - 1.5_f64.atan2(1.).to_degrees()).abs() < 1e-5);
    h.app.snap_to_grid = false;
    h.app.update_modeling(h.at(DVec3::new(1.13, 1.37, 0.)));
    assert!(
        h.app
            .modeling
            .as_ref()
            .unwrap()
            .snap
            .unwrap()
            .point
            .distance(DVec3::new(1.13, 1.37, 0.))
            < 1e-5
    );
}

#[test]
fn invalid_angles_locked_geometry_and_escape_leave_document_and_history_untouched() {
    let mut h = Harness::new();
    let (edge, _) = start(&mut h, DVec3::ZERO, DVec3::X, &[]);
    let before = h.app.doc.model.clone();
    for input in ["bad", "NaN", "inf", "1e999", "90m", "1 / 0"] {
        h.app.length_input = input.into();
        h.key(Key::Enter);
        assert_eq!(h.app.doc.model, before);
        assert!(h.app.modeling.is_some());
        assert!(h.app.doc.history.undo_label().is_none());
    }
    h.key(Key::Escape);
    assert!(h.app.modeling.is_none());
    assert_eq!(h.app.doc.model, before);
    h.app.doc.model.locked.insert(edge);
    h.click(h.at(DVec3::X * 2.));
    assert!(h.app.modeling.is_none());
    assert!(h.app.doc.history.undo_label().is_none());
}

#[test]
fn rotation_acquires_its_own_reference_then_excludes_its_moving_geometry() {
    let mut h = Harness::new();
    start(&mut h, DVec3::ZERO, DVec3::X, &[Key::ArrowUp]);
    h.app.update_modeling(h.at(DVec3::X * 2.2));
    let edit = h.app.modeling.as_ref().unwrap();
    assert_eq!(edit.snap.unwrap().kind, Inference::Plane);
    assert!((edit.value).abs() < 1e-5);
    assert!(edit.error.is_none());
}

#[test]
fn rotation_infers_a_tilted_surface_and_keeps_grid_targets_in_that_plane() {
    let mut h = Harness::new();
    let u = DVec3::X;
    let v = (DVec3::Y + DVec3::Z).normalize();
    let normal = u.cross(v);
    let points = [DVec3::ZERO, u * 4., (u + v) * 4., v * 4.];
    for i in 0..4 {
        h.app
            .doc
            .model
            .add_edge(points[i], points[(i + 1) % 4])
            .unwrap();
    }
    let face = *h.app.doc.model.faces.keys().next().unwrap();
    h.app.set_tool(Tool::Rotate);
    h.app.doc.snap_to_axes = false;
    h.app.selected = BTreeSet::from([face]);
    let pivot = (u + v) * 2.;
    h.click(h.at(pivot));
    let edit = h.app.modeling.as_ref().unwrap();
    let actual_normal = edit.rotation.as_ref().unwrap().normal;
    assert!(actual_normal.cross(normal).length() < 1e-6);
    h.click(h.at(pivot + u * 2.));
    h.app.doc.snap_to_geometry = false;
    h.app.snap_to_grid = true;
    h.app.doc.display.grid_auto_size = false;
    h.app.doc.display.grid_spacing = 0.5;
    h.app.update_modeling(h.at(pivot + u * 1.13 + v * 0.83));
    let edit = h.app.modeling.as_ref().unwrap();
    assert!((edit.snap.unwrap().point - pivot).dot(normal).abs() < 1e-6);
    h.app.length_input = "90deg".into();
    let pointer = edit.pointer;
    h.app.update_modeling(pointer);
    let edit = h.app.modeling.as_ref().unwrap();
    let preview = edit.preview.clone().unwrap();
    let rotation = glam::DMat3::from_axis_angle(actual_normal, std::f64::consts::FRAC_PI_2);
    for (id, vertex) in &edit.base.vertices {
        let expected = pivot + rotation * (vertex.position - pivot);
        assert!(preview.vertices[id].position.distance(expected) < 1e-6);
    }
    h.key(Key::Enter);
    assert_eq!(h.app.doc.model, preview);
}
