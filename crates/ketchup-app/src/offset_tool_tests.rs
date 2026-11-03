use super::*;

fn begin_offset(h: &mut Harness) -> DVec3 {
    modeling_rectangle(h);
    h.app.set_tool(Tool::Offset);
    let anchor = DVec3::new(2., 0.25, 0.);
    h.click(h.at(anchor));
    h.app.modeling.as_ref().unwrap().anchor
}

#[test]
fn offset_single_face_groups_preserve_selection_context_cancel_and_undo() {
    for component in [false, true] {
        for nested in [false, true] {
            let mut h = Harness::new();
            modeling_rectangle(&mut h);
            let inner = h
                .app
                .doc
                .model
                .make_object(&h.app.doc.model.selectable_entities(), component, "Panel")
                .unwrap();
            let group = if nested {
                h.app
                    .doc
                    .model
                    .make_object(&BTreeSet::from([inner]), false, "Assembly")
                    .unwrap()
            } else {
                inner
            };
            let face = *h.app.doc.model.faces.keys().next().unwrap();
            let inner = h.app.doc.model.parent(face).unwrap();
            h.app.selected = BTreeSet::from([group]);
            h.app.set_tool(Tool::Offset);
            assert_eq!(h.app.selected_modeling_face(), Some(face));
            let before = h.app.doc.model.clone();
            let at = h.at(DVec3::new(2., 0.25, 0.));
            h.click(at);
            assert_eq!(h.app.hover, Some(face));
            assert_eq!(h.app.modeling.as_ref().unwrap().face, Some(face));
            h.app.length_input = "0.5m".into();
            h.app.update_modeling(at);
            let preview = h.app.modeling.as_ref().unwrap().preview.clone().unwrap();
            assert_eq!(preview.edit_context, None);
            assert_eq!(preview.parent(face), Some(inner));
            assert!(preview.object_contents(inner).unwrap().faces.len() > 1);
            assert_eq!(h.app.doc.model, before);
            h.key(Key::Escape);
            assert_eq!(h.app.doc.model, before);
            assert_eq!(h.app.selected, BTreeSet::from([group]));
            h.click(at);
            modeling_number(&mut h, "0.5m");
            assert_eq!(h.app.doc.model, preview);
            assert_eq!(h.app.selected, BTreeSet::from([group]));
            assert_eq!(h.app.selected_modeling_face(), None);
            h.app.doc.model.validate().unwrap();
            h.app.undo(&h.ctx, false);
            assert_eq!(h.app.doc.model, before);
        }
    }
}

#[test]
fn offset_does_not_auto_select_hidden_locked_or_multiple_group_faces() {
    let mut h = Harness::new();
    modeling_rectangle(&mut h);
    let group = h
        .app
        .doc
        .model
        .make_object(&h.app.doc.model.selectable_entities(), false, "Panel")
        .unwrap();
    let face = *h.app.doc.model.faces.keys().next().unwrap();
    h.app.selected = BTreeSet::from([group]);
    h.app.set_tool(Tool::Offset);
    for id in [group, face] {
        h.app.doc.model.hidden.insert(id);
        assert_eq!(h.app.selected_modeling_face(), None);
        h.app.doc.model.hidden.remove(&id);
        h.app.doc.model.locked.insert(id);
        assert_eq!(h.app.selected_modeling_face(), None);
        h.app.doc.model.locked.remove(&id);
    }
    h.app.doc.model.edit_context = Some(group);
    h.app.doc.model.offset_face(face, 0.5).unwrap();
    h.app.doc.model.edit_context = None;
    assert_eq!(h.app.selected_modeling_face(), None);
    h.click(h.at(DVec3::new(2., 0.25, 0.)));
    assert!(h.app.modeling.is_none());
}

#[test]
fn offset_locks_work_before_start_and_refresh_outside_the_viewport() {
    for before_start in [false, true] {
        let mut h = Harness::new();
        modeling_rectangle(&mut h);
        h.app.set_tool(Tool::Offset);
        h.app.doc.snap_to_geometry = false;
        h.app.doc.snap_to_axes = false;
        if before_start {
            h.key(Key::ArrowLeft);
        }
        h.click(h.at(DVec3::new(2., 0.25, 0.)));
        if !before_start {
            h.key(Key::ArrowLeft);
        }
        let anchor = h.app.modeling.as_ref().unwrap().anchor;
        let target = anchor + DVec3::Y * 0.8;
        h.frame(
            vec![egui::Event::PointerMoved(h.at(target))],
            egui::Modifiers::NONE,
        );
        let edit = h.app.modeling.as_ref().unwrap();
        assert_eq!(edit.snap.unwrap().kind, Inference::Axis(Axis::Y));
        assert!(edit.snap.unwrap().point.distance(target) < 1e-6);
        assert!((edit.value - 0.8).abs() < 1e-6);
        let preview = edit.preview.clone().unwrap();
        // Toolbar changes must refresh even with the pointer over the toolbar.
        h.frame(
            vec![egui::Event::PointerMoved(egui::pos2(2., 2.))],
            egui::Modifiers::NONE,
        );
        h.app.set_axis_lock(Some(Axis::X), &h.ctx);
        let edit = h.app.modeling.as_ref().unwrap();
        assert!(edit.value.abs() < 1e-6);
        assert!(edit.preview.is_none());
        h.app.set_axis_lock(Some(Axis::Y), &h.ctx);
        assert_eq!(
            h.app.modeling.as_ref().unwrap().preview.as_ref(),
            Some(&preview)
        );
        h.key(Key::ArrowDown);
        assert_eq!(h.app.axis_constraint(), None);
        assert!((h.app.modeling.as_ref().unwrap().value - 0.8).abs() < 1e-6);
        let preview = h.app.modeling.as_ref().unwrap().preview.clone().unwrap();
        h.click(h.at(target));
        assert_eq!(h.app.doc.model, preview);
    }
}

