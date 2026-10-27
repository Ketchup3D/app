use eframe::{egui, egui_wgpu, wgpu};
use egui::{Color32, Rect};
use ketchup_core::{Camera, CameraView, DVec2, DVec3, Face, Model};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    color: [u8; 4],
    lateral: f32,
}

#[derive(Default)]
pub struct SceneCache {
    occlusion: Occlusion,
    edge_surfaces: BTreeMap<(u64, u64), Vec<DVec2>>,
    buffers: BufferPool,
}

type BufferPool = Arc<Mutex<Vec<MeshBuffers>>>;

struct MeshBuffers {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    triangle_ranges: Vec<std::ops::Range<u32>>,
}

pub struct Mesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    triangle_ranges: Vec<std::ops::Range<u32>>,
    edge_surfaces: BTreeMap<(u64, u64), Vec<DVec2>>,
    occlusion: Occlusion,
    size: DVec2,
    pixels_per_point: f64,
    parallel: bool,
    view: CameraView,
    display: ketchup_core::DisplaySettings,
    buffer_pool: Option<BufferPool>,
}
impl Mesh {
    pub fn new(camera: &Camera, size: DVec2) -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            triangle_ranges: Vec::new(),
            edge_surfaces: BTreeMap::new(),
            occlusion: Occlusion::default(),
            size,
            pixels_per_point: 1.,
            parallel: camera.projection == ketchup_core::Projection::Parallel,
            view: camera.view(size),
            display: ketchup_core::DisplaySettings::default(),
            buffer_pool: None,
        }
    }
    pub fn cached(camera: &Camera, size: DVec2, cache: &mut SceneCache) -> Self {
        let mut mesh = Self::new(camera, size);
        mesh.buffer_pool = Some(Arc::clone(&cache.buffers));
        if let Ok(mut buffers) = cache.buffers.lock() {
            if let Some(mut buffers) = buffers.pop() {
                buffers.vertices.clear();
                buffers.indices.clear();
                buffers.triangle_ranges.clear();
                mesh.vertices = buffers.vertices;
                mesh.indices = buffers.indices;
                mesh.triangle_ranges = buffers.triangle_ranges;
            }
        }
        mesh.occlusion = std::mem::take(&mut cache.occlusion);
        mesh.occlusion.begin_frame();
        mesh.edge_surfaces = std::mem::take(&mut cache.edge_surfaces);
        for surfaces in mesh.edge_surfaces.values_mut() {
            surfaces.clear();
        }
        mesh
    }
    pub fn set_display(&mut self, display: &ketchup_core::DisplaySettings) {
        self.display = display.clone();
    }
    pub fn set_pixels_per_point(&mut self, pixels_per_point: f32) {
        self.pixels_per_point = f64::from(pixels_per_point).max(0.01);
    }
    fn fog_color(&self, color: Color32, point: DVec3) -> Color32 {
        if !self.display.fog {
            return color;
        }
        let amount = ((self.view.view_distance(point) - self.display.fog_start)
            / (self.display.fog_end - self.display.fog_start))
            .clamp(0., 1.) as f32;
        let bg = self.display.background;
        let [r, g, b, alpha] = color.to_srgba_unmultiplied();
        Color32::from_rgba_unmultiplied(
            (r as f32 * (1. - amount) + bg[0] as f32 * amount) as u8,
            (g as f32 * (1. - amount) + bg[1] as f32 * amount) as u8,
            (b as f32 * (1. - amount) + bg[2] as f32 * amount) as u8,
            alpha,
        )
    }
    fn section_line(&self, mut a: DVec3, mut b: DVec3) -> Option<(DVec3, DVec3)> {
        if self.display.section_cuts {
            let z = self.display.section_height;
            if a.z > z && b.z > z {
                return None;
            }
            if (a.z > z) != (b.z > z) {
                let cut = a.lerp(b, (z - a.z) / (b.z - a.z));
                if a.z > z {
                    a = cut;
                } else {
                    b = cut;
                }
            }
        }
        Some((a, b))
    }
    pub fn overlay_line(&mut self, a: DVec3, b: DVec3, width: f64, color: Color32, dashed: bool) {
        let Some((a, b)) = self.section_line(a, b) else {
            return;
        };
        let color = self.fog_color(color, (a + b) * 0.5);
        if let Some((a, b)) = self.view.segment(a, b) {
            if dashed {
                let length = a.distance(b).max(1.);
                for i in 0..(length / 8.).ceil() as usize {
                    self.line(
                        a.lerp(b, (i as f64 * 8. / length).min(1.)).extend(0.),
                        a.lerp(b, ((i as f64 * 8. + 4.) / length).min(1.))
                            .extend(0.),
                        width,
                        color,
                    );
                }
            } else {
                self.line(a.extend(0.), b.extend(0.), width, color);
            }
        }
    }
    /// Attach depth slopes only to the boundaries of faces that are drawn.
    /// NDC depth is affine in screen coordinates, even in perspective.
    pub fn face_edges(&mut self, face: &Face, cached: &CachedFace) {
        let gradient = cached.triangles.iter().find_map(|&triangle| {
            let mut gradient = None;
            self.view.for_each_triangle_depth(triangle, |projected| {
                gradient = gradient.or_else(|| depth_gradient(projected));
            });
            gradient
        });
        let Some(gradient) = gradient else { return };
        for ring in std::iter::once(&face.vertices).chain(face.holes.iter()) {
            for i in 0..ring.len() {
                let a = ring[i];
                let b = ring[(i + 1) % ring.len()];
                self.edge_surfaces
                    .entry((a.min(b), a.max(b)))
                    .or_default()
                    .push(gradient);
            }
        }
    }
    pub fn styled_edge(
        &mut self,
        edge: &ketchup_core::Edge,
        model: &Model,
        width: f64,
        color: Color32,
        extensions: bool,
        endpoints: bool,
    ) {
        let (a, b) = model.points(edge);
        let key = (edge.a.min(edge.b), edge.a.max(edge.b));
        let surfaces = self
            .edge_surfaces
            .get_mut(&key)
            .map(std::mem::take)
            .unwrap_or_default();
        self.surface_line(a, b, width, color, extensions, endpoints, &surfaces);
        if let Some(entry) = self.edge_surfaces.get_mut(&key) {
            *entry = surfaces;
        }
    }
    pub fn styled_line(
        &mut self,
        a: DVec3,
        b: DVec3,
        width: f64,
        color: Color32,
        extensions: bool,
        endpoints: bool,
    ) {
        self.surface_line(a, b, width, color, extensions, endpoints, &[]);
    }
    fn surface_line(
        &mut self,
        a: DVec3,
        b: DVec3,
        width: f64,
        color: Color32,
        extensions: bool,
        endpoints: bool,
        surfaces: &[DVec2],
    ) {
        let Some((a, b)) = self.section_line(a, b) else {
            return;
        };
        let color = self.fog_color(color, (a + b) * 0.5);
        if let Some((mut a, mut b)) = self.view.segment_depth(a, b) {
            if extensions {
                let direction = (b.truncate() - a.truncate()).normalize_or_zero() * 5.;
                a -= direction.extend(0.);
                b += direction.extend(0.);
            }
            let visible = self.occlusion.visible_intervals(a, b);
            for &(start, end) in &visible {
                self.surface_stroke(a.lerp(b, start), a.lerp(b, end), width, color, surfaces);
            }
            if endpoints {
                for (t, p) in [(0., a), (1., b)] {
                    if !visible.iter().any(|&(start, end)| start <= t && t <= end) {
                        continue;
                    }
                    self.surface_stroke(
                        p - DVec3::X * 1.5,
                        p + DVec3::X * 1.5,
                        4.,
                        color,
                        surfaces,
                    );
                }
            }
            self.occlusion.spans = visible;
        }
    }
    fn line(&mut self, a: DVec3, b: DVec3, width: f64, color: Color32) {
        self.surface_stroke(a, b, width, color, &[]);
    }
    fn surface_stroke(
        &mut self,
        a: DVec3,
        b: DVec3,
        width: f64,
        color: Color32,
        surfaces: &[DVec2],
    ) {
        let depths = [a.z, b.z];
        let a = a.truncate();
        let b = b.truncate();
        let delta = b - a;
        if delta.length_squared() < 0.01 {
            return;
        }
        let normal = DVec2::new(-delta.y, delta.x).normalize();
        // The projected pixel footprint grows on diagonals. Match the shader's
        // fwidth fringe so it softens the edge without eating into its width.
        let fringe = (normal.x.abs() + normal.y.abs()) / self.pixels_per_point;
        let n = normal * (width + fringe) * 0.5;
        // Shared creases need a center seam: the nearer incident plane can
        // change on either side, but the edge itself keeps its true depth.
        let (strip, indices): (&[(usize, f64)], &[u32]) = if surfaces.len() > 1 {
            (
                &[(0, 1.), (0, 0.), (1, 1.), (1, 0.), (0, -1.), (1, -1.)],
                &[0, 1, 2, 2, 1, 3, 1, 4, 3, 3, 4, 5],
            )
        } else {
            (&[(0, 1.), (0, -1.), (1, 1.), (1, -1.)], &[0, 1, 2, 2, 1, 3])
        };
        let base = self.vertices.len() as u32;
        self.indices.extend(indices.iter().map(|&i| base + i));
        let color = color.to_array();
        for &(endpoint, lateral) in strip {
            let p = [a, b][endpoint] + n * lateral;
            self.vertices.push(Vertex {
                position: [
                    (p.x / self.size.x * 2. - 1.) as f32,
                    (1. - p.y / self.size.y * 2.) as f32,
                    stroke_depth(depths[endpoint], n * lateral, surfaces),
                ],
                color,
                lateral: lateral as f32,
            });
        }
    }

    pub fn world_line(&mut self, _camera: &Camera, a: DVec3, b: DVec3, width: f64, color: Color32) {
        self.styled_line(a, b, width, color, false, false);
    }
    /// Reference axes have world depth, but remain visible reference geometry
    /// when model section cuts and fog are enabled.
    pub fn axis_line(&mut self, a: DVec3, b: DVec3, width: f64, color: Color32) {
        if let Some((a, b)) = self.view.segment_depth(a, b) {
            self.line(a, b, width, color);
        }
    }
    /// Keep projected depth after centerline clipping so the complete stroke
    /// can also be clipped against foreground surfaces before painting.
    pub fn annotation_line(&mut self, a: DVec3, b: DVec3) -> Vec<(DVec3, DVec3)> {
        let Some((a, b)) = self.section_line(a, b) else {
            return vec![];
        };
        let Some((a, b)) = self.view.segment_depth(a, b) else {
            return vec![];
        };
        self.visible_annotation_line(a, b)
    }

    pub fn annotation_tick(&mut self, anchor: DVec3, offset: DVec2) -> Vec<(DVec3, DVec3)> {
        if self.section_line(anchor, anchor).is_none() {
            return vec![];
        }
        let Some(point) = self.view.project_depth(anchor) else {
            return vec![];
        };
        self.visible_annotation_line(point - offset.extend(0.), point + offset.extend(0.))
    }

    pub fn annotation_arrow(&mut self, anchor: DVec3, offset: DVec2) -> Vec<(DVec3, DVec3)> {
        if self.section_line(anchor, anchor).is_none() {
            return vec![];
        }
        let Some(point) = self.view.project_depth(anchor) else {
            return vec![];
        };
        self.visible_annotation_line(point, point + offset.extend(0.))
    }

    fn visible_annotation_line(&mut self, a: DVec3, b: DVec3) -> Vec<(DVec3, DVec3)> {
        let visible = self.occlusion.visible_intervals(a, b);
        let lines = visible
            .iter()
            .map(|&(start, end)| (a.lerp(b, start), a.lerp(b, end)))
            .collect();
        self.occlusion.spans = visible;
        lines
    }

    /// Clip the tessellated stroke footprint as well as its centerline.
    /// Width, end caps, and the antialiasing fringe must all stay behind faces.
    pub fn annotation_stroke(
        &mut self,
        painter: &egui::Painter,
        viewport: Rect,
        a: DVec3,
        b: DVec3,
        stroke: egui::Stroke,
    ) -> Vec<egui::Shape> {
        let screen = |p: DVec3| viewport.min + egui::vec2(p.x as f32, p.y as f32);
        let shape = egui::Shape::line_segment([screen(a), screen(b)], stroke);
        let bounds = shape
            .visual_bounding_rect()
            .expand(1. / painter.ctx().pixels_per_point());
        let local = bounds.translate(-viewport.min.to_vec2());
        let polygon: Vec<_> = [
            local.left_top(),
            local.right_top(),
            local.right_bottom(),
            local.left_bottom(),
        ]
        .map(|p| DVec2::new(f64::from(p.x), f64::from(p.y)))
        .into();
        let delta = (b - a).truncate();
        let gradient = if delta.length_squared() > 1e-12 {
            delta * ((b.z - a.z) / delta.length_squared())
        } else {
            DVec2::ZERO
        };
        let visible = self
            .occlusion
            .visible_surface_polygons(polygon.clone(), a, gradient);
        if visible.len() == 1 && visible[0] == polygon {
            return vec![shape];
        }
        if visible.is_empty() {
            return vec![];
        }
        clipped_annotation_shapes(painter, viewport, shape, &visible)
    }

    /// Labels are screen-facing billboards at their world anchor's depth.
    /// Clip their entire footprint, including glyphs and background, so a
    /// foreground silhouette can cover only part of a label.
    pub fn annotation_label(
        &mut self,
        painter: &egui::Painter,
        viewport: Rect,
        anchor: DVec3,
        bounds: Rect,
        shape: egui::Shape,
    ) -> Vec<egui::Shape> {
        if self.section_line(anchor, anchor).is_none() {
            return vec![];
        }
        let Some(point) = self.view.project_depth(anchor) else {
            return vec![];
        };
        let local = bounds.translate(-viewport.min.to_vec2());
        let polygon: Vec<_> = [
            local.left_top(),
            local.right_top(),
            local.right_bottom(),
            local.left_bottom(),
        ]
        .map(|p| DVec2::new(f64::from(p.x), f64::from(p.y)))
        .into();
        let visible = self.occlusion.visible_polygons(polygon.clone(), point.z);
        if visible.len() == 1 && visible[0] == polygon {
            return vec![shape];
        }
        if visible.is_empty() {
            return vec![];
        }
        clipped_annotation_shapes(painter, viewport, shape, &visible)
    }

    pub fn background_line(
        &mut self,
        _camera: &Camera,
        a: DVec3,
        b: DVec3,
        width: f64,
        color: Color32,
    ) {
        if let Some((a, b)) = self.view.segment(a, b) {
            self.line(a.extend(1.), b.extend(1.), width, color);
        }
    }
    pub fn world_triangle(&mut self, _camera: &Camera, triangle: [DVec3; 3], color: Color32) {
        self.face_triangle(triangle, color, true, true);
    }
    pub fn shadow_face(
        &mut self,
        triangles: &[[DVec3; 3]],
        shadow: &crate::shadows::FaceShadow,
        color: Color32,
        shaded: Color32,
    ) {
        // Lighting subdivisions do not change the opaque surface. Use its
        // original triangles for edge occlusion, keeping the hierarchy small
        // and stable when the sun moves or shadows are toggled.
        for &triangle in triangles {
            self.face_triangle(triangle, color, false, true);
        }
        for &triangle in &shadow.lit {
            self.face_triangle(triangle, color, true, false);
        }
        for &triangle in &shadow.shaded {
            self.face_triangle(triangle, shaded, true, false);
        }
    }
    fn face_triangle(&mut self, triangle: [DVec3; 3], color: Color32, draw: bool, occlude: bool) {
        let start = self.indices.len() as u32;
        // A triangle clipped against the section plane has at most four points.
        let mut polygon = [DVec3::ZERO; 4];
        polygon[..3].copy_from_slice(&triangle);
        let mut len: usize = 3;
        if self.display.section_cuts {
            let z = self.display.section_height;
            len = 0;
            for i in 0..3 {
                let a = triangle[i];
                let b = triangle[(i + 1) % 3];
                if a.z <= z {
                    polygon[len] = a;
                    len += 1;
                }
                if (a.z <= z) != (b.z <= z) {
                    polygon[len] = a.lerp(b, (z - a.z) / (b.z - a.z));
                    len += 1;
                }
            }
        }
        for i in 1..len.saturating_sub(1) {
            let triangle = [polygon[0], polygon[i], polygon[i + 1]];
            let color = self
                .fog_color(color, triangle.iter().sum::<DVec3>() / 3.)
                .to_array();
            let size = self.size;
            let vertices = &mut self.vertices;
            let indices = &mut self.indices;
            let occlusion = &mut self.occlusion;
            self.view.for_each_triangle_depth(triangle, |triangle| {
                if occlude && color[3] == 255 {
                    occlusion.insert(triangle);
                }
                if !draw {
                    return;
                }
                let base = vertices.len() as u32;
                indices.extend([base, base + 1, base + 2]);
                for p in triangle {
                    vertices.push(Vertex {
                        position: [
                            (p.x / size.x * 2. - 1.) as f32,
                            (1. - p.y / size.y * 2.) as f32,
                            p.z.clamp(0., 1.) as f32,
                        ],
                        color,
                        lateral: 0.,
                    });
                }
            });
        }
        let end = self.indices.len() as u32;
        if start != end {
            if let Some(previous) = self.triangle_ranges.last_mut().filter(|r| r.end == start) {
                previous.end = end;
            } else {
                self.triangle_ranges.push(start..end);
            }
        }
    }
    pub fn paint_cached(mut self, painter: &egui::Painter, rect: Rect, cache: &mut SceneCache) {
        self.occlusion.lines.truncate(self.occlusion.line_cursor);
        cache.occlusion = std::mem::take(&mut self.occlusion);
        self.edge_surfaces
            .retain(|_, surfaces| !surfaces.is_empty());
        cache.edge_surfaces = std::mem::take(&mut self.edge_surfaces);
        self.paint(painter, rect);
    }
    fn normalize_parallel_depth(&mut self) {
        if !self.parallel {
            return;
        }
        // Orthographic projection uses a very distant far plane. WebGL's
        // conversion from [0, 1] to [-1, 1] otherwise rounds nearby faces to
        // the same depth, letting rear faces overwrite visible surfaces.
        // Remap only GPU depths; CPU occlusion keeps its original precision.
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for vertex in &self.vertices {
            let depth = f64::from(vertex.position[2]);
            if depth > 0. && depth < 1. {
                lo = lo.min(depth);
                hi = hi.max(depth);
            }
        }
        if !lo.is_finite() {
            return;
        }
        let center = (lo + hi) * 0.5;
        let span = (hi - lo).max(f64::EPSILON);
        for vertex in &mut self.vertices {
            let depth = f64::from(vertex.position[2]);
            // Depths 0 and 1 are reserved for overlays and background lines.
            if depth > 0. && depth < 1. {
                vertex.position[2] = (0.5 + 0.8 * (depth - center) / span) as f32;
            }
        }
    }

    pub fn paint(mut self, painter: &egui::Painter, rect: Rect) {
        self.normalize_parallel_depth();
        painter.add(egui_wgpu::Callback::new_paint_callback(
            rect,
            SceneCallback {
                vertices: self.vertices,
                indices: self.indices,
                triangle_ranges: self.triangle_ranges,
                buffer_pool: self.buffer_pool,
            },
        ));
    }
}

