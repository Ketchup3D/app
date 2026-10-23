//! Partition receiving faces into lit/shaded geometry. Drawing each portion
//! once avoids coplanar shadow overlays fighting with the face depth buffer.
use crate::renderer::{EntityCache, FaceCache};
use ketchup_core::{DVec3, DisplaySettings, FaceStyle};
use std::collections::BTreeMap;

type Triangle = [DVec3; 3];
const CONTACT_EPS: f64 = 1e-7;

#[derive(Default)]
pub struct FaceShadow {
    pub lit: Vec<Triangle>,
    pub shaded: Vec<Triangle>,
}

#[derive(PartialEq)]
struct Key {
    revision: u64,
    angle: f64,
    elevation: f64,
    section: Option<f64>,
    receivers: Vec<u64>,
    casters: Vec<u64>,
}

#[derive(Default)]
pub struct ShadowCache {
    key: Option<Key>,
    enabled: bool,
    faces: BTreeMap<u64, FaceShadow>,
}

impl ShadowCache {
    pub fn get(&self, id: u64) -> Option<&FaceShadow> {
        self.enabled.then(|| self.faces.get(&id)).flatten()
    }

    pub fn update(&mut self, faces: &FaceCache, entities: &EntityCache, d: &DisplaySettings) {
        self.enabled = d.shadows && d.shadow_darkness > 0. && d.face_style != FaceStyle::Wireframe;
        if !self.enabled {
            return;
        }
        let key = Key {
            revision: faces.revision(),
            angle: d.sun_angle,
            elevation: d.sun_elevation,
            section: d.section_cuts.then_some(d.section_height),
            receivers: faces
                .iter()
                .filter(|(id, _)| entities.get(*id).displayed(d))
                .map(|(id, _)| id)
                .collect(),
            casters: faces
                .iter()
                .filter(|(id, _)| !entities.get(*id).hidden)
                .map(|(id, _)| id)
                .collect(),
        };
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let angle = d.sun_angle.to_radians();
        let elevation = d.sun_elevation.to_radians();
        let light = DVec3::new(
            angle.cos() * elevation.cos(),
            angle.sin() * elevation.cos(),
            elevation.sin(),
        );
        let u = DVec3::new(-angle.sin(), angle.cos(), 0.);
        let basis = [u, light.cross(u), light];
        let section = key.section;
        let volumes = key
            .casters
            .iter()
            .flat_map(|&id| {
                faces.get(id).triangles.iter().flat_map(move |&triangle| {
                    section_triangle(triangle, section)
                        .into_iter()
                        .filter_map(move |triangle| Volume::new(id, triangle, basis))
                })
            })
            .collect();
        let tree = VolumeTree::new(volumes);
        self.faces.clear();
        for &id in &key.receivers {
            let mut result = FaceShadow::default();
            for &triangle in &faces.get(id).triangles {
                for triangle in section_triangle(triangle, key.section) {
                    let mut lit = vec![triangle.to_vec()];
                    let mut candidates = Vec::new();
                    tree.query(Bounds::new(triangle, basis), &mut candidates);
                    for index in candidates {
                        let volume = &tree.volumes[index];
                        if volume.face == id {
                            continue;
                        }
                        let mut remaining = Vec::new();
                        for polygon in lit {
                            // Keep outside pieces only if there is a real
                            // intersection; disjoint casters must not fragment
                            // a face simply because their bounds overlap.
                            let (inside, outside) = volume.partition(&polygon);
                            if has_area(&inside) {
                                triangulate(&inside, &mut result.shaded);
                                remaining.extend(outside);
                            } else {
                                remaining.push(polygon);
                            }
                        }
                        lit = remaining;
                        if lit.is_empty() {
                            break;
                        }
                    }
                    for polygon in lit {
                        triangulate(&polygon, &mut result.lit);
                    }
                }
            }
            if !result.shaded.is_empty() {
                self.faces.insert(id, result);
            }
        }
        self.key = Some(key);
    }
}

