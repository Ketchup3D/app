use crate::{Camera, CameraView, Model};
use glam::{DVec2, DVec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Axis {
    X,
    Y,
    Z,
}
impl Axis {
    pub fn vector(self) -> DVec3 {
        match self {
            Self::X => DVec3::X,
            Self::Y => DVec3::Y,
            Self::Z => DVec3::Z,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::X => "Red axis",
            Self::Y => "Green axis",
            Self::Z => "Blue axis",
        }
    }
}

/// An explicit movement direction or a world plane through the starting point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisConstraint {
    Axis(Axis),
    Plane(Axis),
}

impl AxisConstraint {
    pub fn project(self, delta: DVec3) -> DVec3 {
        match self {
            Self::Axis(axis) => axis.vector() * delta.dot(axis.vector()),
            Self::Plane(normal) => delta - normal.vector() * delta.dot(normal.vector()),
        }
    }

    pub fn contains(self, delta: DVec3) -> bool {
        (delta - self.project(delta)).length() < 1e-6
    }

    fn point(self, ray: crate::Ray, start: DVec3) -> Option<DVec3> {
        match self {
            Self::Axis(axis) => ray.axis(start, axis.vector()),
            Self::Plane(normal) => {
                let denominator = ray.direction.dot(normal.vector());
                if denominator.abs() < 1e-9 {
                    return None;
                }
                let t = (start - ray.origin).dot(normal.vector()) / denominator;
                (0.0..1e7)
                    .contains(&t)
                    .then(|| ray.origin + ray.direction * t)
            }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inference {
    Plane,
    Endpoint,
    Midpoint,
    FaceCenter,
    Origin,
    OnEdge,
    OnFace,
    OnAxis(Axis),
    Axis(Axis),
    Aligned,
}
impl Inference {
    pub fn label(self) -> &'static str {
        match self {
            Self::Plane => "On plane",
            Self::Endpoint => "Endpoint",
            Self::Midpoint => "Midpoint",
            Self::FaceCenter => "Face center",
            Self::Origin => "Origin",
            Self::OnEdge => "On edge",
            Self::OnFace => "On face",
            Self::OnAxis(Axis::X) => "On red axis",
            Self::OnAxis(Axis::Y) => "On green axis",
            Self::OnAxis(Axis::Z) => "On blue axis",
            Self::Axis(a) => a.label(),
            Self::Aligned => "Aligned axes",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Snap {
    pub point: DVec3,
    pub kind: Inference,
}

/// Independent geometry targets; edges include endpoints and midpoints.
#[derive(Clone, Copy, Debug)]
pub struct SnapTargets {
    pub edges: bool,
    pub faces: bool,
    pub face_centers: bool,
}

impl Default for SnapTargets {
    fn default() -> Self {
        Self {
            edges: true,
            faces: true,
            face_centers: true,
        }
    }
}

/// Cached face queries for snapping. Scoped queries include descendants of the
/// current editing level; unscoped queries include every visible surface.
pub trait FaceQuery {
    fn nearest(&self, ray: crate::Ray, scoped: bool) -> Option<(u64, f64, DVec3)>;

    /// Reuse inherited visibility when the caller has already prepared it.
    fn is_hidden(&self, model: &Model, id: u64) -> bool {
        model.is_hidden(id)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AxisGuide {
    pub origin: DVec3,
    pub axis: Axis,
}

#[derive(Clone, Debug)]
pub struct MoveInference {
    pub snap: Snap,
    pub guides: Vec<AxisGuide>,
}

/// Combine compatible world-space guides, rather than choosing only one axis.
/// Preserve explicit axis locks and exact geometry targets.
pub fn infer_move_with_snapping(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    cursor: DVec2,
    start: DVec3,
    lock: Option<Axis>,
    snap_to_geometry: bool,
    snap_to_axes: bool,
) -> Option<MoveInference> {
    infer_move(
        model,
        camera,
        size,
        cursor,
        start,
        lock.map(AxisConstraint::Axis),
        snap_to_geometry,
        snap_to_axes,
        None,
        None,
        SnapTargets::default(),
    )
}

/// Geometry and projected alignment guides remain fixed during a Move gesture.
/// Recreate this cache when the source model changes; camera edits invalidate
/// only its projected data and lazily evaluated reference visibility.
pub struct MoveSnapCache {
    geometry: Model,
    faces: MoveFaces,
    guides: Option<MoveGuides>,
}
struct MoveGuides {
    camera: Camera,
    size: DVec2,
    start: DVec3,
    references: Vec<MoveReference>,
}
struct MoveReference {
    origin: DVec3,
    pixel: Option<DVec2>,
    directions: [Option<DVec2>; 3],
    visible: std::cell::OnceCell<bool>,
}
impl MoveSnapCache {
    pub fn new(model: &Model) -> Self {
        Self::excluding(model, &std::collections::BTreeSet::new())
    }

    /// Omit geometry being edited from both targets and occlusion queries.
    pub fn excluding(model: &Model, excluded: &std::collections::BTreeSet<u64>) -> Self {
        // Snapping needs world geometry and scoped occluders, not the full
        // hierarchy. Resolve inherited visibility/membership once per gesture.
        let mut geometry = Model {
            vertices: model.vertices.clone(),
            edges: model
                .edges
                .iter()
                .filter(|(id, _)| !excluded.contains(*id) && !model.is_hidden(**id))
                .map(|(id, edge)| (*id, edge.clone()))
                .collect(),
            faces: model
                .faces
                .iter()
                .filter(|(id, _)| !excluded.contains(*id) && !model.is_hidden(**id))
                .map(|(id, face)| (*id, face.clone()))
                .collect(),
            edit_context: model.edit_context,
            ..Model::default()
        };
        if let Some(scope) = model.edit_context.and_then(|id| model.objects.get(&id)) {
            let context = scope.id;
            let mut scope = scope.clone();
            scope.entities = geometry
                .edges
                .keys()
                .chain(geometry.faces.keys())
                .filter(|id| model.entity_in_context(**id).is_some())
                .copied()
                .collect();
            geometry.objects.insert(context, scope);
        }
        let faces = MoveFaces::new(&geometry);
        Self {
            geometry,
            faces,
            guides: None,
        }
    }
    pub fn infer(
        &mut self,
        camera: &Camera,
        size: DVec2,
        cursor: DVec2,
        start: DVec3,
        lock: Option<Axis>,
        snap_to_geometry: bool,
        snap_to_axes: bool,
    ) -> Option<MoveInference> {
        self.infer_with_targets(
            camera,
            size,
            cursor,
            start,
            lock,
            snap_to_geometry,
            snap_to_axes,
            SnapTargets::default(),
        )
    }

    pub fn infer_with_targets(
        &mut self,
        camera: &Camera,
        size: DVec2,
        cursor: DVec2,
        start: DVec3,
        lock: Option<Axis>,
        snap_to_geometry: bool,
        snap_to_axes: bool,
        targets: SnapTargets,
    ) -> Option<MoveInference> {
        self.infer_with_constraint(
            camera,
            size,
            cursor,
            start,
            lock.map(AxisConstraint::Axis),
            snap_to_geometry,
            snap_to_axes,
            targets,
        )
    }

    pub fn infer_with_constraint(
        &mut self,
        camera: &Camera,
        size: DVec2,
        cursor: DVec2,
        start: DVec3,
        lock: Option<AxisConstraint>,
        snap_to_geometry: bool,
        snap_to_axes: bool,
        targets: SnapTargets,
    ) -> Option<MoveInference> {
        if self
            .guides
            .as_ref()
            .is_none_or(|g| g.camera != *camera || g.size != size || g.start != start)
        {
            self.guides = Some(MoveGuides {
                camera: camera.clone(),
                size,
                start,
                references: move_references(
                    &self.geometry,
                    camera,
                    &camera.view(size),
                    start,
                    true,
                    None,
                ),
            });
        }
        infer_move(
            &self.geometry,
            camera,
            size,
            cursor,
            start,
            lock,
            snap_to_geometry,
            snap_to_axes,
            Some(&self.guides.as_ref().unwrap().references),
            Some(&self.faces),
            targets,
        )
    }
}

fn move_references(
    model: &Model,
    camera: &Camera,
    view: &CameraView,
    start: DVec3,
    geometry: bool,
    faces: Option<&dyn FaceQuery>,
) -> Vec<MoveReference> {
    let is_hidden = |id| match faces {
        Some(query) => query.is_hidden(model, id),
        None => model.is_hidden(id),
    };
    let mut references = vec![start, DVec3::ZERO];
    if geometry {
        let vertices: std::collections::BTreeSet<_> = model
            .edges
            .values()
            .filter(|e| !is_hidden(e.id))
            .flat_map(|e| [e.a, e.b])
            .collect();
        references.extend(vertices.iter().map(|id| model.vertices[id].position));
        references.extend(model.edges.values().filter(|e| !is_hidden(e.id)).map(|e| {
            let (a, b) = model.points(e);
            (a + b) * 0.5
        }));
    }
    references
        .into_iter()
        .map(|origin| {
            let pixel = view.project(origin);
            let directions = [Axis::X, Axis::Y, Axis::Z].map(|axis| {
                let direction = view
                    .project(origin + axis.vector() * camera.distance)
                    .or_else(|| view.project(origin - axis.vector() * camera.distance))?
                    - pixel?;
                (direction.length_squared() >= 1e-8).then_some(direction)
            });
            MoveReference {
                origin,
                pixel,
                directions,
                visible: std::cell::OnceCell::new(),
            }
        })
        .collect()
}

fn infer_move(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    cursor: DVec2,
    start: DVec3,
    lock: Option<AxisConstraint>,
    snap_to_geometry: bool,
    snap_to_axes: bool,
    prepared: Option<&[MoveReference]>,
    faces: Option<&dyn FaceQuery>,
    targets: SnapTargets,
) -> Option<MoveInference> {
    let base = infer_with_constraint(
        model,
        camera,
        size,
        cursor,
        Some(start),
        lock,
        snap_to_geometry,
        snap_to_axes,
        faces,
        targets,
    )?;
    if !snap_to_axes {
        return Some(MoveInference {
            snap: base,
            guides: vec![],
        });
    }
    Some(combine_axis_guides(
        model,
        camera,
        size,
        cursor,
        start,
        lock,
        snap_to_geometry && targets.edges,
        base,
        prepared,
        faces,
    ))
}

/// Refine a drawing snap at the world-space intersection of compatible axis
/// guides. Call only when automatic axis snapping is enabled. The supplied base
/// retains directional/geometry inference and the viewport's cached face query.
pub fn infer_axis_alignment_with_face_query(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    cursor: DVec2,
    start: DVec3,
    lock: Option<Axis>,
    snap_to_geometry: bool,
    base: Snap,
    faces: Option<&dyn FaceQuery>,
) -> MoveInference {
    infer_axis_alignment_with_constraint(
        model,
        camera,
        size,
        cursor,
        start,
        lock.map(AxisConstraint::Axis),
        snap_to_geometry,
        base,
        faces,
    )
}

pub fn infer_axis_alignment_with_constraint(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    cursor: DVec2,
    start: DVec3,
    lock: Option<AxisConstraint>,
    snap_to_geometry: bool,
    base: Snap,
    faces: Option<&dyn FaceQuery>,
) -> MoveInference {
    combine_axis_guides(
        model,
        camera,
        size,
        cursor,
        start,
        lock,
        snap_to_geometry,
        base,
        None,
        faces,
    )
}

fn combine_axis_guides(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    cursor: DVec2,
    start: DVec3,
    lock: Option<AxisConstraint>,
    snap_to_geometry: bool,
    base: Snap,
    prepared: Option<&[MoveReference]>,
    faces: Option<&dyn FaceQuery>,
) -> MoveInference {
    let fallback = MoveInference {
        snap: base,
        guides: vec![],
    };
    if matches!(
        base.kind,
        Inference::Endpoint | Inference::Midpoint | Inference::FaceCenter | Inference::Origin
    ) {
        return fallback;
    }
    let view = camera.view(size);
    let temporary;
    let references = if let Some(prepared) = prepared {
        prepared
    } else {
        temporary = move_references(model, camera, &view, start, snap_to_geometry, faces);
        &temporary
    };
    let axes = [Axis::X, Axis::Y, Axis::Z];
    let guides: [Vec<AxisGuide>; 3] = std::array::from_fn(|i| {
        let mut nearby: Vec<_> = references
            .iter()
            .enumerate()
            .take(if snap_to_geometry {
                references.len()
            } else {
                2
            })
            .filter_map(|(index, reference)| {
                let origin = reference.origin;
                let projected = reference.pixel?;
                let direction = reference.directions[i]?;
                let distance = (cursor - projected).perp_dot(direction).abs() / direction.length();
                if distance > 10.
                    || (index >= 2
                        && !*reference
                            .visible
                            .get_or_init(|| query_visible(model, &view, origin, faces)))
                {
                    return None;
                }
                Some((
                    distance,
                    projected.distance(cursor),
                    AxisGuide {
                        origin,
                        axis: axes[i],
                    },
                ))
            })
            .collect();
        nearby.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        // Bound pair enumeration even for large models.
        nearby
            .into_iter()
            .take(24)
            .map(|(_, _, guide)| guide)
            .collect()
    });
    let mut intersections = Vec::new();
    for i in 0..3 {
        for j in i + 1..3 {
            let k = 3 - i - j;
            for a in &guides[i] {
                for b in &guides[j] {
                    if (a.origin[k] - b.origin[k]).abs() > 1e-7 {
                        continue;
                    }
                    let mut point = a.origin;
                    point[i] = b.origin[i];
                    if point.distance(start) < 1e-7
                        || point.distance(a.origin) < 1e-7
                        || point.distance(b.origin) < 1e-7
                    {
                        continue;
                    }
                    if lock.is_some_and(|constraint| !constraint.contains(point - start)) {
                        continue;
                    }
                    let Some(pixel) = view.project(point) else {
                        continue;
                    };
                    let distance = pixel.distance(cursor);
                    if distance <= 10. {
                        intersections.push((
                            distance,
                            point.distance_squared(base.point),
                            point,
                            *a,
                            *b,
                        ));
                    }
                }
            }
        }
    }
    intersections.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let is_hidden = |id| match faces {
        Some(query) => query.is_hidden(model, id),
        None => model.is_hidden(id),
    };
    for (_, _, point, a, b) in intersections {
        // A guide may refine a surface/edge snap, but must stay on its geometry.
        if base.kind == Inference::OnFace
            && !model
                .faces
                .values()
                .filter(|f| !is_hidden(f.id))
                .any(|f| model.face_contains(f, point, true))
        {
            continue;
        }
        if base.kind == Inference::OnEdge
            && !model.edges.values().filter(|e| !is_hidden(e.id)).any(|e| {
                let (a, b) = model.points(e);
                let t = ((point - a).dot(b - a) / (b - a).length_squared()).clamp(0., 1.);
                (a + (b - a) * t).distance(point) < 1e-7
            })
        {
            continue;
        }
        if !query_visible(model, &view, point, faces) {
            continue;
        }
        let mut combined = vec![a, b];
        for (i, candidates) in guides.iter().enumerate() {
            if combined.iter().any(|g| g.axis == axes[i]) {
                continue;
            }
            if let Some(guide) = candidates.iter().find(|g| {
                (point - g.origin).cross(g.axis.vector()).length() < 1e-7
                    && point.distance(g.origin) > 1e-7
            }) {
                combined.push(*guide);
            }
        }
        return MoveInference {
            snap: Snap {
                point,
                kind: Inference::Aligned,
            },
            guides: combined,
        };
    }
    fallback
}

pub fn infer(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    cursor: DVec2,
    start: Option<DVec3>,
    lock: Option<Axis>,
) -> Option<Snap> {
    infer_with_snapping(model, camera, size, cursor, start, lock, true, true)
}

pub fn infer_with_snapping(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    cursor: DVec2,
    start: Option<DVec3>,
    lock: Option<Axis>,
    snap_to_geometry: bool,
    snap_to_axes: bool,
) -> Option<Snap> {
    infer_with_face_query(
        model,
        camera,
        size,
        cursor,
        start,
        lock,
        snap_to_geometry,
        snap_to_axes,
        None,
    )
}
/// Use prepared face geometry/spatial indexes for repeated pointer queries.
pub fn infer_with_face_query(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    cursor: DVec2,
    start: Option<DVec3>,
    lock: Option<Axis>,
    snap_to_geometry: bool,
    snap_to_axes: bool,
    faces: Option<&dyn FaceQuery>,
) -> Option<Snap> {
    infer_with_targets(
        model,
        camera,
        size,
        cursor,
        start,
        lock,
        snap_to_geometry,
        snap_to_axes,
        faces,
        SnapTargets::default(),
    )
}

/// Infer a target while independently enabling edges, faces, and face centers.
pub fn infer_with_targets(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    cursor: DVec2,
    start: Option<DVec3>,
    lock: Option<Axis>,
    snap_to_geometry: bool,
    snap_to_axes: bool,
    faces: Option<&dyn FaceQuery>,
    targets: SnapTargets,
) -> Option<Snap> {
    infer_with_constraint(
        model,
        camera,
        size,
        cursor,
        start,
        lock.map(AxisConstraint::Axis),
        snap_to_geometry,
        snap_to_axes,
        faces,
        targets,
    )
}

pub fn infer_with_constraint(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    cursor: DVec2,
    start: Option<DVec3>,
    lock: Option<AxisConstraint>,
    snap_to_geometry: bool,
    snap_to_axes: bool,
    faces: Option<&dyn FaceQuery>,
    targets: SnapTargets,
) -> Option<Snap> {
    let snap_to_geometry =
        snap_to_geometry && (targets.edges || targets.faces || targets.face_centers);
    let is_hidden = |id| match faces {
        Some(query) => query.is_hidden(model, id),
        None => model.is_hidden(id),
    };
    let view = camera.view(size);
    let ray = view.ray(cursor);
    let away_from_start = start
        .and_then(|p| view.project(p))
        .is_some_and(|p| p.distance(cursor) > 4.0);
    let local_axis =
        if snap_to_axes && !matches!(lock, Some(AxisConstraint::Axis(_))) && away_from_start {
            start
                .and_then(|origin| axis_target(camera, &view, cursor, origin, false))
                .filter(|target| {
                    lock.is_none_or(|constraint| {
                        constraint.contains(target.snap.point - start.unwrap())
                    })
                })
        } else {
            None
        };
    let mut candidates = vec![(DVec3::ZERO, Inference::Origin)];
    let visible_vertices: std::collections::BTreeSet<_> = model
        .edges
        .values()
        .filter(|e| snap_to_geometry && targets.edges && !is_hidden(e.id))
        .flat_map(|e| [e.a, e.b])
        .collect();
    candidates.extend(
        visible_vertices
            .iter()
            .map(|id| (model.vertices[id].position, Inference::Endpoint)),
    );
    candidates.extend(
        model
            .edges
            .values()
            .filter(|e| snap_to_geometry && targets.edges && !is_hidden(e.id))
            .map(|e| {
                let (a, b) = model.points(e);
                ((a + b) * 0.5, Inference::Midpoint)
            }),
    );
    if snap_to_geometry && targets.face_centers {
        candidates.extend(
            model
                .faces
                .values()
                .filter(|face| !is_hidden(face.id))
                .filter_map(|face| {
                    model
                        .face_center(face)
                        .map(|center| (center, Inference::FaceCenter))
                }),
        );
    }
    let nearby = candidates
        .into_iter()
        .filter(|_| snap_to_geometry)
        .filter(|(p, _)| {
            // Do not let the point just committed swallow short following segments.
            if away_from_start && start.is_some_and(|start| p.distance(start) < 1e-7) {
                return false;
            }
            if let (Some(start), Some(constraint)) = (start, lock) {
                constraint.contains(*p - start)
            } else {
                true
            }
        })
        .filter_map(|(point, kind)| {
            let distance = view.project(point)?.distance(cursor);
            if distance >= 11.0 {
                return None;
            }
            if let (Some(start), Some(target)) = (start, local_axis) {
                // Avoid collapsing short segments onto geometry beside their
                // start, without blocking targets at the far end of a line.
                let on_axis = (point - start).cross(target.axis.vector()).length() < 1e-7;
                let near_start = view
                    .project(point)
                    .zip(view.project(start))
                    .is_some_and(|(a, b)| a.distance(b) < 16.0);
                if near_start && !on_axis && distance > 4.0 && distance > target.distance + 3.0 {
                    return None;
                }
            }
            query_visible(model, &view, point, faces).then_some((distance, Snap { point, kind }))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, snap)) = nearby {
        return Some(snap);
    }
    // Automatic alignment is a soft hint. Prefer a shared axis/geometry target,
    // then allow other nearby geometry unless the user explicitly locked an axis.
    let axis_lock = match lock {
        Some(AxisConstraint::Axis(axis)) => Some(axis),
        _ => None,
    };
    let constraint = start.zip(axis_lock.or(local_axis.map(|target| target.axis)));
    let locked = start.zip(lock);
    if snap_to_geometry {
        let on_edge = model
            .edges
            .values()
            .filter(|edge| targets.edges && !is_hidden(edge.id))
            .filter_map(|edge| {
                let (a, b) = model.points(edge);
                let plane_joint = locked.and_then(|(start, constraint)| {
                    let AxisConstraint::Plane(normal) = constraint else {
                        return None;
                    };
                    let denominator = (b - a).dot(normal.vector());
                    let point = if denominator.abs() > 1e-9 {
                        let t = (start - a).dot(normal.vector()) / denominator;
                        if !(0.0..=1.0).contains(&t) {
                            return None;
                        }
                        a + (b - a) * t
                    } else {
                        ray.segment_point(a, b)
                    };
                    constraint.contains(point - start).then_some(point)
                });
                let joint = constraint.and_then(|(start, axis)| {
                    let point = crate::Ray {
                        origin: start,
                        direction: axis.vector(),
                    }
                    .axis(a, (b - a).normalize())
                    .or_else(|| {
                        let point = ray.segment_point(a, b);
                        ((point - start).cross(axis.vector()).length() < 1e-6).then_some(point)
                    })?;
                    let t = (point - a).dot(b - a) / (b - a).length_squared();
                    if !(0.0..=1.0).contains(&t)
                        || (point - start).cross(axis.vector()).length() > 1e-6
                    {
                        return None;
                    }
                    if away_from_start && point.distance(start) < 1e-7 {
                        return None;
                    }
                    (view.project(point)?.distance(cursor) < 8.).then_some(point)
                });
                let point = joint
                    .or(plane_joint)
                    .or_else(|| locked.is_none().then(|| ray.segment_point(a, b)))?;
                if locked.is_some_and(|(start, constraint)| !constraint.contains(point - start)) {
                    return None;
                }
                if away_from_start && start.is_some_and(|start| point.distance(start) < 1e-7) {
                    return None;
                }
                let distance = view.project(point)?.distance(cursor);
                if distance >= 8. {
                    return None;
                }
                if let (Some(start), Some(target)) = (start, local_axis) {
                    let near_start = view
                        .project(point)
                        .zip(view.project(start))
                        .is_some_and(|(a, b)| a.distance(b) < 16.0);
                    let on_axis = (point - start).cross(target.axis.vector()).length() < 1e-7;
                    if near_start && !on_axis && distance > 4.0 && distance > target.distance + 3.0
                    {
                        return None;
                    }
                }
                query_visible(model, &view, point, faces).then_some((
                    distance,
                    Snap {
                        point,
                        kind: Inference::OnEdge,
                    },
                ))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, snap)) = on_edge {
            return Some(snap);
        }
        if let Some((start, axis)) = constraint {
            let on_face = model
                .faces
                .values()
                .filter(|face| targets.faces && !is_hidden(face.id))
                .filter_map(|face| {
                    let plane = crate::faces::plane(model, &face.vertices)?;
                    let denominator = plane.normal.dot(axis.vector());
                    let point = if denominator.abs() > 1e-9 {
                        start
                            + axis.vector()
                                * ((plane.origin - start).dot(plane.normal) / denominator)
                    } else if (start - plane.origin).dot(plane.normal).abs() < 1e-6 {
                        ray.axis(start, axis.vector())?
                    } else {
                        return None;
                    };
                    if away_from_start && point.distance(start) < 1e-7 {
                        return None;
                    }
                    let distance = view.project(point)?.distance(cursor);
                    if distance >= 11.
                        || !model.face_contains(face, point, true)
                        || !query_visible(model, &view, point, faces)
                    {
                        return None;
                    }
                    Some((
                        distance,
                        Snap {
                            point,
                            kind: Inference::OnFace,
                        },
                    ))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, snap)) = on_face {
                return Some(snap);
            }
        }
        if targets.faces && !matches!(locked, Some((_, AxisConstraint::Axis(_)))) {
            let nearest = match faces {
                Some(faces) => faces.nearest(ray, false),
                None => model.nearest_face(ray),
            };
            if let Some((id, _, point)) = nearest {
                // Keep a vertical preview from being pulled back into the face
                // it starts on. Other surfaces can acquire the moving endpoint.
                let starting_on_plane = start
                    .zip(crate::faces::plane(model, &model.faces[&id].vertices))
                    .is_some_and(|(start, plane)| {
                        (start - plane.origin).dot(plane.normal).abs() < 1e-6
                    });
                if (local_axis.is_none() || !starting_on_plane)
                    && locked.is_none_or(|(start, constraint)| constraint.contains(point - start))
                {
                    return Some(Snap {
                        point,
                        kind: Inference::OnFace,
                    });
                }
            }
        }
    }
    if let (Some(start), Some(AxisConstraint::Axis(axis))) = (start, lock) {
        return Some(Snap {
            point: ray.axis(start, axis.vector())?,
            kind: Inference::Axis(axis),
        });
    }
    if let Some(target) = local_axis {
        return Some(target.snap);
    }
    // The visible model axes are also snap targets before the first click and
    // whenever a later point approaches them outside the local direction guides.
    if snap_to_axes {
        if let Some(target) = axis_target(camera, &view, cursor, DVec3::ZERO, true) {
            if locked
                .is_none_or(|(start, constraint)| constraint.contains(target.snap.point - start))
                && query_visible(model, &view, target.snap.point, faces)
            {
                return Some(target.snap);
            }
        }
    }
    if let Some((start, constraint)) = locked {
        return Some(Snap {
            point: constraint.point(ray, start)?,
            kind: Inference::Plane,
        });
    }
    Some(Snap {
        point: ray.plane_z(start.unwrap_or(DVec3::ZERO).z)?,
        kind: Inference::Plane,
    })
}

#[derive(Clone, Copy)]
struct AxisTarget {
    snap: Snap,
    axis: Axis,
    distance: f64,
}

/// Project onto a world axis through an acquired geometry reference. Optional
/// line and plane constraints must intersect in world space, not just on screen.
pub fn infer_reference_axis(
    camera: &Camera,
    size: DVec2,
    cursor: DVec2,
    reference: DVec3,
    constraint: Option<(DVec3, Axis)>,
    plane: Option<(DVec3, DVec3)>,
) -> Option<(Snap, AxisGuide)> {
    let view = camera.view(size);
    let ray = view.ray(cursor);
    let origin_pixel = view.project(reference)?;
    [Axis::X, Axis::Y, Axis::Z]
        .into_iter()
        .filter_map(|axis| {
            let direction = axis.vector();
            let projected_direction = view
                .project(reference + direction * camera.distance)
                .or_else(|| view.project(reference - direction * camera.distance))?
                - origin_pixel;
            if projected_direction.length_squared() < 1e-8 {
                return None;
            }
            let point = if let Some((start, locked)) = constraint {
                crate::Ray {
                    origin: reference,
                    direction,
                }
                .axis(start, locked.vector())
                .or_else(|| ray.axis(reference, direction))?
            } else if let Some((origin, normal)) = plane {
                let denominator = direction.dot(normal);
                if denominator.abs() > 1e-9 {
                    reference + direction * (origin - reference).dot(normal) / denominator
                } else {
                    ray.axis(reference, direction)?
                }
            } else {
                ray.axis(reference, direction)?
            };
            if (point - reference).cross(direction).length() > 1e-6
                || constraint.is_some_and(|(start, locked)| {
                    (point - start).cross(locked.vector()).length() > 1e-6
                })
                || plane.is_some_and(|(origin, normal)| (point - origin).dot(normal).abs() > 1e-6)
            {
                return None;
            }
            let distance = view.project(point)?.distance(cursor);
            (distance <= 10.).then_some((
                distance,
                Snap {
                    point,
                    kind: Inference::Axis(axis),
                },
                AxisGuide {
                    origin: reference,
                    axis,
                },
            ))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, snap, guide)| (snap, guide))
}

fn axis_target(
    camera: &Camera,
    view: &CameraView,
    cursor: DVec2,
    origin: DVec3,
    world: bool,
) -> Option<AxisTarget> {
    let screen_origin = view.project(origin)?;
    [Axis::X, Axis::Y, Axis::Z]
        .into_iter()
        .filter_map(|axis| {
            let screen_direction = view
                .project(origin + axis.vector() * camera.distance)
                .or_else(|| view.project(origin - axis.vector() * camera.distance))?
                - screen_origin;
            if screen_direction.length_squared() < 1e-8 {
                return None;
            }
            let projected = screen_origin
                + screen_direction
                    * ((cursor - screen_origin).dot(screen_direction)
                        / screen_direction.length_squared());
            let distance = projected.distance(cursor);
            if distance > 10. {
                return None;
            }
            let point = view.ray(projected).axis(origin, axis.vector())?;
            view.project(point)?;
            let kind = if world {
                Inference::OnAxis(axis)
            } else {
                Inference::Axis(axis)
            };
            Some((
                distance,
                AxisTarget {
                    snap: Snap { point, kind },
                    axis,
                    distance,
                },
            ))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, target)| target)
}

struct MoveFace {
    id: u64,
    plane: crate::faces::Plane,
    boundary: Vec<DVec2>,
    holes: Vec<Vec<DVec2>>,
    lo: DVec2,
    hi: DVec2,
    in_context: bool,
}
struct MoveFaces(Vec<MoveFace>);
impl MoveFaces {
    fn new(model: &Model) -> Self {
        Self(
            model
                .faces
                .values()
                .filter_map(|face| {
                    let plane = crate::faces::plane(model, &face.vertices)?;
                    let project = |ring: &[u64]| {
                        ring.iter()
                            .map(|id| plane.project(model.vertices[id].position))
                            .collect::<Vec<_>>()
                    };
                    let boundary = project(&face.vertices);
                    let lo = boundary
                        .iter()
                        .copied()
                        .fold(DVec2::splat(f64::INFINITY), DVec2::min);
                    let hi = boundary
                        .iter()
                        .copied()
                        .fold(DVec2::splat(f64::NEG_INFINITY), DVec2::max);
                    Some(MoveFace {
                        id: face.id,
                        plane,
                        lo,
                        hi,
                        boundary,
                        holes: face.holes.iter().map(|r| project(r)).collect(),
                        in_context: model.entity_in_context(face.id).is_some(),
                    })
                })
                .collect(),
        )
    }
}
impl FaceQuery for MoveFaces {
    fn nearest(&self, ray: crate::Ray, scoped: bool) -> Option<(u64, f64, DVec3)> {
        self.0
            .iter()
            .filter(|face| !scoped || face.in_context)
            .filter_map(|face| {
                let denominator = ray.direction.dot(face.plane.normal);
                if denominator.abs() < 1e-9 {
                    return None;
                }
                let t = (face.plane.origin - ray.origin).dot(face.plane.normal) / denominator;
                if t <= 0.005 {
                    return None;
                }
                let point = ray.origin + ray.direction * t;
                let xy = face.plane.project(point);
                if !xy.cmpge(face.lo - DVec2::splat(1e-6)).all()
                    || !xy.cmple(face.hi + DVec2::splat(1e-6)).all()
                    || (point - face.plane.origin).dot(face.plane.normal).abs() >= 1e-6
                    || !crate::faces::in_polygon(xy, &face.boundary, true)
                    || face
                        .holes
                        .iter()
                        .any(|ring| crate::faces::in_polygon(xy, ring, true))
                {
                    return None;
                }
                Some((face.id, t, point))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
    }
}
fn query_visible(
    model: &Model,
    view: &CameraView,
    point: DVec3,
    faces: Option<&dyn FaceQuery>,
) -> bool {
    let Some(faces) = faces else {
        return crate::selection::visible_in_view(model, view, point);
    };
    let Some(pixel) = view.project(point) else {
        return false;
    };
    let ray = view.ray(pixel);
    faces
        .nearest(ray, true)
        .is_none_or(|(_, t, _)| t + 1e-5 >= point.distance(ray.origin))
}

#[cfg(test)]
mod plane_lock_tests {
    use super::*;

    #[test]
    fn plane_locks_keep_geometry_and_cached_moves_on_every_plane() {
        let camera = Camera::default();
        let size = DVec2::new(1100., 800.);
        let start = DVec3::new(0.23, 0.37, 0.41);
        for normal in [Axis::X, Axis::Y, Axis::Z] {
            let lock = AxisConstraint::Plane(normal);
            let target = start + lock.project(DVec3::new(2.1, 1.7, 1.3));
            let off_plane = target + normal.vector();
            let mut model = Model::default();
            model
                .add_edge(off_plane, off_plane + DVec3::splat(0.2))
                .unwrap();
            // The constraint intersects the edge away from its midpoint/endpoints.
            model
                .add_edge(target - normal.vector() * 2., target + normal.vector())
                .unwrap();
            let mut cache = MoveSnapCache::new(&model);
            for geometry in [false, true] {
                for axes in [false, true] {
                    let cursor = camera.project(target, size).unwrap();
                    let snap = infer_with_constraint(
                        &model,
                        &camera,
                        size,
                        cursor,
                        Some(start),
                        Some(lock),
                        geometry,
                        axes,
                        None,
                        SnapTargets::default(),
                    )
                    .unwrap();
                    assert!(
                        snap.point.distance(target) < 1e-6,
                        "{normal:?} {geometry} {axes}: {snap:?}"
                    );
                    if geometry {
                        assert_eq!(snap.kind, Inference::OnEdge);
                    }
                    let moved = cache
                        .infer_with_constraint(
                            &camera,
                            size,
                            cursor,
                            start,
                            Some(lock),
                            geometry,
                            axes,
                            SnapTargets::default(),
                        )
                        .unwrap();
                    assert!(lock.contains(moved.snap.point - start));
                    assert!(moved.snap.point.distance(target) < 1e-6);
                    let off_cursor = camera.project(off_plane, size).unwrap();
                    let rejected = cache
                        .infer_with_constraint(
                            &camera,
                            size,
                            off_cursor,
                            start,
                            Some(lock),
                            geometry,
                            axes,
                            SnapTargets::default(),
                        )
                        .unwrap();
                    assert!(lock.contains(rejected.snap.point - start));
                    assert!(rejected.snap.point.distance(off_plane) > 0.9);
                }
            }
        }
    }

    #[test]
    fn plane_lock_accepts_exact_endpoints_and_rejects_parallel_rays() {
        let camera = Camera::default();
        let size = DVec2::new(1100., 800.);
        let start = DVec3::new(0.2, 0.3, 0.4);
        let target = start + DVec3::new(2., 3., 0.);
        let mut model = Model::default();
        model.add_edge(target, target + DVec3::splat(0.3)).unwrap();
        let snap = infer_with_constraint(
            &model,
            &camera,
            size,
            camera.project(target, size).unwrap(),
            Some(start),
            Some(AxisConstraint::Plane(Axis::Z)),
            true,
            true,
            None,
            SnapTargets::default(),
        )
        .unwrap();
        assert_eq!(snap.kind, Inference::Endpoint);
        assert!(snap.point.distance(target) < 1e-7);
        assert!(
            AxisConstraint::Plane(Axis::Z)
                .point(
                    crate::Ray {
                        origin: start + DVec3::Z,
                        direction: DVec3::X
                    },
                    start
                )
                .is_none()
        );
    }
}