#[test]
fn offset_measurement_respects_each_axis_and_two_axis_plane() {
    for (axis, key, u, other_key) in [
        (Axis::X, Key::ArrowRight, DVec3::Y, Key::ArrowLeft),
        (Axis::Y, Key::ArrowLeft, DVec3::Z, Key::ArrowUp),
        (Axis::Z, Key::ArrowUp, DVec3::X, Key::ArrowRight),
    ] {
        for plane_lock in [false, true] {
            let mut h = Harness::new();
            let v = axis.vector();
            let points = [DVec3::ZERO, u * 4., (u + v) * 4., v * 4.];
            for i in 0..4 {
                h.app
                    .doc
                    .model
                    .add_edge(points[i], points[(i + 1) % 4])
                    .unwrap();
            }
            h.app.set_tool(Tool::Offset);
            h.app.doc.snap_to_geometry = false;
            h.app.doc.snap_to_axes = false;
            h.click(h.at(u * 2. + v * 0.25));
            h.key(key);
            if plane_lock {
                h.key(other_key);
            }
            let anchor = h.app.modeling.as_ref().unwrap().anchor;
            let target = anchor + v * 0.8;
            h.frame(
                vec![egui::Event::PointerMoved(h.at(target))],
                egui::Modifiers::NONE,
            );
            let edit = h.app.modeling.as_ref().unwrap();
            assert!(
                h.app
                    .axis_constraint()
                    .unwrap()
                    .contains(edit.snap.unwrap().point - anchor)
            );
            assert!((edit.value - 0.8).abs() < 1e-6);
            let preview = edit.preview.clone().unwrap();
            h.click(h.at(target));
            assert_eq!(h.app.doc.model, preview);
            h.app.doc.model.validate().unwrap();
        }
    }
}

#[test]
fn offset_free_movement_keeps_vertical_and_tilted_face_planes() {
    for normal in [DVec3::Z, DVec3::X, DVec3::new(1., 1., 1.).normalize()] {
        let mut h = Harness::new();
        let u = (DVec3::Y - normal * normal.y).normalize();
        let v = normal.cross(u);
        let points = [DVec3::ZERO, u * 4., (u + v) * 4., v * 4.];
        for i in 0..4 {
            h.app
                .doc
                .model
                .add_edge(points[i], points[(i + 1) % 4])
                .unwrap();
        }
        h.app.set_tool(Tool::Offset);
        h.app.doc.snap_to_geometry = false;
        h.app.doc.snap_to_axes = false;
        h.click(h.at(u * 2. + v * 0.25));
        let anchor = h.app.modeling.as_ref().unwrap().anchor;
        let target = anchor + v * 0.6;
        let cursor = h.at(target);
        let ray = h.app.doc.camera.ray(
            dv(cursor - h.app.viewport.min), dv(h.app.viewport.size()),
        );
        // Reconstruct the expected plane hit from the actual f32 cursor;
        // a tilted plane amplifies screen-coordinate rounding in world space.
        let expected = ray.origin + ray.direction
            * ((anchor - ray.origin).dot(normal) / ray.direction.dot(normal));
        h.app.update_modeling(cursor);
        let edit = h.app.modeling.as_ref().unwrap();
        assert!(edit.snap.unwrap().point.distance(expected) < 1e-9);
        assert!((edit.value - (expected - anchor).dot(edit.direction)).abs() < 1e-9);
        assert!((edit.value - 0.6).abs() < 1e-5);
        assert!(
            edit.preview
                .as_ref()
                .unwrap()
                .vertices
                .values()
                .all(|vertex| vertex.position.dot(normal).abs() < 1e-6)
        );
    }
}

