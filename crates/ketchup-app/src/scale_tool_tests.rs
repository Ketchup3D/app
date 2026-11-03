use super::*;

fn selection(h: &mut Harness) -> u64 {
    let edge = h.app.doc.model.add_edge(-DVec3::ONE, DVec3::ONE).unwrap();
    h.app.set_tool(Tool::Scale);
    h.app.selected.insert(edge);
    h.click(h.at(DVec3::ONE));
    h.app.doc.snap_to_geometry = false;
    h.app.doc.snap_to_axes = false;
    edge
}

#[test]
fn scaling_past_the_center_uses_the_factor_magnitude_for_preview_and_commit() {
    let mut h = Harness::new();
    let edge = selection(&mut h);
    let before = h.app.doc.model.clone();
    h.key(Key::ArrowRight);
    let cursor = h.at(DVec3::new(-2., 1., 1.));
    h.frame(
        vec![egui::Event::PointerMoved(cursor)],
        egui::Modifiers::NONE,
    );
    let edit = h.app.modeling.as_ref().unwrap();
    assert!((edit.value - 2.).abs() < 1e-5);
    assert!(edit.error.is_none());
    let vertex = edit.base.edges[&edge].b;
    assert!(
        edit.preview.as_ref().unwrap().vertices[&vertex]
            .position
            .distance(DVec3::new(2., 1., 1.))
            < 1e-5
    );
    let preview = edit.preview.clone().unwrap();
    h.click(cursor);
    assert!(h.app.modeling.is_none());
    assert_eq!(h.app.doc.model, preview);
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
}

#[test]
fn negative_typed_scale_factors_use_their_magnitude_and_zero_still_rejects_collapse() {
    for keys in [
        vec![],
        vec![Key::ArrowRight],
        vec![Key::ArrowRight, Key::ArrowLeft],
    ] {
        for input in ["-0.5", "(1 - 2) / 2"] {
            let mut h = Harness::new();
            let edge = selection(&mut h);
            let before = h.app.doc.model.clone();
            for key in &keys {
                h.key(*key);
            }
            let pointer = h.app.modeling.as_ref().unwrap().pointer;
            h.app.length_input = "-0".into();
            h.app.update_modeling(pointer);
            let edit = h.app.modeling.as_ref().unwrap();
            assert!(edit.error.is_some());
            assert!(edit.preview.is_none());
            assert_eq!(h.app.doc.model, before);

            h.app.length_input = input.into();
            h.app.update_modeling(pointer);
            let edit = h.app.modeling.as_ref().unwrap();
            assert!(edit.error.is_none());
            let mask = edit
                .scale_constraint
                .map_or(DVec3::ONE, |lock| lock.project(DVec3::ONE));
            let vertex = edit.base.edges[&edge].b;
            let expected = DVec3::ONE - mask * 0.5;
            assert!(
                edit.preview.as_ref().unwrap().vertices[&vertex]
                    .position
                    .distance(expected)
                    < 1e-7
            );
            h.key(Key::Enter);
            assert!(h.app.modeling.is_none());
            assert!(
                h.app.doc.model.vertices[&vertex]
                    .position
                    .distance(expected)
                    < 1e-7
            );
        }
    }
}

#[test]
fn scale_axis_and_plane_locks_constrain_pointer_preview_typed_commit_and_history() {
    for keys in [
        vec![],
        vec![Key::ArrowRight],
        vec![Key::ArrowLeft],
        vec![Key::ArrowUp],
        vec![Key::ArrowRight, Key::ArrowLeft],
        vec![Key::ArrowRight, Key::ArrowUp],
        vec![Key::ArrowLeft, Key::ArrowUp],
    ] {
        let mut h = Harness::new();
        // Parallel projection makes a 1.5x world radius a 1.5x screen radius.
        h.app.doc.camera.projection = ketchup_core::Projection::Parallel;
        let edge = selection(&mut h);
        let before = h.app.doc.model.clone();
        for key in keys {
            h.key(key);
        }
        let mask = h
            .app
            .axis_constraint()
            .map_or(DVec3::ONE, |c| c.project(DVec3::ONE));
        let target = DVec3::ONE + mask * 0.5;
        h.frame(
            vec![egui::Event::PointerMoved(h.at(target))],
            egui::Modifiers::NONE,
        );
        let edit = h.app.modeling.as_ref().unwrap();
        assert!(
            (edit.value - 1.5).abs() < 1e-5,
            "{:?}: {}",
            edit.scale_constraint,
            edit.value
        );
        let vertex = before.edges[&edge].b;
        assert!(
            edit.preview.as_ref().unwrap().vertices[&vertex]
                .position
                .distance(target)
                < 1e-5
        );

        h.app.length_input = "2".into();
        h.key(Key::Enter);
        assert!(h.app.modeling.is_none());
        assert!(
            h.app.doc.model.vertices[&vertex]
                .position
                .distance(DVec3::ONE + mask)
                < 1e-7
        );
        let after = h.app.doc.model.clone();
        h.app.undo(&h.ctx, false);
        assert_eq!(h.app.doc.model, before);
        h.app.undo(&h.ctx, true);
        assert_eq!(h.app.doc.model, after);
    }
}

