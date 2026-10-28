use super::*;

fn axis_position(output: &egui::FullOutput, center: Pos2, label: &str) -> Pos2 {
    // Negative buttons sit opposite the always-visible positive labels.
    if let Some(positive) = label.strip_prefix('−') {
        return center - (axis_position(output, center, positive) - center);
    }
    output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                let pos = text.pos + text.galley.size() * 0.5;
                if text.galley.job.text == label && pos.distance(center) <= 39. {
                    return Some(pos);
                }
            }
            None
        })
        .expect("axis letter must be visible")
}

fn has_background(output: &egui::FullOutput, center: Pos2) -> bool {
    output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Circle(circle)
            if circle.center == center && circle.radius == 41.6 && circle.fill != Color32::TRANSPARENT)
    })
}

#[test]
fn compass_background_only_appears_on_hover_and_axis_has_hand_cursor() {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let mut h = Harness::new();
        h.ctx.set_theme(theme);
        let center = KetchupApp::compass_center(h.app.viewport).unwrap();
        let outside = h.app.viewport.center();
        let output = h.frame(
            vec![egui::Event::PointerMoved(outside)],
            egui::Modifiers::NONE,
        );
        assert!(!has_background(&output, center));
        let x = axis_position(&output, center, "X");
        let output = h.frame(vec![egui::Event::PointerMoved(x)], egui::Modifiers::NONE);
        assert!(has_background(&output, center));
        assert_eq!(
            output.platform_output.cursor_icon,
            egui::CursorIcon::PointingHand
        );
        let output = h.frame(
            vec![egui::Event::PointerMoved(outside)],
            egui::Modifiers::NONE,
        );
        assert!(!has_background(&output, center));
        for label in ["−X", "−Y", "−Z"] {
            let button = axis_position(&output, center, label);
            let hovered = h.frame(
                vec![egui::Event::PointerMoved(button)],
                egui::Modifiers::NONE,
            );
            let visible: Vec<_> = hovered
                .shapes
                .iter()
                .filter_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        if ["−X", "−Y", "−Z"].contains(&text.galley.job.text.as_str()) {
                            return Some(text.galley.job.text.as_str());
                        }
                    }
                    None
                })
                .collect();
            assert_eq!(visible, vec![label]);
        }
    }
}

#[test]
fn compass_has_six_larger_tips_and_only_connects_positive_axes() {
    let mut h = Harness::new();
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let center = KetchupApp::compass_center(h.app.viewport).unwrap();
    let tips = output.shapes.iter().filter(|shape| {
        matches!(&shape.shape, egui::Shape::Circle(circle)
            if (circle.radius - 9.1).abs() < 1e-5 && circle.center.distance(center) < 40.)
    });
    assert_eq!(tips.count(), 6);
    let lines: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::LineSegment { points, .. } = &shape.shape {
                if points[0].distance(center) < 1e-5 && points[1].distance(center) < 40. {
                    return Some(*points);
                }
            }
            None
        })
        .collect();
    assert_eq!(lines.len(), 3);
    for label in ["X", "Y", "Z"] {
        let pos = axis_position(&output, center, label);
        assert!(lines.iter().any(|line| line[1].distance(pos) < 1e-4));
    }
    for label in ["−X", "−Y", "−Z"] {
        let pos = axis_position(&output, center, label);
        assert!(lines.iter().all(|line| line[1].distance(pos) > 1.));
    }
}

#[derive(Default)]
struct PreferencesStorage {
    values: std::collections::BTreeMap<String, String>,
    flushed: bool,
}

impl eframe::Storage for PreferencesStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.values.insert(key.to_owned(), value);
    }

    fn remove_string(&mut self, key: &str) {
        self.values.remove(key);
    }

    fn flush(&mut self) {
        self.flushed = true;
    }
}

#[test]
fn compass_clicks_align_and_persist_without_editing_geometry_or_changing_tools() {
    for tool in [Tool::Select, Tool::Line, Tool::Erase, Tool::Orbit] {
        for (label, view) in [
            ("X", ketchup_core::StandardView::Right),
            ("Y", ketchup_core::StandardView::Back),
            ("Z", ketchup_core::StandardView::Top),
            ("−X", ketchup_core::StandardView::Left),
            ("−Y", ketchup_core::StandardView::Front),
            ("−Z", ketchup_core::StandardView::Bottom),
        ] {
            let mut h = Harness::new();
            let edge = h.app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
            h.app.selected.insert(edge);
            h.app.set_tool(tool);
            h.app.doc.camera.target = DVec3::new(2., 3., 4.);
            let model = h.app.doc.model.clone();
            let camera = h.app.doc.camera.clone();
            let output = h.frame(vec![], egui::Modifiers::NONE);
            let center = KetchupApp::compass_center(h.app.viewport).unwrap();
            let letter = axis_position(&output, center, label);
            h.click(letter);

            assert_eq!(h.app.doc.camera.standard_view(), Some(view));
            assert_eq!(h.app.doc.camera.target, camera.target);
            assert_eq!(h.app.doc.camera.distance, camera.distance);
            assert_eq!(h.app.doc.camera.projection, camera.projection);
            assert_eq!(h.app.doc.model, model);
            assert_eq!(h.app.selected, BTreeSet::from([edge]));
            assert_eq!(h.app.tool, tool);
            assert!(h.app.line_start.is_none());
            assert!(h.app.gesture.is_none());

            let mut storage = PreferencesStorage::default();
            h.app
                .persist_inspector_changes(&h.ctx, Some(&mut storage), false);
            assert!(storage.flushed);
            let mut restored = KetchupApp::with_document(Document::default(), None, true);
            restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
            assert_eq!(restored.doc.camera.standard_view(), Some(view));
        }
    }
}