#[test]
fn offset_plane_locks_and_grid_preserve_the_anchor_and_typed_distance() {
    let mut h = Harness::new();
    let anchor = begin_offset(&mut h);
    h.app.doc.snap_to_geometry = false;
    h.app.doc.snap_to_axes = false;
    h.app.snap_to_grid = true;
    h.app.doc.display.grid_auto_size = false;
    h.app.doc.display.grid_spacing = 0.5;
    h.key(Key::ArrowLeft);
    h.app.update_modeling(h.at(anchor + DVec3::Y * 0.9));
    let snap = h.app.modeling.as_ref().unwrap().snap.unwrap();
    assert_eq!(snap.kind, Inference::Axis(Axis::Y));
    assert!(snap.point.distance(DVec3::new(anchor.x, 1., anchor.z)) < 1e-6);
    h.key(Key::ArrowRight);
    assert_eq!(h.app.plane_lock(), Some(Axis::Z));
    h.app.update_modeling(h.at(DVec3::new(2.3, 1.13, 0.)));
    let edit = h.app.modeling.as_ref().unwrap();
    assert!(edit.snap.unwrap().point.distance(DVec3::new(2.5, 1., 0.)) < 1e-6);
    assert!((edit.value - (1. - anchor.y)).abs() < 1e-6);
    h.app.length_input = "37cm".into();
    h.app.update_modeling(h.at(DVec3::new(2.3, 1.13, 0.)));
    let edit = h.app.modeling.as_ref().unwrap();
    assert!(edit.snap.is_none());
    let mut expected = edit.base.clone();
    expected.offset_face(edit.face.unwrap(), 0.37).unwrap();
    assert_eq!(edit.preview.as_ref(), Some(&expected));
    h.key(Key::Enter);
    assert_eq!(h.app.doc.model, expected);
}

#[test]
fn offset_uses_geometry_preferences_and_exact_targets_take_priority_over_grid() {
    let mut h = Harness::new();
    let a = DVec3::new(1., 1.13, 0.7);
    let b = DVec3::new(3., 1.13, 0.7);
    h.app.doc.model.add_edge(a, b).unwrap();
    let anchor = begin_offset(&mut h);
    h.app.doc.snap_to_axes = false;
    h.app.snap_to_grid = true;
    h.app.doc.display.grid_auto_size = false;
    h.app.doc.display.grid_spacing = 0.5;
    h.app.update_modeling(h.at(a));
    let edit = h.app.modeling.as_ref().unwrap();
    assert_eq!(edit.snap.unwrap().kind, Inference::Endpoint);
    assert!(edit.snap.unwrap().point.distance(a) < 1e-6);
    assert!((edit.value - (a.y - anchor.y)).abs() < 1e-6);
    h.app.snap_to_edge = false;
    h.app.update_modeling(h.at(a));
    assert_eq!(
        h.app.modeling.as_ref().unwrap().snap.unwrap().kind,
        Inference::Plane
    );
    h.app.snap_to_edge = true;
    h.app.update_modeling(h.at(a.lerp(b, 0.5)));
    assert_eq!(
        h.app.modeling.as_ref().unwrap().snap.unwrap().kind,
        Inference::Midpoint
    );
    // The source face and its boundary must not consume the free pointer target.
    h.app.snap_to_grid = false;
    h.app.update_modeling(h.at(DVec3::new(1.1, 0., 0.)));
    assert_eq!(
        h.app.modeling.as_ref().unwrap().snap.unwrap().kind,
        Inference::Plane
    );
}

#[test]
fn offset_respects_face_and_face_center_snapping_preferences() {
    let mut h = Harness::new();
    let points = [
        DVec3::new(1., 0.8, 0.7),
        DVec3::new(3., 0.8, 0.7),
        DVec3::new(3., 2.8, 0.7),
        DVec3::new(1., 2.8, 0.7),
    ];
    for i in 0..4 {
        h.app
            .doc
            .model
            .add_edge(points[i], points[(i + 1) % 4])
            .unwrap();
    }
    begin_offset(&mut h);
    h.app.doc.snap_to_axes = false;
    h.app.snap_to_edge = false;
    h.app.snap_to_face = false;
    let center = DVec3::new(2., 1.8, 0.7);
    h.app.update_modeling(h.at(center));
    assert_eq!(
        h.app.modeling.as_ref().unwrap().snap.unwrap().kind,
        Inference::FaceCenter
    );
    h.app.snap_to_face_center = false;
    h.app.update_modeling(h.at(center));
    assert_eq!(
        h.app.modeling.as_ref().unwrap().snap.unwrap().kind,
        Inference::Plane
    );
    h.app.snap_to_face = true;
    let target = DVec3::new(1.4, 1.4, 0.7);
    h.app.update_modeling(h.at(target));
    let snap = h.app.modeling.as_ref().unwrap().snap.unwrap();
    assert_eq!(snap.kind, Inference::OnFace);
    assert!(snap.point.distance(target) < 1e-6);
}

#[test]
fn offset_automatic_axis_snapping_can_be_disabled() {
    let mut h = Harness::new();
    let anchor = begin_offset(&mut h);
    h.app.doc.snap_to_geometry = false;
    let cursor = h.at(anchor + DVec3::Y * 0.8) + egui::vec2(1., 0.);
    h.app.update_modeling(cursor);
    assert_eq!(
        h.app.modeling.as_ref().unwrap().snap.unwrap().kind,
        Inference::Axis(Axis::Y)
    );
    h.app.doc.snap_to_axes = false;
    h.app.update_modeling(cursor);
    assert_eq!(
        h.app.modeling.as_ref().unwrap().snap.unwrap().kind,
        Inference::Plane
    );
}
