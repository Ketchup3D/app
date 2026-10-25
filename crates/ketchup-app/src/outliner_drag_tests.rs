use super::*;

fn object(h: &mut Harness, name: &str, component: bool, offset: DVec3) -> u64 {
    let edge = h.app.doc.model.add_edge(offset, offset + DVec3::X).unwrap();
    h.app
        .doc
        .model
        .make_object(&BTreeSet::from([edge]), component, name)
        .unwrap()
}

fn frame(h: &mut Harness) -> egui::FullOutput {
    h.frame(vec![], egui::Modifiers::NONE)
}

fn row(h: &mut Harness, label: &str) -> Pos2 {
    frame(h);
    text_position(&frame(h), label)
}

fn begin_drag(h: &mut Harness, label: &str) -> egui::FullOutput {
    h.ctx
        .style_mut_of(h.ctx.theme(), |style| style.animation_time = 0.);
    let source = row(h, label);
    h.pointer(source, egui::PointerButton::Primary, true, false);
    h.frame(
        vec![egui::Event::PointerMoved(source + egui::vec2(16., 0.))],
        egui::Modifiers::NONE,
    );
    frame(h);
    frame(h)
}

fn drop_on(h: &mut Harness, source: &str, destination: &str) -> egui::FullOutput {
    let output = begin_drag(h, source);
    let target = text_position(&output, destination);
    h.frame(
        vec![egui::Event::PointerMoved(target)],
        egui::Modifiers::NONE,
    );
    // Wait for a collapsed destination to expand before continuing in its tree.
    h.time += 0.5;
    let output = frame(h);
    let target = text_position(&output, destination);
    h.pointer(target, egui::PointerButton::Primary, false, false)
}

#[test]
fn outliner_drag_moves_selected_objects_into_and_out_of_a_group_with_undo() {
    let mut h = Harness::new();
    let a = object(&mut h, "First", false, DVec3::ZERO);
    let b = object(&mut h, "Second", true, DVec3::Y * 2.);
    let destination = object(&mut h, "Destination", false, DVec3::Y * 4.);
    h.app.selected = BTreeSet::from([a, b]);
    let vertices = h.app.doc.model.vertices.clone();
    drop_on(&mut h, "Group: First", "Group: Destination");
    assert_eq!(h.app.doc.model.parent(a), Some(destination));
    assert_eq!(h.app.doc.model.parent(b), Some(destination));
    assert_eq!(h.app.selected, BTreeSet::from([a, b]));
    assert_eq!(h.app.doc.model.vertices, vertices);
    assert_eq!(h.app.history.undo_label(), Some("Move objects"));
    h.app.doc.model.validate().unwrap();

    let output = begin_drag(&mut h, "Group: First");
    assert!(!has_label(&output, "Move to Model"));
    let top = text_position(&output, "Group: Destination · Editing") - egui::vec2(0., 8.);
    h.frame(vec![egui::Event::PointerMoved(top)], egui::Modifiers::NONE);
    h.pointer(top, egui::PointerButton::Primary, false, false);
    assert_eq!(h.app.doc.model.parent(a), None);
    assert_eq!(h.app.doc.model.parent(b), None);
    assert_eq!(h.app.doc.model.edit_context, None);
    assert_eq!(h.app.selected, BTreeSet::from([a, b]));
    assert_eq!(h.app.doc.model.vertices, vertices);
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.parent(a), Some(destination));
    assert_eq!(h.app.doc.model.parent(b), Some(destination));
    h.app.undo(&h.ctx, true);
    assert_eq!(h.app.doc.model.parent(a), None);
    assert_eq!(h.app.doc.model.parent(b), None);
    h.app.doc.model.validate().unwrap();
}

