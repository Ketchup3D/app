use crate::{Camera, CameraView, Model};
use glam::DVec2;
use std::collections::BTreeSet;

pub fn visible(model: &Model, camera: &Camera, size: DVec2, point: crate::DVec3) -> bool {
    visible_in_view(model, &camera.view(size), point)
}

pub(crate) fn visible_in_view(model: &Model, view: &CameraView, point: crate::DVec3) -> bool {
    let Some(pixel) = view.project(point) else {
        return false;
    };
    model
        .nearest_context_face(view.ray(pixel))
        .is_none_or(|(_, t, _)| t + 1e-5 >= point.distance(view.eye()))
}

pub fn pick_entity(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    point: DVec2,
    radius: f64,
) -> Option<u64> {
    pick_dimension(model, camera, size, point, radius)
        .or_else(|| pick(model, camera, size, point, radius))
        .or_else(|| {
            model
                .nearest_context_face(camera.ray(point, size))
                .and_then(|(id, _, _)| model.entity_in_context(id))
        })
}

pub fn pick_dimension(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    point: DVec2,
    radius: f64,
) -> Option<u64> {
    model
        .dimensions
        .values()
        .filter(|d| !model.is_hidden(d.id))
        .filter_map(|d| {
            let id = model.entity_in_context(d.id)?;
            let [_, _, a, b] = d.layout(model);
            let (a, b) = camera.segment(a, b, size)?;
            let center = (a + b) * 0.5;
            let distance = distance_to_segment(point, a, b);
            let label = (point.x - center.x).abs() < 38. && (point.y - center.y).abs() < 11.;
            (distance <= radius || label).then_some((id, if label { 0. } else { distance }))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

pub fn distance_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    p.distance(closest_point(p, a, b))
}
pub fn closest_point(p: DVec2, a: DVec2, b: DVec2) -> DVec2 {
    let d = b - a;
    let t = if d.length_squared() < 1e-12 {
        0.0
    } else {
        ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
    };
    a + d * t
}
pub fn closest_on_second_segment(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> DVec2 {
    let ab = b - a;
    let cd = d - c;
    let denominator = ab.perp_dot(cd);
    if denominator.abs() > 1e-12 {
        let t = (c - a).perp_dot(cd) / denominator;
        let u = (c - a).perp_dot(ab) / denominator;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            return c + cd * u;
        }
    }
    [c, d, closest_point(a, c, d), closest_point(b, c, d)]
        .into_iter()
        .min_by(|p, q| distance_to_segment(*p, a, b).total_cmp(&distance_to_segment(*q, a, b)))
        .unwrap()
}
pub fn segment_distance(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> f64 {
    let ab = b - a;
    let cd = d - c;
    let denominator = ab.perp_dot(cd);
    if denominator.abs() > 1e-12 {
        let t = (c - a).perp_dot(cd) / denominator;
        let u = (c - a).perp_dot(ab) / denominator;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            return 0.0;
        }
    }
    distance_to_segment(a, c, d)
        .min(distance_to_segment(b, c, d))
        .min(distance_to_segment(c, a, b))
        .min(distance_to_segment(d, a, b))
}
pub fn pick(model: &Model, camera: &Camera, size: DVec2, point: DVec2, radius: f64) -> Option<u64> {
    let view = camera.view(size);
    let ray = view.ray(point);
    model
        .edges
        .values()
        .filter(|e| !model.is_hidden(e.id) && model.entity_in_context(e.id).is_some())
        .filter_map(|e| {
            let (a, b) = model.points(e);
            let (screen_a, screen_b) = view.segment(a, b)?;
            let d = distance_to_segment(point, screen_a, screen_b);
            // Reject distant edges before casting a ray against every face.
            if d > radius || !visible_in_view(model, &view, ray.segment_point(a, b)) {
                return None;
            }
            Some((e.id, d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .and_then(|v| model.entity_in_context(v.0))
}

pub fn intersects_rect(a: DVec2, b: DVec2, lo: DVec2, hi: DVec2) -> bool {
    let delta = b - a;
    let mut enter: f64 = 0.0;
    let mut exit: f64 = 1.0;
    for i in 0..2 {
        if delta[i].abs() < 1e-12 {
            if a[i] < lo[i] || a[i] > hi[i] {
                return false;
            }
        } else {
            let t1 = (lo[i] - a[i]) / delta[i];
            let t2 = (hi[i] - a[i]) / delta[i];
            enter = enter.max(t1.min(t2));
            exit = exit.min(t1.max(t2));
            if enter > exit {
                return false;
            }
        }
    }
    true
}
pub fn box_select(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    start: DVec2,
    end: DVec2,
) -> BTreeSet<u64> {
    let lo = start.min(end);
    let hi = start.max(end);
    let crossing = start.x > end.x;
    let inside = |p: DVec2| p.cmpge(lo).all() && p.cmple(hi).all();
    let mut selected: BTreeSet<_> = model
        .edges
        .values()
        .filter(|e| !model.is_hidden(e.id) && model.entity_in_context(e.id).is_some())
        .filter_map(|e| {
            let (a, b) = model.points(e);
            let (a, b) = if crossing {
                camera.segment(a, b, size)?
            } else {
                (camera.project(a, size)?, camera.project(b, size)?)
            };
            (if crossing {
                intersects_rect(a, b, lo, hi)
            } else {
                inside(a) && inside(b)
            })
            .then_some(e.id)
        })
        .collect();
    for face in model
        .faces
        .values()
        .filter(|f| !model.is_hidden(f.id) && model.entity_in_context(f.id).is_some())
    {
        let hit = if crossing {
            model
                .face_triangles(face)
                .into_iter()
                .flat_map(|tri| camera.triangle_depth(tri, size))
                .any(|tri| {
                    let triangle = tri.map(|p| p.truncate());
                    triangle.iter().any(|&p| inside(p))
                        || [lo, hi, DVec2::new(lo.x, hi.y), DVec2::new(hi.x, lo.y)]
                            .iter()
                            .any(|p| crate::faces::in_polygon(*p, &triangle, true))
                        || (0..3)
                            .any(|i| intersects_rect(triangle[i], triangle[(i + 1) % 3], lo, hi))
                })
        } else {
            face.vertices.iter().all(|id| {
                camera
                    .project(model.vertices[id].position, size)
                    .is_some_and(inside)
            })
        };
        if hit {
            selected.insert(face.id);
        }
    }
    for dimension in model.dimensions.values().filter(|d| !model.is_hidden(d.id)) {
        let points = dimension.layout(model);
        let hit = if crossing {
            [(0, 2), (2, 3), (3, 1)].iter().any(|&(a, b)| {
                camera
                    .segment(points[a], points[b], size)
                    .is_some_and(|(a, b)| intersects_rect(a, b, lo, hi))
            })
        } else {
            points
                .iter()
                .all(|p| camera.project(*p, size).is_some_and(inside))
        };
        if hit {
            selected.insert(dimension.id);
        }
    }
    let hits = selected.clone();
    selected = hits
        .iter()
        .filter_map(|id| model.entity_in_context(*id))
        .collect();
    if !crossing {
        selected.retain(|id| {
            !model.objects.contains_key(id)
                || model
                    .expand_entities(&BTreeSet::from([*id]))
                    .iter()
                    .filter(|id| !model.objects.contains_key(id) && !model.is_hidden(**id))
                    .all(|id| hits.contains(id))
        });
    }
    selected
}
