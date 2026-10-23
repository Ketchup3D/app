//! Planar half-edge walking finds bounded regions adjacent to a newly drawn edge.
//! Faces are explicit document entities: deleting a surface does not regenerate it.
use crate::document::EPS;
use crate::{DVec2, DVec3, Face, Model, Ray};
use std::collections::{BTreeMap, BTreeSet};

const TOL: f64 = 1e-6;

#[derive(Clone, Copy)]
pub struct Plane {
    pub origin: DVec3,
    pub u: DVec3,
    pub v: DVec3,
    pub normal: DVec3,
}
impl Plane {
    pub fn project(self, p: DVec3) -> DVec2 {
        let d = p - self.origin;
        DVec2::new(d.dot(self.u), d.dot(self.v))
    }
}
pub fn plane(model: &Model, ring: &[u64]) -> Option<Plane> {
    if ring.len() < 3 || ring.iter().copied().collect::<BTreeSet<_>>().len() != ring.len() {
        return None;
    }
    let origin = model.vertices.get(&ring[0])?.position;
    let u = (model.vertices.get(&ring[1])?.position - origin).try_normalize()?;
    let normal = ring
        .iter()
        .skip(2)
        .filter_map(|id| model.vertices.get(id))
        .find_map(|v| {
            let cross = u.cross(v.position - origin);
            (cross.length() > TOL).then(|| cross.normalize())
        })?;
    if ring.iter().any(|id| {
        model
            .vertices
            .get(id)
            .is_none_or(|v| (v.position - origin).dot(normal).abs() > TOL)
    }) {
        return None;
    }
    Some(Plane {
        origin,
        u,
        v: normal.cross(u),
        normal,
    })
}
pub fn canonical_ring(ring: &[u64]) -> Vec<u64> {
    if ring.is_empty() {
        return vec![];
    }
    let start = (0..ring.len()).min_by_key(|&i| ring[i]).unwrap();
    let forward: Vec<_> = (0..ring.len())
        .map(|i| ring[(start + i) % ring.len()])
        .collect();
    let reverse: Vec<_> = (0..ring.len())
        .map(|i| ring[(start + ring.len() - i) % ring.len()])
        .collect();
    forward.min(reverse)
}
fn area(points: &[DVec2]) -> f64 {
    (0..points.len())
        .map(|i| points[i].perp_dot(points[(i + 1) % points.len()]))
        .sum::<f64>()
        * 0.5
}
pub fn in_polygon(point: DVec2, ring: &[DVec2], boundary: bool) -> bool {
    in_polygon_points(point, ring.iter().copied(), boundary)
}
fn in_polygon_points(point: DVec2, mut ring: impl Iterator<Item = DVec2>, boundary: bool) -> bool {
    let Some(first) = ring.next() else {
        return false;
    };
    let mut a = first;
    let mut inside = false;
    for b in ring.chain(std::iter::once(first)) {
        if crate::selection::distance_to_segment(point, a, b) < TOL {
            return boundary;
        }
        if (a.y > point.y) != (b.y > point.y)
            && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
        {
            inside = !inside;
        }
        a = b;
    }
    inside
}
fn face_contains_in_plane(
    model: &Model,
    face: &Face,
    p: Plane,
    point: DVec3,
    boundary: bool,
) -> bool {
    let xy = p.project(point);
    let contains = |ring: &[u64], boundary| {
        in_polygon_points(
            xy,
            ring.iter().map(|id| p.project(model.vertices[id].position)),
            boundary,
        )
    };
    (point - p.origin).dot(p.normal).abs() < TOL
        && contains(&face.vertices, boundary)
        && !face.holes.iter().any(|hole| contains(hole, true))
}

fn projected(model: &Model, ring: &[u64], plane: Plane) -> Vec<DVec2> {
    ring.iter()
        .map(|id| plane.project(model.vertices[id].position))
        .collect()
}
fn simple(points: &[DVec2]) -> bool {
    for i in 0..points.len() {
        for j in i + 1..points.len() {
            if j == i + 1 || (i == 0 && j == points.len() - 1) {
                continue;
            }
            if crate::selection::segment_distance(
                points[i],
                points[(i + 1) % points.len()],
                points[j],
                points[(j + 1) % points.len()],
            ) < TOL
            {
                return false;
            }
        }
    }
    true
}
fn strip_spurs(mut ring: Vec<u64>) -> Vec<u64> {
    while ring.len() >= 3 {
        let n = ring.len();
        let Some(i) = (0..n).find(|&i| ring[i] == ring[(i + 2) % n]) else {
            break;
        };
        let mut remove = [(i + 1) % n, (i + 2) % n];
        remove.sort();
        ring.remove(remove[1]);
        ring.remove(remove[0]);
    }
    ring
}
fn contains_ring(model: &Model, outer: &[u64], inner: &[u64]) -> bool {
    let Some(p) = plane(model, outer) else {
        return false;
    };
    let boundary = projected(model, outer, p);
    if inner.iter().any(|id| {
        let point = model.vertices[id].position;
        (point - p.origin).dot(p.normal).abs() > TOL
            || !in_polygon(p.project(point), &boundary, false)
    }) {
        return false;
    }
    let inner = projected(model, inner, p);
    for i in 0..inner.len() {
        for j in 0..boundary.len() {
            if crate::selection::segment_distance(
                inner[i],
                inner[(i + 1) % inner.len()],
                boundary[j],
                boundary[(j + 1) % boundary.len()],
            ) < TOL
            {
                return false;
            }
        }
    }
    true
}