#[test]
fn positive_axes_win_clicks_when_negative_buttons_overlap() {
    use ketchup_core::StandardView;
    for (initial, expected) in [
        (StandardView::Left, StandardView::Right),
        (StandardView::Front, StandardView::Back),
        (StandardView::Bottom, StandardView::Top),
    ] {
        let mut h = Harness::new();
        h.app.doc.camera.set_standard_view(initial);
        h.frame(vec![], egui::Modifiers::NONE);
        let center = KetchupApp::compass_center(h.app.viewport).unwrap();
        h.click(center);
        assert_eq!(h.app.doc.camera.standard_view(), Some(expected));
    }

    // +X and −Y overlap here, with −Y closer to the camera.
    let mut h = Harness::new();
    h.app.doc.camera.yaw = -3. * std::f64::consts::FRAC_PI_4;
    h.app.doc.camera.pitch = 0.;
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let center = KetchupApp::compass_center(h.app.viewport).unwrap();
    h.click(axis_position(&output, center, "X"));
    assert_eq!(h.app.doc.camera.standard_view(), Some(StandardView::Right));
}

#[test]
fn compass_background_click_leaves_camera_and_model_unchanged() {
    let mut h = Harness::new();
    h.app.set_tool(Tool::Line);
    h.frame(vec![], egui::Modifiers::NONE);
    let center = KetchupApp::compass_center(h.app.viewport).unwrap();
    let camera = h.app.doc.camera.clone();
    let model = h.app.doc.model.clone();
    h.click(center + egui::vec2(0., 40.));
    assert_eq!(h.app.doc.camera, camera);
    assert_eq!(h.app.doc.model, model);
    assert!(h.app.line_start.is_none());
    assert!(h.app.gesture.is_none());
}

#[test]
fn compass_drag_orbits_from_background_and_axis_without_editing_the_model() {
    for (button, label) in [
        (egui::PointerButton::Primary, None),
        (egui::PointerButton::Primary, Some("X")),
        (egui::PointerButton::Primary, Some("−Y")),
        (egui::PointerButton::Middle, None),
    ] {
        let mut h = Harness::new();
        let edge = h
            .app
            .doc
            .model
            .add_edge(DVec3::ZERO, DVec3::X * 4.)
            .unwrap();
        h.app.selected.insert(edge);
        h.app.set_tool(Tool::Line);
        h.app.line_start = Some(DVec3::ZERO);
        h.app.doc.camera.target = DVec3::new(4., 2., 1.);
        let model = h.app.doc.model.clone();
        let output = h.frame(vec![], egui::Modifiers::NONE);
        let center = KetchupApp::compass_center(h.app.viewport).unwrap();
        let start = label.map_or(center + egui::vec2(0., 40.), |label| {
            axis_position(&output, center, label)
        });
        let end = start + egui::vec2(-75., -35.);
        assert!(!h.app.compass_contains(end));
        let mut expected = h.app.doc.camera.clone();
        expected.orbit_around(dv(end - start), h.app.orbit_center());
        h.pointer(start, button, true, false);
        let output = h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
        assert_eq!(
            output.platform_output.cursor_icon,
            egui::CursorIcon::Grabbing
        );
        assert!(h.app.navigating());
        h.pointer(end, button, false, false);

        assert_eq!(h.app.doc.camera, expected);
        assert_eq!(h.app.doc.model, model);
        assert_eq!(h.app.selected, BTreeSet::from([edge]));
        assert_eq!(h.app.tool, Tool::Line);
        assert_eq!(h.app.line_start, Some(DVec3::ZERO));
        assert!(h.app.gesture.is_none());
        assert!(!h.app.navigating());

        let mut storage = PreferencesStorage::default();
        h.app
            .persist_inspector_changes(&h.ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
        assert_eq!(restored.doc.camera.yaw, expected.yaw);
        assert_eq!(restored.doc.camera.pitch, expected.pitch);
    }
}

#[test]
fn compass_drag_returning_to_axis_does_not_align_on_release() {
    let mut h = Harness::new();
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let center = KetchupApp::compass_center(h.app.viewport).unwrap();
    let x = axis_position(&output, center, "X");
    let before = h.app.doc.camera.clone();
    h.pointer(x, egui::PointerButton::Primary, true, false);
    h.frame(
        vec![egui::Event::PointerMoved(x + egui::vec2(-30., -15.))],
        egui::Modifiers::NONE,
    );
    h.pointer(x, egui::PointerButton::Primary, false, false);
    assert!((h.app.doc.camera.yaw - before.yaw).abs() < 1e-10);
    assert!((h.app.doc.camera.pitch - before.pitch).abs() < 1e-10);
    assert!(h.app.doc.camera.standard_view().is_none());
    assert!(h.app.gesture.is_none());
}

#[test]
fn compass_orbits_when_release_has_no_pointer_motion_event() {
    let mut h = Harness::new();
    let start = KetchupApp::compass_center(h.app.viewport).unwrap();
    let end = start + egui::vec2(-60., -20.);
    let mut expected = h.app.doc.camera.clone();
    expected.orbit_around(dv(end - start), h.app.orbit_center());
    h.pointer(start, egui::PointerButton::Primary, true, false);
    h.frame(
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
        egui::Modifiers::NONE,
    );
    assert_eq!(h.app.doc.camera, expected);
    assert!(h.app.gesture.is_none());
}
