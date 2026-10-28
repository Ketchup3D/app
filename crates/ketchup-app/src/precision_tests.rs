use super::*;

// Include native key events: unit letters must not select another tool.
fn type_dimension(h: &mut Harness, input: &str) {
    for c in input.chars() {
        let text = c.to_string();
        let key = if c == ' ' {
            Some(Key::Space)
        } else {
            Key::ALL
                .iter()
                .copied()
                .find(|key| key.name().eq_ignore_ascii_case(&text))
        };
        let mut events = vec![];
        let key_event = |key, pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        if let Some(key) = key {
            events.push(key_event(key, true));
        }
        events.push(egui::Event::Text(text));
        if let Some(key) = key {
            events.push(key_event(key, false));
        }
        h.frame(events, egui::Modifiers::NONE);
    }
}

#[test]
fn space_stops_every_drawing_tool_with_or_without_a_typed_dimension() {
    for tool in Tool::DRAWING {
        for input in ["", "2cm"] {
            let mut h = Harness::new();
            h.app.set_tool(tool);
            let start = h.at(DVec3::ZERO);
            let end = h.at(DVec3::new(2., 1., 0.));
            if tool == Tool::Freehand {
                h.pointer(start, egui::PointerButton::Primary, true, false);
            } else {
                h.click(start);
            }
            h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
            assert!(h.app.line_start.is_some() || h.app.drawing.is_some());
            type_dimension(&mut h, input);
            assert_eq!(h.app.length_input, input);
            let before = h.app.doc.model.clone();

            // Native input delivers both a key and its corresponding text event.
            type_dimension(&mut h, " ");

            assert_eq!(h.app.tool, Tool::Select, "{tool:?}: {input}");
            assert!(h.app.line_start.is_none());
            assert!(h.app.drawing.is_none());
            assert!(h.app.gesture.is_none());
            assert!(h.app.lock.is_none());
            assert!(h.app.length_input.is_empty());
            h.pointer(end, egui::PointerButton::Primary, false, false);
            assert_eq!(h.app.doc.model, before);
            assert!(h.app.history.undo_label().is_none());
        }
    }
}

#[test]
fn space_ends_a_line_chain_and_keeps_completed_segments() {
    let mut h = Harness::new();
    h.key(Key::L);
    h.click(h.at(DVec3::ZERO));
    h.click(h.at(DVec3::X * 2.));
    assert_eq!(h.app.doc.model.edges.len(), 1);
    assert!(h.app.line_start.is_some());
    let before = h.app.doc.model.clone();
    type_dimension(&mut h, "5cm");
    h.key(Key::Space);
    assert_eq!(h.app.tool, Tool::Select);
    assert!(h.app.line_start.is_none());
    assert!(h.app.length_input.is_empty());
    assert_eq!(h.app.doc.model, before);
    h.app.undo(&h.ctx, false);
    assert!(h.app.doc.model.edges.is_empty());
}

#[test]
fn line_units_preserve_direction_and_do_not_trigger_tool_shortcuts() {
    for (input, expected) in [
        ("5", 5.),
        ("5m", 5.),
        ("50mm", 0.05),
        ("5cm", 0.05),
        ("2in", 0.0508),
        ("60cm-89mm", 0.511),
        ("(60cm-89mm)/2", 0.2555),
        ("2*5cm", 0.1),
    ] {
        let mut h = Harness::new();
        h.key(Key::L);
        h.click(h.at(DVec3::ZERO));
        h.key(Key::ArrowUp);
        h.frame(
            vec![egui::Event::PointerMoved(h.at(DVec3::Z))],
            egui::Modifiers::NONE,
        );
        type_dimension(&mut h, input);
        assert_eq!(h.app.tool, Tool::Line, "{input}");
        assert_eq!(h.app.length_input, input);
        assert!(
            h.app
                .line_preview_snap()
                .unwrap()
                .point
                .distance(DVec3::Z * expected)
                < 1e-7
        );
        h.key(Key::Enter);
        assert_eq!(h.app.doc.model.edges.len(), 1);
        assert!(
            h.app.line_start.unwrap().distance(DVec3::Z * expected) < 1e-7,
            "{input}"
        );
        assert!(h.app.length_input.is_empty());
        h.app.doc.model.validate().unwrap();
    }
}

#[test]
fn bare_line_lengths_use_the_selected_unit_for_preview_and_commit() {
    let mut h = Harness::new();
    h.app.doc.display.units.length = ketchup_core::LengthUnit::Centimeters;
    h.key(Key::L);
    h.click(h.at(DVec3::ZERO));
    h.key(Key::ArrowUp);
    h.frame(
        vec![egui::Event::PointerMoved(h.at(DVec3::Z))],
        egui::Modifiers::NONE,
    );
    type_dimension(&mut h, "25");
    let expected = DVec3::Z * 0.25;
    assert!(h.app.line_preview_snap().unwrap().point.distance(expected) < 1e-7);
    h.key(Key::Enter);
    assert_eq!(h.app.doc.model.edges.len(), 1);
    assert!(h.app.line_start.unwrap().distance(expected) < 1e-7);
}

