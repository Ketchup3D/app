use super::*;
use ketchup_core::{DVec3, Edge, Face, Vertex};
use std::collections::BTreeMap;

pub(crate) fn triangle_grid(count: usize) -> Model {
    let mut vertices = BTreeMap::new();
    let mut edges = BTreeMap::new();
    let mut faces = BTreeMap::new();
    let mut next = 0u64;
    for i in 0..count {
        let p = DVec3::new((i % 64) as f64 * 3., (i / 64) as f64 * 3., 0.);
        let ids = [next + 1, next + 2, next + 3];
        for (id, position) in ids.into_iter().zip([p, p + DVec3::X, p + DVec3::Y]) {
            vertices.insert(id, Vertex { id, position });
        }
        for j in 0..3 {
            let id = next + 4 + j as u64;
            edges.insert(
                id,
                Edge {
                    id,
                    a: ids[j],
                    b: ids[(j + 1) % 3],
                },
            );
        }
        let id = next + 7;
        faces.insert(
            id,
            Face {
                id,
                vertices: ids.to_vec(),
                holes: vec![],
            },
        );
        next += 7;
    }
    serde_json::from_value(serde_json::json!({
        "vertices":vertices,"edges":edges,"faces":faces,"next_id":next
    }))
    .unwrap()
}

#[test]
fn edge_hover_reuses_projection_and_queries_only_nearby_edges() {
    let mut model = triangle_grid(4096);
    let mut camera = Camera::default();
    let size = DVec2::new(1200., 800.);
    camera.fit(model.bounds(), size);
    let mut entities = EntityCache::default();
    entities.update(&model);
    let mut edges = EdgePickingCache::default();
    edges.update(&model, &camera, size);
    let view = camera.view(size);
    for i in [130, 521, 1300, 2048, 3000] {
        let point = DVec3::new((i % 64) as f64 * 3. + 0.5, (i / 64) as f64 * 3., 0.);
        let Some(pixel) = view.project(point) else {
            continue;
        };
        let actual = edges.candidates(pixel, 2., &entities);
        let mut expected: Vec<_> = model
            .edges
            .values()
            .filter_map(|edge| {
                let (a, b) = model.points(edge);
                let (a, b) = view.segment(a, b)?;
                let distance = selection::distance_to_segment(pixel, a, b);
                (distance <= 2.).then_some((edge.id, distance))
            })
            .collect();
        expected.sort_unstable_by(|(a, x), (b, y)| x.total_cmp(y).then_with(|| b.cmp(a)));
        assert_eq!(actual, expected);
        assert!(
            edges.tested_edges < 256,
            "tested {} of {} edges",
            edges.tested_edges,
            model.edges.len()
        );
        edges.update(&model, &camera, size);
        assert_eq!(
            edges.projection_builds, 1,
            "hover must reuse edge projections"
        );
    }
    let id = *model.edges.keys().next().unwrap();
    model.hidden.insert(id);
    entities.update(&model);
    edges.update(&model, &camera, size);
    assert_eq!(edges.projection_builds, 1);
    let (a, b) = model.points(&model.edges[&id]);
    if let Some(pixel) = view.project((a + b) * 0.5) {
        assert!(
            !edges
                .candidates(pixel, 8., &entities)
                .iter()
                .any(|&(edge, _)| edge == id)
        );
    }
}

