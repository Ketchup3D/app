use crate::*;
use std::collections::BTreeSet;

fn rectangle(model: &mut Model) {
    let points = [
        DVec3::ZERO,
        DVec3::X * 2.,
        DVec3::new(2., 2., 0.),
        DVec3::Y * 2.,
    ];
    for i in 0..4 {
        model.add_edge(points[i], points[(i + 1) % 4]).unwrap();
    }
}
fn object(model: &mut Model, component: bool, name: &str) -> u64 {
    model
        .make_object(&model.selectable_entities(), component, name)
        .unwrap()
}
fn child_object(model: &Model, parent: u64) -> u64 {
    *model.objects[&parent]
        .entities
        .iter()
        .find(|id| model.objects.contains_key(id))
        .unwrap()
}
fn first_face(model: &Model, parent: u64) -> u64 {
    *model
        .expand_entities(&model.objects[&parent].entities)
        .iter()
        .find(|id| model.faces.contains_key(id))
        .unwrap()
}

#[test]
fn nested_scopes_select_only_immediate_children_and_keep_new_geometry_inside() {
    let mut model = Model::default();
    rectangle(&mut model);
    object(&mut model, false, "Inner");
    let outer = object(&mut model, false, "Outer");
    let inner = child_object(&model, outer);
    let outside = model.add_edge(DVec3::X * 5., DVec3::X * 7.).unwrap();
    assert!(model.enter_object(outer));
    assert_eq!(model.selectable_entities(), BTreeSet::from([inner]));
    assert_eq!(model.entity_in_context(outside), None);
    assert!(model.enter_object(inner));
    assert_eq!(model.edit_path(), vec![outer, inner]);
    let edge = model.add_edge(DVec3::ZERO, DVec3::Z).unwrap();
    assert_eq!(model.parent(edge), Some(inner));
    let face = first_face(&model, inner);
    model.erase(&BTreeSet::from([face]));
    assert!(model.objects.contains_key(&inner));
    assert!(model.objects.contains_key(&outer));
    assert!(!model.faces.contains_key(&face));
    assert!(model.edges.contains_key(&outside));
    let group = model
        .make_object(&BTreeSet::from([edge]), false, "Nested again")
        .unwrap();
    assert_eq!(model.parent(group), Some(inner));
    model.dissolve_objects(&BTreeSet::from([group]), true);
    assert_eq!(model.edit_context, Some(inner));
    model.validate().unwrap();
}

#[test]
fn component_geometry_edits_propagate_with_each_instances_position_and_scale() {
    let mut model = Model::default();
    rectangle(&mut model);
    let original = object(&mut model, true, "Panel");
    let copy = model.copy_entities(&BTreeSet::from([original]));
    let peer = *model.paste_entities(&copy, DVec3::X * 10.).first().unwrap();
    model
        .scale_selection(&BTreeSet::from([peer]), DVec3::ZERO, 2.)
        .unwrap();
    model
        .move_selection(&BTreeSet::from([peer]), DVec3::Z * 3.)
        .unwrap();
    let original_frame = model.objects[&original].transform;
    let peer_frame = model.objects[&peer].transform;
    assert!(model.enter_object(original));
    let face = first_face(&model, original);
    model.push_pull(face, 1.).unwrap();
    assert_eq!(model.object_contents(original).unwrap().faces.len(), 6);
    assert_eq!(model.object_contents(peer).unwrap().faces.len(), 6);
    let (lo, hi) = model.object_contents(peer).unwrap().bounds().unwrap();
    assert!(lo.distance(DVec3::new(20., 0., 3.)) < 1e-7);
    assert!(hi.distance(DVec3::new(24., 4., 5.)) < 1e-7);
    model.edit_context = Some(peer);
    model
        .move_selection(&model.selectable_entities(), DVec3::X * 2.)
        .unwrap();
    let (lo, hi) = model.object_contents(original).unwrap().bounds().unwrap();
    assert!(lo.distance(DVec3::X) < 1e-7);
    assert!(hi.distance(DVec3::new(3., 2., 1.)) < 1e-7);
    assert_eq!(model.objects[&original].transform, original_frame);
    assert_eq!(model.objects[&peer].transform, peer_frame);
    model.validate().unwrap();
}