// Test centerlines in f64 before applying screen-space width or rounding to
// GPU depth. A steep incident plane must never resurrect an occluded edge.
// A hierarchy rejects whole groups of faces by position and depth. Queries
// subtract hidden spans immediately and stop once the edge is fully hidden.
#[derive(Default)]
struct Occlusion {
    triangles: Vec<OccludingTriangle>,
    indices: Vec<usize>,
    nodes: Vec<OcclusionNode>,
    pending: Vec<usize>,
    spans: Vec<(f64, f64)>,
    insertion_cursor: Option<usize>,
    line_cursor: usize,
    lines: Vec<CachedVisibility>,
    revision: u64,
    needs_refit: bool,
    tree_quality: f64,
    #[cfg(test)]
    query_work: (usize, usize),
    #[cfg(test)]
    hierarchy_builds: usize,
}

struct CachedVisibility {
    a: DVec3,
    b: DVec3,
    revision: u64,
    spans: Vec<(f64, f64)>,
}

struct OccludingTriangle {
    points: [DVec3; 3],
    gradient: DVec2,
    bounds: OcclusionBounds,
}

#[derive(Clone, Copy)]
struct OcclusionBounds {
    lo: DVec3,
    hi: DVec3,
}

impl OcclusionBounds {
    fn overlaps(self, a: DVec3, b: DVec3) -> bool {
        if self.lo.z >= a.z.max(b.z) {
            return false;
        }
        let mut interval = (0., 1.);
        for axis in 0..2 {
            if !clip_positive(
                &mut interval,
                a[axis] - self.lo[axis],
                b[axis] - self.lo[axis],
            ) || !clip_positive(
                &mut interval,
                self.hi[axis] - a[axis],
                self.hi[axis] - b[axis],
            ) {
                return false;
            }
        }
        true
    }
}

struct OcclusionNode {
    bounds: OcclusionBounds,
    start: usize,
    end: usize,
    children: Option<(usize, usize)>,
}

impl OccludingTriangle {
    fn hidden_interval(&self, a: DVec3, b: DVec3) -> Option<(f64, f64)> {
        let mut interval = (0., 1.);
        let delta = b - a;
        let [p, q, r] = self.points.map(|p| p.truncate());
        let winding = (q - p).perp_dot(r - p).signum();
        for (p, q) in [(p, q), (q, r), (r, p)] {
            let side = |v: DVec3| winding * (q - p).perp_dot(v.truncate() - p);
            if !clip_positive(&mut interval, side(a), side(b)) {
                return None;
            }
        }
        let origin = self.points[0];
        let difference = a.z - origin.z - self.gradient.dot((a - origin).truncate());
        let change = delta.z - self.gradient.dot(delta.truncate());
        // Preserve coplanar boundaries; this tolerance covers f64 arithmetic,
        // rather than allowing a whole f32 depth step through the foreground.
        let tolerance = 64. * f64::EPSILON;
        if !clip_positive(
            &mut interval,
            difference - tolerance,
            difference + change - tolerance,
        ) {
            return None;
        }
        Some(interval)
    }
}

fn clip_positive(interval: &mut (f64, f64), a: f64, b: f64) -> bool {
    if a < 0. && b < 0. {
        return false;
    }
    if a < 0. {
        interval.0 = interval.0.max(a / (a - b));
    } else if b < 0. {
        interval.1 = interval.1.min(a / (a - b));
    }
    interval.0 < interval.1
}

