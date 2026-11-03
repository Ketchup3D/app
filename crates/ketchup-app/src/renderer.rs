use eframe::{egui, egui_wgpu, wgpu};
use egui::{Color32, Rect};
use ketchup_core::{Camera, CameraView, DVec2, DVec3, Face, Model};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Shared reference geometry fade, measured along the camera's forward axis.
pub(crate) struct ReferenceFade {
    eye: DVec3,
    forward: DVec3,
    pub origin_depth: f64,
}

impl ReferenceFade {
    pub fn new(camera: &Camera) -> Self {
        let eye = camera.eye();
        let forward = (camera.target - eye).normalize();
        let depth = (-eye).dot(forward);
        Self {
            eye,
            forward,
            origin_depth: if depth > 0.005 {
                depth
            } else {
                camera.distance
            },
        }
    }

    pub fn depth(&self, point: DVec3) -> f64 {
        (point - self.eye).dot(self.forward)
    }

    pub fn opacity(depth: f64, origin_depth: f64) -> f32 {
        (origin_depth / depth).clamp(0., 1.) as f32
    }

    pub fn axis_length(&self) -> f64 {
        // End below one alpha step, even when the origin is far off screen.
        self.eye.length() + self.origin_depth * 256.
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    color: [u8; 4],
    lateral: f32,
    shadow_color: [u8; 4],
    dash_phase: f32,
}

// A face slope applies only on the face side of its boundary. Extending
// the plane across the silhouette can pull a rear object's stroke forward.
#[derive(Clone, Copy)]
struct EdgeSurface {
    gradient: DVec2,
    inward: DVec2,
}

#[cfg(test)]
impl EdgeSurface {
    fn unbounded(gradient: DVec2) -> Self {
        Self {
            gradient,
            inward: DVec2::ZERO,
        }
    }
}

// Compare document data because callers can edit Model collections directly.
// Share one snapshot across panes and sidebar summaries, rather than cloning a
// large document separately for each camera.
#[derive(Default)]
pub struct ModelSnapshot {
    model: Option<Model>,
    revision: u64,
}

pub(crate) struct GroupMarker {
    pub id: u64,
    pub point: DVec3,
    pub owners: std::collections::BTreeSet<u64>,
}

#[derive(Default)]
pub(crate) struct GroupMarkerCache {
    revision: Option<u64>,
    markers: Arc<[GroupMarker]>,
}

impl GroupMarkerCache {
    pub fn update(&mut self, model: &Model, revision: u64) -> Arc<[GroupMarker]> {
        if self.revision != Some(revision) {
            self.markers = model.objects.values().filter_map(|object| {
                let mut points = model.selection_vertices(&object.entities).into_iter()
                    .filter_map(|id| model.vertices.get(&id).map(|v| v.position));
                let first = points.next()?;
                let (lo, hi) = points.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
                Some(GroupMarker { id: object.id, point: (lo + hi) * 0.5,
                    owners: model.expand_entities(&std::collections::BTreeSet::from([object.id])) })
            }).collect::<Vec<_>>().into();
            self.revision = Some(revision);
        }
        Arc::clone(&self.markers)
    }
}

impl ModelSnapshot {
    pub fn update(&mut self, model: &Model) -> u64 {
        if self.model.as_ref() != Some(model) {
            self.model = Some(model.clone());
            self.revision = self.revision.wrapping_add(1);
        }
        self.revision
    }
}

#[derive(Clone, PartialEq)]
pub struct SceneKey {
    pub joinery_members: Option<[u64; 2]>,
    pub joint_preview: Option<([u64; 2], ketchup_core::joinery::JointSettings)>,
    pub model_revision: u64,
    pub camera: Camera,
    pub size: DVec2,
    pub pixels_per_point: f32,
    pub display: ketchup_core::DisplaySettings,
    pub dark: bool,
    pub tool: ketchup_core::Tool,
    pub selected: std::collections::BTreeSet<u64>,
    pub hover: Option<u64>,
    pub outliner_hover: std::collections::BTreeSet<u64>,
}

#[derive(Default)]
pub struct SceneCache {
    annotations: Option<(Rect, [usize; 2], Vec<egui::Shape>)>,
    joint_surfaces: JointSurfaceCache,
    occlusion: Occlusion,
    selection_occlusion: Occlusion,
    annotation_occluders: Vec<[DVec3; 3]>,
    edge_surfaces: BTreeMap<(u64, u64), Vec<EdgeSurface>>,
    buffers: BufferPool,
    gpu_buffers: GpuBufferPool,
    retained: Option<Arc<SceneCallback>>,
    pub key: Option<SceneKey>,
    #[cfg(test)]
    pub mesh_builds: usize,
}

impl SceneCache {
    pub fn repaint(&self, key: &SceneKey, painter: &egui::Painter, rect: Rect) -> bool {
        if self.key.as_ref() == Some(key)
            && self
                .annotations
                .as_ref()
                .is_none_or(|(saved, atlas, shapes)| {
                    shapes.is_empty()
                        || (*saved == rect
                            && *atlas == painter.ctx().fonts(|f| f.font_image_size()))
                })
        {
            if let Some(callback) = &self.retained {
                painter.add(egui_wgpu::Callback::new_paint_callback(
                    rect,
                    SharedScene(Arc::clone(callback)),
                ));
                if let Some((_, _, annotations)) = &self.annotations {
                    painter.extend(annotations.iter().cloned());
                }
                return true;
            }
        }
        false
    }