#[test]
fn editing_group_inside_component_updates_other_instances_of_its_container() {
    let mut model = Model::default();
    rectangle(&mut model);
    object(&mut model, false, "Inside");
    let component = object(&mut model, true, "Assembly");
    let copy = model.copy_entities(&BTreeSet::from([component]));
    let peer = *model.paste_entities(&copy, DVec3::X * 8.).first().unwrap();
    let group = child_object(&model, component);
    assert!(model.enter_object(component));
    assert!(model.enter_object(group));
    model.add_edge(DVec3::ZERO, DVec3::Z * 3.).unwrap();
    let peer_group = child_object(&model, peer);
    assert_eq!(model.object_contents(group).unwrap().edges.len(), 5);
    assert_eq!(model.object_contents(peer_group).unwrap().edges.len(), 5);
    assert_eq!(model.edit_path(), vec![component, group]);
    model.validate().unwrap();
}

#[test]
fn nested_component_edits_reach_standalone_instances_and_all_container_definitions() {
    let mut model = Model::default();
    rectangle(&mut model);
    let part = object(&mut model, true, "Part");
    let copy = model.copy_entities(&BTreeSet::from([part]));
    let nested = model.paste_entities(&copy, DVec3::X * 5.);
    let assembly = model.make_object(&nested, true, "Assembly").unwrap();
    let assembly_copy = model.copy_entities(&BTreeSet::from([assembly]));
    let other_assembly = *model
        .paste_entities(&assembly_copy, DVec3::X * 10.)
        .first()
        .unwrap();
    assert!(model.enter_object(part));
    model.add_edge(DVec3::ZERO, DVec3::Z).unwrap();
    assert_eq!(model.object_contents(part).unwrap().edges.len(), 5);
    assert_eq!(model.object_contents(assembly).unwrap().edges.len(), 5);
    assert_eq!(
        model.object_contents(other_assembly).unwrap().edges.len(),
        5
    );
    model.edit_context = None;
    assert!(model.enter_object(assembly));
    let nested = child_object(&model, assembly);
    assert!(model.enter_object(nested));
    model
        .add_edge(DVec3::X * 5., DVec3::new(5., 0., 2.))
        .unwrap();
    assert_eq!(model.object_contents(part).unwrap().edges.len(), 6);
    assert_eq!(
        model.object_contents(other_assembly).unwrap().edges.len(),
        6
    );
    model.validate().unwrap();
}

#[test]
fn recursive_component_paste_is_rejected_and_deleting_every_child_remains_valid() {
    let mut model = Model::default();
    rectangle(&mut model);
    let component = object(&mut model, true, "Part");
    let copy = model.copy_entities(&BTreeSet::from([component]));
    let peer = *model.paste_entities(&copy, DVec3::X * 5.).first().unwrap();
    assert!(model.enter_object(component));
    let before = model.clone();
    assert!(model.paste_entities(&copy, DVec3::ZERO).is_empty());
    assert_eq!(model, before);
    model.erase(&model.selectable_entities());
    assert!(model.objects[&component].entities.is_empty());
    assert!(model.objects[&peer].entities.is_empty());
    model.validate().unwrap();
    let mut doc = Document::default();
    doc.model = model;
    let restored = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert!(restored.model.edit_context.is_none());
    restored.model.validate().unwrap();
}

#[test]
fn scoped_history_restores_shared_topology_and_legacy_instances_recover_frames() {
    let mut doc = Document::default();
    rectangle(&mut doc.model);
    let component = object(&mut doc.model, true, "Part");
    let copy = doc.model.copy_entities(&BTreeSet::from([component]));
    let peer = *doc
        .model
        .paste_entities(&copy, DVec3::X * 5.)
        .first()
        .unwrap();
    doc.model
        .scale_selection(&BTreeSet::from([peer]), DVec3::X * 5., 2.)
        .unwrap();
    let mut json = serde_json::to_value(&doc).unwrap();
    for object in json["model"]["objects"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        object.as_object_mut().unwrap().remove("transform");
    }
    doc = Document::from_json(&json.to_string()).unwrap();
    assert!(doc.model.enter_object(component));
    let before = doc.model.clone();
    doc.model.add_edge(DVec3::ZERO, DVec3::Z).unwrap();
    let after = doc.model.clone();
    let mut history = History::default();
    history.record("Edit component", before.clone(), &doc.model);
    assert!(history.undo(&mut doc.model));
    assert_eq!(doc.model, before);
    assert!(history.redo(&mut doc.model));
    assert_eq!(doc.model, after);
    let (lo, hi) = doc.model.object_contents(peer).unwrap().bounds().unwrap();
    assert!(lo.distance(DVec3::X * 5.) < 1e-7);
    assert!(hi.distance(DVec3::new(9., 4., 2.)) < 1e-7);
    doc.model.validate().unwrap();
}

