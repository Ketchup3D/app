use super::*;

fn catalog() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!("../../../assets/release-notes.json")).unwrap()
}

fn open_notes(h: &mut Harness) -> egui::FullOutput {
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, "Help"));
    let menu = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&menu, "Release notes"));
    for _ in 0..10 {
        h.frame(vec![], egui::Modifiers::NONE);
    }
    h.frame(vec![], egui::Modifiers::NONE)
}

#[test]
fn help_release_notes_navigates_every_release_and_reopens_on_latest() {
    let mut h = Harness::new();
    let releases = catalog();
    assert_eq!(releases[0]["version"], env!("CARGO_PKG_VERSION"));
    let mut output = open_notes(&mut h);
    assert!(h.app.release_notes.open);
    assert!(!h.app.release_notes.has_unread());
    text_position(&output, releases[0]["title"].as_str().unwrap());
    // Clicking a disabled boundary must leave the displayed release unchanged.
    h.click(text_position(&output, "Newer"));
    output = h.frame(vec![], egui::Modifiers::NONE);
    text_position(&output, releases[0]["title"].as_str().unwrap());
    for release in &releases[1..] {
        h.click(text_position(&output, "Older"));
        output = h.frame(vec![], egui::Modifiers::NONE);
        text_position(&output, release["title"].as_str().unwrap());
    }
    h.click(text_position(&output, "Older"));
    output = h.frame(vec![], egui::Modifiers::NONE);
    text_position(&output, releases.last().unwrap()["title"].as_str().unwrap());
    for release in releases[..releases.len() - 1].iter().rev() {
        h.click(text_position(&output, "Newer"));
        output = h.frame(vec![], egui::Modifiers::NONE);
        text_position(&output, release["title"].as_str().unwrap());
    }
    // Jump directly to the oldest version using the picker.
    h.click(text_position(
        &output,
        &format!("v{}", releases[0]["version"].as_str().unwrap()),
    ));
    let picker = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(
        &picker,
        &format!("v{}", releases.last().unwrap()["version"].as_str().unwrap()),
    ));
    output = h.frame(vec![], egui::Modifiers::NONE);
    text_position(&output, releases.last().unwrap()["title"].as_str().unwrap());
    h.click(text_position(&output, "Close"));
    assert!(!h.app.release_notes.open);
    let output = open_notes(&mut h);
    text_position(&output, releases[0]["title"].as_str().unwrap());
}

#[test]
fn outside_click_keeps_modal_open_without_editing_the_model() {
    for outside in [
        egui::pos2(8., 450.),
        egui::pos2(1432., 450.),
        egui::pos2(720., 8.),
        egui::pos2(720., 892.),
    ] {
        let mut h = Harness::new();
        let output = open_notes(&mut h);
        let model = h.app.doc.model.clone();
        h.click(text_position(
            &output,
            catalog()[0]["title"].as_str().unwrap(),
        ));
        assert!(
            h.app.release_notes.open,
            "Clicking inside must keep the modal open"
        );
        h.click(outside);
        assert!(h.app.release_notes.open, "Outside click at {outside:?}");
        assert_eq!(h.app.doc.model, model);
        h.key(Key::Escape);
        assert!(!h.app.release_notes.open);
    }
}

#[test]
fn outside_click_shakes_modal_once_then_returns_to_center() {
    let mut h = Harness::new();
    let output = open_notes(&mut h);
    let releases = catalog();
    let title = releases[0]["title"].as_str().unwrap();
    let centered = text_position(&output, title);
    let outside = egui::pos2(8., 450.);
    for _ in 0..2 {
        h.click(outside);
        let mut moved = false;
        for frame in 0..50 {
            let output = h.frame(vec![], egui::Modifiers::NONE);
            let position = text_position(&output, title);
            assert!(h.app.release_notes.open);
            assert!((position.y - centered.y).abs() < 0.1);
            moved |= (position.x - centered.x).abs() > 0.5;
            if frame >= 30 {
                assert!((position.x - centered.x).abs() < 0.1);
            }
        }
        assert!(moved, "Each outside click should trigger a brief shake");
    }
    h.click(centered);
    for _ in 0..30 {
        let output = h.frame(vec![], egui::Modifiers::NONE);
        assert!(text_position(&output, title).distance(centered) < 0.1);
    }
}