#[derive(Clone, Copy)]
struct Plane {
    origin: DVec3,
    normal: DVec3,
    inset: f64,
}
impl Plane {
    fn distance(self, point: DVec3) -> f64 {
        (point - self.origin).dot(self.normal) - self.inset
    }
}

/// Return both halves using identical intersections, so their shared boundary
/// has no gaps. All polygons stay convex and on the original receiver plane.
fn split(polygon: &[DVec3], plane: Plane) -> (Vec<DVec3>, Vec<DVec3>) {
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        let da = plane.distance(a);
        let db = plane.distance(b);
        if da >= 0. {
            inside.push(a);
        } else {
            outside.push(a);
        }
        if (da >= 0.) != (db >= 0.) {
            let point = a.lerp(b, da / (da - db));
            inside.push(point);
            outside.push(point);
        }
    }
    (inside, outside)
}

fn has_area(polygon: &[DVec3]) -> bool {
    polygon.len() >= 3
        && (1..polygon.len() - 1).any(|i| {
            (polygon[i] - polygon[0])
                .cross(polygon[i + 1] - polygon[0])
                .length_squared()
                > 1e-24
        })
}

fn triangulate(polygon: &[DVec3], triangles: &mut Vec<Triangle>) {
    for i in 1..polygon.len().saturating_sub(1) {
        let triangle = [polygon[0], polygon[i], polygon[i + 1]];
        if has_area(&triangle) {
            triangles.push(triangle);
        }
    }
}

fn section_triangle(triangle: Triangle, section: Option<f64>) -> Vec<Triangle> {
    let Some(z) = section else {
        return vec![triangle];
    };
    let (polygon, _) = split(
        &triangle,
        Plane {
            origin: DVec3::Z * z,
            normal: -DVec3::Z,
            inset: 0.,
        },
    );
    let mut triangles = Vec::new();
    triangulate(&polygon, &mut triangles);
    triangles
}

struct Volume {
    face: u64,
    planes: [Plane; 4],
    bounds: Bounds,
}
impl Volume {
    fn new(face: u64, triangle: Triangle, basis: [DVec3; 3]) -> Option<Self> {
        let light = basis[2];
        let [a, b, c] = triangle;
        let mut normal = (b - a).cross(c - a).try_normalize()?;
        let facing = normal.dot(light);
        if facing.abs() < 1e-10 {
            return None;
        }
        if facing < 0. {
            normal = -normal;
        }
        let cap = Plane {
            origin: a,
            normal: -normal,
            // Coplanar faces and shared triangulation edges cannot shadow
            // themselves. Only geometry behind the caster receives a shadow.
            inset: CONTACT_EPS,
        };
        let mut planes = [cap; 4];
        for i in 0..3 {
            let a = triangle[i];
            let b = triangle[(i + 1) % 3];
            let c = triangle[(i + 2) % 3];
            let mut normal = (b - a).cross(light).try_normalize()?;
            if normal.dot(c - a) < 0. {
                normal = -normal;
            }
            planes[i + 1] = Plane {
                origin: a,
                normal,
                inset: 0.,
            };
        }
        Some(Self {
            face,
            planes,
            bounds: Bounds::new(triangle, basis),
        })
    }

    fn partition(&self, polygon: &[DVec3]) -> (Vec<DVec3>, Vec<Vec<DVec3>>) {
        let mut inside = polygon.to_vec();
        let mut outside = Vec::new();
        for &plane in &self.planes {
            let (keep, rest) = split(&inside, plane);
            if has_area(&rest) {
                outside.push(rest);
            }
            inside = keep;
            if !has_area(&inside) {
                break;
            }
        }
        (inside, outside)
    }
}

