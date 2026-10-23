//! Reuse render geometry and a spatial hierarchy for pointer queries. No query
//! reconstructs face planes or searches every group's membership for each edge.
use crate::renderer::{EntityCache, FaceCache};
use ketchup_core::{Camera, DVec2, DVec3, Model, Ray, selection};

#[derive(Clone, Copy)]
struct Bounds {
    lo: DVec3,
    hi: DVec3,
}
impl Bounds {
    fn entry(self, ray: Ray, limit: f64) -> Option<f64> {
        let mut enter = 0.0_f64;
        let mut exit = limit;
        for axis in 0..3 {
            let origin = ray.origin[axis];
            let direction = ray.direction[axis];
            if direction.abs() < 1e-15 {
                if origin < self.lo[axis] - 1e-6 || origin > self.hi[axis] + 1e-6 {
                    return None;
                }
            } else {
                let a = (self.lo[axis] - 1e-6 - origin) / direction;
                let b = (self.hi[axis] + 1e-6 - origin) / direction;
                enter = enter.max(a.min(b));
                exit = exit.min(a.max(b));
                if enter > exit {
                    return None;
                }
            }
        }
        Some(enter)
    }
}
struct Item {
    id: u64,
    bounds: Bounds,
}
struct Node {
    bounds: Bounds,
    start: usize,
    end: usize,
    children: Option<(usize, usize)>,
}
#[derive(Default)]
pub struct PickingCache {
    revision: Option<u64>,
    items: Vec<Item>,
    nodes: Vec<Node>,
    edges: EdgePickingCache,
}
pub struct SnapQuery<'a> {
    picking: &'a PickingCache,
    faces: &'a FaceCache,
    entities: &'a EntityCache,
}
impl ketchup_core::inference::FaceQuery for SnapQuery<'_> {
    fn nearest(&self, ray: Ray, scoped: bool) -> Option<(u64, f64, DVec3)> {
        self.picking
            .nearest_in_scope(self.faces, self.entities, ray, false, scoped, &mut 0)
    }

    fn is_hidden(&self, _model: &Model, id: u64) -> bool {
        self.entities.get(id).hidden
    }
}
impl PickingCache {
    pub fn snap_query<'a>(
        &'a self,
        faces: &'a FaceCache,
        entities: &'a EntityCache,
    ) -> SnapQuery<'a> {
        SnapQuery {
            picking: self,
            faces,
            entities,
        }
    }
    pub fn update(&mut self, faces: &FaceCache) {
        if self.revision == Some(faces.revision()) {
            return;
        }
        self.revision = Some(faces.revision());
        self.items = faces
            .iter()
            .filter(|(_, face)| !face.triangles.is_empty())
            .map(|(id, face)| Item {
                id,
                bounds: Bounds {
                    lo: face.bounds.0,
                    hi: face.bounds.1,
                },
            })
            .collect();
        self.nodes.clear();
        if !self.items.is_empty() {
            self.build(0, self.items.len());
        }
    }
    fn build(&mut self, start: usize, end: usize) -> usize {
        let bounds = self.items[start..end].iter().fold(
            Bounds {
                lo: DVec3::splat(f64::INFINITY),
                hi: DVec3::splat(f64::NEG_INFINITY),
            },
            |bounds, item| Bounds {
                lo: bounds.lo.min(item.bounds.lo),
                hi: bounds.hi.max(item.bounds.hi),
            },
        );
        let index = self.nodes.len();
        self.nodes.push(Node {
            bounds,
            start,
            end,
            children: None,
        });
        if end - start > 8 {
            let extent = bounds.hi - bounds.lo;
            let axis = if extent.x >= extent.y && extent.x >= extent.z {
                0
            } else if extent.y >= extent.z {
                1
            } else {
                2
            };
            let middle = start + (end - start) / 2;
            self.items[start..end].select_nth_unstable_by(middle - start, |a, b| {
                (a.bounds.lo[axis] + a.bounds.hi[axis])
                    .total_cmp(&(b.bounds.lo[axis] + b.bounds.hi[axis]))
            });
            let left = self.build(start, middle);
            let right = self.build(middle, end);
            self.nodes[index].children = Some((left, right));
        }
        index
    }
    pub fn nearest(
        &self,
        faces: &FaceCache,
        entities: &EntityCache,
        ray: Ray,
        editable: bool,
    ) -> Option<(u64, f64, DVec3)> {
        self.nearest_filtered(faces, entities, ray, editable, &mut 0)
    }
    /// Test a world marker against cached surfaces in the active editing level.
    pub fn visible(
        &self,
        faces: &FaceCache,
        entities: &EntityCache,
        camera: &Camera,
        size: DVec2,
        point: DVec3,
    ) -> bool {
        let view = camera.view(size);
        let Some(pixel) = view.project(point) else {
            return false;
        };
        self.nearest(faces, entities, view.ray(pixel), false)
            .is_none_or(|(_, t, _)| t + 1e-5 >= point.distance(view.eye()))
    }
    fn nearest_filtered(
        &self,
        faces: &FaceCache,
        entities: &EntityCache,
        ray: Ray,
        editable: bool,
        visited: &mut usize,
    ) -> Option<(u64, f64, DVec3)> {
        self.nearest_in_scope(faces, entities, ray, editable, true, visited)
    }
    fn nearest_in_scope(
        &self,
        faces: &FaceCache,
        entities: &EntityCache,
        ray: Ray,
        editable: bool,
        scoped: bool,
        visited: &mut usize,
    ) -> Option<(u64, f64, DVec3)> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut best: Option<(u64, f64, DVec3)> = None;
        let mut pending = vec![0];
        while let Some(index) = pending.pop() {
            let node = &self.nodes[index];
            let limit = best.map_or(f64::INFINITY, |(_, t, _)| t);
            if node.bounds.entry(ray, limit).is_none() {
                continue;
            }
            if let Some((left, right)) = node.children {
                let a = self.nodes[left].bounds.entry(ray, limit);
                let b = self.nodes[right].bounds.entry(ray, limit);
                match (a, b) {
                    (Some(a), Some(b)) => {
                        if a < b {
                            pending.extend([right, left]);
                        } else {
                            pending.extend([left, right]);
                        }
                    }
                    (Some(_), None) => pending.push(left),
                    (None, Some(_)) => pending.push(right),
                    _ => {}
                }
                continue;
            }
            for item in &self.items[node.start..node.end] {
                let state = entities.get(item.id);
                if state.hidden
                    || scoped && state.context.is_none()
                    || editable && state.context != Some(item.id)
                {
                    continue;
                }
                if item.bounds.entry(ray, limit).is_none() {
                    continue;
                }
                *visited += 1;
                if let Some((t, p)) = faces.get(item.id).hit(ray) {
                    if best.is_none_or(|(id, best, _)| t < best || t == best && item.id > id) {
                        best = Some((item.id, t, p));
                    }
                }
            }
        }
        best
    }
    pub fn pick(
        &mut self,
        model: &Model,
        faces: &FaceCache,
        entities: &EntityCache,
        camera: &Camera,
        size: DVec2,
        pixel: DVec2,
        radius: f64,
        include_faces: bool,
    ) -> Option<u64> {
        if include_faces {
            if let Some(id) = selection::pick_dimension(model, camera, size, pixel, radius) {
                return Some(id);
            }
        }
        let view = camera.view(size);
        let ray = view.ray(pixel);
        self.edges.update(model, camera, size);
        let candidates = self.edges.candidates(pixel, radius, entities);
        for (id, _) in candidates {
            let edge = &model.edges[&id];
            let (a, b) = model.points(edge);
            let point = ray.segment_point(a, b);
            let Some(pixel) = view.project(point) else {
                continue;
            };
            let obstruction = self.nearest(faces, entities, view.ray(pixel), false);
            if obstruction.is_none_or(|(_, t, _)| t + 1e-5 >= point.distance(view.eye())) {
                return entities.get(edge.id).context;
            }
        }
        if include_faces {
            self.nearest(faces, entities, ray, false)
                .and_then(|(id, _, _)| entities.get(id).context)
        } else {
            None
        }
    }
}

