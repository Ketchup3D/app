use super::*;

fn hover_anchor(h: &mut Harness, point: DVec3) -> (Pos2, Snap) {
    let cursor = h.at(point);
    h.frame(
        vec![egui::Event::PointerMoved(cursor)],
        egui::Modifiers::NONE,
    );
    (cursor, h.app.snap.expect("anchor preview"))
}

fn has_start_marker(output: &egui::FullOutput, at: Pos2) -> bool {
    fn contains(shape: &egui::Shape, at: Pos2) -> bool {
        match shape {
            egui::Shape::Circle(circle) => {
                circle.center.distance(at) < 0.01
                    && circle.radius == 3.5
                    && circle.stroke.width == 1.5
            }
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| contains(shape, at)),
            _ => false,
        }
    }
    output.shapes.iter().any(|shape| contains(&shape.shape, at))
}

#[test]
fn modeling_anchor_preview_and_first_click_share_exact_snaps_inside_groups() {
    for tool in [Tool::Move, Tool::Rotate, Tool::Scale, Tool::Offset, Tool::PushPull] {
        for grouped in [false, true] {
            let mut h = Harness::new();
            let face = modeling_rectangle(&mut h);
            let selected = if grouped {
                h.app
                    .doc
                    .model
                    .make_object(&h.app.doc.model.selectable_entities(), false, "Panel")
                    .unwrap()
            } else {
                face
            };
            h.app.selected = BTreeSet::from([selected]);
            h.app.set_tool(tool);
            h.app.snap_to_grid = true;
            h.app.doc.display.grid_auto_size = false;
            h.app.doc.display.grid_spacing = 0.7;
            let (cursor, snap) = hover_anchor(&mut h, DVec3::new(3.97, 0.03, 0.));
            assert_eq!(
                snap.kind,
                Inference::Endpoint,
                "{tool:?}, grouped={grouped}"
            );
            assert_eq!(snap.point, DVec3::new(4., 0., 0.));
            let output = h.click(cursor);
            assert_eq!(h.app.modeling.as_ref().unwrap().anchor, snap.point);
            assert!(has_start_marker(&output, h.at(snap.point)));
            h.app.doc.display.guides = false;
            h.app.length_input = "0.5".into();
            let output = h.frame(
                vec![egui::Event::PointerMoved(h.at(DVec3::new(3., 1., 0.)))],
                egui::Modifiers::NONE,
            );
            assert!(has_start_marker(&output, h.at(snap.point)));
        }
    }
}

#[test]
fn face_anchors_obey_edge_face_center_and_axis_preferences() {
    for tool in [Tool::Offset, Tool::PushPull] {
        let mut h = Harness::new();
        modeling_rectangle(&mut h);
        h.app.set_tool(tool);
        h.app.doc.snap_to_axes = false;
        let near_edge = DVec3::new(1.3, 0.02, 0.);
        let (_, snap) = hover_anchor(&mut h, near_edge);
        assert_eq!(snap.kind, Inference::OnEdge);
        assert!(snap.point.y.abs() < 1e-6);
        h.app.snap_to_edge = false;
        let (_, snap) = hover_anchor(&mut h, near_edge);
        assert_eq!(snap.kind, Inference::OnFace);
        assert!(snap.point.distance(near_edge) < 1e-6);
        let center = DVec3::new(2., 2., 0.);
        assert_eq!(hover_anchor(&mut h, center).1.kind, Inference::FaceCenter);
        h.app.snap_to_face_center = false;
        assert_eq!(hover_anchor(&mut h, center).1.kind, Inference::OnFace);
        h.app.snap_to_face = false;
        h.app.sync_geometry_snapping();
        assert_eq!(hover_anchor(&mut h, center).1.kind, Inference::Plane);
        h.app.doc.snap_to_axes = true;
        let (_, snap) = hover_anchor(&mut h, near_edge);
        assert_eq!(snap.kind, Inference::OnAxis(Axis::X));
        assert!(snap.point.y.abs() < 1e-6);
        h.app.doc.snap_to_axes = false;
        let (_, snap) = hover_anchor(&mut h, near_edge);
        assert_eq!(snap.kind, Inference::Plane);
        assert!(snap.point.distance(near_edge) < 1e-6);
    }
}

