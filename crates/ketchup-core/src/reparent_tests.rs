use crate::*;
use std::collections::BTreeSet;

fn object(model: &mut Model, name: &str, component: bool, offset: DVec3) -> u64 {
    let edge = model.add_edge(offset, offset + DVec3::X).unwrap();
    model
        .make_object(&BTreeSet::from([edge]), component, name)
        .unwrap()
}

fn instance(model: &mut Model, id: u64, offset: DVec3) -> u64 {
    let copy = model.copy_entities(&BTreeSet::from([id]));
    *model.paste_entities(&copy, offset).first().unwrap()
}

fn child_named(model: &Model, parent: u64, name: &str) -> u64 {
    *model.objects[&parent]
        .entities
        .iter()
        .find(|id| {
            model
                .objects
                .get(id)
                .is_some_and(|object| object.name == name)
        })
        .unwrap()
}

#[test]
fn reparent_preserves_ids_placement_state_and_undo_into_and_out_of_groups() {
    let mut model = Model::default();
    let source = object(&mut model, "Part", true, DVec3::X * 5.);
    let destination = object(&mut model, "Container", false, DVec3::Y * 5.);
    model.hidden.insert(source);
    let tag = model.add_tag("Parts").unwrap();
    model
        .assign_tag(&BTreeSet::from([source]), Some(tag))
        .unwrap();
    let before = model.clone();
    let mut history = History::default();
    assert_eq!(
        model.reparent_objects(&BTreeSet::from([source]), Some(destination)),
        Ok(BTreeSet::from([source]))
    );
    assert_eq!(model.parent(source), Some(destination));
    assert_eq!(model.vertices, before.vertices);
    assert_eq!(model.edges, before.edges);
    assert_eq!(model.objects[&source], before.objects[&source]);
    assert_eq!(model.hidden, before.hidden);
    assert_eq!(model.entity_tags, before.entity_tags);
    model.validate().unwrap();
    let grouped = model.clone();
    history.record("Move objects", before.clone(), &model);
    assert!(history.undo(&mut model));
    assert_eq!(model, before);
    assert!(history.redo(&mut model));
    assert_eq!(model, grouped);

    model
        .reparent_objects(&BTreeSet::from([source]), None)
        .unwrap();
    assert_eq!(model.parent(source), None);
    assert_eq!(model.vertices, before.vertices);
    assert_eq!(model.objects[&source], before.objects[&source]);
    model.validate().unwrap();
}

#[test]
fn reparent_moves_multiple_objects_once_and_ignores_same_parent_drops() {
    let mut model = Model::default();
    let a = object(&mut model, "A", false, DVec3::ZERO);
    let b = object(&mut model, "B", true, DVec3::Y * 2.);
    let parent = object(&mut model, "Parent", false, DVec3::Y * 4.);
    let ids = BTreeSet::from([a, b]);
    model.reparent_objects(&ids, Some(parent)).unwrap();
    assert_eq!(model.parent(a), Some(parent));
    assert_eq!(model.parent(b), Some(parent));
    let before = model.clone();
    model.reparent_objects(&ids, Some(parent)).unwrap();
    assert_eq!(model, before);
    let moved = model
        .reparent_objects(&BTreeSet::from([parent, a, b]), None)
        .unwrap();
    assert_eq!(moved, BTreeSet::from([parent]));
    assert_eq!(model.parent(a), Some(parent));
    model.validate().unwrap();
}

#[test]
fn invalid_drops_leave_the_model_unchanged() {
    let mut model = Model::default();
    let source = object(&mut model, "Source", false, DVec3::ZERO);
    let child = object(&mut model, "Child", false, DVec3::Y * 2.);
    let target = object(&mut model, "Target", false, DVec3::Y * 4.);
    model
        .reparent_objects(&BTreeSet::from([child]), Some(source))
        .unwrap();
    for destination in [Some(source), Some(child), Some(u64::MAX)] {
        let before = model.clone();
        assert!(
            model
                .reparent_objects(&BTreeSet::from([source]), destination)
                .is_err()
        );
        assert_eq!(model, before);
    }
    for locked in [source, child, target] {
        model.locked.insert(locked);
        let before = model.clone();
        assert!(
            model
                .reparent_objects(&BTreeSet::from([source]), Some(target))
                .is_err()
        );
        assert_eq!(model, before);
        model.locked.remove(&locked);
    }
    model.hidden.insert(target);
    let before = model.clone();
    assert!(
        model
            .reparent_objects(&BTreeSet::from([source]), Some(target))
            .is_err()
    );
    assert_eq!(model, before);
    model.validate().unwrap();
}