#[test]
fn outliner_drag_selects_the_dragged_object_without_moving_an_unselected_object() {
    let mut h = Harness::new();
    let a = object(&mut h, "First", false, DVec3::ZERO);
    let b = object(&mut h, "Second", false, DVec3::Y * 2.);
    let destination = object(&mut h, "Destination", false, DVec3::Y * 4.);
    h.app.selected = BTreeSet::from([b]);
    drop_on(&mut h, "Group: First", "Group: Destination");
    assert_eq!(h.app.doc.model.parent(a), Some(destination));
    assert_eq!(h.app.doc.model.parent(b), None);
    assert_eq!(h.app.selected, BTreeSet::from([a]));
}

#[test]
fn outliner_drag_rejects_locked_targets_and_escape_cancels_the_move() {
    let mut h = Harness::new();
    let source = object(&mut h, "Source", false, DVec3::ZERO);
    let destination = object(&mut h, "Destination", false, DVec3::Y * 4.);
    h.app.doc.model.locked.insert(destination);
    let before = h.app.doc.model.clone();
    let output = begin_drag(&mut h, "Group: Source");
    let target = text_position(&output, "Group: Destination · Locked");
    let output = h.frame(
        vec![egui::Event::PointerMoved(target)],
        egui::Modifiers::NONE,
    );
    assert_eq!(
        output.platform_output.cursor_icon,
        egui::CursorIcon::NotAllowed
    );
    h.pointer(target, egui::PointerButton::Primary, false, false);
    assert_eq!(h.app.doc.model, before);
    assert!(h.app.history.undo_label().is_none());

    begin_drag(&mut h, "Group: Source");
    h.key(Key::Escape);
    h.pointer(target, egui::PointerButton::Primary, false, false);
    assert_eq!(h.app.doc.model.parent(source), None);
    assert_eq!(h.app.doc.model, before);
    assert!(h.app.history.undo_label().is_none());
}

fn has_label(output: &egui::FullOutput, label: &str) -> bool {
    fn contains(shape: &egui::Shape, label: &str) -> bool {
        match shape {
            egui::Shape::Text(text) => text.galley.job.text == label,
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| contains(shape, label)),
            _ => false,
        }
    }
    output
        .shapes
        .iter()
        .any(|shape| contains(&shape.shape, label))
}

#[test]
fn outliner_drag_hover_expands_only_after_500ms_without_moving_the_pointer() {
    let mut h = Harness::new();
    let source = object(&mut h, "Source", false, DVec3::ZERO);
    let destination = object(&mut h, "Destination", false, DVec3::Y * 2.);
    let child = object(&mut h, "Child", true, DVec3::Y * 4.);
    h.app
        .doc
        .model
        .reparent_objects(&BTreeSet::from([child]), Some(destination))
        .unwrap();
    h.app.selected = BTreeSet::from([source]);
    let before = h.app.doc.model.clone();
    let output = begin_drag(&mut h, "Group: Source");
    assert!(!has_label(&output, "Component: Child"));
    let target = text_position(&output, "Group: Destination");
    h.frame(
        vec![egui::Event::PointerMoved(target)],
        egui::Modifiers::NONE,
    );
    let hovered_at = h.time;
    h.time = hovered_at + 0.489;
    assert!(!has_label(&frame(&mut h), "Component: Child"));
    h.time = hovered_at + 0.491;
    assert!(has_label(&frame(&mut h), "Component: Child"));
    assert_eq!(h.app.doc.model, before);
}

