use super::*;

fn start_label(h: &mut Harness) {
    h.app.set_tool(Tool::Text);
    h.click(h.at(DVec3::ZERO));
    assert_eq!(
        h.app.measurement.as_ref().unwrap().points,
        vec![DVec3::ZERO]
    );
    let placement = h.at(DVec3::new(1., -2., 0.));
    h.click(placement);
    h.frame(vec![], egui::Modifiers::NONE);
    assert!(h.app.text_editor.is_some());
    assert!(h.app.doc.model.texts.is_empty());
}

#[test]
fn text_tool_places_edits_selects_deletes_and_undoes_a_geometry_attached_label() {
    let mut h = Harness::new();
    let edge = h
        .app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 4.)
        .unwrap();
    start_label(&mut h);
    h.frame(
        vec![egui::Event::Text("Oak beam".into())],
        egui::Modifiers::NONE,
    );
    h.key(Key::Enter);
    assert!(h.app.text_editor.is_none());
    assert_eq!(h.app.doc.model.texts.len(), 1);
    assert_eq!(h.app.history.undo_label(), Some("Add text"));
    let id = *h.app.doc.model.texts.keys().next().unwrap();
    assert_eq!(h.app.doc.model.texts[&id].text, "Oak beam");
    assert_eq!(h.app.doc.model.texts[&id].anchor.weights.len(), 1);
    assert_eq!(h.app.doc.model.edges.len(), 1);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    text_position(&output, "Oak beam");
    h.key(Key::Space);
    h.time += 0.5;
    let position = h.app.doc.model.texts[&id].layout(&h.app.doc.model)[1];
    h.click(h.at(position));
    assert_eq!(h.app.selected, BTreeSet::from([id]));
    h.app.context_action(&h.ctx, ContextAction::EditText);
    h.frame(vec![], egui::Modifiers::NONE);
    h.frame(vec![], egui::Modifiers::NONE);
    h.frame(
        vec![egui::Event::Text("Revised beam".into())],
        egui::Modifiers::NONE,
    );
    h.key(Key::Enter);
    assert_eq!(h.app.doc.model.texts[&id].text, "Revised beam");
    assert_eq!(h.app.history.undo_label(), Some("Edit text"));
    h.key(Key::Delete);
    assert!(h.app.doc.model.texts.is_empty());
    assert!(h.app.doc.model.edges.contains_key(&edge));
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.texts[&id].text, "Revised beam");
    h.app
        .doc
        .model
        .move_selection(&BTreeSet::from([edge]), DVec3::Z * 3.)
        .unwrap();
    assert_eq!(
        h.app.doc.model.texts[&id].layout(&h.app.doc.model)[0],
        DVec3::Z * 3.
    );
}

#[test]
fn text_placement_and_editor_cancel_without_mutations_or_history() {
    let mut h = Harness::new();
    h.app.set_tool(Tool::Text);
    h.click(h.app.viewport.center());
    assert!(h.app.measurement.is_none());
    h.app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 4.)
        .unwrap();
    let before = h.app.doc.model.clone();
    h.click(h.at(DVec3::ZERO));
    assert!(h.app.measurement.is_some());
    h.key(Key::Escape);
    assert!(h.app.measurement.is_none());
    start_label(&mut h);
    h.key(Key::Escape);
    assert!(h.app.text_editor.is_none());
    assert_eq!(h.app.doc.model, before);
    assert!(h.app.history.undo_label().is_none());
    assert_eq!(h.app.tool, Tool::Text);
}

#[test]
fn text_uses_the_object_name_even_when_geometry_snapping_is_disabled() {
    let mut h = Harness::new();
    let edge = h
        .app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 4.)
        .unwrap();
    let group = h
        .app
        .doc
        .model
        .make_object(&BTreeSet::from([edge]), false, "Oak post")
        .unwrap();
    h.app.doc.snap_to_geometry = false;
    start_label(&mut h);
    assert_eq!(h.app.text_editor.as_ref().unwrap().text, "Oak post");
    h.key(Key::Enter);
    let id = *h.app.doc.model.texts.keys().next().unwrap();
    assert_eq!(h.app.doc.model.parent(id), None);
    h.app
        .doc
        .model
        .move_selection(&BTreeSet::from([group]), DVec3::Z)
        .unwrap();
    assert_eq!(
        h.app.doc.model.texts[&id].layout(&h.app.doc.model)[0],
        DVec3::Z
    );
    h.app.doc.model.validate().unwrap();
}

#[test]
fn shift_enter_keeps_the_text_editor_open_for_multiline_labels() {
    let mut h = Harness::new();
    h.app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 4.)
        .unwrap();
    start_label(&mut h);
    h.frame(
        vec![egui::Event::Text("First line".into())],
        egui::Modifiers::NONE,
    );
    h.frame(
        vec![egui::Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        }],
        egui::Modifiers::SHIFT,
    );
    assert!(h.app.text_editor.is_some());
    assert!(h.app.doc.model.texts.is_empty());
    h.frame(
        vec![egui::Event::Text("Second line".into())],
        egui::Modifiers::NONE,
    );
    assert_eq!(
        h.app.text_editor.as_ref().unwrap().text,
        "First line\nSecond line"
    );
    h.key(Key::Enter);
    assert_eq!(
        h.app.doc.model.texts.values().next().unwrap().text,
        "First line\nSecond line"
    );
}