#[test]
fn component_clipboard_uses_current_topology_after_definition_was_edited() {
    let mut model = Model::default();
    rectangle(&mut model);
    let component = object(&mut model, true, "Panel");
    let clipboard = model.copy_entities(&BTreeSet::from([component]));
    assert!(model.enter_object(component));
    model.push_pull(first_face(&model, component), 1.).unwrap();
    model.edit_context = None;
    let peer = *model
        .paste_entities(&clipboard, DVec3::X * 5.)
        .first()
        .unwrap();
    assert_eq!(model.object_contents(peer).unwrap().faces.len(), 6);
    let (lo, hi) = model.object_contents(peer).unwrap().bounds().unwrap();
    assert!(lo.distance(DVec3::X * 5.) < 1e-7);
    assert!(hi.distance(DVec3::new(7., 2., 1.)) < 1e-7);
    model.validate().unwrap();
}

#[test]
fn scoped_dimensions_stay_in_the_object_and_follow_component_geometry_and_scale() {
    let mut model = Model::default();
    rectangle(&mut model);
    let component = object(&mut model, true, "Dimensioned");
    let copy = model.copy_entities(&BTreeSet::from([component]));
    let peer = *model.paste_entities(&copy, DVec3::X * 5.).first().unwrap();
    model
        .scale_selection(&BTreeSet::from([peer]), DVec3::X * 5., 2.)
        .unwrap();
    assert!(model.enter_object(component));
    let dimension = model
        .add_dimension(DVec3::ZERO, DVec3::X * 2., -DVec3::Y)
        .unwrap();
    assert_eq!(model.parent(dimension), Some(component));
    let peer_contents = model.object_contents(peer).unwrap();
    assert_eq!(peer_contents.dimensions.len(), 1);
    let peer_dimension = peer_contents.dimensions.values().next().unwrap();
    assert!((peer_dimension.length(&peer_contents) - 4.).abs() < 1e-7);
    assert!(peer_dimension.offset.distance(-DVec3::Y * 2.) < 1e-7);
    model.erase(&BTreeSet::from([dimension]));
    assert!(model.object_contents(peer).unwrap().dimensions.is_empty());
    model.validate().unwrap();
}

#[test]
fn context_picking_and_box_selection_exclude_other_objects_and_dive_one_level() {
    let mut model = Model::default();
    let edge = model.add_edge(DVec3::ZERO, DVec3::X * 2.).unwrap();
    object(&mut model, false, "Inner");
    let outer = object(&mut model, false, "Outer");
    let inner = child_object(&model, outer);
    let outside = model
        .add_edge(DVec3::Y * 3., DVec3::new(2., 3., 0.))
        .unwrap();
    let camera = Camera::default();
    let size = DVec2::new(1000., 800.);
    let point = camera.project(DVec3::X, size).unwrap();
    assert_eq!(
        selection::pick_entity(&model, &camera, size, point, 8.),
        Some(outer)
    );
    assert!(model.enter_object(outer));
    assert_eq!(
        selection::pick_entity(&model, &camera, size, point, 8.),
        Some(inner)
    );
    assert_eq!(
        selection::box_select(&model, &camera, size, DVec2::ZERO, size),
        BTreeSet::from([inner])
    );
    let outside_point = camera.project(DVec3::new(1., 3., 0.), size).unwrap();
    assert!(selection::pick_entity(&model, &camera, size, outside_point, 8.).is_none());
    assert!(model.enter_object(inner));
    let child = selection::pick_entity(&model, &camera, size, point, 8.).unwrap();
    assert!(model.edges.contains_key(&child));
    assert_ne!(child, edge);
    assert_ne!(child, outside);
}

#[test]
fn dimensions_outside_an_instance_keep_tracking_vertices_after_shared_edits() {
    let mut model = Model::default();
    rectangle(&mut model);
    let component = object(&mut model, true, "Panel");
    let copy = model.copy_entities(&BTreeSet::from([component]));
    model.paste_entities(&copy, DVec3::X * 5.);
    let dimension = model
        .add_dimension(DVec3::X * 5., DVec3::X * 7., -DVec3::Y)
        .unwrap();
    assert!(model.enter_object(component));
    model
        .scale_selection(&model.selectable_entities(), DVec3::ZERO, 1.5)
        .unwrap();
    assert!((model.dimensions[&dimension].length(&model) - 3.).abs() < 1e-7);
    assert!(!model.dimensions[&dimension].start.weights.is_empty());
    model.validate().unwrap();
}