#[test]
fn moving_out_of_a_component_updates_its_other_instances() {
    let mut model = Model::default();
    let part = object(&mut model, "Part", false, DVec3::ZERO);
    let component = model
        .make_object(&BTreeSet::from([part]), true, "Assembly")
        .unwrap();
    let part = child_named(&model, component, "Part");
    let peer = instance(&mut model, component, DVec3::X * 10.);
    let edge = *model.objects[&part].entities.first().unwrap();
    let points = model.points(&model.edges[&edge]);
    model
        .reparent_objects(&BTreeSet::from([part]), None)
        .unwrap();
    assert_eq!(model.parent(part), None);
    assert!(model.objects[&component].entities.is_empty());
    assert!(model.objects[&peer].entities.is_empty());
    assert_eq!(model.points(&model.edges[&edge]), points);
    model.validate().unwrap();
}

#[test]
fn moving_between_component_definitions_updates_both_sides() {
    let mut model = Model::default();
    let part = object(&mut model, "Part", false, DVec3::ZERO);
    let source = model
        .make_object(&BTreeSet::from([part]), true, "Source")
        .unwrap();
    let part = child_named(&model, source, "Part");
    let source_peer = instance(&mut model, source, DVec3::X * 10.);
    let target = object(&mut model, "Container", false, DVec3::Y * 5.);
    let destination = model
        .make_object(&BTreeSet::from([target]), true, "Destination")
        .unwrap();
    let target = child_named(&model, destination, "Container");
    let destination_peer = instance(&mut model, destination, DVec3::X * 20.);
    let edge = *model.objects[&part].entities.first().unwrap();
    let points = model.points(&model.edges[&edge]);

    model
        .reparent_objects(&BTreeSet::from([part]), Some(target))
        .unwrap();
    assert!(model.objects[&source].entities.is_empty());
    assert!(model.objects[&source_peer].entities.is_empty());
    assert_eq!(model.parent(part), Some(target));
    assert_eq!(model.points(&model.edges[&edge]), points);
    let peer_target = child_named(&model, destination_peer, "Container");
    let peer_part = child_named(&model, peer_target, "Part");
    let peer_edge = *model.objects[&peer_part].entities.first().unwrap();
    let (a, b) = model.points(&model.edges[&peer_edge]);
    assert_eq!(
        (a, b),
        (points.0 + DVec3::X * 20., points.1 + DVec3::X * 20.)
    );
    model.validate().unwrap();
}

#[test]
fn moving_between_groups_in_one_component_updates_peer_hierarchies() {
    let mut model = Model::default();
    let part = object(&mut model, "Part", false, DVec3::ZERO);
    let left = model
        .make_object(&BTreeSet::from([part]), false, "Left")
        .unwrap();
    let right = object(&mut model, "Right", false, DVec3::Y * 5.);
    let assembly = model
        .make_object(&BTreeSet::from([left, right]), true, "Assembly")
        .unwrap();
    let left = child_named(&model, assembly, "Left");
    let right = child_named(&model, assembly, "Right");
    let part = child_named(&model, left, "Part");
    let peer = instance(&mut model, assembly, DVec3::X * 10.);
    model
        .reparent_objects(&BTreeSet::from([part]), Some(right))
        .unwrap();
    assert!(model.objects[&left].entities.is_empty());
    assert_eq!(model.parent(part), Some(right));
    let peer_left = child_named(&model, peer, "Left");
    let peer_right = child_named(&model, peer, "Right");
    assert!(model.objects[&peer_left].entities.is_empty());
    child_named(&model, peer_right, "Part");
    model.validate().unwrap();
}

