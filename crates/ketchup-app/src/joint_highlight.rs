//! Dashed cut contours projected onto their owning board, without decorative hatching.
use super::*;
use std::collections::BTreeMap;
use std::sync::Arc;
const EPS: f64 = 1e-8;
#[derive(Clone)]
struct Guide {
    a: DVec3,
    b: DVec3,
    owner: u64,
}
#[derive(Default)]
pub(super) struct Region {
    members: [u64; 2],
    lines: Vec<Guide>,
    stock_surfaces: BTreeMap<u64, Arc<[[DVec3; 3]]>>,
    own_entities: BTreeMap<u64, Arc<BTreeSet<u64>>>,
}

/// Lives for one geometry evaluation, so boards shared by several joints only
/// need one stock triangulation and one removed-volume boolean calculation.
#[derive(Default)]
pub(super) struct BoardGeometryCache {
    boards: BTreeMap<u64, BoardGeometry>,
}
struct BoardGeometry {
    own_entities: Arc<BTreeSet<u64>>,
    stock_surfaces: Arc<[[DVec3; 3]]>,
    cut_edges: Result<Arc<[[DVec3; 2]]>, &'static str>,
}
impl BoardGeometryCache {
    fn get(&mut self, source: &Model, evaluated: &Model, member: u64) -> &BoardGeometry {
        self.boards.entry(member).or_insert_with(|| BoardGeometry {
            own_entities: Arc::new(evaluated.expand_entities(&BTreeSet::from([member]))),
            stock_surfaces: source
                .expand_entities(&BTreeSet::from([member]))
                .into_iter()
                .filter_map(|id| source.faces.get(&id))
                .flat_map(|face| source.face_triangles(face))
                .collect::<Vec<_>>()
                .into(),
            cut_edges: source.joinery_cut_edges(evaluated, member).map(Arc::from),
        })
    }
}
pub(super) fn bounds(model: &Model, id: u64) -> Option<(DVec3, DVec3)> {
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
fn clip_edge(a: DVec3, b: DVec3, lo: DVec3, hi: DVec3) -> Option<(DVec3, DVec3)> {
    let delta = b - a;
    let (mut start, mut end): (f64, f64) = (0., 1.);
    for axis in 0..3 {
        if delta[axis].abs() <= EPS {
            if a[axis] < lo[axis] - EPS || a[axis] > hi[axis] + EPS {
                return None;
            }
        } else {
            let t = [
                (lo[axis] - a[axis]) / delta[axis],
                (hi[axis] - a[axis]) / delta[axis],
            ];
            start = start.max(t[0].min(t[1]));
            end = end.min(t[0].max(t[1]));
            if end <= start {
                return None;
            }
        }
    }
    let (a, b) = (a.lerp(b, start), a.lerp(b, end));
    (a.distance(b) > EPS).then_some((a, b))
}
// Clipping limits the highlight area; its artificial borders are not joint edges.
fn on_face_boundary(a: DVec3, b: DVec3, boundary: &[DVec3]) -> bool {
    let length = a.distance(b);
    if length <= EPS {
        return false;
    }
    let direction = (b - a) / length;
    let mut intervals = Vec::new();
    for i in 0..boundary.len() {
        let start = boundary[i];
        let end = boundary[(i + 1) % boundary.len()];
        let t = [start, end].map(|p| direction.dot(p - a));
        if [start, end]
            .into_iter()
            .zip(t)
            .all(|(p, t)| p.distance(a + direction * t) <= EPS)
        {
            intervals.push((t[0].min(t[1]).max(0.), t[0].max(t[1]).min(length)));
        }
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut covered: f64 = 0.;
    // Triangulation may remove collinear boundary vertices. Accept a side
    // covered by adjoining real edges, without bridging actual gaps.
    for (start, end) in intervals {
        if end < start {
            continue;
        }
        if start > covered + EPS {
            return false;
        }
        covered = covered.max(end);
        if covered >= length - EPS {
            return true;
        }
    }
    false
}
// An untouched stock edge lies on two exterior planes. New cut boundaries
// on an original face lie on only one and must still connect the joint outline.
fn is_stock_edge(a: DVec3, b: DVec3, stock: (DVec3, DVec3)) -> bool {
    (0..3)
        .filter(|&axis| {
            [stock.0[axis], stock.1[axis]]
                .into_iter()
                .any(|plane| (a[axis] - plane).abs() <= EPS && (b[axis] - plane).abs() <= EPS)
        })
        .count()
        >= 2
}
fn key(a: DVec3, b: DVec3) -> ([i64; 3], [i64; 3]) {
    let point = |p: DVec3| p.to_array().map(|x| (x / EPS).round() as i64);
    let (a, b) = (point(a), point(b));
    if a < b { (a, b) } else { (b, a) }
}
impl Region {
    pub fn build(
        source: &Model,
        evaluated: &Model,
        members: [u64; 2],
        settings: &ketchup_core::joinery::JointSettings,
    ) -> Self {
        Self::build_cached(
            source,
            evaluated,
            members,
            settings,
            &mut BoardGeometryCache::default(),
        )
    }

    pub(super) fn build_cached(
        source: &Model,
        evaluated: &Model,
        members: [u64; 2],
        settings: &ketchup_core::joinery::JointSettings,
        boards: &mut BoardGeometryCache,
    ) -> Self {
        let mut result = Self {
            members,
            ..Default::default()
        };
        let (Some(first_stock), Some(second_stock)) =
            (bounds(source, members[0]), bounds(source, members[1]))
        else {
            return result;
        };
        let (lo, hi) = (
            first_stock.0.max(second_stock.0),
            first_stock.1.min(second_stock.1),
        );
        if (hi - lo).min_element() <= EPS {
            return result;
        }
        // Receiver pockets extend laterally beyond the nominal overlap by
        // their clearance. Clipping them to the stock overlap opens their
        // contours and drops the shoulders that connect adjacent cut faces.
        let mut margin = DVec3::splat(EPS * 16.);
        for axis in 0..3 {
            if axis != settings.depth_axis {
                margin[axis] += settings.clearance;
            }
        }
        let (lo, hi) = (lo - margin, hi + margin);
        // Cut contours can cross the very material removed by the joint.
        // Project them onto the original board shell, so a notch cannot clip
        // its own guide before it reaches the next connected cut edge.
        for member in members {
            result.own_entities.insert(member,
                Arc::clone(&boards.get(source, evaluated, member).own_entities));
            result.stock_surfaces.insert(
                member,
                Arc::clone(&boards.get(source, evaluated, member).stock_surfaces),
            );
        }
        // The closed cut-volume contour is the normal path. Build the more
        // expensive retained-face fallback only for members whose cut volume
        // could not be recovered, rather than constructing and discarding it.
        let mut outlines = BTreeMap::new();
        let mut fallback_members = BTreeSet::new();
        for owner in members {
            let stock = if owner == members[0] {
                first_stock
            } else {
                second_stock
            };
            match &boards.get(source, evaluated, owner).cut_edges {
                Ok(edges) => {
                    for &[a, b] in edges.iter() {
                        if let Some((a, b)) = clip_edge(a, b, lo, hi) {
                            // A cut-volume closure can reuse the intersection
                            // of two original stock faces. Those are existing
                            // outer rims, not edges introduced by the cut.
                            if is_stock_edge(a, b, stock) {
                                continue;
                            }
                            outlines
                                .entry((owner, key(a, b)))
                                .or_insert(Guide { a, b, owner });
                        }
                    }
                }
                Err(_) => {
                    fallback_members.insert(owner);
                }
            }
        }
        let entities = evaluated.expand_entities(&fallback_members);
        let mut stock_by_face = BTreeMap::new();
        for member in fallback_members {
            if let Some(stock) = bounds(source, member) {
                for entity in evaluated.expand_entities(&BTreeSet::from([member])) {
                    if evaluated.faces.contains_key(&entity) {
                        stock_by_face.insert(entity, (member, stock));
                    }
                }
            }
        }
        // Retain evaluated face edges, excluding artificial overlap borders
        // and triangulation diagonals.
        for face in entities.iter().filter_map(|id| evaluated.faces.get(id)) {
            let Some((owner, stock)) = stock_by_face.get(&face.id).copied() else {
                continue;
            };
            let rings: Vec<Vec<_>> = std::iter::once(&face.vertices)
                .chain(face.holes.iter())
                .map(|ring| {
                    ring.iter()
                        .map(|id| evaluated.vertices[id].position)
                        .collect()
                })
                .collect();
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
                for i in 0..polygon.len() {
                    let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
                    if a.distance(b) <= EPS
                        || !rings.iter().any(|ring| on_face_boundary(a, b, ring))
                        || is_stock_edge(a, b, stock)
                    {
                        continue;
                    }
                    let entry = boundaries
                        .entry(key(a, b))
                        .or_insert((Guide { a, b, owner }, 0));
                    entry.1 += 1;
                }
            }
            for (key, (guide, count)) in boundaries {
                if count == 1 {
                    outlines.entry((guide.owner, key)).or_insert(guide);
                }
            }
        }
        // Keep the complete joint. A fixed global cap drops later cut faces
        // and can leave either member with only part of its contour.
        result.lines.extend(outlines.into_values());
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
            (false, false) => [0, 100, 92],
            (false, true) => [70, 230, 207],
            (true, false) => [145, 87, 0],
            (true, true) => [255, 202, 80],
        };
        // Use the original outer shell as the continuous guide support.
        // Other boards still occlude guides at that exterior surface's depth.
        let surfaces: BTreeMap<_, _> = self
            .members
            .into_iter()
            .map(|member| {
                (
                    member,
                    mesh.cached_joint_surface_for_entities(member, &self.stock_surfaces[&member], &self.own_entities[&member]),
                )
            })
            .collect();
        for line in &self.lines {
            let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            mesh.joint_guide(
                line.a,
                line.b,
                surfaces.get(&line.owner).unwrap(),
                1.8,
                color,
                true,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn add_board(source: &mut Model, lo: DVec3, hi: DVec3) -> u64 {
        let mut body = Model::default();
        let corners = [
            lo,
            DVec3::new(hi.x, lo.y, lo.z),
            DVec3::new(hi.x, hi.y, lo.z),
            DVec3::new(lo.x, hi.y, lo.z),
        ];
        for i in 0..4 {
            body.add_edge(corners[i], corners[(i + 1) % 4]).unwrap();
        }
        let face = *body.faces.keys().next().unwrap();
        let normal = ketchup_core::faces::plane(&body, &body.faces[&face].vertices)
            .unwrap()
            .normal;
        body.push_pull(face, (hi.z - lo.z) / normal.z).unwrap();
        let ids = source.paste_entities(&body, DVec3::ZERO);
        source.make_object(&ids, false, "Board").unwrap()
    }

    #[test]
    fn three_finger_cuts_keep_lower_cut_edges_without_original_stock_rims() {
        let mut source = Model::default();
        let members = [
            add_board(&mut source, DVec3::ZERO, DVec3::new(1., 1., 0.1)),
            add_board(
                &mut source,
                DVec3::new(0.8, 0., 0.),
                DVec3::new(1.2, 1., 0.1),
            ),
        ];
        let settings = ketchup_core::joinery::JointSettings {
            kind: ketchup_core::joinery::JointKind::Finger,
            fingers: 3,
            clearance: 0.,
            ..Default::default()
        };
        source
            .add_connection(ketchup_core::connections::Connection {
                members,
                entry_face: None,
                operation: ketchup_core::connections::ConnectionOperation::Joinery(
                    settings.clone(),
                ),
            })
            .unwrap();
        let evaluated = source.evaluate_joinery();
        assert!(evaluated.errors.is_empty());
        let mut boards = BoardGeometryCache::default();
        let region =
            Region::build_cached(&source, &evaluated.model, members, &settings, &mut boards);
        let repeated =
            Region::build_cached(&source, &evaluated.model, members, &settings, &mut boards);
        assert_eq!(
            boards.boards.len(),
            2,
            "shared boards must not repeat their removed-volume calculation"
        );
        for member in members {
            assert!(Arc::ptr_eq(
                &region.stock_surfaces[&member],
                &repeated.stock_surfaces[&member]
            ));
        }
        let edges = |region: &Region| {
            region
                .lines
                .iter()
                .map(|line| (line.a, line.b, line.owner))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            edges(&region),
            edges(&repeated),
            "sharing geometry must keep every cut edge unchanged"
        );
        let covers = |a: DVec3, b: DVec3| {
            [a, a.lerp(b, 0.5), b].into_iter().all(|point| {
                region
                    .lines
                    .iter()
                    .filter(|line| line.owner == members[1])
                    .any(|line| {
                        let delta = line.b - line.a;
                        let t = (point - line.a).dot(delta) / delta.length_squared();
                        t >= -EPS
                            && t <= 1. + EPS
                            && point.distance(line.a + delta * t.clamp(0., 1.)) < EPS
                    })
            })
        };
        let stock = bounds(&source, members[1]).unwrap();
        assert!(
            region
                .lines
                .iter()
                .filter(|line| line.owner == members[1])
                .all(|line| !is_stock_edge(line.a, line.b, stock)),
            "original stock rims must not acquire dotted X-Ray guides"
        );
        for i in 0..3 {
            let (lo, hi) = (i as f64 / 3., (i * 2 + 1) as f64 / 6.);
            for y in [lo, hi].into_iter().filter(|&y| y > EPS) {
                assert!(
                    covers(DVec3::new(0.8, y, 0.), DVec3::new(0.8, y, 0.1)),
                    "the side cut must keep its vertical connector"
                );
                assert!(
                    covers(DVec3::new(0.8, y, 0.), DVec3::new(1., y, 0.)),
                    "the lower edge of each cut face must remain in the X-Ray contour"
                );
            }
            for z in [0., 0.1] {
                assert!(
                    !covers(DVec3::new(0.8, lo, z), DVec3::new(0.8, hi, z)),
                    "a closure lying on two original stock faces is not a new cut edge"
                );
            }
        }
    }

    #[test]
    fn contours_span_collinear_boundary_vertices_without_bridging_gaps() {
        let boundary = [
            DVec3::ZERO,
            DVec3::X,
            DVec3::X * 2.,
            DVec3::new(2., 1., 0.),
            DVec3::Y,
        ];
        assert!(on_face_boundary(DVec3::X * 0.2, DVec3::X * 1.8, &boundary));
        // An indentation interrupts the bottom rim between x=0.8 and x=1.2.
        let indented = [
            DVec3::ZERO,
            DVec3::X * 0.8,
            DVec3::new(0.8, 0.2, 0.),
            DVec3::new(1.2, 0.2, 0.),
            DVec3::X * 1.2,
            DVec3::X * 2.,
            DVec3::new(2., 1., 0.),
            DVec3::Y,
        ];
        assert!(!on_face_boundary(DVec3::X * 0.2, DVec3::X * 1.8, &indented));
    }

    #[test]
    fn repeated_cuts_keep_every_contour_on_both_members() {
        let mut source = Model::default();
        let members = std::array::from_fn(|_| {
            let mut stock = Model::default();
            stock.add_edge(DVec3::ZERO, DVec3::ONE).unwrap();
            let ids = source.paste_entities(&stock, DVec3::ZERO);
            source.make_object(&ids, false, "Board").unwrap()
        });
        let mut evaluated = source.clone();
        for owner in members {
            let mut cuts = Model::default();
            for i in 0..20 {
                let x = 0.1 + i as f64 * 0.04;
                let corners = [
                    DVec3::new(x, 0.1, 0.1),
                    DVec3::new(x, 0.9, 0.1),
                    DVec3::new(x, 0.9, 0.9),
                    DVec3::new(x, 0.1, 0.9),
                ];
                for j in 0..4 {
                    cuts.add_edge(corners[j], corners[(j + 1) % 4]).unwrap();
                }
            }
            let ids = evaluated.paste_entities(&cuts, DVec3::ZERO);
            evaluated
                .objects
                .get_mut(&owner)
                .unwrap()
                .entities
                .extend(ids);
        }
        let region = Region::build(&source, &evaluated, members, &Default::default());
        assert_eq!(
            region.lines.len(),
            160,
            "large joints must keep every cut contour and generate no hatching"
        );
        for owner in members {
            for i in 0..20 {
                let x = 0.1 + i as f64 * 0.04;
                let lines: Vec<_> = region
                    .lines
                    .iter()
                    .filter(|line| {
                        line.owner == owner
                            && (line.a.x - x).abs() < EPS
                            && (line.b.x - x).abs() < EPS
                    })
                    .collect();
                assert_eq!(
                    lines.len(),
                    4,
                    "each repeated cut must retain its complete perimeter"
                );
            }
        }
    }

    #[test]
    fn receiver_clearance_keeps_real_cut_contours_closed_outside_the_nominal_overlap() {
        let mut source = Model::default();
        let members = [
            add_board(&mut source, DVec3::ZERO, DVec3::new(1., 1., 0.1)),
            add_board(
                &mut source,
                DVec3::new(0.8, -0.2, -0.1),
                DVec3::new(1.2, 1.2, 0.1),
            ),
        ];
        let settings = ketchup_core::joinery::JointSettings {
            kind: ketchup_core::joinery::JointKind::HalfLap,
            clearance: 0.001,
            ..Default::default()
        };
        source
            .add_connection(ketchup_core::connections::Connection {
                members,
                entry_face: None,
                operation: ketchup_core::connections::ConnectionOperation::Joinery(
                    settings.clone(),
                ),
            })
            .unwrap();
        let evaluated = source.evaluate_joinery();
        assert!(evaluated.errors.is_empty());
        let region = Region::build(&source, &evaluated.model, members, &settings);
        for member in members {
            let original: Vec<_> = source
                .expand_entities(&BTreeSet::from([member]))
                .into_iter()
                .filter_map(|id| source.faces.get(&id))
                .flat_map(|face| source.face_triangles(face))
                .collect();
            assert_eq!(
                region.stock_surfaces[&member].as_ref(),
                original.as_slice(),
                "cut openings must not remove the exterior support for their own contour"
            );
        }
        let lines: Vec<_> = region
            .lines
            .iter()
            .filter(|line| line.owner == members[1])
            .collect();
        assert!(
            lines
                .iter()
                .any(|line| (line.a.z + settings.clearance).abs() < EPS
                    && (line.b.z + settings.clearance).abs() < EPS),
            "the receiver's lower shoulder lies outside the nominal stock overlap"
        );
        for (i, line) in lines.iter().enumerate() {
            for point in [line.a, line.b] {
                assert!(
                    lines.iter().enumerate().any(|(j, other)| {
                        if i == j {
                            return false;
                        }
                        let delta = other.b - other.a;
                        let t = (point - other.a).dot(delta) / delta.length_squared();
                        t >= -EPS
                            && t <= 1. + EPS
                            && point.distance(other.a + delta * t.clamp(0., 1.)) < EPS
                    }),
                    "a real receiver cut contour must not have a dangling endpoint"
                );
            }
        }
    }

    #[test]
    fn new_boundaries_on_stock_faces_connect_cuts_without_highlighting_stock_rims() {
        let stock = (DVec3::ZERO, DVec3::ONE);
        // A shoulder line across the original top face connects the cut sides.
        assert!(!is_stock_edge(
            DVec3::new(0.4, 0.2, 1.),
            DVec3::new(0.4, 0.8, 1.),
            stock
        ));
        // Original outer edges stay unhighlighted, including clipped portions.
        assert!(is_stock_edge(
            DVec3::new(0., 0.2, 1.),
            DVec3::new(0., 0.8, 1.),
            stock
        ));
    }

    #[test]
    fn overlap_clipping_does_not_add_false_joint_edges() {
        let boundary = [
            DVec3::ZERO,
            DVec3::X * 2.,
            DVec3::new(2., 2., 0.),
            DVec3::Y * 2.,
        ];
        let clipped = clip(boundary.to_vec(), DVec3::ZERO, DVec3::ONE);
        let actual: Vec<_> = (0..clipped.len())
            .filter_map(|i| {
                let (a, b) = (clipped[i], clipped[(i + 1) % clipped.len()]);
                on_face_boundary(a, b, &boundary).then_some((a, b))
            })
            .collect();
        assert_eq!(actual.len(), 2);
        assert!(
            actual
                .iter()
                .all(|(a, b)| (a.x == 0. && b.x == 0.) || (a.y == 0. && b.y == 0.))
        );
        // A real sloped dovetail edge remains eligible for highlighting.
        let dovetail = [DVec3::ZERO, DVec3::new(2., 1., 0.), DVec3::Y * 2.];
        assert!(on_face_boundary(
            DVec3::new(0.5, 0.25, 0.),
            DVec3::new(1., 0.5, 0.),
            &dovetail
        ));
    }
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