#[test]
fn face_anchor_grid_snapping_stays_on_vertical_and_tilted_faces() {
    for tool in [Tool::Offset, Tool::PushPull] {
        for normal in [DVec3::X, DVec3::new(1., 1., 1.).normalize()] {
            let mut h = Harness::new();
            let origin = normal * 0.37;
            let u = (DVec3::Y - normal * normal.y).normalize();
            let v = normal.cross(u);
            let points = [
                origin,
                origin + u * 4.,
                origin + (u + v) * 4.,
                origin + v * 4.,
            ];
            for i in 0..4 {
                h.app
                    .doc
                    .model
                    .add_edge(points[i], points[(i + 1) % 4])
                    .unwrap();
            }
            let face = *h.app.doc.model.faces.keys().next().unwrap();
            h.app.set_tool(tool);
            h.app.doc.snap_to_geometry = false;
            h.app.doc.snap_to_axes = false;
            h.app.snap_to_grid = true;
            h.app.doc.display.grid_auto_size = false;
            h.app.doc.display.grid_spacing = 0.5;
            let point = origin + u * 1.13 + v * 0.83;
            let (cursor, snap) = hover_anchor(&mut h, point);
            assert!((snap.point - origin).dot(normal).abs() < 1e-6);
            assert!(
                h.app
                    .doc
                    .model
                    .face_contains(&h.app.doc.model.faces[&face], snap.point, true)
            );
            assert!(snap.point.distance(point) > 0.05);
            h.click(cursor);
            assert_eq!(h.app.modeling.as_ref().unwrap().anchor, snap.point);
        }
    }
}

#[test]
fn offset_boundary_anchor_uses_the_inward_direction() {
    let mut h = Harness::new();
    modeling_rectangle(&mut h);
    h.app.set_tool(Tool::Offset);
    let (cursor, snap) = hover_anchor(&mut h, DVec3::new(2.01, 0.02, 0.));
    assert_eq!(snap.kind, Inference::Midpoint);
    assert_eq!(snap.point, DVec3::new(2., 0., 0.));
    h.click(cursor);
    assert!(
        h.app
            .modeling
            .as_ref()
            .unwrap()
            .direction
            .distance(DVec3::Y)
            < 1e-7
    );
    h.app.doc.snap_to_geometry = false;
    h.app.doc.snap_to_axes = false;
    h.app.update_modeling(h.at(snap.point + DVec3::Y * 0.5));
    let edit = h.app.modeling.as_ref().unwrap();
    assert!((edit.value - 0.5).abs() < 1e-6);
    assert!(edit.preview.is_some());
}

#[test]
fn live_modeling_markers_preserve_the_same_snap_kind_and_position_as_drawing() {
    for tool in [Tool::Move, Tool::Scale, Tool::PushPull, Tool::Offset] {
        let mut h = Harness::new();
        let a = DVec3::new(1., 1.13, 2.);
        let b = DVec3::new(3., 1.13, 2.);
        h.app.doc.model.add_edge(a, b).unwrap();
        let face = modeling_rectangle(&mut h);
        h.app.selected = BTreeSet::from([face]);
        h.app.set_tool(tool);
        h.app.doc.snap_to_axes = false;
        h.click(h.at(DVec3::new(2., 0.25, 0.)));
        let target = a.lerp(b, 0.5);
        h.frame(
            vec![egui::Event::PointerMoved(h.at(target))],
            egui::Modifiers::NONE,
        );
        let snap = h.app.modeling.as_ref().unwrap().snap.unwrap();
        let marker = h.app.modeling_preview_snap().unwrap();
        assert_eq!(marker.kind, Inference::Midpoint, "{tool:?}");
        assert_eq!(marker.kind, snap.kind);
        assert_eq!(marker.point, snap.point);
        assert!(marker.point.distance(target) < 1e-6);
        h.app.doc.display.guides = false;
        h.frame(vec![], egui::Modifiers::NONE);
        assert_eq!(h.app.modeling_preview_snap().unwrap().point, marker.point);
    }
}