#[test]
fn scale_lock_changes_recompute_preview_at_the_last_pointer() {
    let mut h = Harness::new();
    selection(&mut h);
    let cursor = h.at(DVec3::new(2., 1., 1.));
    h.frame(
        vec![egui::Event::PointerMoved(cursor)],
        egui::Modifiers::NONE,
    );
    h.key(Key::ArrowRight);
    let edit = h.app.modeling.as_ref().unwrap();
    assert_eq!(edit.scale_constraint, Some(AxisConstraint::Axis(Axis::X)));
    assert!((edit.value - 2.).abs() < 1e-5);
    h.key(Key::ArrowLeft);
    assert_eq!(
        h.app.modeling.as_ref().unwrap().scale_constraint,
        Some(AxisConstraint::Plane(Axis::Z))
    );
    h.key(Key::ArrowDown);
    let edit = h.app.modeling.as_ref().unwrap();
    assert_eq!(edit.scale_constraint, None);
    let vertex = edit.base.edges.values().next().unwrap().b;
    let p = edit.preview.as_ref().unwrap().vertices[&vertex].position;
    assert!((p.x - p.y).abs() < 1e-7 && (p.y - p.z).abs() < 1e-7);
}

#[test]
fn uniform_pointer_scale_can_shrink_a_raised_reference_toward_the_center() {
    let mut h = Harness::new();
    h.app.doc.camera.projection = ketchup_core::Projection::Parallel;
    let edge = selection(&mut h);
    let target = DVec3::ONE * 0.1;
    h.frame(
        vec![egui::Event::PointerMoved(h.at(target))],
        egui::Modifiers::NONE,
    );
    let edit = h.app.modeling.as_ref().unwrap();
    assert!((edit.value - 0.1).abs() < 1e-5);
    let vertex = edit.base.edges[&edge].b;
    assert!(
        edit.preview.as_ref().unwrap().vertices[&vertex]
            .position
            .distance(target)
            < 1e-5
    );
}

#[test]
fn unlocked_scale_responds_to_every_pointer_direction_without_a_fixed_reference_axis() {
    for projection in [
        ketchup_core::Projection::Perspective,
        ketchup_core::Projection::Parallel,
    ] {
        let mut h = Harness::new();
        let face = modeling_rectangle(&mut h);
        h.app.set_tool(Tool::Scale);
        h.app.selected.insert(face);
        h.app.doc.camera.projection = projection;
        h.app.doc.snap_to_geometry = false;
        h.app.doc.snap_to_axes = false;
        let center = h.at(DVec3::new(2., 2., 0.));
        let start = h.at(DVec3::new(4., 4., 0.));
        h.click(start);
        let radius = start.distance(center) * 1.25;
        for direction in [
            egui::vec2(1., 0.),
            egui::vec2(0., 1.),
            egui::vec2(-1., 0.),
            egui::vec2(0., -1.),
        ] {
            h.frame(
                vec![egui::Event::PointerMoved(center + direction * radius)],
                egui::Modifiers::NONE,
            );
            let edit = h.app.modeling.as_ref().unwrap();
            assert!(edit.scale_constraint.is_none());
            assert!(edit.error.is_none());
            assert!(
                (edit.value - 1.25).abs() < 1e-5,
                "{projection:?}, {direction:?}: {}",
                edit.value
            );
            let preview = edit.preview.as_ref().unwrap();
            assert!((preview.face_area(&preview.faces[&face]) - 25.).abs() < 1e-4);
            let target = h
                .app
                .doc
                .camera
                .project(edit.snap.unwrap().point, dv(h.app.viewport.size()))
                .unwrap();
            let cursor = dv(center + direction * radius - h.app.viewport.min);
            assert!(target.distance(cursor) < 1e-5);
        }
    }
}