/// Shadow volumes extend infinitely away from the sun. In light coordinates
/// their XY footprint is bounded, and their maximum Z is the caster's depth.
#[derive(Clone, Copy)]
struct Bounds {
    lo: DVec3,
    hi: DVec3,
}
impl Bounds {
    fn new(triangle: Triangle, basis: [DVec3; 3]) -> Self {
        triangle.into_iter().fold(
            Self {
                lo: DVec3::splat(f64::INFINITY),
                hi: DVec3::splat(f64::NEG_INFINITY),
            },
            |bounds, point| {
                let p = DVec3::new(
                    point.dot(basis[0]),
                    point.dot(basis[1]),
                    point.dot(basis[2]),
                );
                Self {
                    lo: bounds.lo.min(p),
                    hi: bounds.hi.max(p),
                }
            },
        )
    }

    fn overlaps(self, receiver: Self) -> bool {
        self.lo.x <= receiver.hi.x
            && self.hi.x >= receiver.lo.x
            && self.lo.y <= receiver.hi.y
            && self.hi.y >= receiver.lo.y
            && self.hi.z > receiver.lo.z
    }
}

struct Node {
    bounds: Bounds,
    start: usize,
    end: usize,
    children: Option<(usize, usize)>,
}
struct VolumeTree {
    volumes: Vec<Volume>,
    nodes: Vec<Node>,
}
impl VolumeTree {
    fn new(volumes: Vec<Volume>) -> Self {
        let mut tree = Self {
            volumes,
            nodes: Vec::new(),
        };
        if !tree.volumes.is_empty() {
            tree.build(0, tree.volumes.len());
        }
        tree
    }

    fn build(&mut self, start: usize, end: usize) -> usize {
        let first = self.volumes[start].bounds;
        let bounds = self.volumes[start + 1..end]
            .iter()
            .fold(first, |a, v| Bounds {
                lo: a.lo.min(v.bounds.lo),
                hi: a.hi.max(v.bounds.hi),
            });
        let index = self.nodes.len();
        self.nodes.push(Node {
            bounds,
            start,
            end,
            children: None,
        });
        if end - start > 8 {
            let extent = bounds.hi - bounds.lo;
            let axis = usize::from(extent.y > extent.x);
            let middle = start + (end - start) / 2;
            self.volumes[start..end].select_nth_unstable_by(middle - start, |a, b| {
                (a.bounds.lo[axis] + a.bounds.hi[axis])
                    .total_cmp(&(b.bounds.lo[axis] + b.bounds.hi[axis]))
            });
            let left = self.build(start, middle);
            let right = self.build(middle, end);
            self.nodes[index].children = Some((left, right));
        }
        index
    }

