use super::*;

#[test]
fn guides_can_be_clicked_shift_selected_deleted_and_restored_with_undo() {
    let mut h = Harness::new();
    let first = h
        .app
        .doc
        .model
        .add_guide(-DVec3::Y, Some(DVec3::X))
        .unwrap();
    let second = h.app.doc.model.add_guide(DVec3::Y, Some(DVec3::X)).unwrap();
    h.app.set_tool(Tool::Select);
    // Click far along the infinite line, away from its stored origin.
    h.click(h.at(DVec3::new(2., -1., 0.)));
    assert_eq!(h.app.selected, BTreeSet::from([first]));
    let point = h.at(DVec3::new(2., 1., 0.));
    h.pointer_with_modifiers(
        point,
        egui::PointerButton::Primary,
        true,
        egui::Modifiers::SHIFT,
    );
    h.pointer_with_modifiers(
        point,
        egui::PointerButton::Primary,
        false,
        egui::Modifiers::SHIFT,
    );
    assert_eq!(h.app.selected, BTreeSet::from([first, second]));
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let _ = text_position(&output, "2 guides");
    let loaded = Document::from_json(&serde_json::to_string(&h.app.doc).unwrap()).unwrap();
    let restored = KetchupApp::with_document(loaded, None, true);
    assert_eq!(restored.selected, BTreeSet::from([first, second]));
    h.key(Key::Delete);
    assert!(h.app.doc.model.guides.is_empty());
    assert!(h.app.selected.is_empty());
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.guides.len(), 2);
    h.app.undo(&h.ctx, true);
    assert!(h.app.doc.model.guides.is_empty());
}

#[test]
fn hidden_guides_are_not_picked_and_selection_does_not_require_snapping() {
    let mut h = Harness::new();
    let id = h.app.doc.model.add_guide(DVec3::Y, Some(DVec3::X)).unwrap();
    h.app.set_tool(Tool::Select);
    h.app.doc.snap_to_geometry = false;
    let point = h.at(DVec3::new(1., 1., 0.));
    h.click(point);
    assert_eq!(h.app.selected, BTreeSet::from([id]));
    for hidden_by_option in [true, false] {
        h.app.selected.clear();
        h.app.doc.display.guides = !hidden_by_option;
        if !hidden_by_option {
            h.app.doc.model.hidden.insert(id);
        }
        h.click(point);
        assert!(h.app.selected.is_empty());
        let pixel = dv(point - h.app.viewport.min);
        assert!(
            h.app
                .pick_at(pixel, dv(h.app.viewport.size()), true)
                .is_none()
        );
    }
}

#[test]
fn marquee_selection_and_eraser_strokes_hit_the_visible_guide() {
    let mut h = Harness::new();
    let id = h
        .app
        .doc
        .model
        .add_guide(-DVec3::Y, Some(DVec3::X))
        .unwrap();
    h.app.set_tool(Tool::Select);
    let point = h.at(DVec3::new(1., -1., 0.));
    let start = point - egui::vec2(18., 18.);
    let end = point + egui::vec2(18., 18.);
    h.pointer(start, egui::PointerButton::Primary, true, false);
    h.pointer(end, egui::PointerButton::Primary, false, false);
    assert_eq!(h.app.selected, BTreeSet::from([id]));
    assert!(
        h.app
            .sweep_erase(point - egui::vec2(0., 20.), point + egui::vec2(0., 20.))
            .contains(&id)
    );
    h.app.set_tool(Tool::Erase);
    h.click(point);
    assert!(h.app.doc.model.guides.is_empty());
    h.app.undo(&h.ctx, false);
    assert!(h.app.doc.model.guides.contains_key(&id));
}