impl Occlusion {
    fn begin_frame(&mut self) {
        self.insertion_cursor = Some(0);
        self.line_cursor = 0;
    }

    fn insert(&mut self, points: [DVec3; 3]) {
        let index = self.insertion_cursor.unwrap_or(self.triangles.len());
        if self
            .triangles
            .get(index)
            .is_some_and(|t| t.points == points)
        {
            if let Some(cursor) = &mut self.insertion_cursor {
                *cursor += 1;
            }
            return;
        }
        let Some(gradient) = depth_gradient(points) else {
            return;
        };
        let bounds = OcclusionBounds {
            lo: points.iter().copied().reduce(DVec3::min).unwrap(),
            hi: points.iter().copied().reduce(DVec3::max).unwrap(),
        };
        let triangle = OccludingTriangle {
            points,
            gradient,
            bounds,
        };
        if index < self.triangles.len() {
            self.triangles[index] = triangle;
            self.needs_refit = true;
        } else {
            self.indices.push(index);
            self.triangles.push(triangle);
            self.nodes.clear();
        }
        if let Some(cursor) = &mut self.insertion_cursor {
            *cursor += 1;
        }
        self.revision = self.revision.wrapping_add(1);
    }

    fn finish_collection(&mut self) {
        if let Some(len) = self.insertion_cursor {
            if len < self.triangles.len() {
                self.triangles.truncate(len);
                self.indices.clear();
                self.indices.extend(0..len);
                self.nodes.clear();
                self.revision = self.revision.wrapping_add(1);
            }
        }
        if self.needs_refit && !self.nodes.is_empty() {
            for index in (0..self.nodes.len()).rev() {
                let node = &self.nodes[index];
                let bounds = if let Some((left, right)) = node.children {
                    let a = self.nodes[left].bounds;
                    let b = self.nodes[right].bounds;
                    OcclusionBounds {
                        lo: a.lo.min(b.lo),
                        hi: a.hi.max(b.hi),
                    }
                } else {
                    self.bounds(node.start, node.end)
                };
                self.nodes[index].bounds = bounds;
            }
            // Refit is linear during navigation. Repartition only when the
            // old grouping becomes substantially less useful at the new view.
            if self.quality() > self.tree_quality * 2. {
                self.nodes.clear();
            }
        }
        self.needs_refit = false;
    }

    fn quality(&self) -> f64 {
        let area = |bounds: OcclusionBounds| {
            let extent = bounds.hi - bounds.lo;
            extent.x * extent.y
        };
        self.nodes.iter().map(|n| area(n.bounds)).sum::<f64>()
            / area(self.nodes[0].bounds).max(1e-12)
    }

    fn bounds(&self, start: usize, end: usize) -> OcclusionBounds {
        self.indices[start..end].iter().fold(
            OcclusionBounds {
                lo: DVec3::splat(f64::INFINITY),
                hi: DVec3::splat(f64::NEG_INFINITY),
            },
            |bounds, &index| {
                let triangle = &self.triangles[index];
                OcclusionBounds {
                    lo: bounds.lo.min(triangle.bounds.lo),
                    hi: bounds.hi.max(triangle.bounds.hi),
                }
            },
        )
    }

    fn build(&mut self, start: usize, end: usize) -> usize {
        let bounds = self.bounds(start, end);
        let index = self.nodes.len();
        self.nodes.push(OcclusionNode {
            bounds,
            start,
            end,
            children: None,
        });
        if end - start > 8 {
            let extent = bounds.hi - bounds.lo;
            let (center_lo, center_hi) = self.indices[start..end].iter().fold(
                (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
                |(lo, hi), &index| {
                    let b = self.triangles[index].bounds;
                    let center = (b.lo + b.hi) * 0.5;
                    (lo.min(center), hi.max(center))
                },
            );
            let spread = center_hi - center_lo;
            // Stacked, overlapping faces have almost the same screen bounds.
            // Split those by depth so nearer layers are visited together.
            let axis =
                if spread.x <= extent.x * 0.25 && spread.y <= extent.y * 0.25 && spread.z > 0. {
                    2
                } else {
                    usize::from(spread.y > spread.x)
                };
            let middle = start + (end - start) / 2;
            self.indices[start..end].select_nth_unstable_by(middle - start, |&a, &b| {
                let a = self.triangles[a].bounds;
                let b = self.triangles[b].bounds;
                (a.lo[axis] + a.hi[axis]).total_cmp(&(b.lo[axis] + b.hi[axis]))
            });
            let left = self.build(start, middle);
            let right = self.build(middle, end);
            self.nodes[index].children = Some((left, right));
        }
        index
    }

    fn visible_intervals(&mut self, a: DVec3, b: DVec3) -> Vec<(f64, f64)> {
        self.finish_collection();
        let index = self.line_cursor;
        self.line_cursor += 1;
        let mut visible = std::mem::take(&mut self.spans);
        visible.clear();
        if let Some(cached) = self.lines.get(index) {
            if cached.a == a && cached.b == b && cached.revision == self.revision {
                visible.extend_from_slice(&cached.spans);
                #[cfg(test)]
                {
                    self.query_work = (0, 0);
                }
                return visible;
            }
        }
        self.compute_intervals(a, b, &mut visible);
        if index == self.lines.len() {
            self.lines.push(CachedVisibility {
                a,
                b,
                revision: self.revision,
                spans: visible.clone(),
            });
        } else {
            let cached = &mut self.lines[index];
            cached.a = a;
            cached.b = b;
            cached.revision = self.revision;
            cached.spans.clear();
            cached.spans.extend_from_slice(&visible);
        }
        visible
    }

    fn visible_polygons(&mut self, polygon: Vec<DVec2>, depth: f64) -> Vec<Vec<DVec2>> {
        self.visible_surface_polygons(polygon, DVec3::new(0., 0., depth), DVec2::ZERO)
    }

    fn visible_surface_polygons(
        &mut self,
        polygon: Vec<DVec2>,
        origin: DVec3,
        gradient: DVec2,
    ) -> Vec<Vec<DVec2>> {
        self.finish_collection();
        let mut visible = vec![polygon];
        if self.triangles.is_empty() {
            return visible;
        }
        if self.nodes.is_empty() {
            self.build(0, self.indices.len());
            self.tree_quality = self.quality();
        }
        let depth_at = |p: DVec2| origin.z + gradient.dot(p - origin.truncate());
        let lo = visible[0]
            .iter()
            .map(|&p| p.extend(depth_at(p)))
            .reduce(DVec3::min)
            .unwrap();
        let hi = visible[0]
            .iter()
            .map(|&p| p.extend(depth_at(p)))
            .reduce(DVec3::max)
            .unwrap();
        let overlaps = |bounds: OcclusionBounds| {
            bounds.lo.z < hi.z
                && bounds.lo.x < hi.x
                && bounds.hi.x > lo.x
                && bounds.lo.y < hi.y
                && bounds.hi.y > lo.y
        };
        self.pending.clear();
        self.pending.push(0);
        while let Some(index) = self.pending.pop() {
            let node = &self.nodes[index];
            if !overlaps(node.bounds) {
                continue;
            }
            if let Some((left, right)) = node.children {
                self.pending.extend([left, right]);
                continue;
            }
            for &index in &self.indices[node.start..node.end] {
                let triangle = &self.triangles[index];
                if !overlaps(triangle.bounds) {
                    continue;
                }
                let [a, b, c] = triangle.points.map(|p| p.truncate());
                let winding = (b - a).perp_dot(c - a).signum();
                let mut outside = vec![];
                // The occluded region is the intersection of the triangle's
                // three screen half-planes and its foreground depth half-plane.
                for mut polygon in visible {
                    for (normal, origin, offset) in [
                        (DVec2::new(-(b - a).y, (b - a).x) * winding, a, 0.),
                        (DVec2::new(-(c - b).y, (c - b).x) * winding, b, 0.),
                        (DVec2::new(-(a - c).y, (a - c).x) * winding, c, 0.),
                        (
                            gradient - triangle.gradient,
                            a,
                            depth_at(a) - triangle.points[0].z - 64. * f64::EPSILON,
                        ),
                    ] {
                        let side = |p: DVec2| normal.dot(p - origin) + offset;
                        let piece = clip_polygon(&polygon, |p| -side(p));
                        if piece.len() >= 3 {
                            outside.push(piece);
                        }
                        polygon = clip_polygon(&polygon, side);
                        if polygon.len() < 3 {
                            break;
                        }
                    }
                }
                visible = outside;
                if visible.is_empty() {
                    return visible;
                }
            }
        }
        visible
    }

    fn compute_intervals(&mut self, a: DVec3, b: DVec3, visible: &mut Vec<(f64, f64)>) {
        #[cfg(test)]
        {
            self.query_work = (0, 0);
        }
        // Mesh returns this buffer after emitting the stroke, so ordinary
        // edges reuse both the traversal stack and interval allocation.
        visible.push((0., 1.));
        if self.triangles.is_empty() {
            return;
        }
        // Build once after collecting faces, not once per edge. Late preview
        // or section faces invalidate the hierarchy when they are appended.
        if self.nodes.is_empty() {
            self.build(0, self.indices.len());
            self.tree_quality = self.quality();
            #[cfg(test)]
            {
                self.hierarchy_builds += 1;
            }
        }
        self.pending.clear();
        self.pending.push(0);
        while let Some(index) = self.pending.pop() {
            #[cfg(test)]
            {
                self.query_work.0 += 1;
            }
            let node = &self.nodes[index];
            if !node.bounds.overlaps(a, b) {
                continue;
            }
            if let Some((left, right)) = node.children {
                // Visit nearer groups first to finish fully hidden edges early.
                if self.nodes[left].bounds.lo.z <= self.nodes[right].bounds.lo.z {
                    self.pending.extend([right, left]);
                } else {
                    self.pending.extend([left, right]);
                }
                continue;
            }
            for &index in &self.indices[node.start..node.end] {
                let triangle = &self.triangles[index];
                if !triangle.bounds.overlaps(a, b) {
                    continue;
                }
                #[cfg(test)]
                {
                    self.query_work.1 += 1;
                }
                let Some((lo, hi)) = triangle.hidden_interval(a, b) else {
                    continue;
                };
                subtract_hidden(visible, lo, hi);
                if visible.is_empty() {
                    return;
                }
            }
        }
        visible.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    }
}

fn clip_polygon(polygon: &[DVec2], side: impl Fn(DVec2) -> f64) -> Vec<DVec2> {
    let mut clipped = vec![];
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        let (da, db) = (side(a), side(b));
        if da >= 0. {
            clipped.push(a);
        }
        if (da >= 0.) != (db >= 0.) {
            clipped.push(a.lerp(b, da / (da - db)));
        }
    }
    // Half-plane subtraction can leave coincident vertices along a shared
    // triangle boundary. A zero-area region must not become a clipping mask:
    // its zero-length sides would otherwise admit the entire source mesh.
    if clipped.len() < 3 {
        return vec![];
    }
    let origin = clipped[0];
    let area = (1..clipped.len() - 1)
        .map(|i| (clipped[i] - origin).perp_dot(clipped[i + 1] - origin))
        .sum::<f64>();
    if area.abs() <= 1e-12 {
        return vec![];
    }
    clipped
}