    fn query(&self, receiver: Bounds, hits: &mut Vec<usize>) {
        if self.nodes.is_empty() {
            return;
        }
        let mut pending = vec![0];
        while let Some(index) = pending.pop() {
            let node = &self.nodes[index];
            if !node.bounds.overlaps(receiver) {
                continue;
            }
            if let Some((a, b)) = node.children {
                pending.extend([a, b]);
            } else {
                hits.extend(
                    (node.start..node.end).filter(|&i| self.volumes[i].bounds.overlaps(receiver)),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ketchup_core::Model;
    use std::collections::BTreeSet;

    fn area(triangles: &[Triangle]) -> f64 {
        triangles
            .iter()
            .map(|[a, b, c]| (*b - *a).cross(*c - *a).length() * 0.5)
            .sum()
    }

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }

    fn outline(model: &mut Model, points: &[DVec3]) -> u64 {
        let before: BTreeSet<_> = model.faces.keys().copied().collect();
        for i in 0..points.len() {
            model
                .add_edge(points[i], points[(i + 1) % points.len()])
                .unwrap();
        }
        *model
            .faces
            .keys()
            .find(|&&id| !before.contains(&id))
            .unwrap()
    }

    fn square(model: &mut Model, center: DVec3, half: f64) -> u64 {
        outline(
            model,
            &[
                center + DVec3::new(-half, -half, 0.),
                center + DVec3::new(half, -half, 0.),
                center + DVec3::new(half, half, 0.),
                center + DVec3::new(-half, half, 0.),
            ],
        )
    }

    fn update(cache: &mut ShadowCache, model: &Model, d: &DisplaySettings) {
        let mut faces = FaceCache::default();
        faces.update(model);
        let mut entities = EntityCache::default();
        entities.update(model);
        // A fresh FaceCache may have the same revision as a different model.
        // The app retains one FaceCache, so force a miss in this test helper.
        cache.key = None;
        cache.update(&faces, &entities, d);
    }

    fn settings() -> DisplaySettings {
        DisplaySettings {
            shadows: true,
            sun_angle: 0.,
            sun_elevation: 45.,
            ..Default::default()
        }
    }

    #[test]
    fn shadows_require_a_receiver_and_work_above_and_below_ground() {
        for z in [-2., 0., 4.] {
            let mut model = Model::default();
            square(&mut model, DVec3::Z * (z + 2.), 1.);
            let mut cache = ShadowCache::default();
            update(&mut cache, &model, &settings());
            assert!(
                cache.faces.is_empty(),
                "Empty ground cannot receive a shadow"
            );
            let floor = square(&mut model, DVec3::Z * z, 5.);
            update(&mut cache, &model, &settings());
            let shadow = cache.get(floor).unwrap();
            close(area(&shadow.shaded), 4.);
            close(area(&shadow.lit), 96.);
            assert!(
                shadow
                    .shaded
                    .iter()
                    .flatten()
                    .all(|p| (p.z - z).abs() < 1e-9)
            );
        }
    }

    #[test]
    fn vertical_and_tilted_receivers_are_fully_covered_in_both_windings() {
        let light = DVec3::new(1., 0., 1.).normalize();
        let basis = [DVec3::Y, light.cross(DVec3::Y), light];
        for receiver in [
            [DVec3::ZERO, DVec3::Y * 2., DVec3::Z * 2.],
            [DVec3::ZERO, DVec3::X * 2., DVec3::new(0., 2., 1.)],
        ] {
            for reverse in [false, true] {
                let mut caster = receiver.map(|p| p + light * 3.);
                if reverse {
                    caster.swap(0, 1);
                }
                let volume = Volume::new(1, caster, basis).unwrap();
                let (inside, outside) = volume.partition(&receiver);
                let mut shaded = Vec::new();
                triangulate(&inside, &mut shaded);
                close(area(&shaded), area(&[receiver]));
                assert!(outside.is_empty());
            }
        }
    }

    #[test]
    fn overlapping_casters_do_not_duplicate_shaded_area() {
        let mut model = Model::default();
        let floor = square(&mut model, DVec3::ZERO, 5.);
        square(&mut model, DVec3::Z * 2., 1.);
        // This upper face projects to exactly the same footprint on the floor.
        square(&mut model, DVec3::new(1., 0., 3.), 1.);
        let mut cache = ShadowCache::default();
        update(&mut cache, &model, &settings());
        let shadow = cache.get(floor).unwrap();
        close(area(&shadow.shaded), 4.);
        close(area(&shadow.lit) + area(&shadow.shaded), 100.);
    }

    #[test]
    fn caster_and_receiver_holes_remain_open() {
        for hole_in_caster in [false, true] {
            let mut model = Model::default();
            let floor = square(&mut model, DVec3::ZERO, 5.);
            let caster = square(&mut model, DVec3::Z * 2., 1.);
            let center = if hole_in_caster {
                DVec3::Z * 2.
            } else {
                DVec3::new(-2., 0., 0.)
            };
            let inner = square(&mut model, center, 0.5);
            model.faces.remove(&inner);
            let pierced = if hole_in_caster { caster } else { floor };
            assert!(!model.faces[&pierced].holes.is_empty());
            let mut cache = ShadowCache::default();
            update(&mut cache, &model, &settings());
            let shadow = cache.get(floor).unwrap();
            close(area(&shadow.shaded), 3.);
            close(
                area(&shadow.lit) + area(&shadow.shaded),
                if hole_in_caster { 100. } else { 99. },
            );
        }
    }

    #[test]
    fn hidden_tagged_and_sectioned_casters_do_not_cast_shadows() {
        let mut model = Model::default();
        let floor = square(&mut model, DVec3::ZERO, 5.);
        let caster = square(&mut model, DVec3::Z * 2., 1.);
        let mut cache = ShadowCache::default();
        let mut d = settings();
        model.hidden.insert(caster);
        d.hidden_geometry = true;
        update(&mut cache, &model, &d);
        assert!(cache.get(floor).is_none());
        model.hidden.clear();
        let tag = model.add_tag("Caster").unwrap();
        model
            .assign_tag(&BTreeSet::from([caster]), Some(tag))
            .unwrap();
        model.tags.get_mut(&tag).unwrap().visible = false;
        update(&mut cache, &model, &d);
        assert!(cache.get(floor).is_none());
        model.tags.get_mut(&tag).unwrap().visible = true;
        d.section_cuts = true;
        d.section_height = 1.;
        update(&mut cache, &model, &d);
        assert!(cache.get(floor).is_none());
        d.section_cuts = false;
        update(&mut cache, &model, &d);
        close(area(&cache.get(floor).unwrap().shaded), 4.);
        d.face_style = FaceStyle::Wireframe;
        update(&mut cache, &model, &d);
        assert!(cache.get(floor).is_none());
    }

    #[test]
    fn section_clipping_keeps_the_retained_part_of_a_crossing_caster() {
        let triangle = [DVec3::ZERO, DVec3::X * 2., DVec3::new(0., 2., 2.)];
        let clipped = section_triangle(triangle, Some(1.));
        assert_eq!(clipped.len(), 2);
        assert!(clipped.iter().flatten().all(|p| p.z <= 1.));
        close(area(&clipped), area(&[triangle]) * 0.75);
    }

    #[test]
    fn cache_reuses_camera_independent_geometry_and_invalidates_on_edits() {
        let mut model = Model::default();
        let floor = square(&mut model, DVec3::ZERO, 5.);
        let caster = square(&mut model, DVec3::Z * 2., 1.);
        let mut faces = FaceCache::default();
        let mut entities = EntityCache::default();
        faces.update(&model);
        entities.update(&model);
        let mut cache = ShadowCache::default();
        let mut d = settings();
        cache.update(&faces, &entities, &d);
        let mesh = cache.get(floor).unwrap().shaded.as_ptr();
        d.shadow_darkness = 0.75;
        cache.update(&faces, &entities, &d);
        assert_eq!(mesh, cache.get(floor).unwrap().shaded.as_ptr());
        d.sun_angle = 180.;
        cache.update(&faces, &entities, &d);
        assert!(
            cache
                .get(floor)
                .unwrap()
                .shaded
                .iter()
                .flatten()
                .all(|p| p.x >= 1. - 1e-6)
        );
        for id in &model.faces[&caster].vertices {
            model.vertices.get_mut(id).unwrap().position.x += 20.;
        }
        faces.update(&model);
        cache.update(&faces, &entities, &d);
        assert!(cache.get(floor).is_none());
    }

    #[test]
    fn spatial_tree_returns_all_potential_casters() {
        let basis = [DVec3::X, DVec3::Y, DVec3::Z];
        let volumes = (0..64)
            .map(|i| {
                let a = DVec3::new((i % 8) as f64, (i / 8) as f64, 2.);
                Volume::new(i, [a, a + DVec3::X, a + DVec3::Y], basis).unwrap()
            })
            .collect();
        let tree = VolumeTree::new(volumes);
        for i in 0..64 {
            let a = DVec3::new((i % 8) as f64 + 0.1, (i / 8) as f64 + 0.1, 0.);
            let bounds = Bounds::new([a, a + DVec3::X, a + DVec3::Y], basis);
            let mut hits = Vec::new();
            tree.query(bounds, &mut hits);
            hits.sort_unstable();
            let expected: Vec<_> = tree
                .volumes
                .iter()
                .enumerate()
                .filter(|(_, v)| v.bounds.overlaps(bounds))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(hits, expected);
        }
    }
}