#[test]
fn unlocked_scale_from_center_does_not_discard_motion_perpendicular_to_a_world_axis() {
    let mut h = Harness::new();
    let face = modeling_rectangle(&mut h);
    h.app.set_tool(Tool::Scale);
    h.app.selected.insert(face);
    h.app.doc.snap_to_geometry = false;
    h.app.doc.snap_to_axes = false;
    let start = h.at(DVec3::new(2., 2., 0.));
    h.click(start);
    let y = (h.at(DVec3::new(2., 4., 0.)) - start).normalized();
    for movement in [egui::vec2(30., 0.), egui::vec2(0., 30.)] {
        h.frame(
            vec![egui::Event::PointerMoved(start + movement)],
            egui::Modifiers::NONE,
        );
        let edit = h.app.modeling.as_ref().unwrap();
        assert!(edit.scale_constraint.is_none());
        assert!(edit.error.is_none());
        assert!(
            (edit.value - 1.).abs() > 1e-3,
            "{movement:?}: {}",
            edit.value
        );
    }
    let perpendicular = egui::vec2(-y.y, y.x) * 30.;
    let mut responds_to_perpendicular_motion = false;
    for movement in [perpendicular, -perpendicular] {
        h.frame(
            vec![egui::Event::PointerMoved(start + movement)],
            egui::Modifiers::NONE,
        );
        responds_to_perpendicular_motion |=
            (h.app.modeling.as_ref().unwrap().value - 1.).abs() > 1e-3;
    }
    assert!(responds_to_perpendicular_motion);
    h.frame(
        vec![egui::Event::PointerMoved(start)],
        egui::Modifiers::NONE,
    );
    assert!((h.app.modeling.as_ref().unwrap().value - 1.).abs() < 1e-5);
}

#[test]
fn unlocked_scale_keeps_exact_geometry_targets_when_grid_snapping_is_enabled() {
    let mut h = Harness::new();
    let edge = h.app.doc.model.add_edge(-DVec3::ONE, DVec3::ONE).unwrap();
    let target = DVec3::ONE * 2.;
    h.app.doc.model.add_edge(target, target + DVec3::X).unwrap();
    h.app.set_tool(Tool::Scale);
    h.app.selected.insert(edge);
    h.app.doc.snap_to_axes = false;
    h.app.snap_to_grid = true;
    h.click(h.at(DVec3::ONE));
    h.frame(
        vec![egui::Event::PointerMoved(h.at(target) + egui::vec2(2., 0.))],
        egui::Modifiers::NONE,
    );
    let edit = h.app.modeling.as_ref().unwrap();
    assert!(edit.scale_constraint.is_none());
    assert_eq!(edit.snap.unwrap().kind, Inference::Endpoint);
    assert!((edit.value - 2.).abs() < 1e-7);
    let vertex = edit.base.edges[&edge].b;
    assert!(
        edit.preview.as_ref().unwrap().vertices[&vertex]
            .position
            .distance(target)
            < 1e-7
    );
}

#[test]
fn scale_object_targets_win_over_grid_and_disabled_object_snapping_is_respected() {
    for geometry in [false, true] {
        let mut h = Harness::new();
        let a = DVec3::new(0.23, 0.37, 0.41);
        let b = a + DVec3::X * 2.;
        let target = b + DVec3::X * 2.13;
        let edge = h.app.doc.model.add_edge(a, b).unwrap();
        h.app.doc.model.add_edge(target, target + DVec3::Y).unwrap();
        h.app.set_tool(Tool::Scale);
        h.app.selected.insert(edge);
        h.click(h.at(b));
        h.app.doc.snap_to_geometry = geometry;
        h.app.doc.snap_to_axes = false;
        h.app.snap_to_grid = true;
        h.app.doc.display.grid_auto_size = false;
        h.app.doc.display.grid_spacing = 0.5;
        h.key(Key::ArrowRight);
        let cursor = h.at(target) + egui::vec2(2., 0.);
        h.frame(
            vec![egui::Event::PointerMoved(cursor)],
            egui::Modifiers::NONE,
        );
        let edit = h.app.modeling.as_ref().unwrap();
        let snap = edit.snap.unwrap();
        if geometry {
            assert_eq!(snap.kind, Inference::Endpoint);
            assert!(snap.point.distance(target) < 1e-7);
        } else {
            assert!((snap.point.x / 0.5 - (snap.point.x / 0.5).round()).abs() < 1e-7);
            assert!(snap.point.distance(target) > 0.01);
        }
        assert!((snap.point.y - b.y).abs() < 1e-7);
        assert!((snap.point.z - b.z).abs() < 1e-7);
        let vertex = edit.base.edges[&edge].b;
        assert!(
            edit.preview.as_ref().unwrap().vertices[&vertex]
                .position
                .distance(snap.point)
                < 1e-7
        );
        let preview = edit.preview.clone().unwrap();
        h.click(cursor);
        assert_eq!(h.app.doc.model, preview);
    }
}