impl Model {
    pub fn face_triangles(&self, face: &Face) -> Vec<[DVec3; 3]> {
        let Some(p) = plane(self, &face.vertices) else {
            return vec![];
        };
        let mut points = Vec::new();
        let mut coordinates = Vec::new();
        let mut holes = Vec::new();
        for (i, ring) in std::iter::once(&face.vertices)
            .chain(face.holes.iter())
            .enumerate()
        {
            if i > 0 {
                holes.push(points.len());
            }
            for id in ring {
                let Some(vertex) = self.vertices.get(id) else {
                    return vec![];
                };
                points.push(vertex.position);
                let xy = p.project(vertex.position);
                coordinates.extend([xy.x, xy.y]);
            }
        }
        earcutr::earcut(&coordinates, &holes, 2)
            .unwrap_or_default()
            .chunks_exact(3)
            .map(|i| [points[i[0]], points[i[1]], points[i[2]]])
            .collect()
    }
    pub fn face_area(&self, face: &Face) -> f64 {
        self.face_triangles(face)
            .iter()
            .map(|[a, b, c]| (*b - *a).cross(*c - *a).length() * 0.5)
            .sum()
    }
    pub fn face_contains(&self, face: &Face, point: DVec3, boundary: bool) -> bool {
        let Some(p) = plane(self, &face.vertices) else {
            return false;
        };
        face_contains_in_plane(self, face, p, point, boundary)
    }