#[test]
fn outside_click_dismisses_picker_but_keeps_modal_open() {
    let mut h = Harness::new();
    let output = open_notes(&mut h);
    h.click(text_position(
        &output,
        &format!("v{}", env!("CARGO_PKG_VERSION")),
    ));
    h.frame(vec![], egui::Modifiers::NONE);
    assert!(egui::Popup::is_any_open(&h.ctx));
    let outside = egui::pos2(8., 450.);
    h.click(outside);
    assert!(!egui::Popup::is_any_open(&h.ctx));
    assert!(h.app.release_notes.open);
    let title = catalog()[0]["title"].as_str().unwrap().to_owned();
    let centered = text_position(&output, &title);
    for _ in 0..30 {
        let output = h.frame(vec![], egui::Modifiers::NONE);
        assert!(text_position(&output, &title).distance(centered) < 0.1);
    }
    h.click(outside);
    assert!(h.app.release_notes.open);
    h.key(Key::Escape);
    assert!(!h.app.release_notes.open);
}

#[test]
fn unseen_release_opens_once_at_startup_in_full_and_compact_layouts() {
    fn has_text(shape: &egui::Shape, label: &str) -> bool {
        match shape {
            egui::Shape::Text(text) => text.galley.job.text == label,
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| has_text(shape, label)),
            _ => false,
        }
    }

    for width in [1440., 900.] {
        let mut h = Harness::new();
        h.size.x = width;
        assert!(h.app.release_notes.has_unread());
        // Match startup: restore preferences before checking the read version.
        h.app.restore_inspector_preferences(&h.ctx, None);
        h.app.release_notes.open_if_unread();
        h.frame(vec![], egui::Modifiers::NONE);
        let notes = h.frame(vec![], egui::Modifiers::NONE);
        assert!(h.app.release_notes.open);
        assert!(!h.app.release_notes.has_unread());
        text_position(&notes, catalog()[0]["title"].as_str().unwrap());
        h.click(text_position(&notes, "Close"));
        assert!(!h.app.release_notes.open);
        h.app.release_notes.open_if_unread();
        h.frame(vec![], egui::Modifiers::NONE);
        let output = h.frame(vec![], egui::Modifiers::NONE);
        assert!(!h.app.release_notes.open);
        assert!(
            !output
                .shapes
                .iter()
                .any(|shape| has_text(&shape.shape, "What's new"))
        );
        // Help remains available even after the automatic display is dismissed.
        let notes = open_notes(&mut h);
        assert!(h.app.release_notes.open);
        text_position(&notes, catalog()[0]["title"].as_str().unwrap());
    }
}

#[test]
fn release_notes_blocks_model_and_file_shortcuts_and_preserves_pending_drawing() {
    let mut h = Harness::new();
    h.app.set_tool(Tool::Line);
    h.app.line_start = Some(DVec3::X);
    h.app.lock = Some(Axis::Y);
    h.app.length_input = "2m".into();
    let edge = h.app.doc.model.add_edge(DVec3::ZERO, DVec3::Y).unwrap();
    h.app.selected.insert(edge);
    let model = h.app.doc.model.clone();
    let output = open_notes(&mut h);
    h.key(Key::Delete);
    h.key(Key::R);
    h.frame(
        vec![egui::Event::Key {
            key: Key::N,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
        egui::Modifiers::COMMAND,
    );
    assert!(h.app.project.confirm.is_none());
    assert_eq!(h.app.doc.model, model);
    assert_eq!(h.app.tool, Tool::Line);
    // Escape dismisses a picker before dismissing the modal itself.
    h.click(text_position(
        &output,
        &format!("v{}", env!("CARGO_PKG_VERSION")),
    ));
    h.key(Key::Escape);
    assert!(h.app.release_notes.open);
    h.key(Key::Escape);
    assert!(!h.app.release_notes.open);
    assert_eq!(h.app.line_start, Some(DVec3::X));
    assert_eq!(h.app.lock, Some(Axis::Y));
    assert_eq!(h.app.length_input, "2m");
}
