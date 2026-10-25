use super::*;

fn object(h: &mut Harness, name: &str, component: bool, offset: DVec3) -> u64 {
    let edge = h
        .app
        .doc
        .model
        .add_edge(offset, offset + DVec3::X * 2.)
        .unwrap();
    h.app
        .doc
        .model
        .make_object(&BTreeSet::from([edge]), component, name)
        .unwrap()
}

fn outliner_row(h: &mut Harness, label: &str) -> Pos2 {
    h.frame(vec![], egui::Modifiers::NONE);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    text_position(&output, label)
}

#[test]
fn outliner_shift_click_selects_and_adjusts_ranges_in_both_directions() {
    let mut h = Harness::new();
    let a = object(&mut h, "First", false, DVec3::ZERO);
    let b = object(&mut h, "Second", false, DVec3::Y * 2.);
    let c = object(&mut h, "Hidden", false, DVec3::Y * 4.);
    let d = object(&mut h, "Locked", false, DVec3::Y * 6.);
    let e = object(&mut h, "Last", false, DVec3::Y * 8.);
    h.app.doc.model.hidden.insert(c);
    h.app.doc.model.locked.insert(d);
    let first = outliner_row(&mut h, "Group: First");
    let second = outliner_row(&mut h, "Group: Second");
    let hidden = outliner_row(&mut h, "Group: Hidden · Hidden");
    let last = outliner_row(&mut h, "Group: Last");
    let modified_click = |h: &mut Harness, pos, modifiers| {
        h.pointer_with_modifiers(pos, egui::PointerButton::Primary, true, modifiers);
        h.pointer_with_modifiers(pos, egui::PointerButton::Primary, false, modifiers);
    };

    h.click(first);
    modified_click(&mut h, last, egui::Modifiers::SHIFT);
    assert_eq!(h.app.selected, BTreeSet::from([a, b, c, d, e]));
    modified_click(&mut h, second, egui::Modifiers::SHIFT);
    assert_eq!(h.app.selected, BTreeSet::from([a, b]));
    // Repeating Shift-click keeps the range selected instead of toggling it off.
    modified_click(&mut h, second, egui::Modifiers::SHIFT);
    assert_eq!(h.app.selected, BTreeSet::from([a, b]));
    assert_eq!(h.app.doc.model.edit_context, None);

    h.click(last);
    modified_click(&mut h, second, egui::Modifiers::SHIFT);
    assert_eq!(h.app.selected, BTreeSet::from([b, c, d, e]));

    h.click(first);
    modified_click(&mut h, last, egui::Modifiers::CTRL);
    assert_eq!(h.app.selected, BTreeSet::from([a, e]));
    modified_click(
        &mut h,
        hidden,
        egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
    );
    assert_eq!(h.app.selected, BTreeSet::from([a, c, d, e]));
    assert!(h.app.doc.model.is_hidden(c));
    assert!(h.app.doc.model.is_locked(d));
}

#[test]
fn outliner_modifier_clicks_toggle_visible_hidden_and_locked_items() {
    for modifiers in [egui::Modifiers::CTRL, egui::Modifiers::COMMAND] {
        let mut h = Harness::new();
        let a = object(&mut h, "First", false, DVec3::ZERO);
        let b = object(&mut h, "Second", false, DVec3::Y * 2.);
        let c = object(&mut h, "Hidden", false, DVec3::Y * 4.);
        let d = object(&mut h, "Locked", false, DVec3::Y * 6.);
        h.app.doc.model.hidden.insert(c);
        h.app.doc.model.locked.insert(d);
        let first = outliner_row(&mut h, "Group: First");
        let second = outliner_row(&mut h, "Group: Second");
        let hidden = outliner_row(&mut h, "Group: Hidden · Hidden");
        let locked = outliner_row(&mut h, "Group: Locked · Locked");
        h.click(first);
        assert_eq!(h.app.selected, BTreeSet::from([a]));
        for (pos, expected) in [
            (second, BTreeSet::from([a, b])),
            (hidden, BTreeSet::from([a, b, c])),
            (locked, BTreeSet::from([a, b, c, d])),
            (second, BTreeSet::from([a, c, d])),
            // A rapid second modifier-click must toggle, rather than enter the group.
            (second, BTreeSet::from([a, b, c, d])),
        ] {
            h.pointer_with_modifiers(pos, egui::PointerButton::Primary, true, modifiers);
            h.pointer_with_modifiers(pos, egui::PointerButton::Primary, false, modifiers);
            assert_eq!(h.app.selected, expected, "{modifiers:?}");
            assert_eq!(h.app.doc.model.edit_context, None);
        }
        assert!(h.app.doc.model.hidden.contains(&c));
        assert!(h.app.doc.model.is_locked(d));

        let menu = open_context_menu(&mut h, hidden);
        assert_eq!(h.app.selected, BTreeSet::from([a, b, c, d]));
        h.click(text_position(&menu, "Unhide"));
        assert!(!h.app.doc.model.is_hidden(c));
        assert!(h.app.doc.model.is_locked(d));
    }
}