#[test]
fn edge_hover_cache_tracks_camera_resize_and_direct_edits_to_free_edges() {
    let mut model = Model::default();
    model.add_edge(-DVec3::X, DVec3::X).unwrap();
    let mut camera = Camera::default();
    let size = DVec2::new(1000., 800.);
    let mut edges = EdgePickingCache::default();
    edges.update(&model, &camera, size);
    edges.update(&model, &camera, size);
    assert_eq!(edges.projection_builds, 1);
    camera.yaw += 0.1;
    edges.update(&model, &camera, size);
    assert_eq!(edges.projection_builds, 2);
    edges.update(&model, &camera, size * 0.75);
    assert_eq!(edges.projection_builds, 3);
    model.vertices.values_mut().next().unwrap().position.x -= 0.2;
    edges.update(&model, &camera, size * 0.75);
    assert_eq!(edges.projection_builds, 4);
    model.add_edge(DVec3::Y, DVec3::Y + DVec3::X).unwrap();
    edges.update(&model, &camera, size * 0.75);
    assert_eq!(edges.projection_builds, 5);
    model.edges.clear();
    edges.update(&model, &camera, size * 0.75);
    assert!(edges.items.is_empty());
    assert!(edges.nodes.is_empty());
}

#[test]
fn spatial_queries_visit_nearby_faces_instead_of_the_whole_import() {
    let model = triangle_grid(4096);
    let mut faces = FaceCache::default();
    faces.update(&model);
    let mut entities = EntityCache::default();
    entities.update(&model);
    let mut picking = PickingCache::default();
    picking.update(&faces);
    let ray = Ray {
        origin: DVec3::new(30.2, 30.2, 10.),
        direction: -DVec3::Z,
    };
    let mut visited = 0;
    let hit = picking
        .nearest_filtered(&faces, &entities, ray, false, &mut visited)
        .unwrap();
    assert!(visited <= 8, "tested {visited} faces out of 4096");
    assert_eq!(Some(hit), model.nearest_context_face(ray));
    let revision = faces.revision();
    faces.update(&model);
    assert_eq!(revision, faces.revision());
}