#[derive(Default)]
struct EdgePickingCache {
    view: Option<(Camera, DVec2)>,
    source_edges: Vec<ketchup_core::Edge>,
    source_vertices: Vec<(u64, DVec3)>,
    items: Vec<ScreenEdge>,
    nodes: Vec<ScreenEdgeNode>,
    pending: Vec<usize>,
    #[cfg(test)]
    projection_builds: usize,
    #[cfg(test)]
    tested_edges: usize,
}

struct ScreenEdge {
    id: u64,
    a: DVec2,
    b: DVec2,
}

struct ScreenEdgeNode {
    lo: DVec2,
    hi: DVec2,
    start: usize,
    end: usize,
    children: Option<(usize, usize)>,
}

impl EdgePickingCache {
    fn update(&mut self, model: &Model, camera: &Camera, size: DVec2) {
        // Public model collections permit direct edits, so validate the source
        // with linear iterators. An unchanged hover does no edge projection,
        // map lookup per endpoint, or spatial-index construction.
        let same_geometry = self.source_edges.iter().eq(model.edges.values())
            && self
                .source_vertices
                .iter()
                .copied()
                .eq(model.vertices.iter().map(|(&id, v)| (id, v.position)));
        if same_geometry
            && self
                .view
                .as_ref()
                .is_some_and(|(saved, dimensions)| saved == camera && *dimensions == size)
        {
            return;
        }
        if !same_geometry {
            self.source_edges.clear();
            self.source_edges.extend(model.edges.values().cloned());
            self.source_vertices.clear();
            self.source_vertices
                .extend(model.vertices.iter().map(|(&id, v)| (id, v.position)));
        }
        self.view = Some((camera.clone(), size));
        self.items.clear();
        let view = camera.view(size);
        self.items.extend(model.edges.values().filter_map(|edge| {
            let (a, b) = model.points(edge);
            let (a, b) = view.segment(a, b)?;
            Some(ScreenEdge { id: edge.id, a, b })
        }));
        self.nodes.clear();
        if !self.items.is_empty() {
            self.build(0, self.items.len());
        }
        #[cfg(test)]
        {
            self.projection_builds += 1;
        }
    }