#[test]
fn recursive_component_drops_are_rejected_atomically() {
    let mut model = Model::default();
    let group = object(&mut model, "Container", false, DVec3::ZERO);
    let component = model
        .make_object(&BTreeSet::from([group]), true, "Assembly")
        .unwrap();
    let peer = instance(&mut model, component, DVec3::X * 10.);
    let target = child_named(&model, peer, "Container");
    let before = model.clone();
    assert!(
        model
            .reparent_objects(&BTreeSet::from([component]), Some(target))
            .is_err()
    );
    assert_eq!(model, before);
    model.validate().unwrap();
}

#[test]
fn sibling_reordering_preserves_visual_selection_order_and_survives_undo_and_reload() {
    let mut model = Model::default();
    let a = object(&mut model, "A", false, DVec3::ZERO);
    let b = object(&mut model, "B", true, DVec3::Y * 2.);
    let c = object(&mut model, "C", false, DVec3::Y * 4.);
    model
        .move_objects_before(&BTreeSet::from([c]), None, Some(a))
        .unwrap();
    assert_eq!(model.ordered_objects(&model.root_entities()), vec![c, a, b]);
    let before = model.clone();
    model
        .move_objects_before(&BTreeSet::from([b, c]), None, Some(a))
        .unwrap();
    assert_eq!(model.ordered_objects(&model.root_entities()), vec![c, b, a]);
    assert_eq!(model.vertices, before.vertices);
    let mut history = History::default();
    history.record("Move objects", before.clone(), &model);
    assert!(history.undo(&mut model));
    assert_eq!(model, before);
    assert!(history.redo(&mut model));
    let doc = Document {
        model,
        ..Default::default()
    };
    let restored = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(
        restored
            .model
            .ordered_objects(&restored.model.root_entities()),
        vec![c, b, a]
    );
    let mut legacy = serde_json::to_value(&doc).unwrap();
    for object in legacy["model"]["objects"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        object.as_object_mut().unwrap().remove("outliner_order");
    }
    let legacy = Document::from_json(&legacy.to_string()).unwrap();
    assert_eq!(
        legacy.model.ordered_objects(&legacy.model.root_entities()),
        vec![a, b, c]
    );
}

#[test]
fn insertion_moves_a_child_back_to_root_and_invalid_positions_are_atomic() {
    let mut model = Model::default();
    let child = object(&mut model, "Child", false, DVec3::ZERO);
    let parent = model
        .make_object(&BTreeSet::from([child]), false, "Parent")
        .unwrap();
    let child = child_named(&model, parent, "Child");
    let other = object(&mut model, "Other", false, DVec3::Y * 4.);
    model
        .move_objects_before(&BTreeSet::from([child]), None, Some(parent))
        .unwrap();
    assert_eq!(model.parent(child), None);
    assert_eq!(
        model.ordered_objects(&model.root_entities()),
        vec![child, parent, other]
    );
    let before = model.clone();
    assert!(
        model
            .move_objects_before(&BTreeSet::from([child]), Some(parent), Some(other))
            .is_err()
    );
    assert_eq!(model, before);
}

#[test]
fn sibling_reordering_inside_a_component_updates_its_other_instances() {
    let mut model = Model::default();
    let a = object(&mut model, "A", false, DVec3::ZERO);
    let b = object(&mut model, "B", false, DVec3::Y * 2.);
    let container = model
        .make_object(&BTreeSet::from([a, b]), true, "Assembly")
        .unwrap();
    let peer = instance(&mut model, container, DVec3::X * 10.);
    let a = child_named(&model, container, "A");
    let b = child_named(&model, container, "B");
    model
        .move_objects_before(&BTreeSet::from([b]), Some(container), Some(a))
        .unwrap();
    for id in [container, peer] {
        let names: Vec<_> = model
            .ordered_objects(&model.objects[&id].entities)
            .iter()
            .map(|id| model.objects[id].name.as_str())
            .collect();
        assert_eq!(names, vec!["B", "A"]);
    }
    model.validate().unwrap();
}