    pub fn remember_annotations(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        shapes: Vec<egui::Shape>,
    ) {
        self.annotations = Some((rect, painter.ctx().fonts(|f| f.font_image_size()), shapes));
    }
}

type BufferPool = Arc<Mutex<Vec<MeshBuffers>>>;
type GpuBufferPool = Arc<Mutex<Vec<GpuBuffers>>>;

struct GpuBuffers {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
}

struct MeshBuffers {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    triangle_ranges: Vec<std::ops::Range<u32>>,
}

#[derive(Default)]
pub(super) struct JointSurface {
    occluders: Option<Vec<(Vec<SurfaceCandidate>, Vec<SurfaceCandidate>)>>,
    triangles: Vec<JointSurfaceTriangle>,
    #[cfg(test)]
    preparation_queries: usize,
}
struct SurfaceCandidate {
    index: usize,
    bounds: Option<[DVec2; 2]>,
    planes: Option<[(DVec2, DVec2, f64); 4]>,
}

struct JointSurfaceTriangle {
    points: [DVec3; 3],
    origin: DVec3,
    gradient: DVec2,
}

#[derive(Clone, Default)]
struct JointSurfaceCache {
    camera: Camera,
    size: DVec2,
    sections: [Option<f64>; 3],
    view_revision: u64,
    entries: BTreeMap<u64, CachedJointSurface>,
    used: std::collections::BTreeSet<u64>,
}
#[derive(Clone)]
struct CachedJointSurface {
    stock: Vec<[DVec3; 3]>,
    surface: Arc<JointSurface>,
    view_revision: u64,
}

pub struct Mesh {
    joint_guide_bounds: BTreeMap<u64, (DVec2, DVec2)>,
    visible_joint_surfaces: BTreeMap<u64, (Arc<JointSurface>, Arc<JointSurface>, (u64, u64))>,
    joint_surfaces: JointSurfaceCache,
    record_annotation_occluders: bool,
    gpu_edge_occlusion: bool,
    shadow_vertices: Vec<Vertex>,
    clip_stroke_footprints: bool,
    selection_occlusion: Occlusion,
    annotation_occluders: Vec<[DVec3; 3]>,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    triangle_ranges: Vec<std::ops::Range<u32>>,
    edge_surfaces: BTreeMap<(u64, u64), Vec<EdgeSurface>>,
    occlusion: Occlusion,
    size: DVec2,
    pixels_per_point: f64,
    parallel: bool,
    view: CameraView,
    display: ketchup_core::DisplaySettings,
    buffer_pool: Option<BufferPool>,
    gpu_pool: GpuBufferPool,
}

pub(crate) struct EdgeDraw {
    pub id: u64,
    pub key: (u64, u64),
    pub points: [DVec3; 2],
    pub width: f64,
    pub color: Color32,
    pub back: bool,
    pub visible: bool,
    pub overlay: bool,
}
pub(crate) struct FaceDraw<'a> {
    pub face: &'a Face,
    pub cached: &'a CachedFace,
    pub color: Color32,
    pub shadow: Option<(&'a crate::shadows::FaceShadow, Color32)>,
}
pub(crate) struct JointSurfaceRequest {
    pub owner: u64,
    pub stock: Arc<[[DVec3; 3]]>,
    pub own_entities: Arc<std::collections::BTreeSet<u64>>,
}
impl Mesh {
    pub(crate) fn prepare_joint_surfaces(&mut self, requests: &[JointSurfaceRequest]) {
        self.paint_overlays(requests, 2, |mesh, request| {
            mesh.cached_joint_surface_for_entities(request.owner, &request.stock, &request.own_entities);
        });
    }
    pub fn new(camera: &Camera, size: DVec2) -> Self {
        Self {
            joint_guide_bounds: BTreeMap::new(),
            visible_joint_surfaces: BTreeMap::new(),
            joint_surfaces: JointSurfaceCache {
                camera: camera.clone(),
                size,
                ..Default::default()
            },
            record_annotation_occluders: true,
            gpu_edge_occlusion: false,
            shadow_vertices: Vec::new(),
            clip_stroke_footprints: false,
            selection_occlusion: Occlusion { outline_order: true, ..Default::default() },
            annotation_occluders: Vec::new(),
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
            gpu_pool: Default::default(),
        }
    }
    pub fn cached(camera: &Camera, size: DVec2, cache: &mut SceneCache) -> Self {
        cache.key = None;
        #[cfg(test)]
        {
            cache.mesh_builds += 1;
        }
        let mut mesh = Self::new(camera, size);
        mesh.joint_surfaces = std::mem::take(&mut cache.joint_surfaces);
        mesh.joint_surfaces.used.clear();
        if mesh.joint_surfaces.camera != *camera || mesh.joint_surfaces.size != size {
            mesh.joint_surfaces.camera = camera.clone();
            mesh.joint_surfaces.size = size;
            mesh.joint_surfaces.view_revision = mesh.joint_surfaces.view_revision.wrapping_add(1);
        }
        mesh.buffer_pool = Some(Arc::clone(&cache.buffers));
        mesh.gpu_pool = Arc::clone(&cache.gpu_buffers);
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
        mesh.selection_occlusion = std::mem::take(&mut cache.selection_occlusion);
        mesh.selection_occlusion.outline_order = true;
        mesh.selection_occlusion.begin_frame();
        mesh.annotation_occluders = std::mem::take(&mut cache.annotation_occluders);
        mesh.annotation_occluders.clear();
        mesh.edge_surfaces = std::mem::take(&mut cache.edge_surfaces);
        for surfaces in mesh.edge_surfaces.values_mut() {
            surfaces.clear();
        }
        mesh
    }
    pub fn enable_gpu_edge_occlusion(&mut self) {
        self.gpu_edge_occlusion = true;
    }
    pub fn set_display(&mut self, display: &ketchup_core::DisplaySettings) {
        self.display = display.clone();
    }
    pub fn set_annotation_occlusion_enabled(&mut self, enabled: bool) {
        self.record_annotation_occluders = enabled;
        if !enabled {
            self.annotation_occluders.clear();
        }
    }
    pub fn set_entity(&mut self, entity: Option<u64>) {
        self.occlusion.entity = entity;
        self.selection_occlusion.entity = entity;
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
    fn section_line(&self, a: DVec3, b: DVec3) -> Option<(DVec3, DVec3)> {
        self.display.section_segment(a, b)
    }

    pub fn overlay_line(&mut self, a: DVec3, b: DVec3, width: f64, color: Color32, dashed: bool) {
        let Some((a, b)) = self.section_line(a, b) else {
            return;
        };
        let color = self.fog_color(color, (a + b) * 0.5);
        if let Some((a, b)) = self.view.segment(a, b) {
            if dashed {
                let delta = b - a;
                if delta.length_squared() < 0.01 { return; }
                let length = delta.length();
                let normal = DVec2::new(-delta.y, delta.x) / length;
                let fringe = (normal.x.abs() + normal.y.abs()) / self.pixels_per_point;
                let n = normal * (width + fringe) * 0.5;
                // Screen overlays do not supply annotation/depth masks. Keep
                // their existing four-pixel dash/four-pixel gap on one strip.
                self.emit_stroke_geometry(&[a + n, b + n, b - n, a - n], a.extend(0.),
                    DVec2::ZERO, a, n, color, Some(delta / (length * 8.)));
            } else {
                self.line(a.extend(0.), b.extend(0.), width, color);
            }
        }
    }
    /// Share projections between joints on the same board, and between frames
    /// whose camera and stock are unchanged. Screen-space stroke width and
    /// scene occlusion are still resolved against the current frame.
    #[cfg(test)]
    pub(super) fn cached_joint_surface(
        &mut self,
        owner: u64,
        stock: &[[DVec3; 3]],
    ) -> Arc<JointSurface> {
        self.cached_joint_surface_for_entities(owner, stock, &Default::default())
    }

    pub(super) fn cached_joint_surface_for_entities(
        &mut self,
        owner: u64,
        stock: &[[DVec3; 3]],
        own_entities: &std::collections::BTreeSet<u64>,
    ) -> Arc<JointSurface> {
        #[cfg(not(target_arch = "wasm32"))]
        let _profile = crate::app::viewport_benchmark::scope("guide_surface_cpu");
        let projected = self.projected_joint_surface(owner, stock);
        self.occlusion.finish_collection();
        self.selection_occlusion.finish_collection();
        let revisions = (self.occlusion.revision, self.selection_occlusion.revision);
        if let Some((source, visible, saved)) = self.visible_joint_surfaces.get(&owner) {
            if Arc::ptr_eq(source, &projected) && revisions == *saved {
                return Arc::clone(visible);
            }
        }
        let mut surface = JointSurface::default();
        let mut occluders = Vec::new();
        for triangle in &projected.triangles {
            let mut polygon = triangle.points.map(DVec3::truncate).to_vec();
            if let Some(&(lo, hi)) = self.joint_guide_bounds.get(&owner) {
                for axis in 0..2 {
                    polygon = clip_polygon(&polygon, |p| p[axis] - lo[axis]);
                    polygon = clip_polygon(&polygon, |p| hi[axis] - p[axis]);
                }
            }
            occluders.push((
                self.occlusion.precision_candidates(
                    &polygon,
                    triangle.origin,
                    triangle.gradient,
                    true,
                    own_entities,
                ),
                self.selection_occlusion.precision_candidates(
                    &polygon,
                    triangle.origin,
                    triangle.gradient,
                    true,
                    own_entities,
                ),
            ));
            surface.triangles.push(JointSurfaceTriangle { ..*triangle });
        }
        surface.occluders = Some(occluders);
        let surface = Arc::new(surface);
        self.visible_joint_surfaces
            .insert(owner, (projected, Arc::clone(&surface), revisions));
        surface
    }

    pub(super) fn register_joint_guide(&mut self, owner: u64, a: DVec3, b: DVec3) {
        let Some((a, b)) = self.section_line(a, b) else { return };
        let Some((a, b)) = self.view.segment_depth(a, b) else { return };
        // Include the complete antialiased footprint at every endpoint. The
        // union is registered before any board surface is queried or cached.
        let padding = DVec2::splat(1.8 + 2. / self.pixels_per_point);
        let lo = a.truncate().min(b.truncate()) - padding;
        let hi = a.truncate().max(b.truncate()) + padding;
        self.joint_guide_bounds.entry(owner).and_modify(|bounds| {
            bounds.0 = bounds.0.min(lo); bounds.1 = bounds.1.max(hi);
        }).or_insert((lo, hi));
    }

    fn projected_joint_surface(&mut self, owner: u64, stock: &[[DVec3; 3]]) -> Arc<JointSurface> {
        let sections = self.display.section_limits();
        if self.joint_surfaces.sections != sections {
            self.joint_surfaces.view_revision = self.joint_surfaces.view_revision.wrapping_add(1);
            self.joint_surfaces.sections = sections;
        }
        self.joint_surfaces.used.insert(owner);
        let revision = self.joint_surfaces.view_revision;
        if let Some(entry) = self.joint_surfaces.entries.get(&owner) {
            if entry.view_revision == revision && entry.stock.as_slice() == stock {
                return Arc::clone(&entry.surface);
            }
        }
        let surface = Arc::new(self.joint_surface(stock.iter().copied()));
        self.joint_surfaces
            .entries
            .entry(owner)
            .and_modify(|entry| {
                if entry.stock.as_slice() != stock {
                    entry.stock.clear();
                    entry.stock.extend_from_slice(stock);
                }
                entry.surface = Arc::clone(&surface);
                entry.view_revision = revision;
            })
            .or_insert_with(|| CachedJointSurface {
                stock: stock.to_vec(),
                surface: Arc::clone(&surface),
                view_revision: revision,
            });
        surface
    }

    /// Resolve the visible stock skin once, shared by every contour on this board.
    pub(super) fn joint_surface(
        &self,
        triangles: impl IntoIterator<Item = [DVec3; 3]>,
    ) -> JointSurface {
        #[cfg(not(target_arch = "wasm32"))]
        let _profile = crate::app::viewport_benchmark::scope("stock_skin");
        let mut surface = JointSurface::default();
        let mut occlusion = Occlusion::default();
        for triangle in triangles {
            let (polygon, len) = self.display.section_triangle(triangle);
            for i in 1..len.saturating_sub(1) {
                self.view.for_each_triangle_depth(
                    [polygon[0], polygon[i], polygon[i + 1]],
                    |points| {
                        if let Some(gradient) = depth_gradient(points) {
                            occlusion.insert(points);
                            surface.triangles.push(JointSurfaceTriangle {
                                points,
                                origin: points[0],
                                gradient,
                            });
                        }
                    },
                );
            }
        }
        // Hidden stock faces never need to participate in per-edge clipping.
        // The closed board's front skin is independent of which guide is drawn.
        let projected = std::mem::take(&mut surface.triangles);
        for triangle in projected {
            for polygon in occlusion.visible_surface_polygons(
                triangle.points.map(DVec3::truncate).to_vec(),
                triangle.origin,
                triangle.gradient,
            ) {
                // Coincident projected silhouettes can differ by a few f64
                // arithmetic steps, especially with parallel cameras. Discard
                // invisible slivers rather than supporting guides on rear skin.
                let perimeter: f64 = (0..polygon.len())
                    .map(|i| polygon[i].distance(polygon[(i + 1) % polygon.len()]))
                    .sum();
                let area = polygon.first().map_or(0., |&origin| {
                    polygon
                        .windows(2)
                        .map(|edge| (edge[0] - origin).perp_dot(edge[1] - origin))
                        .sum::<f64>()
                        .abs()
                });
                if area <= (perimeter * 1e-7).max(1e-12) {
                    continue;
                }
                let point = |p: DVec2| {
                    p.extend(
                        triangle.origin.z + triangle.gradient.dot(p - triangle.origin.truncate()),
                    )
                };
                for i in 1..polygon.len().saturating_sub(1) {
                    surface.triangles.push(JointSurfaceTriangle {
                        points: [point(polygon[0]), point(polygon[i]), point(polygon[i + 1])],
                        origin: triangle.origin,
                        gradient: triangle.gradient,
                    });
                }
            }
        }
        #[cfg(test)]
        {
            surface.preparation_queries = occlusion.polygon_queries;
        }
        surface
    }

    /// Project internal joint guides onto their owning board's visible skin.
    /// The skin's real depth lets other objects hide the entire stroke footprint.
    pub(super) fn joint_guide(
        &mut self,
        a: DVec3,
        b: DVec3,
        surface: &JointSurface,
        width: f64,
        color: Color32,
        dashed: bool,
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        let _profile = crate::app::viewport_benchmark::scope("guide_emit_cpu");
        let Some((a, b)) = self.section_line(a, b) else {
            return;
        };
        // The caller supplies cut-created edges only. Show their complete
        // hidden contours through their own stock, including the lower face.
        // Face coplanarity cannot decide whether an edge belongs to the cut.
        let color = self.fog_color(color, (a + b) * 0.5);
        let Some((a, b)) = self.view.segment_depth(a, b) else {
            return;
        };
        let (a, b) = (a.truncate(), b.truncate());
        let delta = b - a;
        let length_squared = delta.length_squared();
        if length_squared < 0.01 {
            return;
        }
        let normal = DVec2::new(-delta.y, delta.x).normalize();
        let fringe = (normal.x.abs() + normal.y.abs()) / self.pixels_per_point;
        let n = normal * (width + fringe) * 0.5;
        let length = delta.length();
        // Fit whole dashes to the complete segment, including both endpoints.
        // Every corner and short connector remains drawn; triangle clipping
        // must not restart the pattern or leave its final corner in a gap.
        let dash_count = (length / 8.).ceil().clamp(1., 64.) as usize;
        let dash_slots = dash_count as f64 - 0.5;
        let mut dash_start = Vec::with_capacity(8);
        let mut dash_end = Vec::with_capacity(8);
        for (index, triangle) in surface.triangles.iter().enumerate() {
            let [p, q, r] = triangle.points.map(DVec3::truncate);
            let winding = (q - p).perp_dot(r - p).signum();
            let sides = [(p, q), (q, r), (r, p)];
            // Expand by the lateral footprint only to bound dash work. Actual
            // pixels are clipped to the exact triangle below, including holes.
            let mut interval = (0., 1.);
            if !sides.iter().all(|&(p, q)| {
                let side = |point: DVec2| winding * (q - p).perp_dot(point - p);
                let allowance = (q - p).perp_dot(n).abs();
                clip_positive(&mut interval, side(a) + allowance, side(b) + allowance)
            }) {
                continue;
            }
            // Clip the complete guide once. Traversing the visibility
            // hierarchies for every individual dash multiplies orbit cost by
            // up to 64 without changing which parts of the guide are visible.
            let (start, end) = (a.lerp(b, interval.0), a.lerp(b, interval.1));
            let mut polygon = vec![start + n, end + n, end - n, start - n];
            for (p, q) in sides {
                polygon = clip_polygon(&polygon, |point| winding * (q - p).perp_dot(point - p));
                if polygon.len() < 3 {
                    break;
                }
            }
            if polygon.len() < 3 {
                continue;
            }
            let polygons = if let Some(occluders) = &surface.occluders {
                #[cfg(not(target_arch = "wasm32"))]
                let _profile = crate::app::viewport_benchmark::scope("stroke_clip");
                let polygons = self.occlusion.visible_surface_candidates(
                    polygon,
                    triangle.origin,
                    triangle.gradient,
                    &occluders[index].0,
                );
                polygons
                    .into_iter()
                    .flat_map(|polygon| {
                        self.selection_occlusion.visible_surface_candidates(
                            polygon,
                            triangle.origin,
                            triangle.gradient,
                            &occluders[index].1,
                        )
                    })
                    .collect()
            } else {
                self.visible_stroke_polygons(polygon, triangle.origin, triangle.gradient)
            };
            for polygon in polygons {
                if !dashed {
                    self.emit_stroke_polygon(
                        &polygon,
                        triangle.origin,
                        triangle.gradient,
                        a,
                        n,
                        color,
                    );
                    continue;
                }
                // One GPU strip carries the dash phase. CPU subdivision is
                // needed only for annotation masks, not for rendered vertices.
                self.emit_stroke_geometry(
                    &polygon,
                    triangle.origin,
                    triangle.gradient,
                    a,
                    n,
                    color,
                    Some(delta * (dash_slots / length_squared)),
                );
                if !self.record_annotation_occluders || color.a() != 255 {
                    continue;
                }
                #[cfg(not(target_arch = "wasm32"))]
                let _profile = crate::app::viewport_benchmark::scope("joint_annotation_masks");
                // Split only the already visible polygons into dashes.
                // Their phase still comes from the complete source edge,
                // so both endpoints and triangle seams stay connected.
                let parameter = |point: DVec2| (point - a).dot(delta) / length_squared;
                let (lo, hi) = polygon.iter().fold((1_f64, 0_f64), |(lo, hi), &point| {
                    let t = parameter(point).clamp(0., 1.);
                    (lo.min(t), hi.max(t))
                });
                for i in (lo * dash_slots).floor() as usize
                    ..((hi * dash_slots).ceil() as usize).min(dash_count)
                {
                    let start = i as f64 / dash_slots;
                    let end = (i as f64 + 0.5) / dash_slots;
                    if start >= hi || end <= lo {
                        continue;
                    }
                    clip_polygon_into(&polygon, |point| parameter(point) - start, &mut dash_start);
                    clip_polygon_into(&dash_start, |point| end - parameter(point), &mut dash_end);
                    if dash_end.len() >= 3 {
                        self.queue_annotation_polygon(
                            &dash_end,
                            triangle.origin,
                            triangle.gradient,
                        );
                    }
                }
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
        let mut position_offset = 0;
        for (ring_index, ring) in std::iter::once(&face.vertices)
            .chain(face.holes.iter())
            .enumerate()
        {
            let projected: Option<Vec<_>> = cached.positions
                [position_offset..position_offset + ring.len()]
                .iter()
                .map(|&point| self.view.project(point))
                .collect();
            position_offset += ring.len();
            let winding = projected.as_ref().map_or(0., |points| {
                let area: f64 = (0..points.len())
                    .map(|i| points[i].perp_dot(points[(i + 1) % points.len()]))
                    .sum();
                area.signum() * if ring_index == 0 { 1. } else { -1. }
            });
            for i in 0..ring.len() {
                let a = ring[i];
                let b = ring[(i + 1) % ring.len()];
                let inward = projected.as_ref().map_or(DVec2::ZERO, |points| {
                    let direction = points[(i + 1) % points.len()] - points[i];
                    DVec2::new(-direction.y, direction.x).normalize_or_zero() * winding
                });
                self.edge_surfaces
                    .entry((a.min(b), a.max(b)))
                    .or_default()
                    .push(EdgeSurface { gradient, inward });
            }
        }
    }
    #[cfg(test)]
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

    fn draw_face(&mut self, face: &FaceDraw<'_>) {
        self.set_entity(Some(face.face.id));
        self.face_edges(face.face, face.cached);
        if let Some((shadow, shaded)) = face.shadow {
            if self.display.shadow_antialiasing {
                self.shadow_receiver(&face.cached.triangles, face.color, shaded);
            } else {
                self.shadow_face(&face.cached.triangles, shadow, face.color, shaded);
            }
        } else {
            for &triangle in &face.cached.triangles {
                self.face_triangle(triangle, face.color, true, true);
            }
        }
    }

    pub(crate) fn draw_faces(&mut self, faces: &[FaceDraw<'_>]) {
        #[cfg(not(target_arch = "wasm32"))]
        if faces.len() >= 64 {
            let workers = crate::viewport_workers::parallelism();
            if workers > 1 {
                let chunks: Vec<_> = faces
                    .chunks(faces.len().div_ceil(workers))
                    .map(|chunk| {
                        let mut mesh = Self::new(&self.joint_surfaces.camera, self.size);
                        mesh.display = self.display.clone();
                        (chunk, mesh)
                    })
                    .collect();
                let profiling = crate::app::viewport_benchmark::profiling();
                let meshes = crate::viewport_workers::map(
                    crate::viewport_workers::Kind::Render,
                    chunks,
                    |(chunk, mut mesh)| {
                        crate::app::viewport_benchmark::profile_worker(profiling, || {
                            for face in chunk {
                                mesh.draw_face(face);
                            }
                            mesh
                        })
                    },
                );
                for (mut mesh, timings) in meshes {
                    crate::app::viewport_benchmark::absorb_worker(timings);
                    let base = self.vertices.len() as u32;
                    let index_base = self.indices.len() as u32;
                    self.vertices.append(&mut mesh.vertices);
                    self.indices.extend(mesh.indices.iter().map(|i| base + i));
                    for range in &mesh.triangle_ranges {
                        let range = range.start + index_base..range.end + index_base;
                        if let Some(previous) = self
                            .triangle_ranges
                            .last_mut()
                            .filter(|previous| previous.end == range.start)
                        {
                            previous.end = range.end;
                        } else {
                            self.triangle_ranges.push(range);
                        }
                    }
                    self.shadow_vertices.append(&mut mesh.shadow_vertices);
                    for triangle in mesh.occlusion.triangles.iter() {
                        self.occlusion.entity = triangle.entity;
                        self.occlusion.insert(triangle.points);
                    }
                    for (key, mut surfaces) in mesh.edge_surfaces {
                        self.edge_surfaces
                            .entry(key)
                            .or_default()
                            .append(&mut surfaces);
                    }
                }
                self.draw_shadow_masks(faces);
                return;
            }
        }
        for face in faces {
            self.draw_face(face);
        }
        self.draw_shadow_masks(faces);
    }

    pub(crate) fn draw_edges(&mut self, edges: &[EdgeDraw], extensions: bool, endpoints: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        let _profile = crate::app::viewport_benchmark::scope("edge_batch");
        let surfaces = std::mem::take(&mut self.edge_surfaces);
        self.paint_overlays_with_outlines(edges, 128, self.clip_stroke_footprints, |mesh, edge| {
            mesh.set_entity(Some(edge.id));
            let [a, b] = edge.points;
            if edge.back {
                mesh.overlay_line(a, b, 1., edge.color.gamma_multiply(0.4), true);
            }
            if edge.visible {
                if edge.overlay {
                    mesh.overlay_line(a, b, edge.width, edge.color, false);
                } else {
                    mesh.surface_line(
                        a,
                        b,
                        edge.width,
                        edge.color,
                        extensions,
                        endpoints,
                        surfaces
                            .get(&edge.key)
                            .map(Vec::as_slice)
                            .unwrap_or_default(),
                    );
                }
            }
        });
        self.edge_surfaces = surfaces;
    }

    /// Prepare independent overlay strokes concurrently, then merge in draw
    /// order. Each worker reads the same completed face/outline masks; only
    /// ordinary edges add outlines, which are merged before the selected pass.
    pub(crate) fn paint_overlays<T: Sync>(
        &mut self,
        items: &[T],
        threshold: usize,
        paint: impl Fn(&mut Mesh, &T) + Sync,
    ) {
        self.paint_overlays_with_outlines(items, threshold, true, paint);
    }

    fn paint_overlays_with_outlines<T: Sync>(
        &mut self,
        items: &[T],
        threshold: usize,
        read_outlines: bool,
        paint: impl Fn(&mut Mesh, &T) + Sync,
    ) {
        #[cfg(target_arch = "wasm32")]
        let _ = (threshold, read_outlines);
        #[cfg(not(target_arch = "wasm32"))]
        if items.len() >= threshold {
            let workers = crate::viewport_workers::parallelism();
            if workers > 1 {
                let sections = self.display.section_limits();
                if self.joint_surfaces.sections != sections {
                    self.joint_surfaces.sections = sections;
                    self.joint_surfaces.view_revision =
                        self.joint_surfaces.view_revision.wrapping_add(1);
                }
                self.occlusion.prepare_queries();
                // Ordinary edges only append outlines. Do not finish their
                // collection here: that would truncate last frame's hierarchy
                // to the axes and force a full rebuild after every edge pass.
                if read_outlines {
                    self.selection_occlusion.prepare_queries();
                }
                let inherited_outlines = if read_outlines {
                    self.selection_occlusion.triangles.len()
                } else {
                    0
                };
                let chunks: Vec<_> = items
                    .chunks(items.len().div_ceil(workers))
                    .map(|chunk| (chunk, self.overlay_worker(read_outlines)))
                    .collect();
                let profiling = crate::app::viewport_benchmark::profiling();
                let meshes = crate::viewport_workers::map(
                    crate::viewport_workers::Kind::Render,
                    chunks,
                    |(chunk, mut mesh)| {
                        crate::app::viewport_benchmark::profile_worker(profiling, || {
                            for item in chunk {
                                paint(&mut mesh, item);
                            }
                            mesh
                        })
                    },
                );
                for (mut mesh, timings) in meshes {
                    crate::app::viewport_benchmark::absorb_worker(timings);
                    let base = self.vertices.len() as u32;
                    self.vertices.append(&mut mesh.vertices);
                    self.indices.extend(mesh.indices.iter().map(|i| base + i));
                    self.annotation_occluders
                        .append(&mut mesh.annotation_occluders);
                    self.visible_joint_surfaces
                        .append(&mut mesh.visible_joint_surfaces);
                    for triangle in &mesh.selection_occlusion.triangles[inherited_outlines..] {
                        self.selection_occlusion.entity = triangle.entity;
                        self.selection_occlusion.insert(triangle.points);
                    }
                    for owner in &mesh.joint_surfaces.used {
                        self.joint_surfaces.used.insert(*owner);
                        if let Some(entry) = mesh.joint_surfaces.entries.remove(owner) {
                            self.joint_surfaces.entries.insert(*owner, entry);
                        }
                    }
                }
                return;
            }
        }
        for item in items {
            paint(self, item);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn overlay_worker(&self, read_outlines: bool) -> Self {
        let mut mesh = Self::new(&self.joint_surfaces.camera, self.size);
        mesh.display = self.display.clone();
        mesh.pixels_per_point = self.pixels_per_point;
        mesh.gpu_edge_occlusion = self.gpu_edge_occlusion;
        mesh.record_annotation_occluders = self.record_annotation_occluders;
        mesh.clip_stroke_footprints = self.clip_stroke_footprints;
        mesh.occlusion = self.occlusion.query_copy();
        if read_outlines {
            mesh.selection_occlusion = self.selection_occlusion.query_copy();
        }
        mesh.joint_surfaces = self.joint_surfaces.clone();
        mesh.joint_surfaces.used.clear();
        mesh.joint_guide_bounds = self.joint_guide_bounds.clone();
        mesh
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
        surfaces: &[EdgeSurface],
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        let _profile = crate::app::viewport_benchmark::scope("model_edges");
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
            let gpu_guard = (self.gpu_edge_occlusion && !self.clip_stroke_footprints).then(|| {
                let direction = (b - a).truncate().normalize_or_zero();
                let normal = DVec2::new(-direction.y, direction.x);
                let fringe = (normal.x.abs() + normal.y.abs()) / self.pixels_per_point;
                let offset = normal * (width + fringe) * 0.5;
                surfaces
                    .iter()
                    .map(|surface| surface.gradient.dot(offset).abs())
                    .fold(0_f64, f64::max)
                    + 4. * f64::from(f32::EPSILON)
            });
            let visible = self.occlusion.visible_intervals_with_guard(a, b, gpu_guard);
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
        surfaces: &[EdgeSurface],
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        let _profile = crate::app::viewport_benchmark::scope("edge_emit");
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
        // Bounded faces and shared creases need a center seam. Each half
        // follows only the faces on that side; the axis keeps its true depth.
        let (strip, indices): (&[(usize, f64)], &[u32]) =
            if surfaces.len() > 1 || surfaces.iter().any(|surface| surface.inward != DVec2::ZERO) {
                (
                    &[(0, 1.), (0, 0.), (1, 1.), (1, 0.), (0, -1.), (1, -1.)],
                    &[0, 1, 2, 2, 1, 3, 1, 4, 3, 3, 4, 5],
                )
            } else {
                (&[(0, 1.), (0, -1.), (1, 1.), (1, -1.)], &[0, 1, 2, 2, 1, 3])
            };
        if self.clip_stroke_footprints && depths.iter().all(|&z| z > 0. && z < 1.) {
            // Clipping just the centerline leaves corners of a wide or oblique
            // selection strip protruding across the foreground edge. Clip each
            // triangle's complete footprint in f64 before GPU depth rounding.
            let projected: Vec<_> = strip
                .iter()
                .map(|&(endpoint, lateral)| {
                    let offset = n * lateral;
                    let depth_offset = surface_depth_offset(offset, surfaces);
                    ([a, b][endpoint] + offset).extend(depths[endpoint] + depth_offset)
                })
                .collect();
            // Each side is planar. Clip its quad once rather than querying
            // the same visibility hierarchies for both triangle halves.
            let quads: &[[usize; 4]] = if projected.len() == 6 {
                &[[0, 1, 3, 2], [1, 4, 5, 3]]
            } else { &[[0, 1, 3, 2]] };
            for quad in quads {
                let points = quad.map(|index| projected[index]);
                let Some(gradient) = depth_gradient([points[0], points[1], points[2]]) else {
                    continue;
                };
                let origin = points[0];
                self.clipped_stroke_polygon(
                    points.map(DVec3::truncate).to_vec(),
                    origin,
                    gradient,
                    a,
                    n,
                    color,
                );
            }
            return;
        }
        let base = self.vertices.len() as u32;
        self.indices.extend(indices.iter().map(|&i| base + i));
        // Share ordinary outlines with annotation clipping instead of merging
        // them into the face hierarchy and rebuilding it a second time.
        // Selected strokes and cut-guide dashes have their own later masks.
        if color.a() == 255 && depths.iter().all(|&z| z > 0. && z < 1.) {
            let projected: Vec<_> = strip
                .iter()
                .map(|&(endpoint, lateral)| {
                    let offset = n * lateral;
                    let depth_offset = surface_depth_offset(offset, surfaces);
                    ([a, b][endpoint] + offset).extend(depths[endpoint] + depth_offset)
                })
                .collect();
            for triangle in indices.chunks_exact(3) {
                self.selection_occlusion.insert([
                    projected[triangle[0] as usize],
                    projected[triangle[1] as usize],
                    projected[triangle[2] as usize],
                ]);
                if self.record_annotation_occluders && !self.gpu_edge_occlusion {
                    self.annotation_occluders.push([
                        projected[triangle[0] as usize],
                        projected[triangle[1] as usize],
                        projected[triangle[2] as usize],
                    ]);
                }
            }
        }
        let color = color.to_array();
        for &(endpoint, lateral) in strip {
            let p = [a, b][endpoint] + n * lateral;
            self.vertices.push(Vertex {
                dash_phase: -1.,
                position: [
                    (p.x / self.size.x * 2. - 1.) as f32,
                    (1. - p.y / self.size.y * 2.) as f32,
                    stroke_depth(depths[endpoint], n * lateral, surfaces),
                ],
                color,
                lateral: lateral as f32,
                shadow_color: [0; 4],
            });
        }
    }

    fn clipped_stroke_polygon(
        &mut self,
        polygon: Vec<DVec2>,
        origin: DVec3,
        gradient: DVec2,
        a: DVec2,
        n: DVec2,
        color: Color32,
    ) {
        for polygon in self.visible_stroke_polygons_with_depth(polygon, origin, gradient, self.gpu_edge_occlusion) {
            self.emit_stroke_polygon(&polygon, origin, gradient, a, n, color);
        }
    }

    fn visible_stroke_polygons(
        &mut self,
        polygon: Vec<DVec2>,
        origin: DVec3,
        gradient: DVec2,
    ) -> Vec<Vec<DVec2>> {
        // Annotation shapes paint in egui's screen layer and cannot defer any
        // occlusion to the viewport depth buffer.
        self.visible_stroke_polygons_with_depth(polygon, origin, gradient, false)
    }

    fn visible_stroke_polygons_with_depth(
        &mut self, polygon: Vec<DVec2>, origin: DVec3, gradient: DVec2, gpu_depth: bool,
    ) -> Vec<Vec<DVec2>> {
        #[cfg(not(target_arch = "wasm32"))]
        let _profile = crate::app::viewport_benchmark::scope("stroke_clip");
        let polygons = if gpu_depth {
            let candidates = self.occlusion.precision_candidates(
                &polygon, origin, gradient, true, &Default::default());
            self.occlusion.visible_surface_candidates(polygon, origin, gradient, &candidates)
        } else {
            self.occlusion.visible_surface_polygons(polygon, origin, gradient)
        };
        // Ordinary outlines occlude strokes only where their real
        // projected surface is nearer, including across wide strokes.
        polygons
            .into_iter()
            .flat_map(|polygon| {
                if gpu_depth {
                    let candidates = self.selection_occlusion.precision_candidates(
                        &polygon, origin, gradient, true, &Default::default());
                    self.selection_occlusion.visible_surface_candidates(polygon, origin, gradient, &candidates)
                } else {
                    self.selection_occlusion.visible_surface_polygons(polygon, origin, gradient)
                }
            })
            .collect()
    }

    fn emit_stroke_polygon(
        &mut self,
        polygon: &[DVec2],
        origin: DVec3,
        gradient: DVec2,
        a: DVec2,
        n: DVec2,
        color: Color32,
    ) {
        self.emit_stroke_geometry(polygon, origin, gradient, a, n, color, None);
        if color.a() == 255 && self.record_annotation_occluders {
            self.queue_annotation_polygon(polygon, origin, gradient);
        }
    }

    fn emit_stroke_geometry(
        &mut self,
        polygon: &[DVec2],
        origin: DVec3,
        gradient: DVec2,
        a: DVec2,
        n: DVec2,
        color: Color32,
        dash_axis: Option<DVec2>,
    ) {
        let base = self.vertices.len() as u32;
        for &p in polygon {
            let depth = origin.z + gradient.dot(p - origin.truncate());
            self.vertices.push(Vertex {
                position: [
                    (p.x / self.size.x * 2. - 1.) as f32,
                    (1. - p.y / self.size.y * 2.) as f32,
                    (depth as f32).next_down().max(0.),
                ],
                color: color.to_array(),
                lateral: ((p - a).dot(n) / n.length_squared()) as f32,
                shadow_color: [0; 4],
                dash_phase: dash_axis.map_or(-1., |axis| (p - a).dot(axis).max(0.) as f32),
            });
        }
        for i in 1..polygon.len().saturating_sub(1) {
            self.indices
                .extend([base, base + i as u32, base + i as u32 + 1]);
        }
    }

    fn queue_annotation_polygon(&mut self, polygon: &[DVec2], origin: DVec3, gradient: DVec2) {
        for i in 1..polygon.len().saturating_sub(1) {
            self.annotation_occluders.push(
                [polygon[0], polygon[i], polygon[i + 1]]
                    .map(|p| p.extend(origin.z + gradient.dot(p - origin.truncate()))),
            );
        }
    }

    pub fn world_line(&mut self, _camera: &Camera, a: DVec3, b: DVec3, width: f64, color: Color32) {
        self.styled_line(a, b, width, color, false, false);
    }
    /// Reference axes have world depth, but remain visible reference geometry
    /// when model section cuts and fog are enabled.
    pub fn axis_line(&mut self, camera: &Camera, a: DVec3, b: DVec3, width: f64, color: Color32) {
        let fade = ReferenceFade::new(camera);
        let inverse = camera.matrix(self.size).inverse();
        let depths = [fade.depth(a), fade.depth(b)];
        let mut cuts = vec![0., 1.];
        // Match the grid's short depth bands, anchoring full opacity at origin
        // depth. This also gives parallel views a smooth reciprocal-depth fade.
        if (depths[1] - depths[0]).abs() > 1e-12 {
            let mut depth = fade.origin_depth;
            while depth < fade.origin_depth * 257. {
                let t = (depth - depths[0]) / (depths[1] - depths[0]);
                if t > 0. && t < 1. {
                    cuts.push(t);
                }
                depth *= 1.05;
            }
        }
        cuts.sort_by(f64::total_cmp);
        for pair in cuts.windows(2) {
            let Some((start, end)) = self
                .view
                .segment_depth(a.lerp(b, pair[0]), a.lerp(b, pair[1]))
            else {
                continue;
            };
            // Recover depth AFTER viewport/near clipping so opacity stays stable
            // as a vanishing point or the camera crosses a viewport boundary.
            let opacity = [start, end].map(|p| {
                let world = inverse
                    * glam::DVec4::new(
                        p.x / self.size.x * 2. - 1.,
                        1. - p.y / self.size.y * 2.,
                        p.z,
                        1.,
                    );
                ReferenceFade::opacity(fade.depth(world.truncate() / world.w), fade.origin_depth)
            });
            let first = self.vertices.len();
            self.line(start, end, width, color);
            // Ordinary axis strokes have two vertices at each endpoint. Keep
            // their existing depth and antialias footprint; vary only color.
            for (i, vertex) in self.vertices[first..].iter_mut().enumerate() {
                vertex.color = color.gamma_multiply(opacity[i / 2]).to_array();
            }
        }
    }
    /// Keep projected depth after centerline clipping so the complete stroke
    /// can also be clipped against foreground surfaces before painting.
    pub fn occlude_annotations_with_edges(&mut self) {
        for triangle in self.annotation_occluders.drain(..) {
            self.occlusion.insert(triangle);
        }
    }

    pub fn begin_selected_edges(&mut self) {
        // Selection has its own outline mask. Keep the face hierarchy free of
        // edge triangles until annotations actually need them; otherwise every
        // selected stroke clips against the same ordinary edges twice.
        self.clip_stroke_footprints = true;
    }

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
        let mut visible = self.occlusion.visible_intervals(a, b);
        if self.gpu_edge_occlusion {
            let outlines = self.selection_occlusion.visible_intervals(a, b);
            let mut intersections = Vec::new();
            for &(lo, hi) in &visible {
                for &(start, end) in &outlines {
                    let (start, end) = (start.max(lo), end.min(hi));
                    if start < end {
                        intersections.push((start, end));
                    }
                }
            }
            self.selection_occlusion.spans = outlines;
            visible.clear();
            visible.extend(intersections);
        }
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
        let segment = [screen(a), screen(b)];
        let shape = egui::Shape::line_segment(segment, stroke);
        let bounds = shape
            .visual_bounding_rect()
            .expand(1. / painter.ctx().pixels_per_point());
        let local = bounds.translate(-viewport.min.to_vec2());
        let [start, end] = segment.map(|p| {
            let p = p - viewport.min;
            DVec2::new(f64::from(p.x), f64::from(p.y))
        });
        let polygon = if start.distance_squared(end) > 1e-12 {
            annotation_stroke_footprint(start, end, f64::from(stroke.width) * 0.5
                + 2. / f64::from(painter.ctx().pixels_per_point()))
        } else {
            [local.left_top(), local.right_top(), local.right_bottom(), local.left_bottom()]
                .map(|p| DVec2::new(f64::from(p.x), f64::from(p.y))).into()
        };
        let delta = (b - a).truncate();
        let gradient = if delta.length_squared() > 1e-12 {
            delta * ((b.z - a.z) / delta.length_squared())
        } else {
            DVec2::ZERO
        };
        let visible = self.visible_annotation_polygons(polygon.clone(), a, gradient);
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
        let visible = self.visible_annotation_polygons(polygon.clone(), point, DVec2::ZERO);
        if visible.len() == 1 && visible[0] == polygon {
            return vec![shape];
        }
        if visible.is_empty() {
            return vec![];
        }
        clipped_annotation_shapes(painter, viewport, shape, &visible)
    }

    fn visible_annotation_polygons(
        &mut self,
        polygon: Vec<DVec2>,
        origin: DVec3,
        gradient: DVec2,
    ) -> Vec<Vec<DVec2>> {
        if self.gpu_edge_occlusion {
            self.visible_stroke_polygons(polygon, origin, gradient)
        } else {
            self.occlusion
                .visible_surface_polygons(polygon, origin, gradient)
        }
    }

    pub fn background_grid_line(
        &mut self,
        points: [DVec2; 2],
        opacity: [f32; 2],
        major: bool,
        color: Color32,
    ) {
        let [a, b] = points;
        let normal = DVec2::new(-(b - a).y, (b - a).x).normalize();
        let width = if major { 0.8 } else { 0.55 };
        let fringe = (normal.x.abs() + normal.y.abs()) / self.pixels_per_point;
        let offset = normal * (width + fringe) * 0.5;
        let base = self.vertices.len() as u32;
        for (endpoint, lateral) in [(0, 1.), (0, -1.), (1, 1.), (1, -1.)] {
            let p = [a, b][endpoint] + offset * lateral;
            self.vertices.push(Vertex {
                dash_phase: -1.,
                position: [
                    (p.x / self.size.x * 2. - 1.) as f32,
                    (1. - p.y / self.size.y * 2.) as f32,
                    1.,
                ],
                color: color.gamma_multiply(opacity[endpoint]).to_array(),
                lateral: lateral as f32,
                shadow_color: [0; 4],
            });
        }
        self.indices
            .extend([base, base + 1, base + 2, base + 2, base + 1, base + 3]);
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
        // Keep the receiving face's original hard silhouette. Only its shadow
        // coverage is multisampled; ordinary geometry never uses that resolve.
        if self.display.shadow_antialiasing {
            self.shadow_receiver(triangles, color, shaded);
            // Replace receivers in the mask with their nonoverlapping lit and
            // shaded partitions. Testing an overlay against the original face
            // fights its independently rounded depth, especially in isometric
            // exports with many small shadow triangles.
            for (triangle, coverage) in shadow
                .lit
                .iter()
                .map(|&triangle| (triangle, [0, 0, 0, 255]))
                .chain(shadow.shaded.iter().map(|&triangle| (triangle, [255; 4])))
            {
                Self::project_shadow_triangle(&self.view, &self.display, self.size,
                    triangle, coverage, &mut self.shadow_vertices);
            }
        } else {
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
    }
    fn shadow_receiver(&mut self, triangles: &[[DVec3; 3]], color: Color32, shaded: Color32) {
        for &triangle in triangles {
            let start = self.vertices.len();
            self.face_triangle(triangle, color, true, true);
            let shadow_color = self
                .fog_color(shaded, triangle.iter().sum::<DVec3>() / 3.)
                .to_array();
            for vertex in &mut self.vertices[start..] {
                vertex.shadow_color = shadow_color;
            }
        }
    }

    fn project_shadow_triangle(
        view: &CameraView,
        display: &ketchup_core::DisplaySettings,
        size: DVec2,
        triangle: [DVec3; 3],
        coverage: [u8; 4],
        vertices: &mut Vec<Vertex>,
    ) {
        let (polygon, len) = display.section_triangle(triangle);
        for i in 1..len.saturating_sub(1) {
            view.for_each_triangle_depth([polygon[0], polygon[i], polygon[i + 1]], |triangle| {
                for p in triangle {
                    vertices.push(Vertex {
                        dash_phase: -1.,
                        position: [
                            (p.x / size.x * 2. - 1.) as f32,
                            (1. - p.y / size.y * 2.) as f32,
                            p.z.clamp(0., 1.) as f32,
                        ],
                        color: coverage,
                        lateral: 0.,
                        shadow_color: [0; 4],
                    });
                }
            });
        }
    }

    fn draw_shadow_masks(&mut self, faces: &[FaceDraw<'_>]) {
        if !self.display.shadow_antialiasing {
            return;
        }
        let triangles: Vec<_> = faces
            .iter()
            .filter_map(|face| face.shadow)
            .flat_map(|(shadow, _)| {
                shadow
                    .lit
                    .iter()
                    .map(|triangle| (triangle, [0, 0, 0, 255]))
                    .chain(shadow.shaded.iter().map(|triangle| (triangle, [255; 4])))
            })
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        if triangles.len() >= 64 && crate::viewport_workers::parallelism() > 1 {
            let chunks = triangles
                .chunks(
                    triangles
                        .len()
                        .div_ceil(crate::viewport_workers::parallelism()),
                )
                .map(|chunk| (chunk, self.joint_surfaces.camera.view(self.size)))
                .collect();
            let display = &self.display;
            let size = self.size;
            let profiling = crate::app::viewport_benchmark::profiling();
            let results = crate::viewport_workers::map(
                crate::viewport_workers::Kind::Render,
                chunks,
                |(chunk, view)| {
                    crate::app::viewport_benchmark::profile_worker(profiling, || {
                        let _profile =
                            crate::app::viewport_benchmark::scope("shadow_projection_cpu");
                        let mut vertices = Vec::with_capacity(chunk.len() * 3);
                        for &(triangle, coverage) in chunk {
                            Self::project_shadow_triangle(
                                &view,
                                display,
                                size,
                                *triangle,
                                coverage,
                                &mut vertices,
                            );
                        }
                        vertices
                    })
                },
            );
            for (mut vertices, timings) in results {
                crate::app::viewport_benchmark::absorb_worker(timings);
                self.shadow_vertices.append(&mut vertices);
            }
            return;
        }
        for (triangle, coverage) in triangles {
            Self::project_shadow_triangle(
                &self.view,
                &self.display,
                self.size,
                *triangle,
                coverage,
                &mut self.shadow_vertices,
            );
        }
    }

    fn face_triangle(&mut self, triangle: [DVec3; 3], color: Color32, draw: bool, occlude: bool) {
        let start = self.indices.len() as u32;
        let (polygon, len) = self.display.section_triangle(triangle);
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
                        dash_phase: -1.,
                        position: [
                            (p.x / size.x * 2. - 1.) as f32,
                            (1. - p.y / size.y * 2.) as f32,
                            p.z.clamp(0., 1.) as f32,
                        ],
                        color,
                        lateral: 0.,
                        shadow_color: [0; 4],
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
        cache.selection_occlusion = std::mem::take(&mut self.selection_occlusion);
        cache.annotation_occluders = std::mem::take(&mut self.annotation_occluders);
        self.edge_surfaces
            .retain(|_, surfaces| !surfaces.is_empty());
        cache.edge_surfaces = std::mem::take(&mut self.edge_surfaces);
        let callback = self.retain(cache);
        painter.add(egui_wgpu::Callback::new_paint_callback(
            rect,
            SharedScene(callback),
        ));
    }

    // Compare the actual render output, including previews and annotations.
    // Even rebuilt frames can keep unchanged meshes resident on the GPU,
    // including those using the uncached preview and annotation paths.
    fn retain(mut self, cache: &mut SceneCache) -> Arc<SceneCallback> {
        let used = &self.joint_surfaces.used;
        self.joint_surfaces
            .entries
            .retain(|owner, _| used.contains(owner));
        self.joint_surfaces.used.clear();
        cache.joint_surfaces = std::mem::take(&mut self.joint_surfaces);
        self.normalize_parallel_depth();
        let candidate = Arc::new(self.into_callback());
        if let Some(previous) = &cache.retained {
            if bytemuck::cast_slice::<Vertex, u8>(&previous.vertices)
                == bytemuck::cast_slice::<Vertex, u8>(&candidate.vertices)
                && previous.indices == candidate.indices
                && previous.triangle_ranges == candidate.triangle_ranges
                && previous.antialiasing == candidate.antialiasing
                && previous.target_size == candidate.target_size
                && previous.shadow_range == candidate.shadow_range
            {
                return Arc::clone(previous);
            }
        }
        cache.retained = Some(Arc::clone(&candidate));
        candidate
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
        for vertex in self.vertices.iter().chain(&self.shadow_vertices) {
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
        for vertex in self.vertices.iter_mut().chain(&mut self.shadow_vertices) {
            let depth = f64::from(vertex.position[2]);
            // Depths 0 and 1 are reserved for overlays and background lines.
            if depth > 0. && depth < 1. {
                vertex.position[2] = (0.5 + 0.8 * (depth - center) / span) as f32;
            }
        }
    }

    fn into_callback(mut self) -> SceneCallback {
        // Rendering already draws all faces before all strokes. Pack the index
        // buffer in that same stable order once, rather than issuing one draw
        // per interleaved fragment on every retained frame (and twice for edges).
        // Vertex data, blending order within each pass, and depth stay unchanged.
        if self.triangle_ranges.len() > 1
            || self
                .triangle_ranges
                .first()
                .is_some_and(|range| range.start != 0)
        {
            let mut packed = Vec::with_capacity(self.indices.len());
            for range in &self.triangle_ranges {
                packed.extend_from_slice(&self.indices[range.start as usize..range.end as usize]);
            }
            let face_end = packed.len() as u32;
            let mut start = 0;
            for range in &self.triangle_ranges {
                packed.extend_from_slice(&self.indices[start..range.start as usize]);
                start = range.end as usize;
            }
            packed.extend_from_slice(&self.indices[start..]);
            self.indices = packed;
            self.triangle_ranges.clear();
            self.triangle_ranges.push(0..face_end);
        }
        let shadow_start = self.indices.len() as u32;
        let vertex_start = self.vertices.len() as u32;
        self.indices.extend(vertex_start..vertex_start + self.shadow_vertices.len() as u32);
        self.vertices.append(&mut self.shadow_vertices);
        let shadow_range = shadow_start..self.indices.len() as u32;
        SceneCallback {
            shadow_range,
            vertices: self.vertices,
            indices: self.indices,
            triangle_ranges: self.triangle_ranges,
            buffer_pool: self.buffer_pool,
            gpu_pool: self.gpu_pool,
            gpu_buffers: OnceLock::new(),
            antialiasing: self.display.shadow_antialiasing,
            target_size: [self.size.x, self.size.y]
                .map(|v| (v * self.pixels_per_point).round().max(1.) as u32),
            antialiased: OnceLock::new(),
        }
    }
}

// Test centerlines in f64 before applying screen-space width or rounding to
// GPU depth. A steep incident plane must never resurrect an occluded edge.
// A hierarchy rejects whole groups of faces by position and depth. Queries
// subtract hidden spans immediately and stop once the edge is fully hidden.
#[derive(Default)]
struct Occlusion {
    entity: Option<u64>,
    outline_order: bool,
    // Workers share the completed hierarchy. Mutations occur only while
    // collecting the next mask, through copy-on-write ownership.
    triangles: Arc<Vec<OccludingTriangle>>,
    indices: Arc<Vec<usize>>,
    nodes: Arc<Vec<OcclusionNode>>,
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
    #[cfg(test)]
    polygon_queries: usize,
}

struct CachedVisibility {
    gpu_guard: Option<f64>,
    a: DVec3,
    b: DVec3,
    revision: u64,
    spans: Vec<(f64, f64)>,
}

#[derive(Clone)]
struct OccludingTriangle {
    entity: Option<u64>,
    points: [DVec3; 3],
    gradient: DVec2,
    bounds: OcclusionBounds,
}

#[derive(Clone, Copy)]
struct OcclusionBounds {
    lo: DVec3,
    hi: DVec3,
}

struct ScreenSegment {
    a: DVec3,
    lo: DVec3,
    hi: DVec3,
    inverse: DVec2,
}
impl ScreenSegment {
    fn new(a: DVec3, b: DVec3) -> Self {
        let delta = b - a;
        Self {
            a,
            lo: a.min(b),
            hi: a.max(b),
            inverse: DVec2::new(
                if delta.x == 0. { 0. } else { delta.x.recip() },
                if delta.y == 0. { 0. } else { delta.y.recip() },
            ),
        }
    }
}
impl OcclusionBounds {
    fn overlaps(self, segment: &ScreenSegment) -> bool {
        if self.lo.z >= segment.hi.z
            || self.lo.x > segment.hi.x
            || self.hi.x < segment.lo.x
            || self.lo.y > segment.hi.y
            || self.hi.y < segment.lo.y
        {
            return false;
        }
        // Slab intersections share the reciprocals across every hierarchy node
        // and triangle tested for this edge. No per-node divisions or closures.
        let mut enter = 0_f64;
        let mut exit = 1_f64;
        for axis in 0..2 {
            if segment.inverse[axis] == 0. {
                continue;
            }
            let a = (self.lo[axis] - segment.a[axis]) * segment.inverse[axis];
            let b = (self.hi[axis] - segment.a[axis]) * segment.inverse[axis];
            enter = enter.max(a.min(b));
            exit = exit.min(a.max(b));
        }
        enter < exit
    }
}

#[derive(Clone)]
struct OcclusionNode {
    bounds: OcclusionBounds,
    start: usize,
    end: usize,
    children: Option<(usize, usize)>,
}

impl OccludingTriangle {
    fn surface_planes(&self, origin: DVec3, gradient: DVec2) -> [(DVec2, DVec2, f64); 4] {
        let [a, b, c] = self.points.map(DVec3::truncate);
        let winding = (b - a).perp_dot(c - a).signum();
        [
            (
                gradient - self.gradient,
                a,
                origin.z + gradient.dot(a - origin.truncate())
                    - self.points[0].z
                    - 64. * f64::EPSILON,
            ),
            (DVec2::new(-(b - a).y, (b - a).x) * winding, a, 0.),
            (DVec2::new(-(c - b).y, (c - b).x) * winding, b, 0.),
            (DVec2::new(-(a - c).y, (a - c).x) * winding, c, 0.),
        ]
    }

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
    #[cfg(not(target_arch = "wasm32"))]
    fn prepare_queries(&mut self) {
        self.finish_collection();
        if self.nodes.is_empty() && !self.indices.is_empty() {
            self.build(0, self.indices.len());
            self.tree_quality = self.quality();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn query_copy(&self) -> Self {
        Self {
            entity: self.entity,
            outline_order: self.outline_order,
            triangles: self.triangles.clone(),
            indices: self.indices.clone(),
            nodes: self.nodes.clone(),
            revision: self.revision,
            tree_quality: self.tree_quality,
            ..Default::default()
        }
    }

    fn begin_frame(&mut self) {
        self.insertion_cursor = Some(0);
        self.line_cursor = 0;
    }

    fn insert(&mut self, points: [DVec3; 3]) {
        let index = self.insertion_cursor.unwrap_or(self.triangles.len());
        if self
            .triangles
            .get(index)
            .is_some_and(|t| t.points == points && t.entity == self.entity)
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
            entity: self.entity,
            points,
            gradient,
            bounds,
        };
        if index < self.triangles.len() {
            Arc::make_mut(&mut self.triangles)[index] = triangle;
            self.needs_refit = true;
        } else {
            Arc::make_mut(&mut self.indices).push(index);
            Arc::make_mut(&mut self.triangles).push(triangle);
            Arc::make_mut(&mut self.nodes).clear();
        }
        if let Some(cursor) = &mut self.insertion_cursor {
            *cursor += 1;
        }
        self.revision = self.revision.wrapping_add(1);
    }

    fn finish_collection(&mut self) {
        if let Some(len) = self.insertion_cursor {
            if len < self.triangles.len() {
                Arc::make_mut(&mut self.triangles).truncate(len);
                Arc::make_mut(&mut self.indices).clear();
                Arc::make_mut(&mut self.indices).extend(0..len);
                Arc::make_mut(&mut self.nodes).clear();
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
                Arc::make_mut(&mut self.nodes)[index].bounds = bounds;
            }
            // Refit is linear during navigation. Repartition only when the
            // old grouping becomes substantially less useful at the new view.
            if self.quality() > self.tree_quality * 2. {
                Arc::make_mut(&mut self.nodes).clear();
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

    // Outline masks contain thousands of tiny triangles. Sort their screen
    // centers once, then construct bounds bottom-up, avoiding repeated bounds
    // scans and median partitioning at every hierarchy level.
    fn build_outline_order(&mut self) -> usize {
        fn spread(value: u32) -> u64 {
            let mut x = u64::from(value);
            x = (x | (x << 8)) & 0x00ff_00ff;
            x = (x | (x << 4)) & 0x0f0f_0f0f;
            x = (x | (x << 2)) & 0x3333_3333;
            (x | (x << 1)) & 0x5555_5555
        }
        let bounds = self.bounds(0, self.indices.len());
        let extent = (bounds.hi - bounds.lo).max(DVec3::splat(1e-12));
        let mut keys: Vec<_> = self.indices.iter().map(|&index| {
            let triangle = self.triangles[index].bounds;
            let p = (((triangle.lo + triangle.hi) * 0.5 - bounds.lo) / extent)
                .clamp(DVec3::ZERO, DVec3::ONE);
            let x = (p.x * 65535.) as u32;
            let y = (p.y * 65535.) as u32;
            let z = (p.z * 65535.) as u64;
            (((spread(x) | (spread(y) << 1)) << 16) | z, index)
        }).collect();
        keys.sort_unstable();
        let indices = Arc::make_mut(&mut self.indices);
        indices.clear();
        indices.extend(keys.into_iter().map(|(_, index)| index));
        Arc::make_mut(&mut self.nodes).reserve(self.indices.len().div_ceil(4));
        self.build_ordered(0, self.indices.len())
    }

    fn build_ordered(&mut self, start: usize, end: usize) -> usize {
        let index = self.nodes.len();
        Arc::make_mut(&mut self.nodes).push(OcclusionNode {
            bounds: OcclusionBounds { lo: DVec3::ZERO, hi: DVec3::ZERO }, start, end, children: None,
        });
        let (bounds, children) = if end - start > 8 {
            let middle = start + (end - start) / 2;
            let left = self.build_ordered(start, middle);
            let right = self.build_ordered(middle, end);
            let a = self.nodes[left].bounds;
            let b = self.nodes[right].bounds;
            (OcclusionBounds { lo: a.lo.min(b.lo), hi: a.hi.max(b.hi) }, Some((left, right)))
        } else { (self.bounds(start, end), None) };
        let node = &mut Arc::make_mut(&mut self.nodes)[index];
        node.bounds = bounds;
        node.children = children;
        index
    }

    fn build(&mut self, start: usize, end: usize) -> usize {
        if self.outline_order && start == 0 && end == self.indices.len() {
            return self.build_outline_order();
        }
        let bounds = self.bounds(start, end);
        let index = self.nodes.len();
        Arc::make_mut(&mut self.nodes).push(OcclusionNode {
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
            Arc::make_mut(&mut self.indices)[start..end].select_nth_unstable_by(middle - start, |&a, &b| {
                let a = self.triangles[a].bounds;
                let b = self.triangles[b].bounds;
                (a.lo[axis] + a.hi[axis]).total_cmp(&(b.lo[axis] + b.hi[axis]))
            });
            let left = self.build(start, middle);
            let right = self.build(middle, end);
            Arc::make_mut(&mut self.nodes)[index].children = Some((left, right));
        }
        index
    }

    fn visible_intervals(&mut self, a: DVec3, b: DVec3) -> Vec<(f64, f64)> {
        self.visible_intervals_with_guard(a, b, None)
    }
    fn visible_intervals_with_guard(
        &mut self,
        a: DVec3,
        b: DVec3,
        gpu_guard: Option<f64>,
    ) -> Vec<(f64, f64)> {
        self.finish_collection();
        let index = self.line_cursor;
        self.line_cursor += 1;
        let mut visible = std::mem::take(&mut self.spans);
        visible.clear();
        if let Some(cached) = self.lines.get(index) {
            if cached.a == a
                && cached.b == b
                && cached.revision == self.revision
                && cached.gpu_guard == gpu_guard
            {
                visible.extend_from_slice(&cached.spans);
                #[cfg(test)]
                {
                    self.query_work = (0, 0);
                }
                return visible;
            }
        }
        self.compute_intervals(a, b, &mut visible, gpu_guard);
        if index == self.lines.len() {
            self.lines.push(CachedVisibility {
                gpu_guard,
                a,
                b,
                revision: self.revision,
                spans: visible.clone(),
            });
        } else {
            let cached = &mut self.lines[index];
            cached.gpu_guard = gpu_guard;
            cached.a = a;
            cached.b = b;
            cached.revision = self.revision;
            cached.spans.clear();
            cached.spans.extend_from_slice(&visible);
        }
        visible
    }

    #[cfg(test)]
    fn visible_polygons(&mut self, polygon: Vec<DVec2>, depth: f64) -> Vec<Vec<DVec2>> {
        self.visible_surface_polygons(polygon, DVec3::new(0., 0., depth), DVec2::ZERO)
    }

    fn visible_surface_polygons(
        &mut self,
        polygon: Vec<DVec2>,
        origin: DVec3,
        gradient: DVec2,
    ) -> Vec<Vec<DVec2>> {
        let candidates = self.surface_candidates(&polygon, origin, gradient);
        self.visible_surface_candidates(polygon, origin, gradient, &candidates)
    }

    fn surface_candidates(
        &mut self,
        polygon: &[DVec2],
        origin: DVec3,
        gradient: DVec2,
    ) -> Vec<SurfaceCandidate> {
        self.finish_collection();
        let mut candidates = Vec::new();
        if self.triangles.is_empty() || polygon.len() < 3 {
            return candidates;
        }
        if self.nodes.is_empty() {
            self.build(0, self.indices.len());
            self.tree_quality = self.quality();
        }
        let depth_at = |p: DVec2| origin.z + gradient.dot(p - origin.truncate());
        let lo = polygon
            .iter()
            .map(|&p| p.extend(depth_at(p)))
            .reduce(DVec3::min)
            .unwrap();
        let hi = polygon
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
                candidates.push(SurfaceCandidate {
                    index,
                    bounds: None,
                    planes: None,
                });
            }
        }
        candidates
    }

    // The depth buffer clips ordinary foreground geometry. Retain f64 CPU
    // clipping only where f32 depth rounding could place a foreground face or
    // outline in the guide's depth layer. The conservative guard covers both
    // the guide's one-step bias and raster interpolation error.
    fn precision_candidates(
        &mut self,
        polygon: &[DVec2],
        origin: DVec3,
        gradient: DVec2,
        gpu_depth: bool,
        own_entities: &std::collections::BTreeSet<u64>,
    ) -> Vec<SurfaceCandidate> {
        let mut candidates = self.surface_candidates(polygon, origin, gradient);
        let mut hidden = Vec::with_capacity(8);
        let mut scratch = Vec::with_capacity(8);
        candidates.retain_mut(|candidate| {
            let triangle = &self.triangles[candidate.index];
            if triangle.entity.is_some_and(|id| own_entities.contains(&id)) {
                return false;
            }
            if gpu_depth
                && triangle.points.iter().all(|p| {
                    origin.z + gradient.dot(p.truncate() - origin.truncate()) - p.z
                        > 4. * f64::from(f32::EPSILON)
                })
            {
                return false;
            }
            let planes = triangle.surface_planes(origin, gradient);
            if planes.iter().any(|&(normal, origin, offset)| {
                polygon
                    .iter()
                    .all(|&p| normal.dot(p - origin) + offset <= 0.)
            }) {
                return false;
            }
            hidden.clear();
            hidden.extend_from_slice(polygon);
            for &(normal, origin, offset) in &planes {
                clip_polygon_into(&hidden, |p| normal.dot(p - origin) + offset, &mut scratch);
                std::mem::swap(&mut hidden, &mut scratch);
                if hidden.len() < 3 {
                    break;
                }
            }
            if gpu_depth {
                let (normal, origin, offset) = planes[0];
                clip_polygon_into(
                    &hidden,
                    |p| 4. * f64::from(f32::EPSILON) - normal.dot(p - origin) - offset,
                    &mut scratch,
                );
                std::mem::swap(&mut hidden, &mut scratch);
            }
            if !has_surface_area(&hidden) {
                return false;
            }
            candidate.bounds = Some([
                hidden.iter().copied().reduce(DVec2::min).unwrap(),
                hidden.iter().copied().reduce(DVec2::max).unwrap(),
            ]);
            candidate.planes = Some(planes);
            true
        });
        candidates
    }

    fn visible_surface_candidates(
        &mut self,
        polygon: Vec<DVec2>,
        origin: DVec3,
        gradient: DVec2,
        candidates: &[SurfaceCandidate],
    ) -> Vec<Vec<DVec2>> {
        #[cfg(test)]
        {
            self.polygon_queries += 1;
        }
        if candidates.is_empty() {
            return vec![polygon];
        }
        let bounds = [
            polygon.iter().copied().reduce(DVec2::min).unwrap(),
            polygon.iter().copied().reduce(DVec2::max).unwrap(),
        ];
        let mut visible = vec![polygon];
        let mut outside = Vec::new();
        for candidate in candidates {
            if candidate.bounds.is_some_and(|[lo, hi]| {
                hi.x < bounds[0].x || hi.y < bounds[0].y || lo.x > bounds[1].x || lo.y > bounds[1].y
            }) {
                continue;
            }
            let planes = candidate.planes.unwrap_or_else(|| self.triangles[candidate.index].surface_planes(origin, gradient));
            outside.clear();
            for polygon in visible.drain(..) {
                if planes.iter().any(|&(normal, origin, offset)| {
                    polygon
                        .iter()
                        .all(|&p| normal.dot(p - origin) + offset <= 0.)
                }) {
                    outside.push(polygon);
                    continue;
                }
                let first_piece = outside.len();
                let mut hidden = polygon.clone();
                for &(normal, origin, offset) in &planes {
                    let side = |p: DVec2| normal.dot(p - origin) + offset;
                    let (inside, piece) = split_screen_polygon(&hidden, side);
                    if !piece.is_empty() { outside.push(piece); }
                    hidden = inside;
                    if hidden.is_empty() { break; }
                }
                if hidden.is_empty() {
                    // Tangencies and misses must preserve the original convex
                    // piece, rather than fragmenting it along irrelevant planes.
                    outside.truncate(first_piece);
                    outside.push(polygon);
                }
            }
            std::mem::swap(&mut visible, &mut outside);
            if visible.is_empty() {
                break;
            }
        }
        visible
    }

    fn compute_intervals(
        &mut self,
        a: DVec3,
        b: DVec3,
        visible: &mut Vec<(f64, f64)>,
        gpu_guard: Option<f64>,
    ) {
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
        let segment = ScreenSegment::new(a, b);
        self.pending.clear();
        self.pending.push(0);
        while let Some(index) = self.pending.pop() {
            #[cfg(test)]
            {
                self.query_work.0 += 1;
            }
            let node = &self.nodes[index];
            if !node.bounds.overlaps(&segment)
                || gpu_guard.is_some_and(|guard| node.bounds.hi.z < segment.lo.z - guard)
            {
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
                if !triangle.bounds.overlaps(&segment)
                    || gpu_guard.is_some_and(|guard| triangle.bounds.hi.z < segment.lo.z - guard)
                {
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

fn has_surface_area(polygon: &[DVec2]) -> bool {
    polygon.len() >= 3
        && polygon.first().is_some_and(|&origin| {
            polygon
                .windows(2)
                .map(|edge| (edge[0] - origin).perp_dot(edge[1] - origin))
                .sum::<f64>()
                .abs()
                > 1e-12
        })
}

fn annotation_stroke_footprint(a: DVec2, b: DVec2, radius: f64) -> Vec<DVec2> {
    let along = (b - a).normalize() * radius;
    let normal = DVec2::new(-along.y, along.x);
    // Annotation mesh clipping expects a positive polygon winding.
    vec![a - along - normal, b + along - normal, b + along + normal, a - along + normal]
}

fn clip_polygon(polygon: &[DVec2], side: impl Fn(DVec2) -> f64) -> Vec<DVec2> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    let mut clipped = Vec::with_capacity(polygon.len() + 1);
    clip_polygon_into(polygon, side, &mut clipped);
    clipped
}

/// Both halves share side tests and intersections. Visibility subtraction
/// formerly clipped once to test coverage and twice more to split each plane.
fn split_screen_polygon(polygon: &[DVec2], side: impl Fn(DVec2) -> f64) -> (Vec<DVec2>, Vec<DVec2>) {
    if polygon.len() < 3 { return (Vec::new(), Vec::new()); }
    let mut inside = Vec::with_capacity(polygon.len() + 1);
    let mut outside = Vec::with_capacity(polygon.len() + 1);
    let first_distance = side(polygon[0]);
    let mut da = first_distance;
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        let db = if i + 1 == polygon.len() { first_distance } else { side(b) };
        if da >= 0. { inside.push(a); }
        if da <= 0. { outside.push(a); }
        let enters = (da >= 0.) != (db >= 0.);
        let leaves = (da <= 0.) != (db <= 0.);
        if enters || leaves {
            let point = a.lerp(b, da / (da - db));
            if enters { inside.push(point); }
            if leaves { outside.push(point); }
        }
        da = db;
    }
    if !has_surface_area(&inside) { inside.clear(); }
    if !has_surface_area(&outside) { outside.clear(); }
    (inside, outside)
}

fn clip_polygon_into(polygon: &[DVec2], side: impl Fn(DVec2) -> f64, clipped: &mut Vec<DVec2>) {
    clipped.clear();
    if polygon.len() < 3 {
        return;
    }
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
        clipped.clear();
        return;
    }
    let origin = clipped[0];
    let area = (1..clipped.len() - 1)
        .map(|i| (clipped[i] - origin).perp_dot(clipped[i + 1] - origin))
        .sum::<f64>();
    if area.abs() <= 1e-12 {
        clipped.clear();
        return;
    }
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

fn surface_depth_offset(offset: DVec2, surfaces: &[EdgeSurface]) -> f64 {
    surfaces
        .iter()
        .filter(|surface| surface.inward.dot(offset) >= -1e-12)
        .map(|surface| surface.gradient.dot(offset))
        .reduce(f64::min)
        .unwrap_or(0.)
}

fn stroke_depth(depth: f64, offset: DVec2, surfaces: &[EdgeSurface]) -> f32 {
    // Match the nearest incident surface across the strip, without moving
    // unrelated faces or giving free edges a slope-dependent allowance.
    let offset = surface_depth_offset(offset, surfaces);
    // One representable step covers rounding between independently projected
    // triangles and strokes without a world-distance or slope-dependent bias.
    ((depth + offset).clamp(0., 1.) as f32).next_down().max(0.)
}

struct Resources {
    face_pipeline: wgpu::RenderPipeline,
    edge_pipeline: wgpu::RenderPipeline,
    edge_depth_pipeline: wgpu::RenderPipeline,
    mask_base: wgpu::RenderPipeline,
    mask_shadow: wgpu::RenderPipeline,
    shadow_faces: wgpu::RenderPipeline,
}
pub fn install(cc: &eframe::CreationContext<'_>) -> bool {
    let Some(state) = cc.wgpu_render_state.as_ref() else {
        return false;
    };
    let device = &state.device;
    install_resources(device, state.target_format, &mut state.renderer.write());
    true
}

pub(crate) fn install_resources(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    renderer: &mut egui_wgpu::Renderer,
) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("KetchUp edges"),
        source: wgpu::ShaderSource::Wgsl(include_str!("edges.wgsl").into()),
    });
    let face_pipeline = create_pipeline(device, &shader, format, PipelinePass::Faces);
    let edge_pipeline = create_pipeline(device, &shader, format, PipelinePass::Edges);
    let edge_depth_pipeline = create_pipeline(device, &shader, format, PipelinePass::EdgeDepth);
    let mask_base = create_pipeline_with_samples(
        device,
        &shader,
        wgpu::TextureFormat::Rgba8Unorm,
        PipelinePass::MaskBase,
        4,
    );
    let mask_shadow = create_pipeline_with_samples(
        device,
        &shader,
        wgpu::TextureFormat::Rgba8Unorm,
        PipelinePass::MaskShadow,
        4,
    );
    let shadow_faces = create_pipeline(device, &shader, format, PipelinePass::ShadowFaces);
    renderer.callback_resources.insert(Resources {
        face_pipeline,
        edge_pipeline,
        edge_depth_pipeline,
        mask_base,
        mask_shadow,
        shadow_faces,
    });
}
#[derive(Clone, Copy, PartialEq)]
enum PipelinePass {
    Faces,
    EdgeDepth,
    Edges,
    MaskBase,
    MaskShadow,
    ShadowFaces,
}

fn create_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    pass: PipelinePass,
) -> wgpu::RenderPipeline {
    create_pipeline_with_samples(device, shader, format, pass, 1)
}

fn create_pipeline_with_samples(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    pass: PipelinePass,
    samples: u32,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(match pass {
            PipelinePass::Faces => "KetchUp filled faces",
            PipelinePass::EdgeDepth => "KetchUp opaque edge depth",
            PipelinePass::Edges => "KetchUp antialiased edges",
            PipelinePass::MaskBase => "KetchUp shadow receiver depth",
            PipelinePass::MaskShadow => "KetchUp shadow coverage",
            PipelinePass::ShadowFaces => "KetchUp faces with antialiased shadows",
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
                attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Unorm8x4,2=>Float32,3=>Unorm8x4,4=>Float32],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(match pass {
                PipelinePass::EdgeDepth => "fragment_edge_depth",
                PipelinePass::MaskBase => "fragment_mask_base",
                PipelinePass::MaskShadow => "fragment_mask_shadow",
                PipelinePass::ShadowFaces => "fragment_shadow_faces",
                _ => "fragment_main",
            }),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: if pass == PipelinePass::EdgeDepth {
                    wgpu::ColorWrites::empty()
                } else {
                    wgpu::ColorWrites::ALL
                },
            })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            // Only opaque stroke cores contribute depth; fringes blend later.
            depth_write_enabled: Some(pass != PipelinePass::Edges),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    })
}

struct AntialiasedScene {
    bind_group: wgpu::BindGroup,
}

struct SceneCallback {
    shadow_range: std::ops::Range<u32>,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    triangle_ranges: Vec<std::ops::Range<u32>>,
    buffer_pool: Option<BufferPool>,
    gpu_pool: GpuBufferPool,
    gpu_buffers: OnceLock<GpuBuffers>,
    antialiasing: bool,
    target_size: [u32; 2],
    antialiased: OnceLock<AntialiasedScene>,
}

struct SharedScene(Arc<SceneCallback>);

impl egui_wgpu::CallbackTrait for SharedScene {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        self.0.prepare(device, queue, screen, encoder, resources)
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        self.0.paint(info, pass, resources);
    }
}

impl Drop for SceneCallback {
    fn drop(&mut self) {
        if let Some(buffers) = self.gpu_buffers.take() {
            if let Ok(mut pool) = self.gpu_pool.lock() {
                if pool.len() < 3 {
                    pool.push(buffers);
                }
            }
        }
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
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(r) = resources.get::<Resources>() else {
            return vec![];
        };
        // Every callback owns its buffers until egui releases it. All prepare
        // calls precede all paint calls, so a shared buffer would render only
        // the last camera in every pane. Pool allocations between frames.
        self.gpu_buffers.get_or_init(|| {
            let vertex_bytes = (self.vertices.len() * std::mem::size_of::<Vertex>()) as u64;
            let index_bytes = (self.indices.len() * std::mem::size_of::<u32>()) as u64;
            let cached = self.gpu_pool.lock().ok().and_then(|mut pool| pool.pop());
            let create = |size: u64, usage, label| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: size.max(4096).next_power_of_two(),
                    usage,
                    mapped_at_creation: false,
                })
            };
            let (vertices, indices) = if let Some(cached) = cached {
                (Some(cached.vertices), Some(cached.indices))
            } else {
                (None, None)
            };
            let buffers = GpuBuffers {
                vertices: vertices
                    .filter(|buffer| buffer.size() >= vertex_bytes)
                    .unwrap_or_else(|| {
                        create(
                            vertex_bytes,
                            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                            "KetchUp vertices",
                        )
                    }),
                indices: indices
                    .filter(|buffer| buffer.size() >= index_bytes)
                    .unwrap_or_else(|| {
                        create(
                            index_bytes,
                            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                            "KetchUp indices",
                        )
                    }),
            };
            if !self.vertices.is_empty() {
                queue.write_buffer(&buffers.vertices, 0, bytemuck::cast_slice(&self.vertices));
            }
            if !self.indices.is_empty() {
                queue.write_buffer(&buffers.indices, 0, bytemuck::cast_slice(&self.indices));
            }
            buffers
        });
        if self.antialiasing && !self.shadow_range.is_empty() {
            // Render once per retained scene. Each pane owns its resolve texture,
            // so preparing another camera cannot overwrite this pane's image.
            self.antialiased.get_or_init(|| {
                let limit = device.limits().max_texture_dimension_2d;
                let [width, height] = self.target_size.map(|v| v.clamp(1, limit));
                let size = wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                };
                let texture = |format, sample_count, usage| {
                    device
                        .create_texture(&wgpu::TextureDescriptor {
                            label: Some("KetchUp antialiased shadow mask"),
                            size,
                            mip_level_count: 1,
                            sample_count,
                            dimension: wgpu::TextureDimension::D2,
                            format,
                            usage,
                            view_formats: &[],
                        })
                        .create_view(&Default::default())
                };
                let color = texture(
                    wgpu::TextureFormat::Rgba8Unorm,
                    4,
                    wgpu::TextureUsages::RENDER_ATTACHMENT,
                );
                let resolved = texture(
                    wgpu::TextureFormat::Rgba8Unorm,
                    1,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                );
                let depth = texture(
                    wgpu::TextureFormat::Depth32Float,
                    4,
                    wgpu::TextureUsages::RENDER_ATTACHMENT,
                );
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("KetchUp 4x antialiasing"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &color,
                            depth_slice: None,
                            resolve_target: Some(&resolved),
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Discard,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &depth,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.),
                                store: wgpu::StoreOp::Discard,
                            }),
                            stencil_ops: None,
                        }),
                        ..Default::default()
                    });
                    let buffers = self.gpu_buffers.get().unwrap();
                    pass.set_vertex_buffer(0, buffers.vertices.slice(..));
                    pass.set_index_buffer(buffers.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.set_pipeline(&r.mask_base);
                    for range in &self.triangle_ranges {
                        pass.draw_indexed(range.clone(), 0, 0..1);
                    }
                    pass.set_pipeline(&r.mask_shadow);
                    pass.draw_indexed(self.shadow_range.clone(), 0, 0..1);
                }
                let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("KetchUp resolved viewport"),
                    layout: &r.shadow_faces.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&resolved),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                    ],
                });
                AntialiasedScene { bind_group }
            });
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
        if let Some(scene) = self.antialiased.get() {
            pass.set_bind_group(0, &scene.bind_group, &[]);
            self.draw(
                pass,
                [&r.shadow_faces, &r.edge_depth_pipeline, &r.edge_pipeline],
            );
        } else {
            self.draw(
                pass,
                [&r.face_pipeline, &r.edge_depth_pipeline, &r.edge_pipeline],
            );
        }
    }
}