    fn build(&mut self, start: usize, end: usize) -> usize {
        let (lo, hi) = self.items[start..end].iter().fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            |(lo, hi), edge| (lo.min(edge.a).min(edge.b), hi.max(edge.a).max(edge.b)),
        );
        let index = self.nodes.len();
        self.nodes.push(ScreenEdgeNode {
            lo,
            hi,
            start,
            end,
            children: None,
        });
        if end - start > 8 {
            let extent = hi - lo;
            let axis = usize::from(extent.y > extent.x);
            let middle = start + (end - start) / 2;
            self.items[start..end].select_nth_unstable_by(middle - start, |a, b| {
                (a.a[axis] + a.b[axis]).total_cmp(&(b.a[axis] + b.b[axis]))
            });
            let left = self.build(start, middle);
            let right = self.build(middle, end);
            self.nodes[index].children = Some((left, right));
        }
        index
    }

    fn candidates(&mut self, pixel: DVec2, radius: f64, entities: &EntityCache) -> Vec<(u64, f64)> {
        #[cfg(test)]
        {
            self.tested_edges = 0;
        }
        let mut candidates = Vec::new();
        if self.nodes.is_empty() {
            return candidates;
        }
        let lo = pixel - DVec2::splat(radius);
        let hi = pixel + DVec2::splat(radius);
        self.pending.clear();
        self.pending.push(0);
        while let Some(index) = self.pending.pop() {
            let node = &self.nodes[index];
            if node.hi.x < lo.x || node.lo.x > hi.x || node.hi.y < lo.y || node.lo.y > hi.y {
                continue;
            }
            if let Some((left, right)) = node.children {
                self.pending.extend([left, right]);
                continue;
            }
            for edge in &self.items[node.start..node.end] {
                let state = entities.get(edge.id);
                if state.hidden || state.context.is_none() {
                    continue;
                }
                #[cfg(test)]
                {
                    self.tested_edges += 1;
                }
                let distance = selection::distance_to_segment(pixel, edge.a, edge.b);
                if distance <= radius {
                    candidates.push((edge.id, distance));
                }
            }
        }
        candidates.sort_unstable_by(|(a, x), (b, y)| x.total_cmp(y).then_with(|| b.cmp(a)));
        candidates
    }
}

#[cfg(test)]
#[path = "picking_tests.rs"]
pub(crate) mod tests;