fn clipped_annotation_shapes(
    painter: &egui::Painter,
    viewport: Rect,
    shape: egui::Shape,
    regions: &[Vec<DVec2>],
) -> Vec<egui::Shape> {
    match shape {
        egui::Shape::Vec(shapes) => shapes
            .into_iter()
            .flat_map(|shape| clipped_annotation_shapes(painter, viewport, shape, regions))
            .collect(),
        egui::Shape::Text(mut text) => {
            // Retain texel UVs in the galley until egui's final tessellation:
            // other UI text can still grow the font atlas later in this frame.
            use egui::emath::GuiRounding as _;
            let pos = if painter
                .ctx()
                .tessellation_options(|o| o.round_text_to_pixels)
            {
                text.pos.round_to_pixels(painter.ctx().pixels_per_point())
            } else {
                text.pos
            };
            let galley = Arc::make_mut(&mut text.galley);
            galley.num_vertices = 0;
            galley.num_indices = 0;
            for placed in &mut galley.rows {
                let origin = pos - viewport.min + placed.pos.to_vec2();
                let row = Arc::make_mut(&mut placed.row);
                row.visuals.mesh = clipped_annotation_mesh(&row.visuals.mesh, regions, origin);
                row.visuals.mesh_bounds = row.visuals.mesh.calc_bounds();
                row.visuals.glyph_vertex_range = 0..row.visuals.mesh.vertices.len();
                row.visuals.glyph_index_start = 0;
                galley.num_vertices += row.visuals.mesh.vertices.len();
                galley.num_indices += row.visuals.mesh.indices.len();
            }
            vec![egui::Shape::Text(text)]
        }
        shape => painter
            .ctx()
            .tessellate(
                vec![egui::epaint::ClippedShape::new(painter.clip_rect(), shape)],
                painter.ctx().pixels_per_point(),
            )
            .into_iter()
            .filter_map(|primitive| {
                let egui::epaint::Primitive::Mesh(source) = primitive.primitive else {
                    return None;
                };
                Some(egui::Shape::mesh(clipped_annotation_mesh(
                    &source,
                    regions,
                    -viewport.min.to_vec2(),
                )))
            })
            .collect(),
    }
}

fn clipped_annotation_mesh(
    source: &egui::Mesh,
    regions: &[Vec<DVec2>],
    origin: egui::Vec2,
) -> egui::Mesh {
    let mut clipped = egui::Mesh::with_texture(source.texture_id);
    for region in regions {
        for triangle in source.indices.chunks_exact(3) {
            let mut vertices: Vec<_> = triangle
                .iter()
                .map(|&i| source.vertices[i as usize])
                .collect();
            for i in 0..region.len() {
                let a = region[i];
                let b = region[(i + 1) % region.len()];
                vertices = clip_annotation_vertices(&vertices, |v| {
                    let p = v.pos.to_vec2() + origin;
                    (b - a).perp_dot(DVec2::new(f64::from(p.x), f64::from(p.y)) - a)
                });
            }
            let base = clipped.vertices.len() as u32;
            for i in 1..vertices.len().saturating_sub(1) {
                clipped
                    .indices
                    .extend([base, base + i as u32, base + i as u32 + 1]);
            }
            clipped.vertices.extend(vertices);
        }
    }
    clipped
}

fn clip_annotation_vertices(
    vertices: &[egui::epaint::Vertex],
    side: impl Fn(&egui::epaint::Vertex) -> f64,
) -> Vec<egui::epaint::Vertex> {
    let mut clipped = vec![];
    for i in 0..vertices.len() {
        let a = vertices[i];
        let b = vertices[(i + 1) % vertices.len()];
        let (da, db) = (side(&a), side(&b));
        if da >= 0. {
            clipped.push(a);
        }
        if (da >= 0.) != (db >= 0.) {
            let t = (da / (da - db)) as f32;
            let [r, g, b_color, alpha] = std::array::from_fn(|i| {
                egui::lerp(f32::from(a.color[i])..=f32::from(b.color[i]), t).round() as u8
            });
            clipped.push(egui::epaint::Vertex {
                pos: a.pos.lerp(b.pos, t),
                uv: a.uv.lerp(b.uv, t),
                color: Color32::from_rgba_premultiplied(r, g, b_color, alpha),
            });
        }
    }
    clipped
}

fn subtract_hidden(visible: &mut Vec<(f64, f64)>, lo: f64, hi: f64) {
    let mut index = 0;
    while index < visible.len() {
        let (start, end) = visible[index];
        if hi <= start || lo >= end {
            index += 1;
            continue;
        }
        if lo <= start && hi >= end {
            visible.swap_remove(index);
            continue;
        }
        if lo > start && hi < end {
            visible.push((hi, end));
            visible[index].1 = lo;
        } else if lo <= start {
            visible[index].0 = hi;
        } else {
            visible[index].1 = lo;
        }
        index += 1;
    }
}

fn depth_gradient([a, b, c]: [DVec3; 3]) -> Option<DVec2> {
    let u = b - a;
    let v = c - a;
    let determinant = u.truncate().perp_dot(v.truncate());
    if determinant.abs() < 1e-10 {
        return None;
    }
    let gradient = DVec2::new(u.z * v.y - v.z * u.y, u.x * v.z - v.x * u.z) / determinant;
    gradient.is_finite().then_some(gradient)
}

fn stroke_depth(depth: f64, offset: DVec2, surfaces: &[DVec2]) -> f32 {
    // Match the nearest incident surface across the strip, without moving
    // unrelated faces or giving free edges a slope-dependent allowance.
    let offset = surfaces
        .iter()
        .map(|g| g.dot(offset))
        .reduce(f64::min)
        .unwrap_or(0.);
    // One representable step covers rounding between independently projected
    // triangles and strokes without a world-distance or slope-dependent bias.
    ((depth + offset).clamp(0., 1.) as f32).next_down().max(0.)
}

struct Resources {
    face_pipeline: wgpu::RenderPipeline,
    edge_pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    capacity: u64,
    index_buffer: wgpu::Buffer,
    index_capacity: u64,
}
pub fn install(cc: &eframe::CreationContext<'_>) -> bool {
    let Some(state) = cc.wgpu_render_state.as_ref() else {
        return false;
    };
    let device = &state.device;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("KetchUp edges"),
        source: wgpu::ShaderSource::Wgsl(include_str!("edges.wgsl").into()),
    });
    let face_pipeline = create_pipeline(device, &shader, state.target_format, true);
    let edge_pipeline = create_pipeline(device, &shader, state.target_format, false);
    let capacity = 4096;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("KetchUp vertices"),
        size: capacity,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("KetchUp indices"),
        size: capacity,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    state.renderer.write().callback_resources.insert(Resources {
        face_pipeline,
        edge_pipeline,
        buffer,
        capacity,
        index_buffer,
        index_capacity: capacity,
    });
    true
}
fn create_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    faces: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(if faces {
            "KetchUp filled faces"
        } else {
            "KetchUp antialiased edges"
        }),
        layout: None,
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(if format.is_srgb() {
                "vertex_main_srgb"
            } else {
                "vertex_main"
            }),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Unorm8x4,2=>Float32],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fragment_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            // Stroke fringes must not occlude subsequent strokes.
            depth_write_enabled: Some(faces),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            // Faces retain their actual depth. Only their own boundary strips
            // follow the surface slope; unrelated hidden edges get no allowance.
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

struct SceneCallback {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    triangle_ranges: Vec<std::ops::Range<u32>>,
    buffer_pool: Option<BufferPool>,
}

impl Drop for SceneCallback {
    fn drop(&mut self) {
        let Some(pool) = &self.buffer_pool else {
            return;
        };
        if let Ok(mut buffers) = pool.lock() {
            // egui may retain callbacks for overlapping frames. Bound the
            // pool while allowing their allocations to rotate between frames.
            if buffers.len() < 3 {
                buffers.push(MeshBuffers {
                    vertices: std::mem::take(&mut self.vertices),
                    indices: std::mem::take(&mut self.indices),
                    triangle_ranges: std::mem::take(&mut self.triangle_ranges),
                });
            }
        }
    }
}
impl egui_wgpu::CallbackTrait for SceneCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(r) = resources.get_mut::<Resources>() else {
            return vec![];
        };
        let bytes = (self.vertices.len() * std::mem::size_of::<Vertex>()) as u64;
        if bytes > r.capacity {
            r.capacity = bytes.next_power_of_two();
            r.buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("KetchUp vertices"),
                size: r.capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if bytes > 0 {
            queue.write_buffer(&r.buffer, 0, bytemuck::cast_slice(&self.vertices));
        }
        let index_bytes = (self.indices.len() * std::mem::size_of::<u32>()) as u64;
        if index_bytes > r.index_capacity {
            r.index_capacity = index_bytes.next_power_of_two();
            r.index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("KetchUp indices"),
                size: r.index_capacity,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if index_bytes > 0 {
            queue.write_buffer(&r.index_buffer, 0, bytemuck::cast_slice(&self.indices));
        }
        vec![]
    }
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(r) = resources.get::<Resources>() else {
            return;
        };
        if self.vertices.is_empty() {
            return;
        }
        pass.set_vertex_buffer(0, r.buffer.slice(..));
        pass.set_index_buffer(r.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        // Populate depth with faces first, then test edges against that stable
        // surface depth. Grid/overlay depths keep their existing semantics.
        pass.set_pipeline(&r.face_pipeline);
        for range in &self.triangle_ranges {
            pass.draw_indexed(range.clone(), 0, 0..1);
        }
        pass.set_pipeline(&r.edge_pipeline);
        let mut start = 0;
        for range in &self.triangle_ranges {
            if start < range.start {
                pass.draw_indexed(start..range.start, 0, 0..1);
            }
            start = range.end;
        }
        if start < self.indices.len() as u32 {
            pass.draw_indexed(start..self.indices.len() as u32, 0, 0..1);
        }
    }
}