#[test]
fn selected_guides_move_with_an_exact_distance_and_do_not_snap_to_their_old_line() {
    let mut h = Harness::new();
    let id = h
        .app
        .doc
        .model
        .add_guide(-DVec3::Y, Some(DVec3::X))
        .unwrap();
    h.app.set_tool(Tool::Select);
    h.click(h.at(-DVec3::Y));
    h.app.set_tool(Tool::Move);
    h.click(h.at(DVec3::new(1., -1., 0.)));
    assert!(h.app.modeling.as_ref().unwrap().snap_excluded.contains(&id));
    h.frame(
        vec![egui::Event::PointerMoved(h.at(DVec3::new(1., -2., 0.)))],
        egui::Modifiers::NONE,
    );
    h.app.length_input = "500mm".into();
    h.key(Key::Enter);
    assert!(h.app.doc.model.guides[&id].origin.distance(-DVec3::Y * 1.5) < 1e-6);
    assert_eq!(h.app.doc.model.guides[&id].direction, Some(DVec3::X));
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.guides[&id].origin, -DVec3::Y);
}

#[test]
fn selected_guides_render_a_distinct_highlight() {
    let mut h = Harness::new();
    let id = h.app.doc.model.add_guide(DVec3::Y, Some(DVec3::X)).unwrap();
    h.app.set_tool(Tool::Select);
    h.click(h.at(DVec3::Y));
    assert_eq!(h.app.selected, BTreeSet::from([id]));
    let rect = h.app.viewport;
    let mut color = Color32::TRANSPARENT;
    let output = h.ctx.run_test_ui(egui::RawInput::default(), |ui| {
        color = Palette::for_ui(ui).accent;
        h.app.paint_construction_guides(ui.painter(), rect);
    });
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::LineSegment { stroke, .. } if stroke.color == color && stroke.width == 2.5)));
}

#[test]
fn tape_guide_follows_the_tape_from_an_edge_and_supports_exact_entry_and_undo() {
    let mut h = Harness::new();
    h.app
        .doc
        .model
        .add_edge(-DVec3::X * 2., DVec3::X * 2.)
        .unwrap();
    let geometry = h.app.doc.model.clone();
    h.app.set_tool(Tool::TapeMeasure);
    h.app.doc.snap_to_axes = false;
    h.click(h.at(DVec3::ZERO));
    h.frame(
        vec![egui::Event::PointerMoved(h.at(DVec3::new(0.5, -1., 0.)))],
        egui::Modifiers::NONE,
    );
    let tape_direction = h.app.snap.unwrap().point.normalize();
    assert!(tape_direction.x > 0.1 && tape_direction.y < -0.1);
    let preview = h.app.measurement_guide().unwrap();
    assert!(preview.direction.unwrap().distance(tape_direction) < 1e-6);
    h.app.length_input = "250mm".into();
    h.key(Key::Enter);
    let guide = h.app.doc.model.guides.values().next().unwrap();
    assert!(guide.direction.unwrap().distance(tape_direction) < 1e-6);
    assert!(guide.origin.distance(tape_direction * 0.25) < 1e-6);
    assert!(guide.origin.cross(guide.direction.unwrap()).length() < 1e-6);
    assert_eq!(h.app.doc.model.edges, geometry.edges);
    assert_eq!(h.app.doc.model.vertices, geometry.vertices);
    assert!(h.app.doc.model.faces.is_empty());
    h.app.doc.model.validate().unwrap();
    let saved = Document::from_json(&serde_json::to_string(&h.app.doc).unwrap()).unwrap();
    assert_eq!(saved.model.guides, h.app.doc.model.guides);
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, geometry);
    h.app.undo(&h.ctx, true);
    assert_eq!(h.app.doc.model.guides.len(), 1);
    // Exact entry after placement adjusts the same guide, without duplicating it.
    h.app.set_tool(Tool::TapeMeasure);
    h.click(h.at(DVec3::ZERO));
    h.click(h.at(DVec3::new(0., -1., 0.)));
    h.app.length_input = "500mm".into();
    h.key(Key::Enter);
    assert_eq!(h.app.doc.model.guides.len(), 2);
    assert!(
        h.app
            .doc
            .model
            .guides
            .values()
            .any(|g| (g.origin.y + 0.5).abs() < 1e-6)
    );
}