#[test]
fn scale_from_center_has_a_usable_pointer_reference_and_typed_factors_override_snapping() {
    let mut h = Harness::new();
    let edge = h.app.doc.model.add_edge(-DVec3::X, DVec3::X).unwrap();
    h.app.set_tool(Tool::Scale);
    h.app.selected.insert(edge);
    h.click(h.at(DVec3::ZERO));
    h.app.doc.snap_to_geometry = false;
    h.app.doc.snap_to_axes = false;
    h.key(Key::ArrowRight);
    h.frame(
        vec![egui::Event::PointerMoved(h.at(DVec3::X))],
        egui::Modifiers::NONE,
    );
    assert!((h.app.modeling.as_ref().unwrap().value - 2.).abs() < 1e-5);
    h.app.snap_to_grid = true;
    h.app.length_input = "0.37".into();
    h.key(Key::Enter);
    let (a, b) = h.app.doc.model.points(&h.app.doc.model.edges[&edge]);
    assert!(a.distance(-DVec3::X * 0.37) < 1e-7);
    assert!(b.distance(DVec3::X * 0.37) < 1e-7);
}

#[test]
fn rectangle_scale_near_center_stays_proportional_for_three_pixel_movements() {
    for geometry in [false, true] {
        for distance in [18., 180.] {
            for keys in [
                vec![],
                vec![Key::ArrowRight],
                vec![Key::ArrowRight, Key::ArrowLeft],
            ] {
                for offset in [egui::vec2(0., 0.), egui::vec2(1., -1.), egui::vec2(5., 2.)] {
                    let mut h = Harness::new();
                    let face = modeling_rectangle(&mut h);
                    h.app.set_tool(Tool::Scale);
                    h.app.selected.insert(face);
                    h.app.doc.camera.distance = distance;
                    h.app.doc.snap_to_geometry = geometry;
                    h.app.doc.snap_to_axes = false;
                    for key in &keys {
                        h.key(*key);
                    }
                    let start = h.at(DVec3::new(2., 2., 0.)) + offset;
                    h.click(start);
                    let edit = h.app.modeling.as_ref().unwrap();
                    assert!(
                        (edit.value - 1.).abs() < 1e-5,
                        "initial factor: {}",
                        edit.value
                    );
                    for delta in [
                        egui::vec2(3., 0.),
                        egui::vec2(-3., 0.),
                        egui::vec2(0., 3.),
                        egui::vec2(0., -3.),
                    ] {
                        h.frame(
                            vec![egui::Event::PointerMoved(start + delta)],
                            egui::Modifiers::NONE,
                        );
                        let edit = h.app.modeling.as_ref().unwrap();
                        assert!(
                            (0.85..1.15).contains(&edit.value),
                            "geometry={geometry}, distance={distance}, keys={keys:?}, offset={offset:?}, delta={delta:?}: {}",
                            edit.value,
                        );
                        assert!(edit.preview.is_some());
                    }
                }
            }
        }
    }
}

#[test]
fn near_center_scale_uses_the_selection_extent_for_grid_and_exact_object_targets() {
    let mut h = Harness::new();
    let face = modeling_rectangle(&mut h);
    let center = DVec3::new(2., 2., 0.);
    let target = center + DVec3::X * 4.;
    h.app.doc.model.add_edge(target, target + DVec3::Z).unwrap();
    h.app.set_tool(Tool::Scale);
    h.app.selected.insert(face);
    h.app.doc.snap_to_axes = false;
    h.click(h.at(center) + egui::vec2(1., 0.));
    h.key(Key::ArrowRight);
    h.app.snap_to_grid = true;
    h.app.doc.display.grid_auto_size = false;
    h.app.doc.display.grid_spacing = 0.25;
    h.frame(
        vec![egui::Event::PointerMoved(h.at(target) + egui::vec2(2., 0.))],
        egui::Modifiers::NONE,
    );
    let edit = h.app.modeling.as_ref().unwrap();
    assert_eq!(edit.snap.unwrap().kind, Inference::Endpoint);
    assert!((edit.value - 3.).abs() < 1e-7);
}