#[test]
fn outliner_and_viewport_offer_the_same_object_commands() {
    let mut h = Harness::new();
    let id = object(&mut h, "Beam", false, DVec3::ZERO);
    let row = outliner_row(&mut h, "Group: Beam");
    let menu = open_context_menu(&mut h, row);
    assert_eq!(h.app.selected, BTreeSet::from([id]));
    let commands = [
        "Rename…",
        "Edit Group",
        "Make Group…",
        "Create Component…",
        "Ungroup",
        "Explode",
        "Cut",
        "Copy",
        "Paste",
        "Paste in Place",
        "Delete",
        "Hide",
        "Lock",
    ];
    for command in commands {
        text_position(&menu, command);
    }
    h.key(Key::Escape);
    let point = h.at(DVec3::X);
    let menu = open_context_menu(&mut h, point);
    for command in commands {
        text_position(&menu, command);
    }
}

#[test]
fn outliner_context_menu_preserves_multiselection_and_runs_shared_commands() {
    let mut h = Harness::new();
    let a = object(&mut h, "First", false, DVec3::ZERO);
    let b = object(&mut h, "Second", false, DVec3::Y * 2.);
    h.app.selected = BTreeSet::from([a, b]);
    let row = outliner_row(&mut h, "Group: First");
    let menu = open_context_menu(&mut h, row);
    assert_eq!(h.app.selected, BTreeSet::from([a, b]));
    h.click(text_position(&menu, "Hide"));
    assert_eq!(h.app.doc.model.hidden, BTreeSet::from([a, b]));
    assert!(h.app.selected.is_empty());
    assert_eq!(h.app.history.undo_label(), Some("Hide"));

    let row = outliner_row(&mut h, "Group: First · Hidden");
    let menu = open_context_menu(&mut h, row);
    assert_eq!(h.app.selected, BTreeSet::from([a]));
    h.click(text_position(&menu, "Unhide"));
    assert_eq!(h.app.doc.model.hidden, BTreeSet::from([b]));
    assert_eq!(h.app.history.undo_label(), Some("Unhide"));
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.hidden, BTreeSet::from([a, b]));
    h.app.undo(&h.ctx, true);
    assert_eq!(h.app.doc.model.hidden, BTreeSet::from([b]));
}

#[test]
fn renaming_from_either_menu_is_undoable_and_can_be_cancelled() {
    for component in [false, true] {
        let mut h = Harness::new();
        let id = object(&mut h, "Beam", component, DVec3::ZERO);
        let label = if component {
            "Component: Beam"
        } else {
            "Group: Beam"
        };
        let row = outliner_row(&mut h, label);
        let point = if component { h.at(DVec3::X) } else { row };
        let menu = open_context_menu(&mut h, point);
        h.click(text_position(&menu, "Rename…"));
        assert_eq!(h.app.object_rename, Some((id, "Beam".into())));
        h.frame(
            vec![egui::Event::Text("  Joist  ".into())],
            egui::Modifiers::NONE,
        );
        let dialog = h.frame(vec![], egui::Modifiers::NONE);
        h.click(text_position(&dialog, "Save"));
        assert!(h.app.object_rename.is_none());
        assert_eq!(h.app.doc.model.objects[&id].name, "Joist");
        assert_eq!(h.app.history.undo_label(), Some("Rename"));
        h.app.doc.model.validate().unwrap();
        h.app.undo(&h.ctx, false);
        assert_eq!(h.app.doc.model.objects[&id].name, "Beam");
        h.app.undo(&h.ctx, true);
        assert_eq!(h.app.doc.model.objects[&id].name, "Joist");

        h.app.object_rename = Some((id, "Discard".into()));
        let dialog = h.frame(vec![], egui::Modifiers::NONE);
        h.click(text_position(&dialog, "Cancel"));
        assert!(h.app.object_rename.is_none());
        assert_eq!(h.app.doc.model.objects[&id].name, "Joist");

        h.app.object_rename = Some((id, "Discard".into()));
        h.key(Key::Escape);
        assert!(h.app.object_rename.is_none());
        assert_eq!(h.app.doc.model.objects[&id].name, "Joist");
    }
}

#[test]
fn outliner_context_menu_lock_disables_rename_and_delete_until_unlocked() {
    let mut h = Harness::new();
    let id = object(&mut h, "Beam", false, DVec3::ZERO);
    h.app.doc.model.locked.insert(id);
    let row = outliner_row(&mut h, "Group: Beam · Locked");
    let menu = open_context_menu(&mut h, row);
    assert_eq!(h.app.selected, BTreeSet::from([id]));
    h.click(text_position(&menu, "Rename…"));
    assert!(h.app.object_rename.is_none());
    h.click(text_position(&menu, "Delete"));
    assert!(h.app.doc.model.objects.contains_key(&id));
    let menu = open_context_menu(&mut h, row);
    h.click(text_position(&menu, "Unlock"));
    assert!(!h.app.doc.model.is_locked(id));
}