#[test]
fn tape_can_follow_the_starting_edge_instead_of_forcing_a_perpendicular_offset() {
    let mut h = Harness::new();
    h.app
        .doc
        .model
        .add_edge(-DVec3::X * 2., DVec3::X * 2.)
        .unwrap();
    h.app.set_tool(Tool::TapeMeasure);
    h.click(h.at(DVec3::ZERO));
    h.click(h.at(DVec3::X));
    let guide = h.app.doc.model.guides.values().next().unwrap();
    assert!(guide.direction.unwrap().dot(DVec3::X) > 1. - 1e-6);
    assert!(guide.origin.distance(DVec3::X) < 1e-6);
}

#[test]
fn tape_endpoint_creates_a_snappable_full_line_and_hidden_guides_do_not_snap() {
    let mut h = Harness::new();
    let start = DVec3::new(-2., 0., 0.);
    h.app
        .doc
        .model
        .add_edge(start, DVec3::new(-1., 0., 0.))
        .unwrap();
    h.app.set_tool(Tool::TapeMeasure);
    h.click(h.at(start));
    let point = DVec3::new(-2., -1., 0.);
    h.click(h.at(point));
    let guide = h.app.doc.model.guides.values().next().unwrap();
    assert!(guide.direction.unwrap().dot(-DVec3::Y) > 1. - 1e-6);
    assert!(guide.origin.distance(point) < 1e-6);
    h.app.set_tool(Tool::Line);
    let snap = h.app.infer_snap(h.at(point), None, None, false).unwrap();
    assert_eq!(snap.kind, Inference::OnGuide);
    assert!(snap.point.distance(point) < 1e-6);
    h.app.doc.display.guides = false;
    assert!(h.app.construction_snap(h.at(point), None, None).is_none());
}

#[test]
fn tape_free_point_and_vertical_offsets_create_lines_and_exact_entry_keeps_the_direction() {
    for (key, target) in [(Key::ArrowLeft, DVec3::Y), (Key::ArrowUp, DVec3::Z)] {
        let mut h = Harness::new();
        h.app.set_tool(Tool::TapeMeasure);
        h.click(h.at(DVec3::ZERO));
        h.key(key);
        h.frame(
            vec![egui::Event::PointerMoved(h.at(target))],
            egui::Modifiers::NONE,
        );
        let preview = h.app.measurement_guide().unwrap();
        let direction = preview.direction.expect("a free start must preview a line");
        assert!(direction.dot(target) > 1. - 1e-6);
        h.app.length_input = "250mm".into();
        h.key(Key::Enter);
        let guide = h.app.doc.model.guides.values().next().unwrap();
        assert!(guide.origin.distance(target * 0.25) < 1e-6);
        assert_eq!(guide.direction, Some(direction));
        let saved = Document::from_json(&serde_json::to_string(&h.app.doc).unwrap()).unwrap();
        assert_eq!(saved.model.guides, h.app.doc.model.guides);
    }
}

#[test]
fn guide_preview_and_saved_line_render_cyan_across_the_viewport_in_both_themes() {
    fn cyan_points(shape: &egui::Shape, points: &mut Vec<Pos2>) {
        match shape {
            egui::Shape::LineSegment {
                points: ends,
                stroke,
            } if stroke.color == Color32::from_rgb(0, 174, 214) => points.extend(ends),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    cyan_points(shape, points);
                }
            }
            _ => {}
        }
    }
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let mut h = Harness::new();
        h.ctx.set_theme(theme);
        h.app.set_tool(Tool::TapeMeasure);
        h.click(h.at(DVec3::ZERO));
        let end = h.at(-DVec3::Y);
        h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
        for saved in [false, true] {
            if saved {
                h.click(end);
                h.app.set_tool(Tool::Select);
            }
            let rect = h.app.viewport;
            let output = h.ctx.run_test_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, h.size)),
                    ..Default::default()
                },
                |ui| h.app.paint_construction_guides(ui.painter(), rect),
            );
            let mut points = vec![];
            for shape in &output.shapes {
                cyan_points(&shape.shape, &mut points);
            }
            assert!(
                points.len() > 20,
                "a full dashed line must replace the small X"
            );
            let bounds = Rect::from_points(&points);
            assert!(bounds.size().length() > rect.width().min(rect.height()) * 0.9);
        }
        h.app.doc.display.guides = false;
        let output = h.ctx.run_test_ui(egui::RawInput::default(), |ui| {
            h.app
                .paint_construction_guides(ui.painter(), h.app.viewport)
        });
        let mut points = vec![];
        for shape in &output.shapes {
            cyan_points(&shape.shape, &mut points);
        }
        assert!(points.is_empty());
    }
}