impl SceneCallback {
    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, pipelines: [&wgpu::RenderPipeline; 3]) {
        let Some(buffers) = self.gpu_buffers.get() else {
            return;
        };
        pass.set_vertex_buffer(0, buffers.vertices.slice(..));
        pass.set_index_buffer(buffers.indices.slice(..), wgpu::IndexFormat::Uint32);
        // Populate depth with faces first, then test edges against that stable
        // surface depth. Grid/overlay depths keep their existing semantics.
        pass.set_pipeline(pipelines[0]);
        for range in &self.triangle_ranges {
            pass.draw_indexed(range.clone(), 0, 0..1);
        }
        // Resolve edge-to-edge occlusion before blending any colors. Otherwise
        // later strokes (including wide selection strokes) overwrite nearer
        // edges because the antialiased color pass cannot safely write depth.
        for pipeline in [pipelines[1], pipelines[2]] {
            pass.set_pipeline(pipeline);
            let mut start = 0;
            for range in &self.triangle_ranges {
                if start < range.start {
                    pass.draw_indexed(start..range.start, 0, 0..1);
                }
                start = range.end;
            }
            if start < self.shadow_range.start {
                pass.draw_indexed(start..self.shadow_range.start, 0, 0..1);
            }
        }
    }
}

/// Keep triangulation independent of camera, selection, hover, and materials.
/// Compare source geometry as Model's collections also allow direct edits.
#[derive(Default)]
pub struct FaceCache {
    faces: BTreeMap<u64, CachedFace>,
    source_vertices: Vec<(u64, DVec3)>,
    adjacent_faces: BTreeMap<(u64, u64), Vec<u64>>,
    revision: u64,
    #[cfg(test)]
    topology_builds: usize,
}
pub struct CachedFace {
    boundary: Face,
    positions: Vec<DVec3>,
    pub normal: DVec3,
    pub center: DVec3,
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
        let same_topology = self.faces.len() == model.faces.len()
            && self
                .faces
                .values()
                .zip(model.faces.values())
                .all(|(cached, face)| cached.boundary == *face);
        if same_topology
            && self
                .source_vertices
                .iter()
                .copied()
                .eq(model.vertices.iter().map(|(&id, v)| (id, v.position)))
        {
            return;
        }
        if !same_topology {
            self.adjacent_faces.clear();
            for face in model.faces.values() {
                for ring in std::iter::once(&face.vertices).chain(face.holes.iter()) {
                    for i in 0..ring.len() {
                        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                        self.adjacent_faces
                            .entry((a.min(b), a.max(b)))
                            .or_default()
                            .push(face.id);
                    }
                }
            }
            #[cfg(test)]
            {
                self.topology_builds += 1;
            }
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
            let center = points
                .iter()
                .take(face.vertices.len())
                .copied()
                .sum::<DVec3>()
                / face.vertices.len() as f64;
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
                    center,
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
    pub fn adjacent_faces(&self, a: u64, b: u64) -> &[u64] {
        self.adjacent_faces
            .get(&(a.min(b), a.max(b)))
            .map_or(&[], Vec::as_slice)
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
    pub fn contains_point(&self, point: DVec3) -> bool {
        let Some(plane) = self.plane else { return false };
        if (point - plane.origin).dot(plane.normal).abs() > ketchup_core::document::EPS {
            return false;
        }
        let xy = plane.project(point);
        ketchup_core::faces::in_polygon(xy, &self.rings[0], true)
            && !self.rings.iter().skip(1).any(|ring|
                ketchup_core::faces::in_polygon(xy, ring, true))
    }

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
    #[test]
    fn group_marker_geometry_is_shared_across_views_and_replaced_after_model_changes() {
        use super::*;
        let mut model = Model::default();
        model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        model.add_edge(DVec3::X, DVec3::X + DVec3::Y).unwrap();
        let group = model.make_object(&model.selectable_entities(), false, "Group").unwrap();
        let mut cache = GroupMarkerCache::default();
        let first = cache.update(&model, 1);
        let point = first.iter().find(|marker| marker.id == group).unwrap().point;
        assert!(Arc::ptr_eq(&first, &cache.update(&model, 1)));
        model.move_selection(&std::collections::BTreeSet::from([group]), DVec3::X * 2.).unwrap();
        let moved = cache.update(&model, 2);
        let marker = moved.iter().find(|marker| marker.id == group).unwrap();
        assert!(marker.point.distance(point + DVec3::X * 2.) < 1e-10);
        assert_eq!(marker.owners, model.expand_entities(&std::collections::BTreeSet::from([group])));
        assert!(cache.update(&Model::default(), 3).is_empty());
    }

    #[test]
    fn narrow_annotation_queries_cover_actual_tessellation_and_antialiasing() {
        use super::*;
        for pixels in [1., 2., 2.5] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(pixels);
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                for (a, b) in [(DVec2::new(10., 15.), DVec2::new(400., 330.)),
                    (DVec2::new(10., 15.), DVec2::new(10., 330.)),
                    (DVec2::new(10., 15.), DVec2::new(400., 15.))] {
                    let footprint = annotation_stroke_footprint(a, b, 0.75 + 2. / f64::from(pixels));
                    let winding = (footprint[1] - footprint[0]).perp_dot(footprint[2] - footprint[1]).signum();
                    assert!(winding > 0., "annotation clipping expects positive winding");
                    let shape = egui::Shape::line_segment([egui::pos2(a.x as f32, a.y as f32),
                        egui::pos2(b.x as f32, b.y as f32)], egui::Stroke::new(1.5, Color32::WHITE));
                    let primitives = ui.ctx().tessellate(vec![egui::epaint::ClippedShape::new(Rect::EVERYTHING, shape)], pixels);
                    assert!(!primitives.is_empty());
                    for primitive in primitives {
                        if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                            for vertex in mesh.vertices {
                                let p = DVec2::new(f64::from(vertex.pos.x), f64::from(vertex.pos.y));
                                assert!((0..4).all(|i| winding * (footprint[(i+1)%4] - footprint[i])
                                    .perp_dot(p - footprint[i]) >= -1e-5), "stroke vertex outside query: {p:?}");
                            }
                        }
                    }
                    if (b-a).min_element() > 0. {
                        assert!(polygon_area(&[footprint]) < (b.x-a.x)*(b.y-a.y)*0.15);
                    }
                }
            });
        }
    }

    #[test]
    fn screen_ordered_outline_hierarchy_matches_general_visibility_queries() {
        use super::*;
        let mut general = Occlusion::default();
        let mut outlines = Occlusion { outline_order: true, ..Default::default() };
        for i in 0..512 {
            let lo = DVec2::new((i * 37 % 100) as f64, (i * 53 % 100) as f64);
            let hi = lo + DVec2::new(3., 0.7);
            let depth = 0.2 + (i % 7) as f64 * 0.01;
            occluding_rectangle(&mut general, lo, hi, depth);
            occluding_rectangle(&mut outlines, lo, hi, depth);
        }
        for i in 0..100 {
            let a = DVec3::new(0., i as f64 + 0.1, 0.7);
            let b = DVec3::new(104., 100. - i as f64, 0.7);
            let expected = general.visible_intervals(a, b);
            let actual = outlines.visible_intervals(a, b);
            assert_eq!(expected.len(), actual.len());
            for (a, b) in expected.iter().zip(&actual) {
                assert!((a.0 - b.0).abs() < 1e-10 && (a.1 - b.1).abs() < 1e-10);
            }
        }
        let polygon = vec![DVec2::ZERO, DVec2::new(104., 0.), DVec2::splat(104.), DVec2::new(0., 104.)];
        let area = |regions: Vec<Vec<DVec2>>| regions.iter().map(|p| {
            (1..p.len()-1).map(|i| (p[i]-p[0]).perp_dot(p[i+1]-p[0]).abs()*0.5).sum::<f64>()
        }).sum::<f64>();
        assert!((area(general.visible_polygons(polygon.clone(), 0.7)) - area(outlines.visible_polygons(polygon, 0.7))).abs() < 1e-7);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn ordinary_parallel_edges_preserve_the_completed_outline_hierarchy_between_frames() {
        use super::*;
        let camera = Camera::default();
        let size = DVec2::splat(512.);
        let inverse = camera.matrix(size).inverse();
        let world = |x, y| inverse.project_point3(DVec3::new(x / 256. - 1., 1. - y / 256., 0.4));
        let edges: Vec<_> = (0..256).map(|i| EdgeDraw {
            id: i, key: (0, 1), points: [world(10., i as f64 * 1.8), world(500., i as f64 * 1.8)],
            color: Color32::BLACK, width: 1.8, visible: true, back: false, overlay: false,
        }).collect();
        let mut mesh = Mesh::new(&camera, size);
        mesh.draw_edges(&edges, true, true);
        mesh.selection_occlusion.prepare_queries();
        assert!(!mesh.selection_occlusion.nodes.is_empty());
        let hierarchy = Arc::clone(&mesh.selection_occlusion.nodes);
        let revision = mesh.selection_occlusion.revision;
        mesh.selection_occlusion.begin_frame();
        mesh.draw_edges(&edges, true, true);
        mesh.selection_occlusion.prepare_queries();
        assert_eq!(mesh.selection_occlusion.revision, revision);
        assert!(Arc::ptr_eq(&hierarchy, &mesh.selection_occlusion.nodes),
            "collecting ordinary edges must not truncate/rebuild the existing hierarchy");
    }

    #[test]
    fn shared_polygon_splits_match_independent_half_plane_clipping() {
        use super::*;
        let polygon = vec![DVec2::new(-2., -1.), DVec2::new(0., -2.),
            DVec2::new(3., -1.), DVec2::new(2., 2.), DVec2::new(-1., 3.)];
        for i in 0..2000 {
            let angle = i as f64 * 0.37;
            let normal = DVec2::new(angle.cos(), angle.sin());
            let offset = (i % 41) as f64 * 0.2 - 4.;
            let side = |p: DVec2| normal.dot(p) + offset;
            let (inside, outside) = split_screen_polygon(&polygon, side);
            assert_eq!(inside, clip_polygon(&polygon, side));
            assert_eq!(outside, clip_polygon(&polygon, |p| -side(p)));
        }
        for offset in [-2. - 1e-14, -2., -2. + 1e-14, 1., 3.] {
            let side = |p: DVec2| p.x + offset;
            let (inside, outside) = split_screen_polygon(&polygon, side);
            assert_eq!(inside, clip_polygon(&polygon, side));
            assert_eq!(outside, clip_polygon(&polygon, |p| -side(p)));
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn parallel_joint_guides_match_serial_clipping_dashes_and_annotation_masks() {
        use super::*;
        let camera = Camera::default();
        let size = DVec2::splat(512.);
        let inverse = camera.matrix(size).inverse();
        let world = |x, y, z| inverse.project_point3(DVec3::new(x / 256. - 1., 1. - y / 256., z));
        let stock = [
            [world(10., 10., 0.4), world(500., 10., 0.4), world(500., 500., 0.4)],
            [world(10., 10., 0.4), world(500., 500., 0.4), world(10., 500., 0.4)],
        ];
        let lines: Vec<_> = (0..256).map(|i| {
            let y = 15. + f64::from(i) * 1.8;
            [world(15., y, 0.6), world(495., 510. - y, 0.6)]
        }).collect();
        let build = |parallel: bool| {
            let mut mesh = Mesh::new(&camera, size);
            mesh.enable_gpu_edge_occlusion();
            for triangle in stock { mesh.world_triangle(&camera, triangle, Color32::WHITE); }
            mesh.world_triangle(&camera, [world(200., 0., 0.4 - 1e-9), world(260., 512., 0.4 - 1e-9), world(330., 0., 0.4 - 1e-9)], Color32::GRAY);
            mesh.styled_line(world(0., 240., 0.4 - 1e-9), world(512., 240., 0.4 - 1e-9), 2., Color32::BLACK, false, false);
            for [a, b] in &lines { mesh.register_joint_guide(1, *a, *b); }
            let surface = mesh.cached_joint_surface_for_entities(1, &stock, &Default::default());
            let paint = |mesh: &mut Mesh, &[a, b]: &[DVec3; 2]| {
                mesh.joint_guide(a, b, &surface, 1.8, Color32::GREEN, true);
            };
            if parallel { mesh.paint_overlays(&lines, 64, paint); }
            else { for line in &lines { paint(&mut mesh, line); } }
            mesh
        };
        let serial = build(false);
        let parallel = build(true);
        assert_eq!(serial.indices, parallel.indices);
        assert_eq!(bytemuck::cast_slice::<_, u8>(&serial.vertices), bytemuck::cast_slice::<_, u8>(&parallel.vertices));
        assert_eq!(serial.annotation_occluders, parallel.annotation_occluders);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn parallel_face_batches_match_serial_mesh_depth_and_shadow_coverage() {
        use super::*;
        let camera = Camera::default();
        let size = DVec2::splat(512.);
        let inverse = camera.matrix(size).inverse();
        let world = |x, y, z| inverse.project_point3(DVec3::new(x / 256. - 1., 1. - y / 256., z));
        let mut model = Model::default();
        for i in 0..96 {
            let x = 10. + f64::from(i % 12) * 40.;
            let y = 10. + f64::from(i / 12) * 60.;
            let points = [world(x, y, 0.4), world(x + 30., y, 0.4), world(x, y + 40., 0.4)];
            for j in 0..3 { model.add_edge(points[j], points[(j + 1) % 3]).unwrap(); }
        }
        let mut cache = FaceCache::default();
        cache.update(&model);
        assert!(model.faces.len() >= 64);
        let shadows: Vec<_> = model.faces.keys().map(|id| {
            let [a, b, c] = cache.get(*id).triangles[0];
            let mid = (b + c) * 0.5;
            crate::shadows::FaceShadow { lit: vec![[a, b, mid]], shaded: vec![[a, mid, c]] }
        }).collect();
        for antialias in [false, true] {
            let build = |parallel: bool| {
                let mut mesh = Mesh::new(&camera, size);
                mesh.display.fog = true;
                mesh.display.shadow_antialiasing = antialias;
                // An existing prefix checks both vertex and index offsets.
                mesh.world_triangle(&camera, [world(0., 0., 0.8), world(512., 0., 0.8), world(0., 512., 0.8)], Color32::WHITE);
                let faces: Vec<_> = model.faces.values().zip(&shadows).enumerate().map(|(i, (face, shadow))| FaceDraw {
                    face, cached: cache.get(face.id), color: Color32::from_rgb(100, 160, 200),
                    shadow: (i % 3 != 0).then_some((shadow, Color32::DARK_GRAY)),
                }).collect();
                if parallel { mesh.draw_faces(&faces); }
                else { for face in &faces { mesh.draw_faces(std::slice::from_ref(face)); } }
                mesh
            };
            let serial = build(false);
            let parallel = build(true);
            assert_eq!(serial.indices, parallel.indices);
            assert_eq!(serial.triangle_ranges, parallel.triangle_ranges);
            assert_eq!(bytemuck::cast_slice::<_, u8>(&serial.vertices), bytemuck::cast_slice::<_, u8>(&parallel.vertices));
            assert_eq!(bytemuck::cast_slice::<_, u8>(&serial.shadow_vertices), bytemuck::cast_slice::<_, u8>(&parallel.shadow_vertices));
            assert_eq!(serial.occlusion.triangles.iter().map(|t| (t.entity, t.points)).collect::<Vec<_>>(),
                parallel.occlusion.triangles.iter().map(|t| (t.entity, t.points)).collect::<Vec<_>>());
            assert!(serial.edge_surfaces.iter().all(|(key, surfaces)| {
                let other = &parallel.edge_surfaces[key];
                surfaces.iter().map(|s| (s.gradient, s.inward)).eq(other.iter().map(|s| (s.gradient, s.inward)))
            }));
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn parallel_edge_batches_match_serial_geometry_and_outline_masks() {
        use super::*;
        let camera = Camera::default();
        let size = DVec2::splat(512.);
        let inverse = camera.matrix(size).inverse();
        let world = |x, y, z| inverse.project_point3(DVec3::new(x / 256. - 1., 1. - y / 256., z));
        for gpu in [false, true] {
            let build = |parallel: bool| {
                let mut mesh = Mesh::new(&camera, size);
                if gpu { mesh.enable_gpu_edge_occlusion(); }
                mesh.set_entity(Some(9000));
                mesh.world_triangle(&camera, [world(200., 30., 0.3), world(330., 460., 0.3), world(400., 40., 0.3)], Color32::WHITE);
                for selected in [false, true] {
                    if selected { mesh.begin_selected_edges(); }
                    let edges: Vec<_> = (0..288).map(|i| {
                        let y = 20. + f64::from(i) * 1.5;
                        EdgeDraw { id: i as u64 + u64::from(selected) * 1000,
                            key: (0, 1), points: [world(10., y, 0.4), world(490., 500. - y, 0.4)],
                            color: if selected { Color32::RED } else { Color32::BLACK },
                            width: if selected { 3. } else { 1.8 },
                            back: i % 7 == 0, visible: i % 11 != 0, overlay: i % 13 == 0 }
                    }).collect();
                    if parallel { mesh.draw_edges(&edges, true, true); }
                    else { for edge in &edges { mesh.draw_edges(std::slice::from_ref(edge), true, true); } }
                }
                mesh
            };
            let serial = build(false);
            let parallel = build(true);
            assert_eq!(serial.indices, parallel.indices);
            assert_eq!(bytemuck::cast_slice::<_, u8>(&serial.vertices), bytemuck::cast_slice::<_, u8>(&parallel.vertices));
            assert_eq!(serial.annotation_occluders, parallel.annotation_occluders);
            assert_eq!(serial.selection_occlusion.triangles.iter().map(|t| (t.entity, t.points)).collect::<Vec<_>>(),
                parallel.selection_occlusion.triangles.iter().map(|t| (t.entity, t.points)).collect::<Vec<_>>());
        }
    }

    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "Requires a GPU adapter; compares hybrid cut-guide clipping against the full f64 reference"]
    fn gpu_cut_guides_match_full_cpu_clipping_for_foreground_faces_outlines_and_dash_gaps() {
        use wgpu::util::DeviceExt;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(include_str!("edges.wgsl").into()),
        });
        let pipelines = [
            PipelinePass::Faces,
            PipelinePass::EdgeDepth,
            PipelinePass::Edges,
        ]
        .map(|pass| create_pipeline(&device, &shader, wgpu::TextureFormat::Rgba8Unorm, pass));
        let size = wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        };
        let texture = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = texture(
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = texture(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * 64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let render = |mesh: Mesh| {
            let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &color_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.set_pipeline(&pipelines[0]);
                for range in &mesh.triangle_ranges {
                    pass.draw_indexed(range.clone(), 0, 0..1);
                }
                let edge_start = mesh.triangle_ranges.last().map_or(0, |range| range.end);
                for pipeline in &pipelines[1..] {
                    pass.set_pipeline(pipeline);
                    pass.draw_indexed(edge_start..mesh.indices.len() as u32, 0, 0..1);
                }
            }
            encoder.copy_texture_to_buffer(
                color.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(64),
                    },
                },
                size,
            );
            queue.submit([encoder.finish()]);
            let (send, receive) = std::sync::mpsc::channel();
            readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    send.send(result).unwrap();
                });
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(std::time::Duration::from_secs(30)),
                })
                .unwrap();
            receive.recv().unwrap().unwrap();
            let pixels = readback.slice(..).get_mapped_range().unwrap().to_vec();
            readback.unmap();
            pixels
        };
        for projection in [
            ketchup_core::Projection::Parallel,
            ketchup_core::Projection::Perspective,
        ] {
            let camera = Camera {
                projection,
                ..Default::default()
            };
            let viewport = DVec2::splat(64.);
            let inverse = camera.matrix(viewport).inverse();
            let world = |x: f64, y: f64, z: f64| {
                inverse.project_point3(DVec3::new(x / 32. - 1., 1. - y / 32., z))
            };
            for foreground in [0.2, 0.4 - 1e-9] {
                let build = |hybrid| {
                    let mut mesh = Mesh::new(&camera, viewport);
                    if hybrid {
                        mesh.enable_gpu_edge_occlusion();
                    }
                    let rectangle = |lo: DVec2, hi: DVec2, z| {
                        let p = [
                            world(lo.x, lo.y, z),
                            world(hi.x, lo.y, z),
                            world(hi.x, hi.y, z),
                            world(lo.x, hi.y, z),
                        ];
                        [[p[0], p[1], p[2]], [p[0], p[2], p[3]]]
                    };
                    let stock = rectangle(DVec2::splat(6.), DVec2::splat(58.), 0.4);
                    for triangle in stock {
                        mesh.world_triangle(&camera, triangle, Color32::WHITE);
                    }
                    for triangle in
                        rectangle(DVec2::new(25., 10.), DVec2::new(39., 54.), foreground)
                    {
                        mesh.world_triangle(&camera, triangle, Color32::GRAY);
                    }
                    mesh.styled_line(
                        world(46., 6., 0.3),
                        world(46., 58., 0.3),
                        4.,
                        Color32::BLACK,
                        false,
                        false,
                    );
                    mesh.styled_line(
                        world(10., 44., 0.3),
                        world(54., 44., 0.3),
                        2.,
                        Color32::BLUE,
                        false,
                        false,
                    );
                    mesh.styled_line(
                        world(10., 52., 0.5),
                        world(54., 52., 0.5),
                        2.,
                        Color32::BLACK,
                        false,
                        false,
                    );
                    let surface = if hybrid {
                        mesh.register_joint_guide(7, world(9., 28., 0.8), world(55., 28., 0.8));
                        mesh.register_joint_guide(7, world(12., 48., 0.8), world(52., 48., 0.8));
                        mesh.cached_joint_surface(7, &stock)
                    } else {
                        Arc::new(mesh.joint_surface(stock))
                    };
                    mesh.joint_guide(
                        world(9., 28., 0.8),
                        world(55., 28., 0.8),
                        &surface,
                        2.,
                        Color32::from_rgb(0, 100, 92),
                        true,
                    );
                    mesh.joint_guide(
                        world(12., 48., 0.8),
                        world(54., 15., 0.8),
                        &surface,
                        2.,
                        Color32::from_rgb(0, 100, 92),
                        true,
                    );
                    mesh
                };
                let reference = render(build(false));
                let hybrid = render(build(true));
                assert!(
                    reference.chunks_exact(4).any(|p| p[0] == 0 && p[1] == 100),
                    "draw real dotted cut guides"
                );
                assert!(
                    reference
                        .chunks_exact(4)
                        .zip(hybrid.chunks_exact(4))
                        .enumerate()
                        .all(|(i, (a, b))| {
                            a.iter().zip(b).all(|(a,b)| a.abs_diff(*b) <= 2)
                    // The full CPU reference masks even the zero-opacity
                    // boundary of the outline's antialias fringe. GPU depth
                    // keeps a guide visible at that transparent boundary;
                    // the actual outline core must still match exactly.
                    || ((matches!(i % 64, 43 | 48) || matches!(i / 64, 42 | 45))
                        && a == [255,255,255,255] && b[0] < b[1] && b[1] >= 100)
                        }),
                    "GPU foreground occlusion must match f64 clipping: {projection:?}, {foreground}; differences={:?}",
                    reference
                        .chunks_exact(4)
                        .zip(hybrid.chunks_exact(4))
                        .enumerate()
                        .filter(|(_, (a, b))| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > 2))
                        .map(|(i, (a, b))| (i % 64, i / 64, a, b))
                        .take(12)
                        .collect::<Vec<_>>()
                );
            }
        }
        // Back edges use the same shader dash mask, with the original fixed
        // four-pixel dash and gap rather than the guide's fitted end pattern.
        let camera = Camera::default();
        let size = DVec2::splat(64.);
        let inverse = camera.matrix(size).inverse();
        let world = |p: DVec2| inverse.project_point3(DVec3::new(p.x / 32. - 1., 1. - p.y / 32., 0.4));
        for (a, b) in [(DVec2::new(5.17, 15.3), DVec2::new(59.1, 15.3)),
            (DVec2::new(23.2, 4.1), DVec2::new(23.2, 59.7)),
            (DVec2::new(5.3, 8.2), DVec2::new(58.2, 49.1))] {
            let color = Color32::from_rgba_unmultiplied(60, 130, 80, 160);
            let mut reference = Mesh::new(&camera, size);
            let mut optimized = Mesh::new(&camera, size);
            let (a, b) = camera.view(size).segment(world(a), world(b)).unwrap();
            let length = a.distance(b).max(1.);
            for i in 0..(length / 8.).ceil() as usize {
                reference.line(a.lerp(b, (i as f64 * 8. / length).min(1.)).extend(0.),
                    a.lerp(b, ((i as f64 * 8. + 4.) / length).min(1.)).extend(0.), 1., color);
            }
            optimized.overlay_line(world(a), world(b), 1., color, true);
            assert_eq!(optimized.vertices.len(), 4);
            let expected = render(reference);
            let actual = render(optimized);
            for (i, (expected, actual)) in expected.chunks_exact(4).zip(actual.chunks_exact(4)).enumerate() {
                let point = DVec2::new((i % 64) as f64 + 0.5, (i / 64) as f64 + 0.5);
                let phase = ((point - a).dot((b-a).normalize()) / 8.).rem_euclid(1.);
                assert!(expected.iter().zip(actual).all(|(a,b)| a.abs_diff(*b) <= 3)
                    || phase.min(1.-phase).min((phase-0.5).abs()) < 0.005,
                    "shader overlay dash must match subdivided reference at {point:?}: {expected:?}, {actual:?}");
            }
        }

    }

    #[test]
    fn annotations_share_outline_masks_without_changing_line_or_label_visibility() {
        let draw = |shared| {
            let mut mesh = Mesh::new(&Camera::default(), DVec2::splat(100.));
            if shared {
                mesh.enable_gpu_edge_occlusion();
            }
            mesh.line(
                DVec3::new(50., 0., 0.3),
                DVec3::new(50., 100., 0.3),
                3.,
                Color32::BLACK,
            );
            mesh.begin_selected_edges();
            mesh.surface_stroke(
                DVec3::new(0., 65., 0.4),
                DVec3::new(100., 65., 0.4),
                4.,
                Color32::RED,
                &[],
            );
            mesh.occlude_annotations_with_edges();
            mesh
        };
        let mut reference = draw(false);
        let mut shared = draw(true);
        assert!(
            shared.occlusion.triangles.len() < reference.occlusion.triangles.len(),
            "do not duplicate ordinary outlines into the face hierarchy"
        );
        for depth in [0.2, 0.3 + 1e-10, 0.6] {
            let (a, b) = (DVec3::new(10., 50., depth), DVec3::new(90., 50., depth));
            let expected = reference.visible_annotation_line(a, b);
            let actual = shared.visible_annotation_line(a, b);
            assert_eq!(expected.len(), actual.len());
            for ((a, b), (c, d)) in expected.into_iter().zip(actual) {
                assert!(a.distance(c) < 1e-8 && b.distance(d) < 1e-8);
            }
            let polygon = vec![
                DVec2::new(20., 20.),
                DVec2::new(80., 20.),
                DVec2::new(80., 80.),
                DVec2::new(20., 80.),
            ];
            let expected = reference.visible_annotation_polygons(polygon.clone(), a, DVec2::ZERO);
            let actual = shared.visible_annotation_polygons(polygon, a, DVec2::ZERO);
            assert!((polygon_area(&expected) - polygon_area(&actual)).abs() < 1e-7);
        }
    }

    #[test]
    fn gpu_edge_visibility_preserves_precision_and_invalidates_cached_intervals_when_modes_change()
    {
        let mut occlusion = Occlusion::default();
        occluding_rectangle(&mut occlusion, DVec2::ZERO, DVec2::splat(100.), 0.3);
        let (a, b) = (DVec3::new(10., 50., 0.5), DVec3::new(90., 50., 0.5));
        assert!(occlusion.visible_intervals(a, b).is_empty());
        occlusion.line_cursor = 0;
        let visible = occlusion.visible_intervals_with_guard(a, b, Some(1e-6));
        assert_eq!(
            visible,
            vec![(0., 1.)],
            "distant faces are resolved by GPU depth"
        );
        occlusion.line_cursor = 0;
        assert!(
            occlusion.visible_intervals(a, b).is_empty(),
            "selected edges retain f64 clipping"
        );
        occlusion.line_cursor = 0;
        assert!(
            occlusion
                .visible_intervals_with_guard(a, b, Some(0.3))
                .is_empty(),
            "a steep stroke footprint still requires CPU clipping"
        );
        let (a, b) = (
            DVec3::new(10., 50., 0.3 + 1e-9),
            DVec3::new(90., 50., 0.3 + 1e-9),
        );
        assert!(
            occlusion
                .visible_intervals_with_guard(a, b, Some(1e-6))
                .is_empty(),
            "foreground differences lost in GPU depth must still hide the edge"
        );
    }

    #[test]
    fn joint_precision_masks_ignore_own_entities_and_track_foreign_owner_changes() {
        let polygon = [
            DVec2::new(0., 0.),
            DVec2::new(100., 0.),
            DVec2::new(100., 100.),
            DVec2::new(0., 100.),
        ];
        let origin = DVec3::new(0., 0., 0.4);
        let mut occlusion = Occlusion::default();
        let own = std::collections::BTreeSet::from([10]);
        occlusion.begin_frame();
        occlusion.entity = Some(10);
        occluding_rectangle(&mut occlusion, DVec2::ZERO, DVec2::splat(100.), 0.4 - 1e-9);
        assert!(
            occlusion
                .precision_candidates(&polygon, origin, DVec2::ZERO, false, &own)
                .is_empty()
        );
        let revision = occlusion.revision;
        // Identical geometry assigned to a different piece must invalidate the
        // cached identity and remain an occluder, even inside a GPU depth step.
        occlusion.begin_frame();
        occlusion.entity = Some(20);
        occluding_rectangle(&mut occlusion, DVec2::ZERO, DVec2::splat(100.), 0.4 - 1e-9);
        assert_ne!(occlusion.revision, revision);
        let candidates = occlusion.precision_candidates(&polygon, origin, DVec2::ZERO, true, &own);
        assert_eq!(candidates.len(), 2);
        assert!(
            occlusion
                .visible_surface_candidates(polygon.to_vec(), origin, DVec2::ZERO, &candidates)
                .is_empty()
        );
    }

    #[test]
    fn joint_surface_candidates_preserve_foreground_faces_and_outlines_and_refresh_after_edits() {
        for projection in [
            ketchup_core::Projection::Parallel,
            ketchup_core::Projection::Perspective,
        ] {
            let camera = Camera {
                projection,
                ..Default::default()
            };
            let size = DVec2::splat(100.);
            let inverse = camera.matrix(size).inverse();
            let world = |x: f64, y: f64, z: f64| {
                inverse.project_point3(DVec3::new(x / 50. - 1., 1. - y / 50., z))
            };
            let p = [
                world(10., 10., 0.4),
                world(90., 10., 0.4),
                world(90., 90., 0.4),
                world(10., 90., 0.4),
            ];
            let stock = [[p[0], p[1], p[2]], [p[0], p[2], p[3]]];
            let render = |prepared: bool, dashed: bool| {
                let mut mesh = Mesh::new(&camera, size);
                // Most geometry is behind the stock; one face and one outline
                // are in front, and must still mask the complete guide width.
                for i in 0..30 {
                    occluding_rectangle(
                        &mut mesh.occlusion,
                        DVec2::new(i as f64, 0.),
                        DVec2::new(i as f64 + 20., 100.),
                        0.6,
                    );
                }
                occluding_rectangle(
                    &mut mesh.occlusion,
                    DVec2::new(45., 0.),
                    DVec2::new(55., 100.),
                    0.4 - 1e-9,
                );
                mesh.line(
                    DVec3::new(70., 0., 0.4 - 2e-9),
                    DVec3::new(70., 100., 0.4 - 2e-9),
                    4.,
                    Color32::BLACK,
                );
                mesh.vertices.clear();
                mesh.indices.clear();
                let surface = if prepared {
                    mesh.cached_joint_surface(7, &stock)
                } else {
                    Arc::new(mesh.joint_surface(stock))
                };
                if prepared {
                    assert!(Arc::ptr_eq(&surface, &mesh.cached_joint_surface(7, &stock)));
                    let candidates = surface.occluders.as_ref().unwrap();
                    assert!(
                        candidates.iter().all(|(faces, _)| faces.len() <= 2),
                        "background geometry must be rejected once for the board"
                    );
                }
                for (a, b) in [
                    (world(15., 35., 0.8), world(85., 65., 0.8)),
                    (world(20., 80., 0.8), world(80., 20., 0.8)),
                ] {
                    mesh.joint_guide(a, b, &surface, 3., Color32::WHITE, dashed);
                }
                mesh
            };
            for dashed in [false, true] {
                let original = render(false, dashed);
                let prepared = render(true, dashed);
                // Candidate filtering preserves the exact clipping output, not
                // just the centerline or an approximate visibility threshold.
                assert_eq!(
                    bytemuck::cast_slice::<Vertex, u8>(&original.vertices),
                    bytemuck::cast_slice::<Vertex, u8>(&prepared.vertices)
                );
                assert_eq!(original.indices, prepared.indices);
            }
            let mut mesh = Mesh::new(&camera, size);
            let before = mesh.cached_joint_surface(7, &stock);
            occluding_rectangle(&mut mesh.occlusion, DVec2::ZERO, size, 0.4 - 1e-9);
            let after = mesh.cached_joint_surface(7, &stock);
            assert!(
                !Arc::ptr_eq(&before, &after),
                "foreground edits invalidate candidates"
            );
            mesh.joint_guide(
                world(20., 50., 0.8),
                world(80., 50., 0.8),
                &after,
                2.,
                Color32::WHITE,
                true,
            );
            assert!(mesh.vertices.is_empty());
        }
    }

    #[test]
    fn projected_joint_surfaces_reuse_stock_and_invalidate_on_view_geometry_and_sections() {
        let camera = Camera::default();
        let size = DVec2::new(640., 480.);
        let stock = [[DVec3::ZERO, DVec3::X, DVec3::Y]];
        let mut cache = SceneCache::default();
        let mut first_frame = Mesh::cached(&camera, size, &mut cache);
        let first = first_frame.projected_joint_surface(7, &stock);
        assert!(
            Arc::ptr_eq(&first, &first_frame.projected_joint_surface(7, &stock)),
            "joints sharing a board must reuse its projected skin"
        );
        first_frame.retain(&mut cache);

        let mut next_frame = Mesh::cached(&camera, size, &mut cache);
        next_frame.set_display(&Default::default());
        next_frame.set_pixels_per_point(2.);
        assert!(
            Arc::ptr_eq(&first, &next_frame.projected_joint_surface(7, &stock)),
            "hover redraws and pixel density must not rebuild the unchanged stock projection"
        );
        let moved_stock = stock.map(|triangle| triangle.map(|p| p + DVec3::Z * 0.1));
        let moved = next_frame.projected_joint_surface(7, &moved_stock);
        assert!(!Arc::ptr_eq(&first, &moved));
        let sections = ketchup_core::DisplaySettings {
            section_cuts: true,
            section_axes: [false, false, true],
            section_height: 0.,
            ..Default::default()
        };
        next_frame.set_display(&sections);
        let sectioned = next_frame.projected_joint_surface(7, &moved_stock);
        assert!(!Arc::ptr_eq(&moved, &sectioned));
        assert!(sectioned.triangles.is_empty());
        next_frame.retain(&mut cache);

        let mut default_display = Mesh::cached(&camera, size, &mut cache);
        let unsectioned = default_display.projected_joint_surface(7, &moved_stock);
        assert!(!Arc::ptr_eq(&sectioned, &unsectioned));
        assert!(
            !unsectioned.triangles.is_empty(),
            "default display settings must not inherit stale section clipping"
        );
        default_display.retain(&mut cache);

        let mut orbit = camera.clone();
        orbit.yaw += 0.2;
        let mut orbit_frame = Mesh::cached(&orbit, size, &mut cache);
        orbit_frame.set_display(&sections);
        let rotated = orbit_frame.projected_joint_surface(7, &moved_stock);
        assert!(!Arc::ptr_eq(&sectioned, &rotated));
        orbit_frame.retain(&mut cache);
        let mut resized = Mesh::cached(&orbit, size * 2., &mut cache);
        resized.set_display(&sections);
        assert!(!Arc::ptr_eq(
            &rotated,
            &resized.projected_joint_surface(7, &moved_stock)
        ));
        resized.retain(&mut cache);
        Mesh::cached(&orbit, size * 2., &mut cache).retain(&mut cache);
        assert!(
            cache.joint_surfaces.entries.is_empty(),
            "unused boards must not accumulate in the cache"
        );
    }

    #[test]
    fn disabling_unused_annotation_masks_preserves_strokes_selection_and_joint_geometry() {
        let camera = Camera::default();
        let size = DVec2::splat(100.);
        let inverse = camera.matrix(size).inverse();
        let world = |x: f64, y: f64, z: f64| {
            inverse.project_point3(DVec3::new(x / 50. - 1., 1. - y / 50., z))
        };
        let draw = |annotations| {
            let mut mesh = Mesh::new(&camera, size);
            mesh.set_annotation_occlusion_enabled(annotations);
            mesh.line(
                DVec3::new(50., 0., 0.3),
                DVec3::new(50., 100., 0.3),
                3.,
                Color32::BLACK,
            );
            mesh.begin_selected_edges();
            mesh.surface_stroke(
                DVec3::new(10., 50., 0.35),
                DVec3::new(90., 50., 0.35),
                6.,
                Color32::RED,
                &[],
            );
            let p = [
                world(0., 0., 0.25),
                world(100., 0., 0.25),
                world(100., 100., 0.25),
                world(0., 100., 0.25),
            ];
            let surface = mesh.joint_surface([[p[0], p[1], p[2]], [p[0], p[2], p[3]]]);
            mesh.joint_guide(
                world(10., 40., 0.8),
                world(90., 40., 0.8),
                &surface,
                1.8,
                Color32::WHITE,
                true,
            );
            mesh
        };
        let mut enabled = draw(true);
        let mut disabled = draw(false);
        assert_eq!(
            bytemuck::cast_slice::<Vertex, u8>(&enabled.vertices),
            bytemuck::cast_slice::<Vertex, u8>(&disabled.vertices)
        );
        assert_eq!(enabled.indices, disabled.indices);
        assert!(!enabled.annotation_occluders.is_empty());
        assert!(disabled.annotation_occluders.is_empty());
        assert!(
            !disabled.selection_occlusion.triangles.is_empty(),
            "ordinary edge masking must remain active"
        );
        enabled.occlude_annotations_with_edges();
        disabled.occlude_annotations_with_edges();
        assert!(!enabled.occlusion.triangles.is_empty());
        assert!(
            disabled.occlusion.triangles.is_empty(),
            "an unannotated scene must not collect unused stroke triangles"
        );
    }

    #[test]
    fn all_lower_cut_face_edges_render_as_dotted_xray_through_their_own_board() {
        let points = [
            DVec3::new(-1., -1., -1.),
            DVec3::new(1., -1., -1.),
            DVec3::new(1., 1., -1.),
            DVec3::new(-1., 1., -1.),
            DVec3::new(-1., -1., 1.),
            DVec3::new(1., -1., 1.),
            DVec3::new(1., 1., 1.),
            DVec3::new(-1., 1., 1.),
        ];
        let triangles: Vec<_> = [
            [0, 1, 2, 3],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [1, 2, 6, 5],
            [2, 3, 7, 6],
            [3, 0, 4, 7],
        ]
        .into_iter()
        .flat_map(|p| {
            [
                [points[p[0]], points[p[1]], points[p[2]]],
                [points[p[0]], points[p[2]], points[p[3]]],
            ]
        })
        .collect();
        let floor = [
            DVec3::new(-0.6, -0.4, -0.5),
            DVec3::new(0.6, -0.4, -0.5),
            DVec3::new(0.6, 0.4, -0.5),
            DVec3::new(-0.6, 0.4, -0.5),
        ];
        for projection in [
            ketchup_core::Projection::Parallel,
            ketchup_core::Projection::Perspective,
        ] {
            let camera = Camera {
                projection,
                ..Default::default()
            };
            let mut mesh = Mesh::new(&camera, DVec2::new(800., 600.));
            let surface = mesh.joint_surface(triangles.iter().copied());
            for triangle in &surface.triangles {
                mesh.occlusion.insert(triangle.points);
            }
            for i in 0..4 {
                let start = mesh.indices.len();
                mesh.joint_guide(
                    floor[i],
                    floor[(i + 1) % 4],
                    &surface,
                    1.8,
                    Color32::WHITE,
                    true,
                );
                assert!(
                    mesh.indices.len() > start,
                    "lower cut-face edge {i} disappeared in {projection:?}"
                );
            }
            // A connector may share an exterior side plane without becoming
            // an ordinary stock rim. Its recessed contour must remain visible.
            let start = mesh.indices.len();
            mesh.joint_guide(
                DVec3::new(1., -0.4, -0.5),
                DVec3::new(1., 0.4, -0.5),
                &surface,
                1.8,
                Color32::WHITE,
                true,
            );
            assert!(
                mesh.indices.len() > start,
                "coplanarity with an outer face must not suppress cut edges"
            );
        }
    }

    #[test]
    fn joint_visibility_work_does_not_scale_with_dash_count() {
        let camera = Camera::default();
        let size = DVec2::new(1024., 512.);
        let inverse = camera.matrix(size).inverse();
        let world = |x: f64, y: f64, z: f64| {
            inverse.project_point3(DVec3::new(x / size.x * 2. - 1., 1. - y / size.y * 2., z))
        };
        let render = |dashed| {
            let mut mesh = Mesh::new(&camera, size);
            let p = [
                world(0., 0., 0.4),
                world(size.x, 0., 0.4),
                world(size.x, size.y, 0.4),
                world(0., size.y, 0.4),
            ];
            let mut surface = mesh.joint_surface([[p[0], p[1], p[2]], [p[0], p[2], p[3]]]);
            occluding_rectangle(
                &mut mesh.occlusion,
                DVec2::new(400., 0.),
                DVec2::new(600., size.y),
                0.2,
            );
            mesh.joint_guide(
                world(10., 256., 0.8),
                world(1010., 256., 0.8),
                &mut surface,
                1.8,
                Color32::WHITE,
                dashed,
            );
            let work = (
                surface.preparation_queries,
                mesh.occlusion.polygon_queries,
                mesh.selection_occlusion.polygon_queries,
            );
            (mesh, work)
        };
        let (solid, solid_work) = render(false);
        let (dashed, dashed_work) = render(true);
        assert!(solid_work.1 > 0);
        assert_eq!(
            dashed_work, solid_work,
            "64 dashes must use the same visibility work as a continuous guide"
        );
        assert_eq!(
            dashed.indices, solid.indices,
            "dashes must not multiply GPU triangles"
        );
        assert_eq!(dashed.vertices.len(), solid.vertices.len());
        assert!(dashed.vertices.iter().all(|v| v.dash_phase >= 0.));
        let covers = |x: f64| {
            let point = DVec2::new(x / size.x * 2. - 1., 0.);
            dashed.indices.chunks_exact(3).any(|indices| {
                let p = [indices[0], indices[1], indices[2]].map(|i| {
                    let p = dashed.vertices[i as usize].position;
                    DVec2::new(f64::from(p[0]), f64::from(p[1]))
                });
                let denominator = (p[1] - p[0]).perp_dot(p[2] - p[0]);
                let winding = denominator.signum();
                let inside = winding != 0.
                    && [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])]
                        .into_iter()
                        .all(|(a, b)| winding * (b - a).perp_dot(point - a) >= -1e-10);
                if !inside {
                    return false;
                }
                let u = (point - p[0]).perp_dot(p[2] - p[0]) / denominator;
                let v = (p[1] - p[0]).perp_dot(point - p[0]) / denominator;
                let phase = [indices[0], indices[1], indices[2]]
                    .map(|i| f64::from(dashed.vertices[i as usize].dash_phase));
                let phase = phase[0] + u * (phase[1] - phase[0]) + v * (phase[2] - phase[0]);
                phase < 0. || phase.rem_euclid(1.) <= 0.5
            })
        };
        for i in 0..64 {
            let dash_center = 10. + (i as f64 + 0.25) / 63.5 * 1000.;
            assert_eq!(
                covers(dash_center),
                !(400. ..600.).contains(&dash_center),
                "visible dashes must retain their phase and foreground masking"
            );
            if i < 63 {
                let gap_center = 10. + (i as f64 + 0.75) / 63.5 * 1000.;
                assert!(
                    !covers(gap_center),
                    "dash gaps must remain empty after clipping"
                );
            }
        }
    }

    #[test]
    fn dashed_cut_guides_reach_both_endpoints_including_short_connectors() {
        let camera = Camera::default();
        let size = DVec2::splat(100.);
        let inverse = camera.matrix(size).inverse();
        let world = |x: f64, y: f64, z: f64| {
            inverse.project_point3(DVec3::new(x / 50. - 1., 1. - y / 50., z))
        };
        for length in [1., 4., 8., 13., 31., 70.] {
            let mut mesh = Mesh::new(&camera, size);
            let p = [
                world(0., 0., 0.4),
                world(100., 0., 0.4),
                world(100., 100., 0.4),
                world(0., 100., 0.4),
            ];
            let mut surface = mesh.joint_surface([[p[0], p[1], p[2]], [p[0], p[2], p[3]]]);
            mesh.joint_guide(
                world(10., 50., 0.8),
                world(10. + length, 50., 0.8),
                &mut surface,
                1.8,
                Color32::WHITE,
                true,
            );
            let xs: Vec<_> = mesh
                .vertices
                .iter()
                .map(|v| (v.position[0] + 1.) * 50.)
                .collect();
            assert!(!xs.is_empty());
            let lo = xs.iter().copied().reduce(f32::min).unwrap();
            let hi = xs.iter().copied().reduce(f32::max).unwrap();
            assert!((lo - 10.).abs() < 1e-4);
            assert!(
                (f64::from(hi) - 10. - length).abs() < 1e-4,
                "the last dash must reach the next contour segment"
            );
        }
    }

    #[test]
    fn internal_joint_guides_project_to_the_owner_skin_and_hide_only_behind_nearer_faces() {
        for projection in [
            ketchup_core::Projection::Parallel,
            ketchup_core::Projection::Perspective,
        ] {
            let mut camera = Camera::default();
            camera.projection = projection;
            let size = DVec2::splat(100.);
            let inverse = camera.matrix(size).inverse();
            let world = |x: f64, y: f64, z: f64| {
                inverse.project_point3(DVec3::new(x / 50. - 1., 1. - y / 50., z))
            };
            let face = [
                world(10., 10., 0.6),
                world(90., 10., 0.6),
                world(50., 90., 0.6),
            ];
            for dashed in [false, true] {
                for alpha in [230, 255] {
                    for occluder in [None, Some(0.5), Some(0.6), Some(0.7)] {
                        let mut mesh = Mesh::new(&camera, size);
                        if let Some(depth) = occluder {
                            occluding_rectangle(&mut mesh.occlusion, DVec2::ZERO, size, depth);
                        }
                        let back = [
                            world(10., 10., 0.85),
                            world(90., 10., 0.85),
                            world(50., 90., 0.85),
                        ];
                        let mut surface = mesh.joint_surface([face, back]);
                        for triangle in &surface.triangles {
                            mesh.occlusion.insert(triangle.points);
                        }
                        mesh.joint_guide(
                            world(30., 50., 0.8),
                            world(70., 50., 0.8),
                            &mut surface,
                            1.5,
                            Color32::from_rgba_unmultiplied(0, 100, 92, alpha),
                            dashed,
                        );
                        if occluder == Some(0.5) {
                            assert!(
                                mesh.vertices.is_empty(),
                                "hidden guides must disappear completely"
                            );
                        } else {
                            assert!(!mesh.vertices.is_empty());
                            assert!(
                                mesh.vertices
                                    .iter()
                                    .all(|v| (v.position[2] - 0.6).abs() < 1e-5),
                                "projection={projection:?}, dashed={dashed}, occluder={occluder:?}: depths={:?}",
                                mesh.vertices
                                    .iter()
                                    .map(|v| v.position[2])
                                    .collect::<Vec<_>>()
                            );
                            assert!(mesh.vertices.iter().all(|v| v.color[3] == alpha));
                            mesh.normalize_parallel_depth();
                            assert!(mesh.vertices.iter().all(|v| v.position[2] > 0.));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn joint_guides_follow_sloping_faces_and_clip_the_full_width_at_foreground_edges() {
        let camera = Camera::default();
        let size = DVec2::splat(100.);
        let inverse = camera.matrix(size).inverse();
        let world = |x: f64, y: f64, z: f64| {
            inverse.project_point3(DVec3::new(x / 50. - 1., 1. - y / 50., z))
        };
        let projected = [
            DVec3::new(10., 10., 0.42),
            DVec3::new(90., 10., 0.42),
            DVec3::new(50., 90., 0.58),
        ];
        for dashed in [false, true] {
            let mut mesh = Mesh::new(&camera, size);
            mesh.occlusion.insert(projected);
            occluding_rectangle(&mut mesh.occlusion, DVec2::new(50., 0.), size, 0.3);
            let mut surface = mesh.joint_surface([projected.map(|p| world(p.x, p.y, p.z))]);
            mesh.joint_guide(
                world(40., 30., 0.8),
                world(60., 70., 0.8),
                &mut surface,
                6.,
                Color32::WHITE,
                dashed,
            );
            assert!(!mesh.vertices.is_empty());
            assert!(
                !mesh.clip_stroke_footprints,
                "restore the caller's clipping state"
            );
            for vertex in &mesh.vertices {
                let [x, y, z] = vertex.position;
                assert!(
                    x <= 1e-5,
                    "the full guide width must stop at the foreground edge"
                );
                let screen_y = (1. - y) * 50.;
                assert!(
                    (z - (0.4 + screen_y * 0.002)).abs() < 1e-5,
                    "the stroke width must follow its supporting surface depth"
                );
            }
        }
    }

    #[test]
    fn stock_supported_cut_contours_cross_their_own_notches_but_hide_behind_other_pieces() {
        let camera = Camera::default();
        let size = DVec2::splat(100.);
        let inverse = camera.matrix(size).inverse();
        let world = |x: f64, y: f64, z: f64| {
            inverse.project_point3(DVec3::new(x / 50. - 1., 1. - y / 50., z))
        };
        for foreground in [false, true] {
            let mut mesh = Mesh::new(&camera, size);
            // The evaluated board has a notch in the middle of this contour.
            for (lo, hi) in [(20., 45.), (55., 80.)] {
                occluding_rectangle(
                    &mut mesh.occlusion,
                    DVec2::new(lo, 20.),
                    DVec2::new(hi, 80.),
                    0.6,
                );
            }
            if foreground {
                occluding_rectangle(
                    &mut mesh.occlusion,
                    DVec2::new(48., 20.),
                    DVec2::new(52., 80.),
                    0.2,
                );
            }
            let p = [
                world(20., 20., 0.4),
                world(80., 20., 0.4),
                world(80., 80., 0.4),
                world(20., 80., 0.4),
            ];
            let mut surface = mesh.joint_surface([[p[0], p[1], p[2]], [p[0], p[2], p[3]]]);
            mesh.joint_guide(
                world(30., 50., 0.8),
                world(70., 50., 0.8),
                &mut surface,
                1.8,
                Color32::WHITE,
                false,
            );
            let center = DVec2::new(0., 0.);
            let covers_center = mesh.indices.chunks_exact(3).any(|indices| {
                let p = [indices[0], indices[1], indices[2]].map(|i| {
                    let p = mesh.vertices[i as usize].position;
                    DVec2::new(f64::from(p[0]), f64::from(p[1]))
                });
                let winding = (p[1] - p[0]).perp_dot(p[2] - p[0]).signum();
                [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])]
                    .into_iter()
                    .all(|(a, b)| winding * (b - a).perp_dot(center - a) >= -1e-10)
            });
            assert_eq!(
                covers_center, !foreground,
                "a notch must not interrupt its own contour, while another foreground piece must"
            );
        }
    }

    #[test]
    fn projected_joint_guides_stay_inside_their_owner_silhouette_and_leave_gaps_clear() {
        let camera = Camera::default();
        let size = DVec2::splat(100.);
        let inverse = camera.matrix(size).inverse();
        let world = |x: f64, y: f64, z: f64| {
            inverse.project_point3(DVec3::new(x / 50. - 1., 1. - y / 50., z))
        };
        for dashed in [false, true] {
            let mut mesh = Mesh::new(&camera, size);
            let mut triangles = Vec::new();
            // Two disconnected visible patches leave an opening in the owner.
            for (lo, hi) in [(20., 45.), (55., 80.)] {
                let p = [
                    world(lo, 40., 0.4),
                    world(hi, 40., 0.4),
                    world(hi, 60., 0.4),
                    world(lo, 60., 0.4),
                ];
                triangles.extend([[p[0], p[1], p[2]], [p[0], p[2], p[3]]]);
            }
            let mut surface = mesh.joint_surface(triangles);
            // Geometry behind the exterior but ahead of the internal cut must
            // not hide a guide projected onto the exterior.
            occluding_rectangle(&mut mesh.occlusion, DVec2::ZERO, size, 0.6);
            mesh.joint_guide(
                world(0., 50., 0.8),
                world(100., 50., 0.8),
                &mut surface,
                30.,
                Color32::WHITE,
                dashed,
            );
            assert!(!mesh.indices.is_empty());
            for triangle in mesh.indices.chunks_exact(3) {
                let points = triangle
                    .iter()
                    .map(|&i| mesh.vertices[i as usize].position)
                    .map(|p| [(p[0] + 1.) * 50., (1. - p[1]) * 50., p[2]])
                    .collect::<Vec<_>>();
                assert!(points.iter().all(|p| p[0] >= 20. - 1e-4
                    && p[0] <= 80. + 1e-4
                    && p[1] >= 40. - 1e-4
                    && p[1] <= 60. + 1e-4
                    && (p[2] - 0.4).abs() < 1e-5));
                assert!(
                    points.iter().all(|p| p[0] <= 45. + 1e-4)
                        || points.iter().all(|p| p[0] >= 55. - 1e-4),
                    "guides must not bridge the owner's opening"
                );
            }
        }
    }

    #[test]
    fn shadow_antialiasing_leaves_faces_strokes_and_grid_unchanged() {
        let build = |enabled| {
            let camera = Camera::default();
            let mut mesh = Mesh::new(&camera, DVec2::splat(100.));
            mesh.display.shadow_antialiasing = enabled;
            mesh.world_triangle(&camera, [DVec3::ZERO, DVec3::X, DVec3::Z], Color32::RED);
            mesh.line(
                DVec3::new(8., 32., 0.5),
                DVec3::new(56., 32., 0.5),
                2.,
                Color32::BLACK,
            );
            mesh.background_grid_line(
                [DVec2::new(8., 32.), DVec2::new(56., 32.)],
                [1., 1.],
                true,
                Color32::GRAY,
            );
            mesh.into_callback()
        };
        let enabled = build(true);
        let disabled = build(false);
        assert_eq!(
            bytemuck::cast_slice::<Vertex, u8>(&enabled.vertices),
            bytemuck::cast_slice::<Vertex, u8>(&disabled.vertices)
        );
        assert_eq!(enabled.indices, disabled.indices);
        assert_eq!(enabled.triangle_ranges, disabled.triangle_ranges);
        assert!(enabled.shadow_range.is_empty());
        assert!(disabled.shadow_range.is_empty());
    }

    #[test]
    fn shadow_antialiasing_preserves_receiver_silhouette() {
        let camera = Camera::default();
        let size = DVec2::new(1000., 800.);
        let triangle = [
            DVec3::new(-2., -2., 0.),
            DVec3::new(2., -2., 0.),
            DVec3::new(0., 2., 2.),
        ];
        let shadow = crate::shadows::FaceShadow {
            lit: vec![],
            shaded: vec![triangle],
        };
        let mut actual = Mesh::new(&camera, size);
        actual.shadow_face(&[triangle], &shadow, Color32::WHITE, Color32::GRAY);
        let mut original = Mesh::new(&camera, size);
        original.world_triangle(&camera, triangle, Color32::WHITE);
        assert!(!actual.vertices.is_empty());
        assert_eq!(actual.indices, original.indices);
        assert_eq!(actual.triangle_ranges, original.triangle_ranges);
        for (a, b) in actual.vertices.iter().zip(&original.vertices) {
            assert_eq!(a.position, b.position);
            assert_eq!(a.color, b.color);
            assert_eq!(a.shadow_color, Color32::GRAY.to_array());
        }
        let callback = actual.into_callback();
        assert!(!callback.shadow_range.is_empty());
        assert_eq!(callback.shadow_range.start, original.indices.len() as u32);
    }

    #[test]
    fn shadow_mask_partitions_include_lit_occluders_at_their_original_depth() {
        for projection in [
            ketchup_core::Projection::Perspective,
            ketchup_core::Projection::Parallel,
        ] {
            let mut camera = Camera {
                projection,
                ..Default::default()
            };
            camera.set_standard_view(ketchup_core::StandardView::Iso);
            let [a, b, c] = [
                DVec3::new(-2., -2., 0.),
                DVec3::new(2., -2., 0.),
                DVec3::new(0., 2., 0.),
            ];
            let midpoint = (a + b) * 0.5;
            let shadow = crate::shadows::FaceShadow {
                lit: vec![[a, midpoint, c]],
                shaded: vec![[midpoint, b, c]],
            };
            let mut mesh = Mesh::new(&camera, DVec2::new(640., 480.));
            mesh.display.shadow_antialiasing = true;
            mesh.shadow_face(&[[a, b, c]], &shadow, Color32::WHITE, Color32::GRAY);
            assert_eq!(mesh.shadow_vertices.len(), 6);
            for (vertices, (triangle, coverage)) in mesh.shadow_vertices.chunks_exact(3).zip([
                (shadow.lit[0], [0, 0, 0, 255]),
                (shadow.shaded[0], [255; 4]),
            ]) {
                for (vertex, point) in vertices.iter().zip(triangle) {
                    let projected = mesh.view.project_depth(point).unwrap();
                    assert_eq!(
                        vertex.position[2], projected.z as f32,
                        "mask depth must not be offset"
                    );
                    assert_eq!(
                        vertex.color, coverage,
                        "lit pieces must also occlude rear shadows"
                    );
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "Requires a GPU adapter; verifies only shadow pixels change through the shadow mask resolve"]
    fn shadow_antialiasing_smooths_shadows_without_smoothing_silhouettes() {
        use egui_wgpu::CallbackTrait;
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        eprintln!("Shadow mask backend: {:?}", adapter.get_info().backend);
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ] {
            let mut renderer = egui_wgpu::Renderer::new(
                &device,
                format,
                egui_wgpu::RendererOptions {
                    depth_stencil_format: Some(wgpu::TextureFormat::Depth32Float),
                    ..Default::default()
                },
            );
            install_resources(&device, format, &mut renderer);
            let screen = egui_wgpu::ScreenDescriptor {
                size_in_pixels: [64, 64],
                pixels_per_point: 1.,
            };
            let make_callback = |enabled| {
                let mut vertices = [
                    [-0.8, -0.8, 0.5],
                    [0.8, -0.8, 0.5],
                    [0.8, 0.8, 0.5],
                    [-0.8, 0.8, 0.5],
                ]
                .map(|position| Vertex {
                    dash_phase: -1.,
                    position,
                    color: [255, 0, 0, 255],
                    lateral: 0.,
                    shadow_color: if enabled { [0, 0, 0, 255] } else { [0; 4] },
                })
                .to_vec();
                vertices.extend(
                    [
                        [-0.5, -0.5, 0.5_f32.next_down()],
                        [0.6, -0.3, 0.5_f32.next_down()],
                        [0.1, 0.6, 0.5_f32.next_down()],
                    ]
                    .map(|position| Vertex {
                        dash_phase: -1.,
                        position,
                        color: if enabled { [255; 4] } else { [0, 0, 0, 255] },
                        lateral: 0.,
                        shadow_color: [0; 4],
                    }),
                );
                SceneCallback {
                    vertices,
                    indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6],
                    triangle_ranges: vec![0..if enabled { 6 } else { 9 }],
                    shadow_range: if enabled { 6..9 } else { 9..9 },
                    buffer_pool: None,
                    gpu_pool: Default::default(),
                    gpu_buffers: OnceLock::new(),
                    antialiasing: enabled,
                    target_size: [64, 64],
                    antialiased: OnceLock::new(),
                }
            };
            let hard = make_callback(false);
            let smooth = make_callback(true);
            let other_pane = make_callback(true);
            // The mask must not compare a partition against its own receiver,
            // even if independent projection rounds it behind the original.
            let mut rounded = make_callback(true);
            for vertex in &mut rounded.vertices[4..] {
                vertex.position[2] = 0.5_f32.next_up();
            }
            let mut hidden = make_callback(true);
            // A separate, unshadowed foreground face still hides rear masks.
            for vertex in &mut hidden.vertices[..4] {
                vertex.shadow_color = [0; 4];
            }
            for vertex in &mut hidden.vertices[4..] {
                vertex.position[2] = 0.51;
            }
            // Exercise actual orthographic depth normalization with thousands
            // of fragments on a sloped receiver, rather than a flat overlay.
            let mut fragmented_mesh = Mesh::new(
                &Camera {
                    projection: ketchup_core::Projection::Parallel,
                    ..Default::default()
                },
                DVec2::splat(64.),
            );
            fragmented_mesh.display.shadow_antialiasing = true;
            let receiver = make_callback(true);
            fragmented_mesh.vertices = receiver.vertices[..4].to_vec();
            fragmented_mesh.indices = vec![0, 1, 2, 0, 2, 3];
            fragmented_mesh.triangle_ranges = vec![0..6];
            let depth =
                |x: f32, y: f32| (1e-7 + (0.19 * f64::from(x) + 0.13 * f64::from(y)) * 1e-8) as f32;
            for vertex in &mut fragmented_mesh.vertices {
                vertex.position[2] = depth(vertex.position[0], vertex.position[1]);
            }
            for x in 0..48 {
                for y in 0..48 {
                    let x0 = -0.8 + 1.6 * x as f32 / 48.;
                    let x1 = -0.8 + 1.6 * (x + 1) as f32 / 48.;
                    let y0 = -0.8 + 1.6 * y as f32 / 48.;
                    let y1 = -0.8 + 1.6 * (y + 1) as f32 / 48.;
                    for [x, y] in [[x0, y0], [x1, y0], [x1, y1], [x0, y0], [x1, y1], [x0, y1]] {
                        fragmented_mesh.shadow_vertices.push(Vertex {
                            dash_phase: -1.,
                            position: [x, y, depth(x, y)],
                            color: [255; 4],
                            lateral: 0.,
                            shadow_color: [0; 4],
                        });
                    }
                }
            }
            fragmented_mesh.normalize_parallel_depth();
            let fragmented = fragmented_mesh.into_callback();
            // A lit receiver partition must erase rear shadow coverage even
            // when it is drawn after that coverage in the same mask pass.
            let mut lit_foreground = make_callback(true);
            for index in [0, 1, 2, 0, 2, 3] {
                let mut vertex = lit_foreground.vertices[index];
                vertex.position[2] = 0.49;
                vertex.color = [0, 0, 0, 255];
                vertex.shadow_color = [0; 4];
                lit_foreground
                    .indices
                    .push(lit_foreground.vertices.len() as u32);
                lit_foreground.vertices.push(vertex);
            }
            lit_foreground.shadow_range.end = lit_foreground.indices.len() as u32;
            let texture = |format, usage| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: 64,
                        height: 64,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let color = texture(
                format,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            let depth = texture(
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            let color_view = color.create_view(&Default::default());
            let depth_view = depth.create_view(&Default::default());
            // Repeated frames also exercise the retained resolve texture.
            let mut baseline_silhouette = None;
            for (callback, visible_shadow) in [
                (&hard, true),
                (&smooth, true),
                (&other_pane, true),
                (&smooth, true),
                (&rounded, true),
                (&hidden, false),
                (&fragmented, true),
                (&lit_foreground, false),
            ] {
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: 256 * 64,
                    mapped_at_creation: false,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                });
                let mut encoder = device.create_command_encoder(&Default::default());
                callback.prepare(
                    &device,
                    &queue,
                    &screen,
                    &mut encoder,
                    &mut renderer.callback_resources,
                );
                {
                    let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &color_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &depth_view,
                            stencil_ops: None,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.),
                                store: wgpu::StoreOp::Store,
                            }),
                        }),
                        ..Default::default()
                    });
                    let viewport = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64., 64.));
                    callback.paint(
                        egui::PaintCallbackInfo {
                            viewport,
                            clip_rect: viewport,
                            pixels_per_point: 1.,
                            screen_size_px: [64, 64],
                        },
                        &mut pass.forget_lifetime(),
                        &renderer.callback_resources,
                    );
                }
                encoder.copy_texture_to_buffer(
                    color.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &readback,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(256),
                            rows_per_image: None,
                        },
                    },
                    wgpu::Extent3d {
                        width: 64,
                        height: 64,
                        depth_or_array_layers: 1,
                    },
                );
                queue.submit([encoder.finish()]);
                let (send, receive) = std::sync::mpsc::channel();
                readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        send.send(result).unwrap();
                    });
                device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(std::time::Duration::from_secs(30)),
                    })
                    .unwrap();
                receive.recv().unwrap().unwrap();
                let mapped = readback.slice(..).get_mapped_range().unwrap();
                if visible_shadow {
                    assert!(
                        mapped.chunks_exact(4).any(|pixel| pixel[0] == 0),
                        "coplanar shadow must render, including after a rounding step"
                    );
                }
                let center = &mapped[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 4];
                assert!(
                    if visible_shadow {
                        center[0] < 8
                    } else {
                        center[0] > 248
                    },
                    "coplanar rounding must not erase shadows or expose hidden ones: {center:?}"
                );
                if std::ptr::eq(callback, &fragmented) {
                    for y in 10..54 {
                        for x in 10..54 {
                            assert!(
                                mapped[(y * 64 + x) * 4] < 8,
                                "fragmented isometric shadow must have no lit holes at {x},{y}"
                            );
                        }
                    }
                }
                let silhouette: Vec<_> = mapped.chunks_exact(4).map(|p| p[1]).collect();
                if let Some(baseline) = &baseline_silhouette {
                    assert_eq!(&silhouette, baseline);
                } else {
                    baseline_silhouette = Some(silhouette);
                }
                assert_eq!(
                    mapped
                        .chunks_exact(4)
                        .any(|pixel| pixel[0] > 0 && pixel[0] < 255),
                    callback.antialiasing && visible_shadow,
                    "AA must resolve partial shadow coverage"
                );
            }
            assert_ne!(
                smooth.antialiased.get().unwrap().bind_group,
                other_pane.antialiased.get().unwrap().bind_group
            );
        }
    }

    #[test]
    fn retained_mesh_reuses_identical_output_and_keeps_old_frames_alive() {
        let camera = Camera::default();
        let size = DVec2::new(640., 480.);
        let mut cache = SceneCache::default();
        let build = |cache: &mut SceneCache, color| {
            let mut mesh = Mesh::cached(&camera, size, cache);
            mesh.world_triangle(&camera, [DVec3::ZERO, DVec3::X, DVec3::Y], color);
            mesh.retain(cache)
        };
        let first = build(&mut cache, Color32::WHITE);
        let same = build(&mut cache, Color32::WHITE);
        assert!(Arc::ptr_eq(&first, &same));
        assert_eq!(cache.buffers.lock().unwrap().len(), 1);
        let changed = build(&mut cache, Color32::RED);
        assert!(!Arc::ptr_eq(&first, &changed));
        assert_eq!(first.vertices[0].color, Color32::WHITE.to_array());
        assert_eq!(changed.vertices[0].color, Color32::RED.to_array());
        assert!(Arc::ptr_eq(cache.retained.as_ref().unwrap(), &changed));
        // Retained frames keep their allocations until their last reference ends.
        assert!(cache.buffers.lock().unwrap().is_empty());
        drop(first);
        drop(same);
        assert_eq!(cache.buffers.lock().unwrap().len(), 1);
        assert!(!changed.vertices.is_empty());
    }

    #[test]
    fn retained_mesh_compares_indices_draw_ranges_and_normalized_parallel_depth() {
        let camera = Camera {
            projection: ketchup_core::Projection::Parallel,
            ..Default::default()
        };
        let size = DVec2::new(640., 480.);
        let mut cache = SceneCache::default();
        let build = |cache: &mut SceneCache| {
            let mut mesh = Mesh::cached(&camera, size, cache);
            mesh.world_triangle(&camera, [DVec3::ZERO, DVec3::X, DVec3::Y], Color32::WHITE);
            mesh
        };
        let first = build(&mut cache).retain(&mut cache);
        let same = build(&mut cache).retain(&mut cache);
        assert!(Arc::ptr_eq(&first, &same));
        let mut mesh = build(&mut cache);
        mesh.indices.swap(0, 1);
        let reordered = mesh.retain(&mut cache);
        assert!(!Arc::ptr_eq(&first, &reordered));
        let mut mesh = build(&mut cache);
        mesh.triangle_ranges.clear();
        let strokes = mesh.retain(&mut cache);
        assert!(!Arc::ptr_eq(&first, &strokes));
    }

    #[test]
    fn callback_batches_interleaved_faces_and_strokes_in_stable_pass_order() {
        let camera = Camera::default();
        let mut mesh = Mesh::new(&camera, DVec2::new(1000., 800.));
        // Use distinct indices to prove both pass ordering and completeness.
        mesh.indices = (0..27).collect();
        mesh.triangle_ranges = vec![6..12, 18..21, 24..27];
        let callback = mesh.into_callback();
        assert_eq!(callback.triangle_ranges, vec![0..12]);
        assert_eq!(
            callback.indices,
            [
                6, 7, 8, 9, 10, 11, 18, 19, 20, 24, 25, 26, 0, 1, 2, 3, 4, 5, 12, 13, 14, 15, 16,
                17, 21, 22, 23,
            ]
        );
        for ranges in [vec![], vec![0..27], vec![6..12]] {
            let mut mesh = Mesh::new(&camera, DVec2::new(1000., 800.));
            mesh.indices = (0..27).collect();
            mesh.triangle_ranges = ranges.clone();
            let expected_faces: Vec<_> = ranges.iter().flat_map(|range| range.clone()).collect();
            let expected_edges: Vec<_> = (0..27)
                .filter(|i| !ranges.iter().any(|r| r.contains(i)))
                .collect();
            let callback = mesh.into_callback();
            let face_end = callback
                .triangle_ranges
                .first()
                .map_or(0, |range| range.end as usize);
            assert_eq!(&callback.indices[..face_end], expected_faces);
            assert_eq!(&callback.indices[face_end..], expected_edges);
        }
    }

    #[test]
    fn adjacency_survives_vertex_movement_and_refreshes_after_topology_edits() {
        let mut model = Model::default();
        for (a, b) in [
            (DVec3::ZERO, DVec3::X),
            (DVec3::X, DVec3::Y),
            (DVec3::Y, DVec3::ZERO),
        ] {
            model.add_edge(a, b).unwrap();
        }
        let face = model.faces.values().next().unwrap().clone();
        let (a, b) = (face.vertices[0], face.vertices[1]);
        let mut cache = FaceCache::default();
        cache.update(&model);
        assert_eq!(cache.adjacent_faces(a, b), &[face.id]);
        for vertex in model.vertices.values_mut() {
            vertex.position.z += 1.;
        }
        cache.update(&model);
        assert_eq!(cache.adjacent_faces(a, b), &[face.id]);
        assert_eq!(cache.topology_builds, 1);
        model.faces.clear();
        cache.update(&model);
        assert!(cache.adjacent_faces(a, b).is_empty());
        model.faces.insert(face.id, face.clone());
        cache.update(&model);
        assert_eq!(cache.adjacent_faces(b, a), &[face.id]);
        assert_eq!(cache.topology_builds, 3);
    }

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
                dash_phase: -1.,
                position: [0., 0., z],
                color: Color32::WHITE.to_array(),
                lateral: 0.,
                shadow_color: [0; 4],
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
        assert_eq!(std::mem::size_of::<Vertex>(), 28);
        assert_eq!(std::mem::offset_of!(Vertex, shadow_color), 20);
        assert_eq!(std::mem::offset_of!(Vertex, dash_phase), 24);
        assert_eq!(std::mem::offset_of!(Vertex, color), 12);
        assert_eq!(std::mem::offset_of!(Vertex, lateral), 16);
        let size = DVec2::new(1000., 800.);
        for shared in [false, true] {
            for angle in [0_f64, 0.3, std::f64::consts::FRAC_PI_4, 1.57, 2.4] {
                let mut mesh = Mesh::new(&Camera::default(), size);
                let a = DVec2::new(500., 400.);
                let b = a + DVec2::new(angle.cos(), angle.sin()) * 100.;
                let surfaces: &[EdgeSurface] = if shared {
                    &[
                        EdgeSurface::unbounded(DVec2::new(0.002, 0.)),
                        EdgeSurface::unbounded(DVec2::new(-0.003, 0.)),
                    ]
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
            shadow_range: mesh.indices.len() as u32..mesh.indices.len() as u32,
            vertices: mesh.vertices,
            indices: mesh.indices,
            triangle_ranges: mesh.triangle_ranges,
            buffer_pool: mesh.buffer_pool,
            gpu_pool: mesh.gpu_pool,
            gpu_buffers: OnceLock::new(),
            antialiasing: false,
            target_size: [64, 64],
            antialiased: OnceLock::new(),
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "Requires a GPU adapter; verifies simultaneous viewport uploads and buffer recycling"]
    fn simultaneous_viewports_own_distinct_gpu_buffers_until_released() {
        use egui_wgpu::CallbackTrait;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("GPU adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut renderer = egui_wgpu::Renderer::new(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            egui_wgpu::RendererOptions {
                depth_stencil_format: Some(wgpu::TextureFormat::Depth32Float),
                ..Default::default()
            },
        );
        install_resources(&device, wgpu::TextureFormat::Rgba8Unorm, &mut renderer);
        let gpu_pool = GpuBufferPool::default();
        let callback = || SceneCallback {
            shadow_range: 1..1,
            vertices: vec![Vertex {
                dash_phase: -1.,
                position: [0., 0., 0.5],
                color: [255; 4],
                lateral: 0.,
                shadow_color: [0; 4],
            }],
            indices: vec![0],
            triangle_ranges: vec![],
            buffer_pool: None,
            gpu_pool: gpu_pool.clone(),
            gpu_buffers: OnceLock::new(),
            antialiasing: false,
            target_size: [64, 64],
            antialiased: OnceLock::new(),
        };
        let first = callback();
        let second = callback();
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [1000, 700],
            pixels_per_point: 1.,
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        first.prepare(
            &device,
            &queue,
            &screen,
            &mut encoder,
            &mut renderer.callback_resources,
        );
        second.prepare(
            &device,
            &queue,
            &screen,
            &mut encoder,
            &mut renderer.callback_resources,
        );
        let original = first.gpu_buffers.get().unwrap().vertices.clone();
        assert_ne!(original, second.gpu_buffers.get().unwrap().vertices);
        assert_ne!(
            first.gpu_buffers.get().unwrap().indices,
            second.gpu_buffers.get().unwrap().indices
        );
        assert!(gpu_pool.lock().unwrap().is_empty());
        drop(first);
        assert_eq!(gpu_pool.lock().unwrap().len(), 1);
        let third = callback();
        third.prepare(
            &device,
            &queue,
            &screen,
            &mut encoder,
            &mut renderer.callback_resources,
        );
        assert_eq!(original, third.gpu_buffers.get().unwrap().vertices);
        assert_ne!(
            third.gpu_buffers.get().unwrap().vertices,
            second.gpu_buffers.get().unwrap().vertices
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "Requires a GPU adapter; verifies incident face depth outside its silhouette"]
    fn rear_face_boundary_does_not_hide_foreground_selection_outside_the_face() {
        use wgpu::util::DeviceExt;
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(include_str!("edges.wgsl").into()),
        });
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let pipelines = [PipelinePass::EdgeDepth, PipelinePass::Edges]
            .map(|pass| create_pipeline(&device, &shader, format, pass));
        let size = wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        };
        let texture = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = texture(
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = texture(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());

        let camera = Camera::default();
        let viewport = DVec2::splat(64.);
        let inverse = camera.matrix(viewport).inverse();
        let unproject = |p: DVec3| {
            let q = inverse * glam::DVec4::new(p.x / 32. - 1., 1. - p.y / 32., p.z, 1.);
            q.truncate() / q.w
        };
        // A rear triangle exists only to the RIGHT of its vertical boundary.
        // Its depth slopes toward the eye if extrapolated LEFT, outside the face.
        let points = [
            DVec3::new(32., 8., 0.6),
            DVec3::new(34., 8., 0.9),
            DVec3::new(32., 56., 0.6),
        ]
        .map(unproject);
        let mut model = Model::default();
        let boundary = model.add_edge(points[0], points[2]).unwrap();
        model.add_edge(points[2], points[1]).unwrap();
        model.add_edge(points[1], points[0]).unwrap();
        let mut faces = FaceCache::default();
        faces.update(&model);
        let mut mesh = Mesh::new(&camera, viewport);
        for face in model.faces.values() {
            mesh.face_edges(face, faces.get(face.id));
            for &triangle in &faces.get(face.id).triangles {
                mesh.world_triangle(&camera, triangle, Color32::WHITE);
            }
        }
        assert_eq!(model.faces.len(), 1, "exercise a real incident face");
        mesh.styled_edge(
            &model.edges[&boundary],
            &model,
            1.8,
            Color32::BLACK,
            false,
            false,
        );
        mesh.begin_selected_edges();
        mesh.line(
            DVec3::new(8., 32., 0.55),
            DVec3::new(56., 32., 0.55),
            3.,
            Color32::RED,
        );
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * 64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
            let faces = create_pipeline(&device, &shader, format, PipelinePass::Faces);
            pass.set_pipeline(&faces);
            for range in &mesh.triangle_ranges {
                pass.draw_indexed(range.clone(), 0, 0..1);
            }
            let edge_start = mesh.triangle_ranges.last().map_or(0, |range| range.end);
            for pipeline in &pipelines {
                pass.set_pipeline(pipeline);
                pass.draw_indexed(edge_start..mesh.indices.len() as u32, 0, 0..1);
            }
        }
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(64),
                },
            },
            size,
        );
        queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap();
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .unwrap();
        receive.recv().unwrap().unwrap();
        let mapped = readback.slice(..).get_mapped_range().unwrap();

        let sample = &mapped[(32 * 256 + 31 * 4)..(32 * 256 + 31 * 4 + 4)];
        assert_eq!(
            sample,
            [255, 0, 0, 255],
            "rear boundary must not advance beyond its face silhouette"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "Requires a GPU adapter; verifies edge occlusion and overlay pixels"]
    fn crossing_selection_edges_respect_depth_in_both_draw_orders() {
        use wgpu::util::DeviceExt;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(include_str!("edges.wgsl").into()),
        });
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let pipelines = [PipelinePass::EdgeDepth, PipelinePass::Edges]
            .map(|pass| create_pipeline(&device, &shader, format, pass));
        let size = wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        };
        let texture = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = texture(
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = texture(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        for (reverse, hybrid) in [false, true].into_iter().flat_map(|reverse| [false, true].map(|hybrid| (reverse, hybrid))) {
            // Depth zero is used by intentional x-ray/wireframe overlays.
            for rear_depth in [0.7, 0.] {
                let mut mesh = Mesh::new(&Camera::default(), DVec2::splat(64.));
                if hybrid { mesh.enable_gpu_edge_occlusion(); }
                let strokes = [
                    (
                        DVec3::new(8., 32., 0.3),
                        DVec3::new(56., 32., 0.3),
                        1.8,
                        Color32::BLACK,
                    ),
                    (
                        DVec3::new(32., 8., rear_depth),
                        DVec3::new(32., 56., rear_depth),
                        3.,
                        Color32::RED,
                    ),
                ];
                for index in if reverse { [1, 0] } else { [0, 1] } {
                    let (a, b, width, color) = strokes[index];
                    mesh.line(a, b, width, color);
                }
                let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&mesh.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&mesh.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: 256 * 64,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &color_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        ..Default::default()
                    });
                    pass.set_vertex_buffer(0, vertices.slice(..));
                    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                    for pipeline in &pipelines {
                        pass.set_pipeline(pipeline);
                        pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
                    }
                }
                encoder.copy_texture_to_buffer(
                    color.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &readback,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(256),
                            rows_per_image: Some(64),
                        },
                    },
                    size,
                );
                queue.submit([encoder.finish()]);
                let (send, receive) = std::sync::mpsc::channel();
                readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        send.send(result).unwrap();
                    });
                device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(std::time::Duration::from_secs(30)),
                    })
                    .unwrap();
                receive.recv().unwrap().unwrap();
                let mapped = readback.slice(..).get_mapped_range().unwrap();
                let pixel = |x: usize, y: usize| &mapped[(y * 256 + x * 4)..(y * 256 + x * 4 + 4)];
                if rear_depth > 0. {
                    let crossing = pixel(32, 32);
                    assert!(crossing[0] < 64, "near edge wins, reverse={reverse}");
                    assert_eq!(crossing[0], crossing[1]);
                    assert_eq!(crossing[1], crossing[2]);
                } else if !reverse {
                    assert_eq!(pixel(32, 32), [255, 0, 0, 255], "overlay stays visible");
                }
                assert_eq!(
                    pixel(32, 16),
                    [255, 0, 0, 255],
                    "selection color is preserved"
                );
                assert!(pixel(33, 16)[1] > 0, "antialiased fringe still blends");
            }
        }
    }

    #[test]
    fn wide_diagonal_selection_clipping_respects_depth_across_its_entire_footprint() {
        for rear_depth in [0.6, 0.3, 0.2] {
            let mut mesh = Mesh::new(&Camera::default(), DVec2::splat(100.));
            mesh.line(
                DVec3::new(50., 0., 0.3),
                DVec3::new(50., 100., 0.3),
                2.,
                Color32::WHITE,
            );
            mesh.begin_selected_edges();
            let start = mesh.indices.len();
            mesh.surface_stroke(
                DVec3::new(20., 20., rear_depth),
                DVec3::new(80., 80., rear_depth),
                6.,
                Color32::RED,
                &[],
            );
            assert!(mesh.indices.len() > start);
            let mut crosses_ordinary_outline = false;
            for triangle in mesh.indices[start..].chunks_exact(3) {
                let xs = [triangle[0], triangle[1], triangle[2]]
                    .map(|i| (f64::from(mesh.vertices[i as usize].position[0]) + 1.) * 50.);
                let crosses =
                    xs.iter().any(|&x| x < 48.5 - 1e-5) && xs.iter().any(|&x| x > 51.5 + 1e-5);
                crosses_ordinary_outline |= crosses;
                if rear_depth > 0.3 {
                    assert!(
                        xs.iter().all(|&x| x <= 48.5 + 1e-5)
                            || xs.iter().all(|&x| x >= 51.5 - 1e-5),
                        "rear selection triangle crosses the foreground stroke: {xs:?}"
                    );
                }
            }
            if rear_depth <= 0.3 {
                assert!(
                    crosses_ordinary_outline,
                    "visible selection was clipped at depth {rear_depth}"
                );
            }
        }
    }

    #[test]
    fn edge_occlusion_preserves_depth_differences_lost_by_gpu_rounding() {
        let mut mesh = Mesh::new(&Camera::default(), DVec2::splat(100.));
        let near = 0.5_f64;
        let rear = near + 1e-10;
        assert_eq!(near as f32, rear as f32);
        mesh.line(
            DVec3::new(50., 10., near),
            DVec3::new(50., 90., near),
            1.8,
            Color32::BLACK,
        );
        mesh.occlude_annotations_with_edges();
        let spans = mesh
            .occlusion
            .visible_intervals(DVec3::new(10., 50., rear), DVec3::new(90., 50., rear));
        assert_eq!(spans.len(), 2);
        assert!(spans[0].1 < 0.5 && spans[1].0 > 0.5);
    }

    #[test]
    fn foreground_edges_clip_annotation_strokes_and_label_footprints() {
        let mut mesh = Mesh::new(&Camera::default(), DVec2::splat(100.));
        mesh.line(
            DVec3::new(50., 10., 0.3),
            DVec3::new(50., 90., 0.3),
            2.,
            Color32::BLACK,
        );
        mesh.occlude_annotations_with_edges();
        let a = DVec3::new(10., 50., 0.6);
        let b = DVec3::new(90., 50., 0.6);
        let lines = mesh.visible_annotation_line(a, b);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].1.x < 49. && lines[1].0.x > 51.);
        assert_eq!(
            mesh.visible_annotation_line(a.truncate().extend(0.2), b.truncate().extend(0.2)),
            vec![(a.truncate().extend(0.2), b.truncate().extend(0.2))]
        );
        let polygon = vec![
            DVec2::new(40., 40.),
            DVec2::new(60., 40.),
            DVec2::new(60., 60.),
            DVec2::new(40., 60.),
        ];
        let visible = mesh.occlusion.visible_polygons(polygon, 0.6);
        assert!((polygon_area(&visible) - 340.).abs() < 1e-7);
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

    #[test]
    fn coplanar_occluders_do_not_fragment_visible_strokes() {
        let mut occlusion = Occlusion::default();
        let plane = |x, y| DVec3::new(x, y, 0.4 + x * 0.001);
        for i in 0..128 {
            let offset = f64::from(i) * 0.1;
            occlusion.insert([
                plane(0., offset),
                plane(100., offset),
                plane(50., 100. + offset),
            ]);
        }
        let polygon = vec![
            DVec2::new(40., 40.),
            DVec2::new(60., 40.),
            DVec2::new(60., 60.),
            DVec2::new(40., 60.),
        ];
        let visible = occlusion.visible_surface_polygons(
            polygon.clone(),
            plane(0., 0.),
            DVec2::new(0.001, 0.),
        );
        assert_eq!(visible, vec![polygon]);
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
                &[
                    EdgeSurface::unbounded(DVec2::new(0., 0.1)),
                    EdgeSurface::unbounded(DVec2::new(0., -0.1)),
                ],
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
                    shadow_antialiasing: false,
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
            assert!(
                stroke_depth(0.5, offset, &[EdgeSurface::unbounded(gradient)]) <= surface_depth
            );
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
                    &[
                        EdgeSurface::unbounded(DVec2::new(0.002, 0.)),
                        EdgeSurface::unbounded(DVec2::new(-0.003, 0.)),
                    ],
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
                let surface = mesh.edge_surfaces[&(a.min(b), a.max(b))][0];
                let midpoint = mesh
                    .view
                    .project((model.vertices[&a].position + model.vertices[&b].position) * 0.5)
                    .unwrap();
                assert!(
                    cache
                        .get(face.id)
                        .hit(mesh.view.ray(midpoint + surface.inward * 0.25))
                        .is_some(),
                    "face-side allowance must point into material, including at holes"
                );
                assert!(
                    cache
                        .get(face.id)
                        .hit(mesh.view.ray(midpoint - surface.inward * 0.25))
                        .is_none(),
                    "silhouette-side allowance must not extrapolate into empty space"
                );
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
    revision: u64,
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
        self.revision = self.revision.wrapping_add(1);
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
    pub fn revision(&self) -> u64 {
        self.revision
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

#[cfg(test)]
mod reference_axis_tests {
    use super::*;

    #[test]
    fn axes_fade_without_changing_projected_depth_or_direction_colors() {
        for projection in [
            ketchup_core::Projection::Perspective,
            ketchup_core::Projection::Parallel,
        ] {
            for distance in [0.1, 15., 1000.] {
                for size in [DVec2::new(1200., 800.), DVec2::new(400., 900.)] {
                    let camera = Camera {
                        projection,
                        distance,
                        ..Default::default()
                    };
                    let fade = ReferenceFade::new(&camera);
                    let direction = (camera.target - camera.eye()).normalize();
                    // A diagonal receding reference line remains visible over a
                    // useful depth range in both projection modes.
                    let side = direction.cross(DVec3::Z).normalize();
                    let a = DVec3::ZERO;
                    let b = a + (direction + side * 0.1) * distance * 8.;
                    for color in [Color32::RED, Color32::GREEN, Color32::BLUE] {
                        let mut faded = Mesh::new(&camera, size);
                        faded.axis_line(&camera, a, b, 1.1, color);
                        assert!(!faded.vertices.is_empty());
                        let alpha: Vec<_> = faded.vertices.iter().map(|v| v.color[3]).collect();
                        assert!(alpha.windows(2).all(|v| v[0] >= v[1]));
                        assert!(alpha.last().unwrap() < alpha.first().unwrap());
                        for v in &faded.vertices {
                            assert!(v.position.iter().all(|x| x.is_finite()));
                            for channel in 0..3 {
                                if color[channel] == 0 {
                                    assert_eq!(v.color[channel], 0);
                                }
                            }
                        }
                    }
                    assert!(
                        ReferenceFade::opacity(fade.depth(DVec3::ZERO), fade.origin_depth) >= 0.999
                    );
                    assert!(fade.axis_length() > camera.eye().length() + distance * 200.);
                }
            }
        }
    }
}