    pub fn face_hit(&self, face: &Face, ray: Ray) -> Option<(f64, DVec3)> {
        let p = plane(self, &face.vertices)?;
        let denominator = ray.direction.dot(p.normal);
        if denominator.abs() < 1e-9 {
            return None;
        }
        let t = (p.origin - ray.origin).dot(p.normal) / denominator;
        let point = ray.origin + ray.direction * t;
        (t > 0.005 && face_contains_in_plane(self, face, p, point, true)).then_some((t, point))
    }
    pub fn nearest_face(&self, ray: Ray) -> Option<(u64, f64, DVec3)> {
        self.faces
            .values()
            .filter(|face| !self.is_hidden(face.id))
            .filter_map(|face| self.face_hit(face, ray).map(|(t, p)| (face.id, t, p)))
            .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
    }
    pub(crate) fn create_faces(&mut self, a: u64, b: u64) -> bool {
        self.create_faces_with_edges(a, b, &self.drawing_edges())
    }
    pub(crate) fn create_faces_with_edges(
        &mut self,
        a: u64,
        b: u64,
        drawing_edges: &BTreeSet<u64>,
    ) -> bool {
        let origin = self.vertices[&a].position;
        let direction = (self.vertices[&b].position - origin).normalize();
        let mut adjacency: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        for edge in drawing_edges.iter().map(|id| &self.edges[id]) {
            adjacency.entry(edge.a).or_default().push(edge.b);
            adjacency.entry(edge.b).or_default().push(edge.a);
        }
        let mut normals: Vec<DVec3> = Vec::new();
        let mut visited = BTreeSet::new();
        let mut stack = vec![a, b];
        while let Some(id) = stack.pop() {
            if !visited.insert(id) {
                continue;
            }
            for &neighbor in &adjacency[&id] {
                let cross = direction.cross(self.vertices[&neighbor].position - origin);
                if cross.length() < TOL {
                    stack.push(neighbor);
                    continue;
                }
                let mut normal = cross.normalize();
                let axis = normal.abs().max_element();
                let sign = if normal.x.abs() == axis {
                    normal.x
                } else if normal.y.abs() == axis {
                    normal.y
                } else {
                    normal.z
                };
                if sign < 0. {
                    normal = -normal;
                }
                if normals.iter().all(|n| n.dot(normal) < 1. - 1e-8) {
                    normals.push(normal);
                }
            }
        }
        let mut rings: BTreeMap<Vec<u64>, Vec<u64>> = BTreeMap::new();
        for normal in normals {
            let p = Plane {
                origin,
                u: direction,
                v: normal.cross(direction),
                normal,
            };
            let mut flat: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
            for (&id, neighbors) in &adjacency {
                if (self.vertices[&id].position - origin).dot(normal).abs() > TOL {
                    continue;
                }
                let mut neighbors: Vec<_> = neighbors
                    .iter()
                    .copied()
                    .filter(|n| (self.vertices[n].position - origin).dot(normal).abs() < TOL)
                    .collect();
                let center = p.project(self.vertices[&id].position);
                neighbors.sort_by(|left, right| {
                    let l = p.project(self.vertices[left].position) - center;
                    let r = p.project(self.vertices[right].position) - center;
                    l.y.atan2(l.x).total_cmp(&r.y.atan2(r.x))
                });
                flat.insert(id, neighbors);
            }
            for (start, end) in [(a, b), (b, a)] {
                let mut previous = start;
                let mut current = end;
                let mut ring = vec![start];
                let mut closed = false;
                for _ in 0..self.edges.len() * 2 + 1 {
                    ring.push(current);
                    let Some(neighbors) = flat.get(&current) else {
                        break;
                    };
                    let Some(index) = neighbors.iter().position(|id| *id == previous) else {
                        break;
                    };
                    let next = neighbors[(index + neighbors.len() - 1) % neighbors.len()];
                    previous = current;
                    current = next;
                    if previous == start && current == end {
                        ring.pop();
                        closed = true;
                        break;
                    }
                }
                let ring = strip_spurs(ring);
                if !closed
                    || ring.len() < 3
                    || ring.iter().copied().collect::<BTreeSet<_>>().len() != ring.len()
                    || !(0..ring.len()).any(|i| {
                        let x = ring[i];
                        let y = ring[(i + 1) % ring.len()];
                        (x == a && y == b) || (x == b && y == a)
                    })
                {
                    continue;
                }
                let xy = projected(self, &ring, p);
                if area(&xy) <= EPS || !simple(&xy) {
                    continue;
                }
                rings.insert(canonical_ring(&ring), ring);
            }
        }
        let loose_faces: BTreeSet<_> = self
            .faces
            .keys()
            .copied()
            .filter(|id| self.parent(*id).is_none() && !self.is_hidden(*id) && !self.is_locked(*id))
            .collect();
        let existing: BTreeSet<_> = self
            .faces
            .values()
            .map(|f| canonical_ring(&f.vertices))
            .collect();
        let fresh: Vec<_> = rings
            .into_iter()
            .filter(|(key, _)| !existing.contains(key))
            .map(|(_, ring)| ring)
            .collect();
        if fresh.is_empty() {
            return false;
        }
        let hole_candidates: Vec<_> = self
            .faces
            .values()
            .filter(|f| loose_faces.contains(&f.id))
            .flat_map(|f| std::iter::once(f.vertices.clone()).chain(f.holes.clone()))
            .chain(fresh.iter().cloned())
            .collect();
        // A new boundary-to-boundary divider replaces the containing surface with its smaller regions.
        let midpoint = (origin + self.vertices[&b].position) * 0.5;
        let replaced: BTreeSet<_> = self
            .faces
            .values()
            .filter(|face| {
                loose_faces.contains(&face.id)
                    && self.face_contains(face, midpoint, false)
                    && fresh.iter().any(|ring| {
                        (0..ring.len()).any(|i| {
                            let a = ring[i];
                            let b = ring[(i + 1) % ring.len()];
                            (0..face.vertices.len()).any(|j| {
                                let c = face.vertices[j];
                                let d = face.vertices[(j + 1) % face.vertices.len()];
                                (a == c && b == d) || (a == d && b == c)
                            })
                        })
                    })
            })
            .map(|f| f.id)
            .collect();
        self.faces.retain(|id, _| !replaced.contains(id));
        self.face_colors.retain(|id, _| self.faces.contains_key(id));
        for vertices in fresh {
            let id = self.id();
            self.faces.insert(
                id,
                Face {
                    id,
                    vertices,
                    holes: vec![],
                },
            );
        }
        // Keep nested outlines as holes even when their independently selectable fill is deleted.
        let holes: Vec<_> = self
            .faces
            .values()
            .filter(|face| {
                self.parent(face.id).is_none()
                    && !self.is_hidden(face.id)
                    && !self.is_locked(face.id)
            })
            .map(|face| {
                let candidates: BTreeMap<_, _> = hole_candidates
                    .iter()
                    .filter(|ring| contains_ring(self, &face.vertices, ring))
                    .map(|r| (canonical_ring(r), r.clone()))
                    .collect();
                let rings: Vec<_> = candidates.into_values().collect();
                let holes = rings
                    .iter()
                    .filter(|ring| {
                        !rings.iter().any(|other| {
                            canonical_ring(ring) != canonical_ring(other)
                                && contains_ring(self, other, ring)
                        })
                    })
                    .cloned()
                    .collect();
                (face.id, holes)
            })
            .collect();
        for (id, holes) in holes {
            self.faces.get_mut(&id).unwrap().holes = holes;
        }
        true
    }
}