#[test]
fn cached_queries_follow_geometry_visibility_and_editing_scope() {
    let mut model = triangle_grid(2);
    let ids = model
        .root_entities()
        .into_iter()
        .filter(|id| *id <= 7)
        .collect();
    let group = model.make_object(&ids, false, "First triangle").unwrap();
    let mut faces = FaceCache::default();
    let mut entities = EntityCache::default();
    let mut picking = PickingCache::default();
    let ray = Ray {
        origin: DVec3::new(0.2, 0.2, 10.),
        direction: -DVec3::Z,
    };
    faces.update(&model);
    entities.update(&model);
    picking.update(&faces);
    let hit = picking.nearest(&faces, &entities, ray, false).unwrap();
    assert_eq!(entities.get(hit.0).context, Some(group));
    assert!(picking.nearest(&faces, &entities, ray, true).is_none());
    model.edit_context = Some(group);
    entities.update(&model);
    assert_eq!(
        picking.nearest(&faces, &entities, ray, true),
        model.nearest_editable_face(ray)
    );
    assert_eq!(
        faces.context_bounds(&model, &entities),
        model.object_contents(group).unwrap().bounds()
    );
    model.hidden.insert(group);
    entities.update(&model);
    assert!(picking.nearest(&faces, &entities, ray, false).is_none());
    model.hidden.clear();
    model.edit_context = None;
    for vertex in model.vertices.values_mut() {
        vertex.position.x += 20.;
    }
    faces.update(&model);
    entities.update(&model);
    picking.update(&faces);
    assert!(picking.nearest(&faces, &entities, ray, false).is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cached_polygon_queries_keep_holes_and_edge_precedence() {
    let model = crate::interchange::import_skp(
        include_bytes!("../tests/fixtures/placed-window.skp"),
        "Window",
    )
    .unwrap();
    let mut faces = FaceCache::default();
    faces.update(&model);
    let mut entities = EntityCache::default();
    entities.update(&model);
    let mut picking = PickingCache::default();
    picking.update(&faces);
    for x in [0.4, 1.0, 1.524, 2.5] {
        let ray = Ray {
            origin: DVec3::new(x, 6.35, 20.),
            direction: -DVec3::Z,
        };
        assert_eq!(
            picking.nearest(&faces, &entities, ray, false),
            model.nearest_context_face(ray)
        );
    }
    let camera = Camera::default();
    let size = DVec2::new(900., 700.);
    for point in [
        DVec3::new(0.254, 5.08, 0.508),
        DVec3::new(0.3, 5.2, 0.508),
        DVec3::new(1.524, 6.35, 0.508),
    ] {
        if let Some(pixel) = camera.project(point, size) {
            assert_eq!(
                picking.pick(&model, &faces, &entities, &camera, size, pixel, 8., true),
                selection::pick_entity(&model, &camera, size, pixel, 8.)
            );
        }
    }
}

#[test]
fn cached_snapping_matches_geometry_locks_visibility_and_editing_contexts() {
    use ketchup_core::{
        Axis,
        inference::{FaceQuery, infer_with_face_query, infer_with_snapping},
    };
    let mut model = triangle_grid(6);
    let ids = model
        .root_entities()
        .into_iter()
        .filter(|id| *id <= 7)
        .collect();
    let group = model.make_object(&ids, false, "Triangle").unwrap();
    let outer = model
        .make_object(&[group].into(), false, "Nested triangle")
        .unwrap();
    let tag = model.add_tag("Hidden triangle").unwrap();
    model.tags.get_mut(&tag).unwrap().visible = false;
    let mut faces = FaceCache::default();
    let mut entities = EntityCache::default();
    let mut picking = PickingCache::default();
    for state in 0..6 {
        model.edit_context = (state == 1).then_some(group);
        model.hidden = match state {
            2 => [group].into(),
            4 => [outer].into(),
            _ => Default::default(),
        };
        model.entity_tags.clear();
        if matches!(state, 3 | 5) {
            model
                .entity_tags
                .insert(if state == 3 { group } else { outer }, tag);
        }
        faces.update(&model);
        entities.update(&model);
        picking.update(&faces);
        let query = picking.snap_query(&faces, &entities);
        for id in model
            .edges
            .keys()
            .chain(model.faces.keys())
            .chain(model.objects.keys())
        {
            assert_eq!(query.is_hidden(&model, *id), model.is_hidden(*id));
        }
        for yaw in [-0.9, 0.7] {
            let mut camera = Camera {
                yaw,
                ..Default::default()
            };
            let size = DVec2::new(900., 700.);
            camera.fit(model.bounds(), size);
            for i in 0..35 {
                let pixel = DVec2::new(170. + (i % 7) as f64 * 90., 180. + (i / 7) as f64 * 75.);
                let ray = camera.ray(pixel, size);
                assert_eq!(query.nearest(ray, false), model.nearest_face(ray));
                assert_eq!(query.nearest(ray, true), model.nearest_context_face(ray));
                for point in [
                    DVec3::ZERO,
                    ray.origin + ray.direction * 10.,
                    ray.origin + ray.direction * 100.,
                ] {
                    assert_eq!(
                        picking.visible(&faces, &entities, &camera, size, point),
                        selection::visible(&model, &camera, size, point),
                        "state={state}, point={point:?}"
                    );
                }
                for (start, lock) in [
                    (None, None),
                    (Some(DVec3::ZERO), None),
                    (Some(DVec3::ZERO), Some(Axis::X)),
                    (Some(DVec3::ZERO), Some(Axis::Z)),
                ] {
                    for (geometry, axes) in
                        [(true, true), (true, false), (false, true), (false, false)]
                    {
                        let expected = infer_with_snapping(
                            &model, &camera, size, pixel, start, lock, geometry, axes,
                        );
                        let actual = infer_with_face_query(
                            &model,
                            &camera,
                            size,
                            pixel,
                            start,
                            lock,
                            geometry,
                            axes,
                            Some(&query),
                        );
                        assert_eq!(
                            actual.map(|s| s.kind),
                            expected.map(|s| s.kind),
                            "state={state}, pixel={pixel:?}"
                        );
                        if let (Some(a), Some(b)) = (actual, expected) {
                            assert!(
                                a.point.distance(b.point) < 1e-7,
                                "state={state}, {a:?} != {b:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
