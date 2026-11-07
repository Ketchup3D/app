use super::*;

#[test]
fn tape_creates_a_parallel_offset_without_changing_geometry_and_supports_undo() {
    let mut h = Harness::new();
    h.app
        .doc
        .model
        .add_edge(-DVec3::X * 2., DVec3::X * 2.)
        .unwrap();
    let geometry = h.app.doc.model.clone();
    h.app.set_tool(Tool::TapeMeasure);
    h.click(h.at(DVec3::ZERO));
    h.frame(
        vec![egui::Event::PointerMoved(h.at(DVec3::new(0.5, -1., 0.)))],
        egui::Modifiers::NONE,
    );
    h.app.length_input = "250mm".into();
    h.key(Key::Enter);
    let guide = h.app.doc.model.guides.values().next().unwrap();
    assert_eq!(guide.direction, Some(DVec3::X));
    assert!((guide.origin.y + 0.25).abs() < 1e-6);
    assert!(guide.origin.x.abs() < 1e-6);
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
fn tape_endpoint_creates_a_snappable_guide_point_and_hidden_guides_do_not_snap() {
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
    assert_eq!(guide.direction, None);
    assert!(guide.origin.distance(point) < 1e-6);
    h.app.set_tool(Tool::Line);
    let snap = h.app.infer_snap(h.at(point), None, None, false).unwrap();
    assert_eq!(snap.kind, Inference::GuidePoint);
    assert!(snap.point.distance(point) < 1e-6);
    h.app.doc.display.guides = false;
    assert!(h.app.construction_snap(h.at(point), None, None).is_none());
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