/// Keep triangulation independent of camera, selection, hover, and materials.
/// Compare source geometry as Model's collections also allow direct edits.
#[derive(Default)]
pub struct FaceCache {
    faces: BTreeMap<u64, CachedFace>,
    source_vertices: Vec<(u64, DVec3)>,
    revision: u64,
}
pub struct CachedFace {
    boundary: Face,
    positions: Vec<DVec3>,
    pub normal: DVec3,
    pub triangles: Vec<[DVec3; 3]>,
    pub area: f64,
    pub bounds: (DVec3, DVec3),
    plane: Option<ketchup_core::faces::Plane>,
    rings: Vec<Vec<DVec2>>,
}
impl FaceCache {
    pub fn update(&mut self, model: &Model) {
        // Hover and orbit do not change topology. Validate in collection
        // order rather than looking up every face endpoint in the model.
        if self.faces.len() == model.faces.len()
            && self
                .faces
                .values()
                .zip(model.faces.values())
                .all(|(cached, face)| cached.boundary == *face)
            && self
                .source_vertices
                .iter()
                .copied()
                .eq(model.vertices.iter().map(|(&id, v)| (id, v.position)))
        {
            return;
        }
        let previous_len = self.faces.len();
        self.faces.retain(|id, _| model.faces.contains_key(id));
        if previous_len != self.faces.len() {
            self.revision = self.revision.wrapping_add(1);
        }
        for face in model.faces.values() {
            let positions = || {
                face.vertices
                    .iter()
                    .chain(face.holes.iter().flatten())
                    .map(|id| model.vertices[id].position)
            };
            if self.faces.get(&face.id).is_some_and(|cached| {
                cached.boundary == *face && cached.positions.iter().copied().eq(positions())
            }) {
                continue;
            }
            let plane = ketchup_core::faces::plane(model, &face.vertices);
            let triangles = model.face_triangles(face);
            let area = triangles
                .iter()
                .map(|[a, b, c]| (*b - *a).cross(*c - *a).length() * 0.5)
                .sum();
            let points: Vec<_> = positions().collect();
            let bounds = points.iter().fold(
                (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
                |(lo, hi), p| (lo.min(*p), hi.max(*p)),
            );
            let rings = plane
                .map(|plane| {
                    std::iter::once(&face.vertices)
                        .chain(face.holes.iter())
                        .map(|ring| {
                            ring.iter()
                                .map(|id| plane.project(model.vertices[id].position))
                                .collect()
                        })
                        .collect()
                })
                .unwrap_or_default();
            self.revision = self.revision.wrapping_add(1);
            self.faces.insert(
                face.id,
                CachedFace {
                    boundary: face.clone(),
                    positions: points,
                    normal: plane.map_or(DVec3::Z, |plane| plane.normal),
                    triangles,
                    area,
                    bounds,
                    plane,
                    rings,
                },
            );
        }
        self.source_vertices.clear();
        self.source_vertices
            .extend(model.vertices.iter().map(|(&id, v)| (id, v.position)));
    }
    pub fn get(&self, id: u64) -> &CachedFace {
        &self.faces[&id]
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn iter(&self) -> impl Iterator<Item = (u64, &CachedFace)> {
        self.faces.iter().map(|(id, face)| (*id, face))
    }
    pub fn context_bounds(&self, model: &Model, entities: &EntityCache) -> Option<(DVec3, DVec3)> {
        model.edit_context?;
        let active = |id| {
            let state = entities.get(id);
            state.context.is_some() && !state.hidden
        };
        let points = model
            .edges
            .values()
            .filter(|e| active(e.id))
            .flat_map(|edge| {
                let (a, b) = model.points(edge);
                [a, b]
            })
            .chain(model.faces.values().filter(|f| active(f.id)).flat_map(|f| {
                let (lo, hi) = self.get(f.id).bounds;
                [lo, hi]
            }))
            .chain(
                model
                    .dimensions
                    .values()
                    .filter(|d| active(d.id))
                    .flat_map(|d| d.layout(model)),
            );
        let mut points = points.chain(
            model
                .texts
                .values()
                .filter(|t| active(t.id))
                .flat_map(|t| t.layout(model)),
        );
        let first = points.next()?;
        Some(points.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }
}

impl CachedFace {
    pub fn hit(&self, ray: ketchup_core::Ray) -> Option<(f64, DVec3)> {
        let plane = self.plane?;
        let denominator = ray.direction.dot(plane.normal);
        if denominator.abs() < 1e-9 {
            return None;
        }
        let t = (plane.origin - ray.origin).dot(plane.normal) / denominator;
        if t <= 0.005 {
            return None;
        }
        let point = ray.origin + ray.direction * t;
        let xy = plane.project(point);
        (ketchup_core::faces::in_polygon(xy, &self.rings[0], true)
            && !self
                .rings
                .iter()
                .skip(1)
                .any(|ring| ketchup_core::faces::in_polygon(xy, ring, true)))
        .then_some((t, point))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_faces_remain_distinct_after_webgl_depth_conversion() {
        let camera = Camera {
            projection: ketchup_core::Projection::Parallel,
            ..Default::default()
        };
        let mut mesh = Mesh::new(&camera, DVec2::new(640., 480.));
        let depths = [0., 1e-7_f32, 1.01e-7_f32, 1.];
        let webgl_depth = |z: f32| (z * 2. - 1.) * 0.5 + 0.5;
        assert_eq!(webgl_depth(depths[1]), webgl_depth(depths[2]));
        mesh.vertices = depths
            .into_iter()
            .map(|z| Vertex {
                position: [0., 0., z],
                color: Color32::WHITE.to_array(),
                lateral: 0.,
            })
            .collect();
        mesh.normalize_parallel_depth();
        let mapped: Vec<_> = mesh.vertices.iter().map(|v| v.position[2]).collect();
        assert_eq!(mapped[0], 0., "overlays stay in front");
        assert_eq!(mapped[3], 1., "background lines stay behind geometry");
        assert!(webgl_depth(mapped[1]) < webgl_depth(mapped[2]));
        assert!(mapped[1] > 0. && mapped[2] < 1.);

        let mut perspective = Mesh::new(&Camera::default(), DVec2::new(640., 480.));
        perspective.vertices = mesh.vertices;
        perspective.normalize_parallel_depth();
        assert_eq!(
            perspective
                .vertices
                .iter()
                .map(|v| v.position[2])
                .collect::<Vec<_>>(),
            mapped
        );
    }

    #[test]
    fn indexed_strokes_preserve_coverage_with_compact_vertices() {
        assert_eq!(std::mem::size_of::<Vertex>(), 20);
        assert_eq!(std::mem::offset_of!(Vertex, color), 12);
        assert_eq!(std::mem::offset_of!(Vertex, lateral), 16);
        let size = DVec2::new(1000., 800.);
        for shared in [false, true] {
            for angle in [0_f64, 0.3, std::f64::consts::FRAC_PI_4, 1.57, 2.4] {
                let mut mesh = Mesh::new(&Camera::default(), size);
                let a = DVec2::new(500., 400.);
                let b = a + DVec2::new(angle.cos(), angle.sin()) * 100.;
                let surfaces: &[DVec2] = if shared {
                    &[DVec2::new(0.002, 0.), DVec2::new(-0.003, 0.)]
                } else {
                    &[]
                };
                mesh.surface_stroke(a.extend(0.5), b.extend(0.5), 3., Color32::BLACK, surfaces);
                assert_eq!(mesh.vertices.len(), if shared { 6 } else { 4 });
                assert_eq!(mesh.indices.len(), if shared { 12 } else { 6 });
                let screen = |index: u32| {
                    let p = mesh.vertices[index as usize].position;
                    DVec2::new(
                        (f64::from(p[0]) + 1.) * size.x * 0.5,
                        (1. - f64::from(p[1])) * size.y * 0.5,
                    )
                };
                let area: f64 = mesh
                    .indices
                    .chunks_exact(3)
                    .map(|t| {
                        let [a, b, c] = [screen(t[0]), screen(t[1]), screen(t[2])];
                        let area = (b - a).perp_dot(c - a) * 0.5;
                        assert!(area > 0., "indexed triangles must preserve stroke winding");
                        area
                    })
                    .sum();
                let expected = 100. * (3. + angle.sin().abs() + angle.cos().abs());
                assert!(
                    (area - expected).abs() < 0.01,
                    "indexed strips must cover the full stroke"
                );
            }
        }
    }

    #[test]
    fn mesh_buffers_are_reused_after_the_last_frame_reference_is_released() {
        let mut cache = SceneCache::default();
        let camera = Camera::default();
        let size = DVec2::new(1000., 800.);
        let mut mesh = Mesh::cached(&camera, size, &mut cache);
        mesh.vertices.reserve(1000);
        mesh.indices.reserve(1000);
        mesh.triangle_ranges.reserve(16);
        mesh.line(
            DVec3::new(10., 10., 0.5),
            DVec3::new(90., 10., 0.5),
            1.8,
            Color32::BLACK,
        );
        let vertices = mesh.vertices.as_ptr();
        let indices = mesh.indices.as_ptr();
        let callback = Arc::new(SceneCallback {
            vertices: mesh.vertices,
            indices: mesh.indices,
            triangle_ranges: mesh.triangle_ranges,
            buffer_pool: mesh.buffer_pool,
        });
        let retained = Arc::clone(&callback);
        drop(callback);
        assert!(cache.buffers.lock().unwrap().is_empty());
        drop(retained);
        assert_eq!(cache.buffers.lock().unwrap().len(), 1);
        let next = Mesh::cached(&camera, size, &mut cache);
        assert_eq!(next.vertices.as_ptr(), vertices);
        assert_eq!(next.indices.as_ptr(), indices);
        assert!(
            next.vertices.is_empty() && next.indices.is_empty() && next.triangle_ranges.is_empty()
        );
        assert!(next.vertices.capacity() >= 1000 && next.indices.capacity() >= 1000);
    }

    fn occluding_rectangle(occlusion: &mut Occlusion, lo: DVec2, hi: DVec2, depth: f64) {
        let p = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)].map(|p| p.extend(depth));
        occlusion.insert([p[0], p[1], p[2]]);
        occlusion.insert([p[0], p[2], p[3]]);
    }

    #[test]
    fn dimension_lines_and_ticks_hide_only_behind_foreground_faces() {
        for projection in [
            ketchup_core::Projection::Perspective,
            ketchup_core::Projection::Parallel,
        ] {
            let camera = Camera {
                projection,
                ..Camera::default()
            };
            let size = DVec2::new(1000., 800.);
            let a = camera.target - camera.right() * 2.;
            let b = camera.target + camera.right() * 2.;
            let mut mesh = Mesh::new(&camera, size);
            let center = mesh.view.project_depth(camera.target).unwrap();
            let pa = mesh.view.project(a).unwrap();
            let pb = mesh.view.project(b).unwrap();
            let lo = center.truncate() - DVec2::new(10., 20.);
            let hi = center.truncate() + DVec2::new(10., 20.);
            occluding_rectangle(&mut mesh.occlusion, lo, hi, center.z - 0.001);
            let lines = mesh.annotation_line(a, b);
            assert_eq!(lines.len(), 2);
            assert!(lines[0].0.truncate().distance(pa) < 1e-8);
            assert!((lines[0].1.x - lo.x).abs() < 1e-8);
            assert!((lines[1].0.x - hi.x).abs() < 1e-8);
            assert!(lines[1].1.truncate().distance(pb) < 1e-8);
            assert!(
                mesh.annotation_tick(camera.target, DVec2::new(3., 4.))
                    .is_empty()
            );

            // Moving the covering face behind the dimension restores every part.
            mesh.occlusion = Occlusion::default();
            occluding_rectangle(&mut mesh.occlusion, lo, hi, center.z + 0.001);
            let lines = mesh.annotation_line(a, b);
            assert_eq!(lines.len(), 1);
            assert!(
                lines[0].0.truncate().distance(pa) < 1e-8
                    && lines[0].1.truncate().distance(pb) < 1e-8
            );
            assert_eq!(
                mesh.annotation_tick(camera.target, DVec2::new(3., 4.))
                    .len(),
                1
            );
            mesh.display.section_cuts = true;
            mesh.display.section_height = -1.;
            assert!(mesh.annotation_line(a, b).is_empty());
            assert!(
                mesh.annotation_tick(camera.target, DVec2::new(3., 4.))
                    .is_empty()
            );
        }
    }

    #[test]
    fn dimension_stroke_width_and_antialiasing_do_not_bleed_across_silhouettes() {
        for pixels_per_point in [1., 2., 2.5] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(pixels_per_point);
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let viewport = Rect::from_min_size(egui::pos2(100., 50.), egui::vec2(100., 100.));
                // The first centerline is wholly outside the face. Its width
                // crosses the silhouette; the second stroke's end cap crosses it.
                for (a, b, face_lo, face_depth) in [
                    (DVec3::new(49.75, 20., 0.5), DVec3::new(49.75, 80., 0.5), DVec2::new(50., 0.), 0.4),
                    (DVec3::new(10., 50., 0.5), DVec3::new(50., 50., 0.5), DVec2::new(50., 0.), 0.4),
                    // A perspective stroke's interpolated depth crosses the face.
                    (DVec3::new(20., 50., 0.3), DVec3::new(80., 50., 0.7), DVec2::ZERO, 0.5),
                ] {
                    let mut mesh = Mesh::new(&Camera::default(), DVec2::splat(100.));
                    occluding_rectangle(&mut mesh.occlusion, face_lo, DVec2::splat(100.), face_depth);
                    let shapes = mesh.annotation_stroke(ui.painter(), viewport, a, b, egui::Stroke::new(1.5, Color32::WHITE));
                    assert!(!shapes.is_empty());
                    let mut triangle_count = 0;
                    for shape in shapes {
                        let egui::Shape::Mesh(stroke) = shape else { panic!("a partially covered stroke must clip its full mesh") };
                        triangle_count += stroke.indices.len() / 3;
                        assert!(stroke.vertices.iter().all(|v| v.pos.x <= viewport.min.x + 50.0001), "stroke width or antialiasing leaked behind the face at scale {pixels_per_point}: {a:?} to {b:?}, vertices {:?}", stroke.vertices);
                    }
                    assert!(triangle_count > 0, "preserve the exposed part of the stroke");
                    mesh.occlusion = Occlusion::default();
                    occluding_rectangle(&mut mesh.occlusion, face_lo, DVec2::splat(100.), 0.9);
                    let shapes = mesh.annotation_stroke(ui.painter(), viewport, a, b, egui::Stroke::new(1.5, Color32::WHITE));
                    assert!(matches!(shapes.as_slice(), [egui::Shape::LineSegment { .. }]));
                }
            });
            output.textures_delta.clear();
        }
    }

    fn polygon_area(polygons: &[Vec<DVec2>]) -> f64 {
        polygons
            .iter()
            .map(|p| {
                (0..p.len())
                    .map(|i| p[i].perp_dot(p[(i + 1) % p.len()]))
                    .sum::<f64>()
                    .abs()
                    * 0.5
            })
            .sum()
    }

    #[test]
    fn dimension_label_footprints_follow_silhouettes_and_sloping_depth() {
        let polygon = vec![
            DVec2::new(10., 10.),
            DVec2::new(90., 10.),
            DVec2::new(90., 90.),
            DVec2::new(10., 90.),
        ];
        for reversed in [false, true] {
            for (near, far, area) in [
                (0.4, 0.4, 0.),
                (0.6, 0.6, 6400.),
                (0.5, 0.5, 6400.),
                (0.4, 0.6, 3200.),
            ] {
                let mut occlusion = Occlusion::default();
                let points = [
                    DVec3::new(0., 0., near),
                    DVec3::new(100., 0., far),
                    DVec3::new(100., 100., far),
                    DVec3::new(0., 100., near),
                ];
                for mut triangle in [
                    [points[0], points[1], points[2]],
                    [points[0], points[2], points[3]],
                ] {
                    if reversed {
                        triangle.swap(0, 2);
                    }
                    occlusion.insert(triangle);
                }
                let visible = occlusion.visible_polygons(polygon.clone(), 0.5);
                assert!((polygon_area(&visible) - area).abs() < 1e-7);
            }
        }
        let mut occlusion = Occlusion::default();
        occluding_rectangle(
            &mut occlusion,
            DVec2::new(40., 0.),
            DVec2::new(60., 100.),
            0.4,
        );
        assert!(
            (polygon_area(&occlusion.visible_polygons(polygon.clone(), 0.5)) - 4800.).abs() < 1e-7
        );
        // Hidden or removed faces must stop clipping labels on the next frame.
        occlusion.begin_frame();
        assert_eq!(
            occlusion.visible_polygons(polygon.clone(), 0.5),
            vec![polygon]
        );
    }

    #[test]
    fn partially_occluded_labels_preserve_texture_coordinates_and_viewport_offset() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let viewport = Rect::from_min_size(egui::pos2(100., 50.), egui::vec2(1000., 800.));
            let mut mesh = Mesh::new(&Camera::default(), DVec2::new(1000., 800.));
            let anchor = DVec3::ZERO;
            let center = mesh.view.project_depth(anchor).unwrap();
            let at = viewport.min + egui::vec2(center.x as f32, center.y as f32);
            let bounds = Rect::from_center_size(at, egui::vec2(80., 20.));
            let mut label = egui::Mesh::default();
            label.add_rect_with_uv(
                bounds,
                Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(1., 1.)),
                Color32::WHITE,
            );
            occluding_rectangle(
                &mut mesh.occlusion,
                center.truncate() - DVec2::new(100., 100.),
                center.truncate() + DVec2::new(0., 100.),
                center.z - 0.001,
            );
            let shapes = mesh.annotation_label(
                ui.painter(),
                viewport,
                anchor,
                bounds,
                egui::Shape::mesh(label),
            );
            assert!(!shapes.is_empty());
            let mut area = 0.;
            for shape in shapes {
                let egui::Shape::Mesh(label) = shape else {
                    panic!("clipped label must be a mesh")
                };
                for v in &label.vertices {
                    assert!(v.pos.x >= at.x - 1e-4);
                    assert!((v.uv.x - (v.pos.x - bounds.left()) / bounds.width()).abs() < 1e-5);
                    assert!((v.uv.y - (v.pos.y - bounds.top()) / bounds.height()).abs() < 1e-5);
                    assert_eq!(v.color, Color32::WHITE);
                }
                for indices in label.indices.chunks_exact(3) {
                    let [a, b, c] = [indices[0], indices[1], indices[2]]
                        .map(|i| label.vertices[i as usize].pos);
                    area += f64::from((b - a).rot90().dot(c - a).abs()) * 0.5;
                }
            }
            assert!((area - 800.).abs() < 1e-3);

            let galley = ui.painter().layout_no_wrap(
                "123.45 m".into(),
                egui::FontId::monospace(12.),
                Color32::WHITE,
            );
            let original_vertices = galley.num_vertices;
            let text_at = at - galley.size() * 0.5;
            let bounds = Rect::from_min_size(text_at, galley.size()).expand(4.);
            let shapes = mesh.annotation_label(
                ui.painter(),
                viewport,
                anchor,
                bounds,
                egui::Shape::galley(text_at, Arc::clone(&galley), Color32::WHITE),
            );
            let [egui::Shape::Text(text)] = shapes.as_slice() else {
                panic!("keep clipped glyphs in a galley until final tessellation")
            };
            assert_eq!(
                galley.num_vertices, original_vertices,
                "never mutate cached font layouts"
            );
            assert!(text.galley.num_indices > 0);
            assert!(
                text.galley
                    .rows
                    .iter()
                    .flat_map(|r| &r.visuals.mesh.vertices)
                    .any(|v| v.uv.x > 1.)
            );
            use egui::emath::GuiRounding as _;
            let text_pos = text.pos.round_to_pixels(ctx.pixels_per_point());
            for row in &text.galley.rows {
                for v in &row.visuals.mesh.vertices {
                    assert!(text_pos.x + row.pos.x + v.pos.x >= at.x - 1e-4);
                }
            }
        });
        output.textures_delta.clear();
    }

    #[test]
    fn hover_frames_reuse_visibility_and_changed_views_refit_the_hierarchy() {
        let mut occlusion = Occlusion::default();
        let a = DVec3::new(10., 50., 0.5);
        let b = DVec3::new(90., 50., 0.5);
        occlusion.begin_frame();
        occluding_rectangle(&mut occlusion, DVec2::ZERO, DVec2::splat(100.), 0.4);
        assert!(occlusion.visible_intervals(a, b).is_empty());
        let revision = occlusion.revision;
        assert_eq!(occlusion.hierarchy_builds, 1);

        for _ in 0..10 {
            occlusion.begin_frame();
            occluding_rectangle(&mut occlusion, DVec2::ZERO, DVec2::splat(100.), 0.4);
            assert!(occlusion.visible_intervals(a, b).is_empty());
            assert_eq!(
                occlusion.query_work,
                (0, 0),
                "hover must not traverse faces again"
            );
            assert_eq!(occlusion.revision, revision);
        }
        occlusion.begin_frame();
        occluding_rectangle(&mut occlusion, DVec2::ZERO, DVec2::splat(100.), 0.6);
        assert_eq!(occlusion.visible_intervals(a, b), vec![(0., 1.)]);
        assert_ne!(occlusion.revision, revision);
        assert_eq!(occlusion.hierarchy_builds, 1, "same topology should refit");
        assert_eq!(occlusion.nodes[0].bounds.lo.z, 0.6);

        // Removing faces or changing an opaque fill to a translucent fill
        // must discard the previous frame's occluders and cached intervals.
        occlusion.begin_frame();
        assert_eq!(occlusion.visible_intervals(a, b), vec![(0., 1.)]);
        assert!(occlusion.triangles.is_empty());
    }

    #[test]
    fn occlusion_hierarchy_prunes_large_models_and_stacked_faces() {
        let mut grid = Occlusion::default();
        for y in 0..64 {
            for x in 0..64 {
                let lo = DVec2::new(f64::from(x * 16), f64::from(y * 16));
                occluding_rectangle(&mut grid, lo, lo + DVec2::splat(8.), 0.4);
            }
        }
        assert!(
            grid.visible_intervals(DVec3::new(514., 514., 0.8), DVec3::new(518., 514., 0.8))
                .is_empty()
        );
        assert!(
            grid.query_work.0 < 64,
            "visited {} hierarchy nodes",
            grid.query_work.0
        );
        assert!(
            grid.query_work.1 <= 8,
            "tested {} triangles",
            grid.query_work.1
        );
        // Fully overlapping layers used to create an enormous per-edge
        // candidate set even when the nearest face hid the entire edge.
        let mut layers = Occlusion::default();
        for layer in 0..2048 {
            occluding_rectangle(
                &mut layers,
                DVec2::ZERO,
                DVec2::splat(100.),
                0.4 + f64::from(layer) * 0.0001,
            );
        }
        assert!(
            layers
                .visible_intervals(DVec3::new(10., 50., 0.9), DVec3::new(90., 50., 0.9))
                .is_empty()
        );
        assert!(layers.query_work.0 < 64);
        assert!(layers.query_work.1 <= 8);
    }

    #[test]
    fn occlusion_hierarchy_matches_exhaustive_clipping_and_updates_for_new_faces() {
        let mut state = 42_u64;
        let mut random = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 32) as f64 / f64::from(u32::MAX)
        };
        let mut occlusion = Occlusion::default();
        for _ in 0..100 {
            let points = std::array::from_fn(|_| {
                DVec3::new(random() * 800., random() * 600., 0.4 + random() * 0.2)
            });
            occlusion.insert(points);
        }
        for _ in 0..100 {
            let a = DVec3::new(random() * 800., random() * 600., 0.4 + random() * 0.2);
            let b = DVec3::new(random() * 800., random() * 600., 0.4 + random() * 0.2);
            let mut hidden: Vec<_> = occlusion
                .triangles
                .iter()
                .filter_map(|t| t.hidden_interval(a, b))
                .collect();
            hidden.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut expected = Vec::new();
            let mut start: f64 = 0.;
            for (lo, hi) in hidden {
                if lo > start {
                    expected.push((start, lo));
                }
                start = start.max(hi);
            }
            if start < 1. {
                expected.push((start, 1.));
            }
            let actual = occlusion.visible_intervals(a, b);
            assert_eq!(actual.len(), expected.len());
            for ((a, b), (c, d)) in actual.into_iter().zip(expected) {
                assert!((a - c).abs() < 1e-12 && (b - d).abs() < 1e-12);
            }
        }
        assert!(!occlusion.nodes.is_empty());
        occluding_rectangle(&mut occlusion, DVec2::ZERO, DVec2::new(800., 600.), 0.1);
        assert!(occlusion.nodes.is_empty());
        assert!(
            occlusion
                .visible_intervals(DVec3::new(10., 50., 0.9), DVec3::new(90., 50., 0.9))
                .is_empty()
        );
    }

    #[test]
    fn hidden_centerlines_are_removed_even_within_one_gpu_depth_step() {
        let mut occlusion = Occlusion::default();
        occluding_rectangle(&mut occlusion, DVec2::ZERO, DVec2::splat(100.), 0.999);
        let a = DVec3::new(10., 50., 0.999 + 1e-9);
        let b = DVec3::new(90., 50., a.z);
        assert_eq!(a.z as f32, 0.999_f32);
        assert!(occlusion.visible_intervals(a, b).is_empty());
        // Coplanar outlines and edges in front of a face remain visible.
        for depth in [0.999, 0.998] {
            assert_eq!(
                occlusion.visible_intervals(a.truncate().extend(depth), b.truncate().extend(depth)),
                vec![(0., 1.)]
            );
        }
    }

    #[test]
    fn centerline_clipping_keeps_exposed_ends_and_real_gaps() {
        let mut occlusion = Occlusion::default();
        for x in [100., 300.] {
            occluding_rectangle(
                &mut occlusion,
                DVec2::new(x, 0.),
                DVec2::new(x + 100., 100.),
                0.4,
            );
        }
        assert_eq!(
            occlusion.visible_intervals(DVec3::new(0., 50., 0.5), DVec3::new(500., 50., 0.5)),
            vec![(0., 0.2), (0.4, 0.6), (0.8, 1.)]
        );
    }

    #[test]
    fn centerline_clipping_handles_both_windings_and_depth_crossings() {
        for reverse in [false, true] {
            let mut occlusion = Occlusion::default();
            let mut points = [
                DVec3::new(0., 0., 0.5),
                DVec3::new(200., 0., 0.5),
                DVec3::new(0., 200., 0.5),
            ];
            if reverse {
                points.swap(0, 1);
            }
            occlusion.insert(points);
            let visible =
                occlusion.visible_intervals(DVec3::new(10., 50., 0.4), DVec3::new(90., 50., 0.6));
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].0, 0.);
            assert!((visible[0].1 - 0.5).abs() < 1e-12);
        }
    }

    #[test]
    fn grazing_surface_adjustments_cannot_resurrect_a_hidden_edge() {
        let camera = Camera::default();
        let size = DVec2::new(1000., 800.);
        let toward_eye = (camera.eye() - camera.target).normalize();
        let right = DVec3::Z.cross(toward_eye).normalize();
        let up = toward_eye.cross(right);
        let mut mesh = Mesh::new(&camera, size);
        mesh.world_triangle(
            &camera,
            [
                camera.target - right * 2. - up * 2.,
                camera.target + right * 2. - up * 2.,
                camera.target + up * 2.,
            ],
            Color32::WHITE,
        );
        let count = mesh.vertices.len();
        for width in [1.8, 3., 8.] {
            mesh.surface_line(
                camera.target - right * 0.2 - toward_eye * 0.01,
                camera.target + right * 0.2 - toward_eye * 0.01,
                width,
                Color32::BLACK,
                false,
                true,
                &[DVec2::new(0., 0.1), DVec2::new(0., -0.1)],
            );
            assert_eq!(mesh.vertices.len(), count);
        }
        // Transparent fills must not participate in opaque occlusion.
        let mut translucent = Mesh::new(&camera, size);
        translucent.world_triangle(
            &camera,
            [
                camera.target - right - up,
                camera.target + right - up,
                camera.target + up,
            ],
            Color32::from_rgba_unmultiplied(255, 255, 255, 64),
        );
        assert!(translucent.occlusion.triangles.is_empty());
    }

    #[test]
    fn shadow_subdivisions_preserve_draw_geometry_and_original_occluders() {
        let camera = Camera::default();
        let size = DVec2::new(1000., 800.);
        let [a, b, c] = [
            DVec3::new(-2., -2., 0.),
            DVec3::new(2., -2., 0.),
            DVec3::new(0., 2., 2.),
        ];
        let ab = (a + b) * 0.5;
        let bc = (b + c) * 0.5;
        let ca = (c + a) * 0.5;
        let shadow = crate::shadows::FaceShadow {
            lit: vec![[a, ab, ca], [ab, b, bc], [ca, bc, c]],
            shaded: vec![[ab, bc, ca]],
        };
        for section in [false, true] {
            for alpha in [64, 255] {
                let display = ketchup_core::DisplaySettings {
                    section_cuts: section,
                    section_height: 0.75,
                    fog: true,
                    ..Default::default()
                };
                let color = Color32::from_rgba_unmultiplied(200, 200, 200, alpha);
                let shaded = Color32::from_rgba_unmultiplied(100, 100, 100, alpha);
                let mut actual = Mesh::new(&camera, size);
                actual.set_display(&display);
                actual.shadow_face(&[[a, b, c]], &shadow, color, shaded);
                let mut expected = Mesh::new(&camera, size);
                expected.set_display(&display);
                for &triangle in &shadow.lit {
                    expected.world_triangle(&camera, triangle, color);
                }
                for &triangle in &shadow.shaded {
                    expected.world_triangle(&camera, triangle, shaded);
                }
                assert_eq!(
                    bytemuck::cast_slice::<Vertex, u8>(&actual.vertices),
                    bytemuck::cast_slice::<Vertex, u8>(&expected.vertices)
                );
                assert_eq!(actual.indices, expected.indices);
                assert_eq!(actual.triangle_ranges, expected.triangle_ranges);
                let mut original = Mesh::new(&camera, size);
                original.set_display(&display);
                original.world_triangle(&camera, [a, b, c], color);
                let points = |mesh: &Mesh| {
                    mesh.occlusion
                        .triangles
                        .iter()
                        .map(|t| t.points)
                        .collect::<Vec<_>>()
                };
                assert_eq!(points(&actual), points(&original));
                if alpha == 255 {
                    assert!(actual.occlusion.triangles.len() < expected.occlusion.triangles.len());
                } else {
                    assert!(actual.occlusion.triangles.is_empty());
                }
            }
        }
    }

    #[test]
    fn sloping_faces_do_not_give_unrelated_hidden_edges_a_depth_allowance() {
        let gradient = depth_gradient([
            DVec3::new(0., 0., 0.4),
            DVec3::new(100., 0., 0.6),
            DVec3::new(0., 100., 0.5),
        ])
        .unwrap();
        assert!((gradient - DVec2::new(0.002, 0.001)).length() < 1e-12);
        for offset in [DVec2::new(-3., -2.), DVec2::new(3., 2.)] {
            let surface_depth = (0.5 + gradient.dot(offset)) as f32;
            assert!(stroke_depth(0.5, offset, &[gradient]) <= surface_depth);
            // A free edge just behind the surface must stay hidden. Previously
            // the face's slope bias exposed it for every stroke width/DPI.
            assert!(stroke_depth(f64::from(surface_depth) + 0.0001, offset, &[]) > surface_depth);
        }
    }

    #[test]
    fn shared_crease_keeps_center_depth_for_wide_and_high_dpi_strokes() {
        for scale in [1., 2., 3.] {
            for width in [1.8, 3., 8.] {
                let mut mesh = Mesh::new(&Camera::default(), DVec2::new(1000., 800.));
                mesh.set_pixels_per_point(scale);
                mesh.surface_stroke(
                    DVec3::new(500., 300., 0.5),
                    DVec3::new(500., 500., 0.5),
                    width,
                    Color32::BLACK,
                    &[DVec2::new(0.002, 0.), DVec2::new(-0.003, 0.)],
                );
                let seam: Vec<_> = mesh.vertices.iter().filter(|v| v.lateral == 0.).collect();
                assert_eq!(seam.len(), 2);
                for v in seam {
                    assert_eq!(v.position[0], 0.);
                    assert_eq!(v.position[2], 0.5_f32.next_down());
                }
                assert!(
                    mesh.vertices
                        .iter()
                        .filter(|v| v.lateral != 0.)
                        .all(|v| v.position[2] < 0.499)
                );
            }
        }
    }

    #[test]
    fn edge_allowance_is_local_to_incident_visible_faces_and_includes_holes() {
        let mut model = Model::default();
        for (lo, hi) in [(0., 4.), (1., 3.)] {
            let points = [
                DVec3::new(lo, lo, 0.),
                DVec3::new(hi, lo, 0.),
                DVec3::new(hi, hi, 0.),
                DVec3::new(lo, hi, 0.),
            ];
            for i in 0..4 {
                model.add_edge(points[i], points[(i + 1) % 4]).unwrap();
            }
        }
        let mut cache = FaceCache::default();
        cache.update(&model);
        let face = model.faces.values().find(|f| !f.holes.is_empty()).unwrap();
        let mut mesh = Mesh::new(&Camera::default(), DVec2::new(1000., 800.));
        mesh.face_edges(face, cache.get(face.id));
        assert_eq!(mesh.edge_surfaces.len(), 8);
        for ring in std::iter::once(&face.vertices).chain(face.holes.iter()) {
            for i in 0..ring.len() {
                let a = ring[i];
                let b = ring[(i + 1) % ring.len()];
                assert_eq!(mesh.edge_surfaces[&(a.min(b), a.max(b))].len(), 1);
            }
        }
        assert!(depth_gradient([DVec3::ZERO, DVec3::X, DVec3::X * 2.]).is_none());
    }

    #[test]
    fn stroke_width_is_constant_across_angles_and_display_scales() {
        let size = DVec2::new(1000., 800.);
        for scale in [1., 1.25, 2., 3.] {
            for angle in [0_f64, 0.3, std::f64::consts::FRAC_PI_4, 1.57, 2.4] {
                let mut mesh = Mesh::new(&Camera::default(), size);
                mesh.set_pixels_per_point(scale);
                let a = DVec2::new(500., 400.);
                let b = a + DVec2::new(angle.cos(), angle.sin()) * 100.;
                mesh.line(a.extend(0.5), b.extend(0.5), 1.8, Color32::BLACK);
                let screen = |v: &Vertex| {
                    DVec2::new(
                        (f64::from(v.position[0]) + 1.) * size.x * 0.5,
                        (1. - f64::from(v.position[1])) * size.y * 0.5,
                    )
                };
                let outer_width = screen(&mesh.vertices[0]).distance(screen(&mesh.vertices[1]));
                let fringe = (angle.sin().abs() + angle.cos().abs()) / f64::from(scale);
                assert!((outer_width - fringe - 1.8).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn face_batches_exclude_interleaved_strokes_and_clipped_faces() {
        let camera = Camera::default();
        let mut mesh = Mesh::new(&camera, DVec2::new(1000., 800.));
        let line = |mesh: &mut Mesh| {
            mesh.line(
                DVec3::new(50., 50., 0.5),
                DVec3::new(100., 100., 0.5),
                1.8,
                Color32::BLACK,
            )
        };
        let triangle = [DVec3::ZERO, DVec3::X, DVec3::Y];
        line(&mut mesh);
        mesh.world_triangle(&camera, triangle, Color32::WHITE);
        mesh.world_triangle(&camera, triangle, Color32::WHITE);
        line(&mut mesh);
        mesh.world_triangle(&camera, triangle, Color32::WHITE);
        mesh.set_display(&ketchup_core::DisplaySettings {
            section_cuts: true,
            section_height: -1.,
            ..Default::default()
        });
        mesh.world_triangle(&camera, triangle, Color32::WHITE);
        assert_eq!(mesh.triangle_ranges, vec![6..12, 18..21]);
        assert_eq!(mesh.vertices.len(), 17);
        assert_eq!(mesh.indices.len(), 21);
    }

    #[test]
    fn face_cache_reuses_meshes_and_tracks_geometry_holes_and_undo() {
        let mut model = Model::default();
        for (lo, hi) in [(0., 4.), (1., 3.)] {
            let points = [
                DVec3::new(lo, lo, 0.),
                DVec3::new(hi, lo, 0.),
                DVec3::new(hi, hi, 0.),
                DVec3::new(lo, hi, 0.),
            ];
            for i in 0..4 {
                model.add_edge(points[i], points[(i + 1) % 4]).unwrap();
            }
        }
        let original = model.clone();
        let outer = model
            .faces
            .values()
            .find(|f| !f.holes.is_empty())
            .unwrap()
            .id;
        let mut cache = FaceCache::default();
        cache.update(&model);
        let mesh = cache.get(outer).triangles.as_ptr();
        model.face_colors.insert(outer, [100, 120, 140]);
        model.hidden.insert(outer);
        cache.update(&model);
        assert_eq!(mesh, cache.get(outer).triangles.as_ptr());
        for vertex in model.vertices.values_mut() {
            vertex.position.x *= 2.;
        }
        cache.update(&model);
        assert_eq!(
            cache.get(outer).triangles,
            model.face_triangles(&model.faces[&outer])
        );
        assert_ne!(
            cache.get(outer).triangles,
            original.face_triangles(&original.faces[&outer])
        );
        model.faces.get_mut(&outer).unwrap().holes.clear();
        cache.update(&model);
        assert_eq!(
            cache.get(outer).triangles,
            model.face_triangles(&model.faces[&outer])
        );
        cache.update(&original);
        assert_eq!(
            cache.get(outer).triangles,
            original.face_triangles(&original.faces[&outer])
        );
        cache.update(&Model::default());
        assert!(cache.faces.is_empty());
    }
}

#[derive(Clone, Copy)]
pub struct EntityState {
    pub hidden: bool,
    pub locked: bool,
    pub context: Option<u64>,
    pub entity_hidden: bool,
    pub object_hidden: bool,
    pub geometry_hidden: bool,
    pub tag_hidden: bool,
    pub tag_color: Option<[u8; 3]>,
}

impl EntityState {
    pub fn displayed(self, d: &ketchup_core::DisplaySettings) -> bool {
        !self.tag_hidden
            && (!self.object_hidden || d.hidden_objects)
            && (!self.geometry_hidden || d.hidden_geometry)
    }
}

/// Moving vertices/instance frames does not change hierarchy or inherited state.
#[derive(Default)]
pub struct EntityCache {
    objects: Vec<(u64, std::collections::BTreeSet<u64>)>,
    hidden: std::collections::BTreeSet<u64>,
    locked: std::collections::BTreeSet<u64>,
    context: Option<u64>,
    states: BTreeMap<u64, EntityState>,
    tags: BTreeMap<u64, ketchup_core::Tag>,
    entity_tags: BTreeMap<u64, u64>,
}
impl EntityCache {
    pub fn update(&mut self, model: &Model) {
        if self.context == model.edit_context
            && self.hidden == model.hidden
            && self.locked == model.locked
            && self.tags == model.tags
            && self.entity_tags == model.entity_tags
            && self.objects.len() == model.objects.len()
            && self
                .objects
                .iter()
                .zip(&model.objects)
                .all(|((id, entities), (key, object))| id == key && *entities == object.entities)
        {
            return;
        }
        self.tags.clone_from(&model.tags);
        self.entity_tags.clone_from(&model.entity_tags);
        self.context = model.edit_context;
        self.hidden.clone_from(&model.hidden);
        self.locked.clone_from(&model.locked);
        self.objects = model
            .objects
            .iter()
            .map(|(id, o)| (*id, o.entities.clone()))
            .collect();
        let parents: BTreeMap<_, _> = self
            .objects
            .iter()
            .flat_map(|(id, entities)| entities.iter().map(|child| (*child, *id)))
            .collect();
        self.states.clear();
        for id in parents
            .keys()
            .chain(model.objects.keys())
            .chain(model.entity_tags.keys())
            .chain(model.hidden.iter())
        {
            let mut current = *id;
            let mut hidden = false;
            let mut locked = false;
            let mut object_hidden = false;
            let mut geometry_hidden = false;
            let mut tag_hidden = false;
            let mut tag_color = None;
            let mut context = None;
            loop {
                hidden |= self.hidden.contains(&current);
                object_hidden |=
                    self.hidden.contains(&current) && model.objects.contains_key(&current);
                geometry_hidden |=
                    self.hidden.contains(&current) && !model.objects.contains_key(&current);
                if let Some(tag) = model
                    .entity_tags
                    .get(&current)
                    .and_then(|id| model.tags.get(id))
                {
                    tag_hidden |= !tag.visible;
                    tag_color = tag_color.or(Some(tag.color));
                }
                locked |= self.locked.contains(&current);
                let parent = parents.get(&current).copied();
                if context.is_none() && Some(current) != self.context && parent == self.context {
                    context = Some(current);
                }
                let Some(parent) = parent else {
                    break;
                };
                current = parent;
            }
            // The editing context and its ancestors are never selectable.
            if Some(*id) == self.context {
                context = None;
            }
            self.states.insert(
                *id,
                EntityState {
                    hidden: hidden || tag_hidden,
                    entity_hidden: hidden,
                    object_hidden,
                    geometry_hidden,
                    tag_hidden,
                    tag_color,
                    locked,
                    context,
                },
            );
        }
    }
    pub fn get(&self, id: u64) -> EntityState {
        self.states.get(&id).copied().unwrap_or(EntityState {
            hidden: self.hidden.contains(&id),
            entity_hidden: self.hidden.contains(&id),
            object_hidden: false,
            geometry_hidden: self.hidden.contains(&id),
            tag_hidden: false,
            tag_color: None,
            locked: self.locked.contains(&id),
            context: self.context.is_none().then_some(id),
        })
    }
}

#[cfg(test)]
mod entity_cache_tests {
    use super::*;
    use std::collections::BTreeSet;
    #[test]
    fn entity_cache_matches_nested_visibility_locks_and_editing_scopes() {
        let mut model = Model::default();
        let edge = model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        let inner = model
            .make_object(&BTreeSet::from([edge]), false, "Inner")
            .unwrap();
        let outer = model
            .make_object(&BTreeSet::from([inner]), false, "Outer")
            .unwrap();
        model.add_edge(DVec3::Y, DVec3::Y + DVec3::X).unwrap();
        let mut cache = EntityCache::default();
        for context in [None, Some(outer), Some(inner), None] {
            model.edit_context = context;
            for hidden in [true, false] {
                if hidden {
                    model.hidden.insert(outer);
                    model.locked.insert(inner);
                } else {
                    model.hidden.clear();
                    model.locked.clear();
                }
                cache.update(&model);
                for id in model
                    .edges
                    .keys()
                    .chain(model.faces.keys())
                    .chain(model.objects.keys())
                {
                    let state = cache.get(*id);
                    assert_eq!(state.hidden, model.is_hidden(*id));
                    assert_eq!(state.locked, model.is_locked(*id));
                    assert_eq!(state.context, model.entity_in_context(*id));
                }
            }
        }
        model.edit_context = None;
        model
            .move_selection(&BTreeSet::from([outer]), DVec3::Z)
            .unwrap();
        cache.update(&model);
        for id in model.edges.keys() {
            assert_eq!(cache.get(*id).context, model.entity_in_context(*id));
        }
        cache.update(&Model::default());
        assert!(cache.states.is_empty());
    }
}

#[cfg(test)]
mod display_tests {
    use super::*;
    use std::collections::BTreeSet;
    #[test]
    fn hidden_geometry_objects_and_tag_visibility_remain_independent_in_cache() {
        let mut model = Model::default();
        let edge = model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        let object = model
            .make_object(&BTreeSet::from([edge]), false, "Group")
            .unwrap();
        let edge = *model.edges.keys().next().unwrap();
        model.hidden.insert(object);
        let mut cache = EntityCache::default();
        let mut d = ketchup_core::DisplaySettings::default();
        cache.update(&model);
        assert!(!cache.get(edge).displayed(&d));
        d.hidden_geometry = true;
        assert!(!cache.get(edge).displayed(&d));
        d.hidden_objects = true;
        assert!(cache.get(edge).displayed(&d));
        let tag = model.add_tag("Structure").unwrap();
        model
            .assign_tag(&BTreeSet::from([object]), Some(tag))
            .unwrap();
        model.tags.get_mut(&tag).unwrap().visible = false;
        cache.update(&model);
        assert!(!cache.get(edge).displayed(&d));
        model.tags.get_mut(&tag).unwrap().visible = true;
        cache.update(&model);
        assert!(cache.get(edge).displayed(&d));
        assert_eq!(cache.get(edge).tag_color, Some(model.tags[&tag].color));
        model.hidden.clear();
        model.hidden.insert(edge);
        d.hidden_geometry = false;
        cache.update(&model);
        assert!(!cache.get(edge).displayed(&d));
        d.hidden_geometry = true;
        assert!(cache.get(edge).displayed(&d));
    }

    #[test]
    fn section_clipping_rejects_geometry_above_plane_and_fog_blends_at_distance() {
        let camera = Camera::default();
        let mut mesh = Mesh::new(&camera, DVec2::new(1000., 800.));
        let mut d = ketchup_core::DisplaySettings {
            section_cuts: true,
            section_height: 1.,
            ..Default::default()
        };
        mesh.set_display(&d);
        mesh.world_triangle(
            &camera,
            [
                DVec3::Z * 2.,
                DVec3::X + DVec3::Z * 2.,
                DVec3::Y + DVec3::Z * 2.,
            ],
            Color32::WHITE,
        );
        mesh.world_line(
            &camera,
            DVec3::Z * 2.,
            DVec3::X + DVec3::Z * 2.,
            2.,
            Color32::WHITE,
        );
        assert!(mesh.vertices.is_empty());
        mesh.world_triangle(
            &camera,
            [DVec3::ZERO, DVec3::X, DVec3::Y + DVec3::Z * 2.],
            Color32::WHITE,
        );
        assert!(!mesh.vertices.is_empty());
        d.fog = true;
        d.fog_start = 1.;
        d.fog_end = 2.;
        d.background = [100, 120, 140];
        mesh.set_display(&d);
        assert_eq!(mesh.fog_color(Color32::WHITE, camera.eye()), Color32::WHITE);
        assert_eq!(
            mesh.fog_color(Color32::WHITE, DVec3::ZERO),
            Color32::from_rgb(100, 120, 140)
        );
    }
}