#[test]
fn display_precision_does_not_round_typed_geometry() {
    let mut h = Harness::new();
    h.app.doc.display.units.length = ketchup_core::LengthUnit::Centimeters;
    h.app.doc.display.units.precision.length = Some(0);
    h.key(Key::L);
    h.click(h.at(DVec3::ZERO));
    h.key(Key::ArrowUp);
    h.frame(
        vec![egui::Event::PointerMoved(h.at(DVec3::Z))],
        egui::Modifiers::NONE,
    );
    type_dimension(&mut h, "25.123456");
    let expected = DVec3::Z * 0.25123456;
    assert!(h.app.line_preview_snap().unwrap().point.distance(expected) < 1e-9);
    h.key(Key::Enter);
    assert!(h.app.line_start.unwrap().distance(expected) < 1e-9);
    assert_eq!(h.app.doc.display.units.format_length(expected.z), "25 cm");
}

#[test]
fn rectangles_accept_mixed_units_and_subpixel_dimensions_on_their_drawing_plane() {
    for (input, height) in [
        ("5m,50mm", 0.05),
        ("5,0.5mm", 0.0005),
        ("6m-100cm,60mm-10mm", 0.05),
    ] {
        for vertical in [false, true] {
            let mut h = Harness::new();
            if vertical {
                let corners = [
                    DVec3::new(-10., 0., 0.),
                    DVec3::new(10., 0., 0.),
                    DVec3::new(10., 0., 10.),
                    DVec3::new(-10., 0., 10.),
                ];
                for i in 0..4 {
                    h.app
                        .doc
                        .model
                        .add_edge(corners[i], corners[(i + 1) % 4])
                        .unwrap();
                }
            }
            h.key(Key::R);
            let origin = if vertical {
                DVec3::new(-2., 0., 2.)
            } else {
                DVec3::ZERO
            };
            h.click(h.at(origin));
            let drawing = h.app.drawing.as_ref().unwrap();
            let (u, v, normal) = (drawing.u, drawing.v, drawing.normal);
            let pointer = h.at(origin - u * 2. + v);
            h.frame(
                vec![egui::Event::PointerMoved(pointer)],
                egui::Modifiers::NONE,
            );
            type_dimension(&mut h, input);
            assert_eq!(h.app.tool, Tool::Rectangle);
            let preview = h.app.drawing_points();
            assert!((preview[0].distance(preview[1]) - 5.).abs() < 1e-7);
            assert!((preview[1].distance(preview[2]) - height).abs() < 1e-7);
            assert!((preview[1] - origin).dot(u) < 0.);
            assert!(
                preview
                    .iter()
                    .all(|p| (*p - origin).dot(normal).abs() < 1e-7)
            );
            let before = h.app.doc.model.clone();
            h.key(Key::Enter);
            assert!(h.app.drawing.is_none());
            assert!(h.app.length_input.is_empty());
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

#[test]
fn circles_and_polygons_accept_a_radius_with_units() {
    for (tool, input, radius) in [
        (Tool::Circle, "5cm", 0.05),
        (Tool::Polygon, "5cm", 0.05),
        (Tool::Circle, "60cm-89mm", 0.511),
        (Tool::Polygon, "60cm-89mm", 0.511),
    ] {
        let mut h = Harness::new();
        h.app.set_tool(tool);
        h.app.polygon_sides = 7;
        h.click(h.at(DVec3::ZERO));
        type_dimension(&mut h, input);
        assert_eq!(h.app.tool, tool);
        let preview = h.app.drawing_points();
        assert!(preview.iter().all(|p| (p.length() - radius).abs() < 1e-7));
        h.key(Key::Enter);
        assert!(h.app.drawing.is_none());
        assert_eq!(
            h.app.doc.model.edges.len(),
            if tool == Tool::Circle { 48 } else { 7 }
        );
        assert!(
            h.app
                .doc
                .model
                .vertices
                .values()
                .all(|v| (v.position.length() - radius).abs() < 1e-7)
        );
        h.app.doc.model.validate().unwrap();
    }
}

#[test]
fn rectangle_dimensions_can_be_entered_in_the_focused_measurements_field() {
    let mut h = Harness::new();
    h.key(Key::R);
    h.click(h.at(DVec3::ZERO));
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, "5m,50mm"));
    assert!(h.ctx.egui_wants_keyboard_input());
    type_dimension(&mut h, "(6m - 100cm),60mm - 10mm");
    h.key(Key::Enter);
    assert_eq!(h.app.tool, Tool::Rectangle);
    assert!(h.app.drawing.is_none());
    let (lo, hi) = h.app.doc.model.bounds().unwrap();
    assert!(((hi - lo).x - 5.).abs() < 1e-7);
    assert!(((hi - lo).y - 0.05).abs() < 1e-7);
    h.app.doc.model.validate().unwrap();
}

#[test]
fn freehand_units_set_total_stroke_length_and_enter_finishes_the_gesture() {
    let mut h = Harness::new();
    h.key(Key::D);
    h.pointer(h.at(DVec3::ZERO), egui::PointerButton::Primary, true, false);
    for point in [DVec3::X, DVec3::X + DVec3::Y] {
        h.frame(
            vec![egui::Event::PointerMoved(h.at(point))],
            egui::Modifiers::NONE,
        );
    }
    type_dimension(&mut h, "60cm-89mm");
    assert_eq!(h.app.tool, Tool::Freehand);
    let preview = h.app.drawing_points();
    let length: f64 = preview.windows(2).map(|p| p[0].distance(p[1])).sum();
    assert!((length - 0.511).abs() < 1e-7);
    h.key(Key::Enter);
    assert!(h.app.gesture.is_none());
    assert!(h.app.drawing.is_none());
    let length: f64 = h
        .app
        .doc
        .model
        .edges
        .values()
        .map(|e| h.app.doc.model.edge_length(e))
        .sum();
    assert!((length - 0.511).abs() < 1e-7);
    let before = h.app.doc.model.clone();
    h.pointer(
        h.at(DVec3::X + DVec3::Y),
        egui::PointerButton::Primary,
        false,
        false,
    );
    assert_eq!(h.app.doc.model, before);
    h.app.doc.model.validate().unwrap();
}

#[test]
fn invalid_drawing_dimensions_preserve_the_model_and_can_be_corrected() {
    for tool in [Tool::Rectangle, Tool::Circle, Tool::Polygon] {
        let mut h = Harness::new();
        h.app.set_tool(tool);
        h.click(h.at(DVec3::ZERO));
        for input in [
            "5kg",
            "5m,",
            "NaN",
            "1e999m",
            "-5cm",
            "5m,0mm",
            "5m,50mm,2cm",
            "60cm -",
            "1m / 0",
            "(5m",
            "1m * 2m",
        ] {
            h.app.length_input = input.into();
            h.key(Key::Enter);
            assert!(h.app.doc.model.edges.is_empty(), "{tool:?}: {input}");
            assert!(h.app.drawing.is_some());
            assert!(h.app.history.undo_label().is_none());
            assert_eq!(h.app.length_input, input);
        }
        h.app.length_input = if tool == Tool::Rectangle {
            "5m,50mm"
        } else {
            "5cm"
        }
        .into();
        h.key(Key::Enter);
        assert!(!h.app.doc.model.edges.is_empty());
    }
}

#[test]
fn modeling_distances_use_the_same_units_in_previews_and_commits() {
    for tool in [Tool::Move, Tool::PushPull, Tool::Offset] {
        let mut h = Harness::new();
        let face = modeling_rectangle(&mut h);
        h.app.selected = BTreeSet::from([face]);
        h.app.set_tool(tool);
        let anchor = if tool == Tool::Offset {
            DVec3::new(2., 0.25, 0.)
        } else {
            DVec3::new(2., 2., 0.)
        };
        h.click(h.at(anchor));
        h.frame(
            vec![egui::Event::PointerMoved(
                h.at(anchor + DVec3::Z + DVec3::X),
            )],
            egui::Modifiers::NONE,
        );
        type_dimension(&mut h, "60cm-550mm");
        assert_eq!(h.app.tool, tool);
        let check_dimensions = |model: &Model| match tool {
            Tool::Move => assert!(
                (model
                    .selection_center(&BTreeSet::from([face]))
                    .unwrap()
                    .distance(DVec3::new(2., 2., 0.))
                    - 0.05)
                    .abs()
                    < 1e-7
            ),
            Tool::PushPull => {
                let (lo, hi) = model.bounds().unwrap();
                assert!((hi.z - lo.z - 0.05).abs() < 1e-7);
            }
            Tool::Offset => {
                for point in [DVec3::new(0.05, 0.05, 0.), DVec3::new(3.95, 3.95, 0.)] {
                    assert!(
                        model
                            .vertices
                            .values()
                            .any(|v| v.position.distance(point) < 1e-7)
                    );
                }
            }
            _ => unreachable!(),
        };
        check_dimensions(h.app.modeling.as_ref().unwrap().preview.as_ref().unwrap());
        h.key(Key::Enter);
        check_dimensions(&h.app.doc.model);
        assert!(h.app.modeling.is_none());
        h.app.doc.model.validate().unwrap();
    }
}