#[test]
fn control_and_command_taps_toggle_guides_but_shortcuts_do_not() {
    for modifiers in [
        egui::Modifiers::CTRL,
        egui::Modifiers {
            mac_cmd: true,
            command: true,
            ..Default::default()
        },
    ] {
        let mut h = Harness::new();
        h.app.set_tool(Tool::TapeMeasure);
        h.frame(vec![], modifiers);
        h.frame(vec![], egui::Modifiers::NONE);
        assert!(!h.app.create_guides);
        h.click(h.at(DVec3::ZERO));
        h.click(h.at(DVec3::Y));
        assert!(h.app.measurement.as_ref().unwrap().complete);
        assert!(h.app.doc.model.guides.is_empty());
        assert!(!h.app.project.modified);
        h.app.set_tool(Tool::Protractor);
        h.frame(vec![], modifiers);
        h.frame(vec![], egui::Modifiers::NONE);
        assert!(h.app.create_guides);
        // The shortcut key arrives in a later frame than the modifier press.
        h.frame(vec![], modifiers);
        h.frame(
            vec![egui::Event::Key {
                key: Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            modifiers,
        );
        h.frame(vec![], egui::Modifiers::NONE);
        assert!(h.app.create_guides);
    }
}

#[test]
fn protractor_creates_an_exact_angled_guide_and_deleting_guides_is_undoable() {
    let mut h = Harness::new();
    h.app.set_tool(Tool::Protractor);
    h.click(h.at(DVec3::ZERO));
    h.click(h.at(DVec3::X));
    h.app.length_input = "45deg".into();
    h.key(Key::Enter);
    let guide = h.app.doc.model.guides.values().next().unwrap();
    assert_eq!(guide.origin, DVec3::ZERO);
    let direction = guide.direction.unwrap();
    assert!((direction.x - 0.5_f64.sqrt()).abs() < 1e-6);
    assert!((direction.y.abs() - 0.5_f64.sqrt()).abs() < 1e-6);
    h.app.delete_construction_guides(&h.ctx);
    assert!(h.app.doc.model.guides.is_empty());
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.guides.len(), 1);
}

#[test]
fn guide_intersections_and_explicit_axis_locks_remain_exact() {
    let mut h = Harness::new();
    let point = DVec3::new(1.2, -0.7, 0.);
    h.app.doc.model.add_guide(point, Some(DVec3::X)).unwrap();
    h.app.doc.model.add_guide(point, Some(DVec3::Y)).unwrap();
    let cursor = h.at(point) + egui::vec2(2., 2.);
    let snap = h.app.construction_snap(cursor, None, None).unwrap();
    assert_eq!(snap.kind, Inference::GuidePoint);
    assert!(snap.point.distance(point) < 1e-6);
    let origin = point - DVec3::Y;
    h.app.lock = Some(Axis::Y);
    let snap = h
        .app
        .construction_snap(cursor, Some(origin), Some(Axis::Y))
        .unwrap();
    assert!((snap.point - origin).cross(DVec3::Y).length() < 1e-6);
}

#[test]
fn invalid_distances_and_cancel_do_not_leave_guides_behind() {
    let mut h = Harness::new();
    h.app.set_tool(Tool::TapeMeasure);
    h.click(h.at(DVec3::ZERO));
    h.frame(
        vec![egui::Event::PointerMoved(h.at(DVec3::Y))],
        egui::Modifiers::NONE,
    );
    for input in ["invalid", "0m", "-2m"] {
        h.app.length_input = input.into();
        h.key(Key::Enter);
        assert!(h.app.doc.model.guides.is_empty());
        assert!(!h.app.measurement.as_ref().unwrap().complete);
        assert!(!h.app.project.modified);
    }
    h.key(Key::Escape);
    assert!(h.app.measurement.is_none());
    assert!(h.app.doc.model.guides.is_empty());
}