#[test]
fn outliner_drag_hover_delay_restarts_when_leaving_or_switching_groups() {
    let mut h = Harness::new();
    object(&mut h, "Source", false, DVec3::ZERO);
    let destination = object(&mut h, "Destination", false, DVec3::Y * 2.);
    object(&mut h, "Other", false, DVec3::Y * 4.);
    let child = object(&mut h, "Child", true, DVec3::Y * 6.);
    h.app
        .doc
        .model
        .reparent_objects(&BTreeSet::from([child]), Some(destination))
        .unwrap();
    let output = begin_drag(&mut h, "Group: Source");
    let target = text_position(&output, "Group: Destination");
    let other = text_position(&output, "Group: Other");
    for away in [other, h.app.viewport.center()] {
        h.frame(
            vec![egui::Event::PointerMoved(target)],
            egui::Modifiers::NONE,
        );
        h.time += 0.3;
        assert!(!has_label(&frame(&mut h), "Component: Child"));
        h.frame(vec![egui::Event::PointerMoved(away)], egui::Modifiers::NONE);
        h.time += 0.3;
        frame(&mut h);
    }
    h.frame(
        vec![egui::Event::PointerMoved(target)],
        egui::Modifiers::NONE,
    );
    h.time += 0.3;
    assert!(!has_label(&frame(&mut h), "Component: Child"));
    h.time += 0.2;
    assert!(has_label(&frame(&mut h), "Component: Child"));
}

#[test]
fn outliner_drop_before_hover_delay_moves_objects_without_expanding_the_group() {
    let mut h = Harness::new();
    let source = object(&mut h, "Source", false, DVec3::ZERO);
    let destination = object(&mut h, "Destination", false, DVec3::Y * 2.);
    let output = begin_drag(&mut h, "Group: Source");
    let target = text_position(&output, "Group: Destination");
    h.frame(
        vec![egui::Event::PointerMoved(target)],
        egui::Modifiers::NONE,
    );
    h.pointer(target, egui::PointerButton::Primary, false, false);
    assert_eq!(h.app.doc.model.parent(source), Some(destination));
    assert!(!has_label(&frame(&mut h), "Group: Source"));
}

#[test]
fn outliner_row_edges_reorder_objects_and_shift_selection_uses_the_visible_order() {
    let mut h = Harness::new();
    let a = object(&mut h, "First", false, DVec3::ZERO);
    let b = object(&mut h, "Second", true, DVec3::Y * 2.);
    let c = object(&mut h, "Third", false, DVec3::Y * 4.);
    let before = h.app.doc.model.clone();
    let output = begin_drag(&mut h, "Group: Third");
    assert!(!has_label(&output, "Move to Model"));
    let target = text_position(&output, "Group: First") - egui::vec2(0., 8.);
    h.frame(
        vec![egui::Event::PointerMoved(target)],
        egui::Modifiers::NONE,
    );
    h.pointer(target, egui::PointerButton::Primary, false, false);
    assert_eq!(
        h.app
            .doc
            .model
            .ordered_objects(&h.app.doc.model.root_entities()),
        vec![c, a, b]
    );
    assert_eq!(h.app.doc.model.vertices, before.vertices);
    let output = frame(&mut h);
    assert!(text_position(&output, "Group: Third").y < text_position(&output, "Group: First").y);
    let third = text_position(&output, "Group: Third");
    h.click(third);
    let second = row(&mut h, "Component: Second");
    h.pointer(second, egui::PointerButton::Primary, true, true);
    h.pointer(second, egui::PointerButton::Primary, false, true);
    assert_eq!(h.app.selected, BTreeSet::from([a, b, c]));
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
}

#[test]
fn outliner_empty_space_accepts_a_root_drop_without_a_button() {
    let mut h = Harness::new();
    let child = object(&mut h, "Child", false, DVec3::ZERO);
    object(&mut h, "Parent", false, DVec3::Y * 2.);
    drop_on(&mut h, "Group: Child", "Group: Parent");
    let output = begin_drag(&mut h, "Group: Child");
    let parent_pos = text_position(&output, "Group: Parent · Editing");
    assert!(!has_label(&output, "Move to Model"));
    let last = text_position(&output, "Geometry (1)");
    let target = Pos2::new(parent_pos.x, last.y + 28.);
    h.frame(
        vec![egui::Event::PointerMoved(target)],
        egui::Modifiers::NONE,
    );
    h.pointer(target, egui::PointerButton::Primary, false, false);
    assert_eq!(h.app.doc.model.parent(child), None);
}
