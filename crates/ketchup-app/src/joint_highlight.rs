//! Cached surface guides, clipped to the real overlap rather than a bounding box.
use super::*;
use std::collections::BTreeMap;
const EPS: f64 = 1e-8;
const MAX_LINES: usize = 256;
#[derive(Clone)]
struct Guide {
    a: DVec3,
    b: DVec3,
    face: [DVec3; 3],
    hatch: bool,
}
#[derive(Default)]
pub(super) struct Region {
    members: [u64; 2],
    lines: Vec<Guide>,
}
fn bounds(model: &Model, id: u64) -> Option<(DVec3, DVec3)> {
    let entities = model.expand_entities(&BTreeSet::from([id]));
    let mut points = entities
        .iter()
        .filter_map(|id| model.edges.get(id))
        .flat_map(|edge| [edge.a, edge.b])
        .filter_map(|id| model.vertices.get(&id).map(|v| v.position));
    let first = points.next()?;
    Some(points.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
}
fn clip(mut polygon: Vec<DVec3>, lo: DVec3, hi: DVec3) -> Vec<DVec3> {
    for axis in 0..3 {
        for (boundary, sign) in [(lo[axis], 1.), (hi[axis], -1.)] {
            let input = std::mem::take(&mut polygon);
            if input.is_empty() {
                return polygon;
            }
            let mut previous = *input.last().unwrap();
            for point in input {
                let before = (previous[axis] - boundary) * sign;
                let after = (point[axis] - boundary) * sign;
                if (before >= -EPS) != (after >= -EPS) {
                    let t = (boundary - previous[axis]) / (point[axis] - previous[axis]);
                    polygon.push(previous.lerp(point, t.clamp(0., 1.)));
                }
                if after >= -EPS {
                    polygon.push(point);
                }
                previous = point;
            }
        }
    }
    polygon
}
fn key(a: DVec3, b: DVec3) -> ([i64; 3], [i64; 3]) {
    let point = |p: DVec3| p.to_array().map(|x| (x / EPS).round() as i64);
    let (a, b) = (point(a), point(b));
    if a < b { (a, b) } else { (b, a) }
}
impl Region {
    pub fn build(source: &Model, evaluated: &Model, members: [u64; 2]) -> Self {
        let mut result = Self {
            members,
            ..Default::default()
        };
        let (Some(a), Some(b)) = (bounds(source, members[0]), bounds(source, members[1])) else {
            return result;
        };
        let (lo, hi) = (a.0.max(b.0), a.1.min(b.1));
        if (hi - lo).min_element() <= EPS {
            return result;
        }
        let center = (lo + hi) * 0.5;
        let spacing = (hi - lo).length() / 9.;
        let entities = evaluated.expand_entities(&BTreeSet::from(members));
        let mut hatches = Vec::new();
        // Cancel triangulation diagonals within each planar face; retain only
        // the clipped surface boundary and actual ridges in the joint region.
        let mut outlines = BTreeMap::new();
        for face in entities.iter().filter_map(|id| evaluated.faces.get(id)) {
            let mut boundaries = BTreeMap::new();
            for triangle in evaluated.face_triangles(face) {
                let polygon = clip(triangle.to_vec(), lo, hi);
                if polygon.len() < 3 {
                    continue;
                }
                let normal = (triangle[1] - triangle[0])
                    .cross(triangle[2] - triangle[0])
                    .normalize_or_zero();
                if normal.length_squared() < 0.5 {
                    continue;
                }
                let seed = if normal.x.abs() < 0.8 {
                    DVec3::X
                } else {
                    DVec3::Y
                };
                let u = (seed - normal * normal.dot(seed)).normalize();
                let axis = (u + normal.cross(u) * 0.65).normalize();
                for i in 0..polygon.len() {
                    let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
                    if a.distance(b) <= EPS {
                        continue;
                    }
                    let entry = boundaries.entry(key(a, b)).or_insert((
                        Guide {
                            a,
                            b,
                            face: triangle,
                            hatch: false,
                        },
                        0,
                    ));
                    entry.1 += 1;
                }
                if hatches.len() >= MAX_LINES / 2 {
                    continue;
                }
                for offset in -5..=5 {
                    let level = axis.dot(center) + offset as f64 * spacing;
                    let mut intersections = Vec::new();
                    for i in 0..polygon.len() {
                        let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
                        let (da, db) = (axis.dot(a) - level, axis.dot(b) - level);
                        if (da < 0.) != (db < 0.) {
                            intersections.push(a.lerp(b, da / (da - db)));
                        }
                    }
                    if intersections.len() == 2 && intersections[0].distance(intersections[1]) > EPS
                    {
                        hatches.push(Guide {
                            a: intersections[0],
                            b: intersections[1],
                            face: triangle,
                            hatch: true,
                        });
                        if hatches.len() >= MAX_LINES / 2 {
                            break;
                        }
                    }
                }
            }
            for (key, (guide, count)) in boundaries {
                if count == 1 {
                    outlines.entry(key).or_insert(guide);
                }
            }
        }
        result
            .lines
            .extend(outlines.into_values().take(MAX_LINES / 2));
        result.lines.extend(hatches);
        result
    }
    pub fn paint(
        &self,
        mesh: &mut renderer::Mesh,
        model: &Model,
        display: &ketchup_core::DisplaySettings,
        dark: bool,
        preview: bool,
    ) {
        if self.members.iter().any(|id| {
            !model.objects.contains_key(id)
                || (model.is_hidden(*id) && !display.hidden_objects)
                || (display.group_emphasis
                    && model.edit_context.is_some()
                    && model.entity_in_context(*id).is_none())
        }) {
            return;
        }
        let rgb = match (preview, dark) {
            (false, false) => [49, 128, 121],
            (false, true) => [112, 186, 171],
            (true, false) => [168, 116, 47],
            (true, true) => [216, 173, 102],
        };
        for line in &self.lines {
            let alpha = if line.hatch { 85 } else { 195 };
            let color = Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], alpha);
            mesh.surface_guide(
                line.a,
                line.b,
                line.face,
                if line.hatch { 0.7 } else { 1.1 },
                color,
                line.hatch || preview,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn object_bounds_resolve_vertices_through_owned_edges() {
        let mut model = Model::default();
        let edge = model.add_edge(DVec3::ZERO, DVec3::ONE).unwrap();
        let object = model
            .make_object(&BTreeSet::from([edge]), false, "Part")
            .unwrap();
        assert_eq!(bounds(&model, object), Some((DVec3::ZERO, DVec3::ONE)));
    }
    #[test]
    fn surface_clipping_stays_in_overlap_and_keeps_plane() {
        let polygon = vec![
            DVec3::new(-1., 0., 0.),
            DVec3::new(2., 0., 0.),
            DVec3::new(0., 2., 0.),
        ];
        let result = clip(polygon, DVec3::ZERO, DVec3::ONE);
        assert!(result.len() >= 3);
        assert!(
            result
                .iter()
                .all(|p| p.min_element() >= -EPS && p.max_element() <= 1. + EPS && p.z.abs() < EPS)
        );
    }
}