#[test]
fn typed_move_and_scale_markers_track_the_transformed_anchor_vertex() {
    for tool in [Tool::Move, Tool::Scale] {
        for constraint in [
            None,
            Some(AxisConstraint::Axis(Axis::X)),
            Some(AxisConstraint::Plane(Axis::Z)),
        ] {
            let mut h = Harness::new();
            let face = modeling_rectangle(&mut h);
            let point = DVec3::new(4., 0., 0.);
            let vertex = h
                .app
                .doc
                .model
                .vertices
                .values()
                .find(|v| v.position == point)
                .unwrap()
                .id;
            h.app.selected = BTreeSet::from([face]);
            h.app.set_tool(tool);
            h.click(h.at(point));
            assert_eq!(h.app.modeling.as_ref().unwrap().anchor, point);
            match constraint {
                Some(AxisConstraint::Axis(_)) => h.app.set_axis_lock(Some(Axis::X), &h.ctx),
                Some(AxisConstraint::Plane(_)) => {
                    h.key(Key::ArrowRight);
                    h.key(Key::ArrowLeft);
                }
                None => {}
            }
            h.app.doc.snap_to_geometry = false;
            h.app.doc.snap_to_axes = false;
            h.app.length_input = "2".into();
            h.frame(
                vec![egui::Event::PointerMoved(h.at(DVec3::new(5., 1., 1.)))],
                egui::Modifiers::NONE,
            );
            let edit = h.app.modeling.as_ref().unwrap();
            let marker = h.app.modeling_preview_snap().unwrap();
            let preview = edit.preview.as_ref().unwrap();
            assert!(
                marker.point.distance(preview.vertices[&vertex].position) < 1e-6,
                "{tool:?}, {constraint:?}"
            );
            let preview = preview.clone();
            h.key(Key::Enter);
            assert_eq!(h.app.doc.model, preview);
        }
    }
}

#[test]
fn face_tool_anchors_stay_visible_in_empty_space_without_starting_an_edit() {
    fn live_marker(shape: &egui::Shape, at: Pos2) -> bool {
        match shape {
            egui::Shape::Circle(circle) => {
                circle.center.distance(at) < 0.01
                    && circle.radius == 4.
                    && circle.stroke.width == 1.5
            }
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| live_marker(shape, at)),
            _ => false,
        }
    }
    for tool in [Tool::PushPull, Tool::Offset] {
        for grid in [false, true] {
            let mut h = Harness::new();
            h.app.doc.snap_to_geometry = false;
            h.app.doc.snap_to_axes = false;
            h.app.snap_to_grid = grid;
            h.app.doc.display.grid_auto_size = false;
            h.app.doc.display.grid_spacing = 0.5;
            let before = h.app.doc.model.clone();
            let point = DVec3::new(1.13, -1.17, 0.);
            h.app.set_tool(Tool::Line);
            let (cursor, drawing_snap) = hover_anchor(&mut h, point);
            h.app.set_tool(tool);
            let output = h.frame(
                vec![egui::Event::PointerMoved(cursor)],
                egui::Modifiers::NONE,
            );
            let snap = h.app.snap.expect("free anchor over the drawing plane");
            assert_eq!(snap.kind, drawing_snap.kind);
            assert_eq!(snap.point, drawing_snap.point);
            assert!(
                output
                    .shapes
                    .iter()
                    .any(|shape| live_marker(&shape.shape, h.at(snap.point)))
            );
            assert!(h.app.hover.is_none());
            h.click(cursor);
            assert!(h.app.modeling.is_none());
            assert_eq!(h.app.doc.model, before);

            // A cursor above the horizon has no valid drawing-plane target.
            h.app.doc.camera.pitch = 0.;
            let cursor = h.app.viewport.center() - egui::vec2(0., 100.);
            let output = h.frame(
                vec![egui::Event::PointerMoved(cursor)],
                egui::Modifiers::NONE,
            );
            assert!(h.app.snap.is_none());
            assert!(
                output
                    .shapes
                    .iter()
                    .any(|shape| live_marker(&shape.shape, cursor))
            );
            h.click(cursor);
            assert!(h.app.modeling.is_none());
            assert_eq!(h.app.doc.model, before);
            assert!(h.app.doc.history.undo_label().is_none());
        }
    }
}
