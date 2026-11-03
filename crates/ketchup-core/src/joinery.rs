//! Parametric joinery: source boards and their persistent relations stay intact.
//! Work on a clone and publish both members together, only after solid validation.
use crate::{
    DVec3, Model, ObjectKind,
    csg::{self, Boolean, Polygon},
    solids::{self, Frame},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const EPS: f64 = 1e-7;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum JointKind {
    #[default]
    MortiseTenon,
    HalfLap,
    TongueGroove,
    Rabbet,
    Dovetail,
    Finger,
}
impl JointKind {
    pub const ALL: [Self; 6] = [
        Self::MortiseTenon,
        Self::HalfLap,
        Self::TongueGroove,
        Self::Rabbet,
        Self::Dovetail,
        Self::Finger,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::MortiseTenon => "Mortise and tenon",
            Self::HalfLap => "Half lap",
            Self::TongueGroove => "Tongue and groove",
            Self::Rabbet => "Rabbet",
            Self::Dovetail => "Dovetail",
            Self::Finger => "Finger joint",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JointSettings {
    pub kind: JointKind,
    /// World axis along the insertion direction; width is a distinct world axis.
    pub depth_axis: usize,
    pub width_axis: usize,
    pub width_ratio: f64,
    pub height_ratio: f64,
    pub fingers: u32,
    /// Number of evenly spaced dovetail tails across the joint width.
    pub dovetails: u32,
    /// Tail width in metres; absent for legacy count-based joints.
    pub dovetail_width: Option<f64>,
    pub taper_degrees: f64,
    /// Extra lateral clearance removed from the receiving part, in metres.
    pub clearance: f64,
}
impl Default for JointSettings {
    fn default() -> Self {
        Self {
            kind: JointKind::MortiseTenon,
            depth_axis: 0,
            width_axis: 1,
            width_ratio: 0.5,
            height_ratio: 0.5,
            fingers: 4,
            dovetails: 1,
            dovetail_width: None,
            taper_degrees: 8.,
            clearance: 0.0001,
        }
    }
}
impl JointSettings {
    /// Fit fixed-width tails evenly across the overlap, retaining legacy layouts.
    pub fn dovetail_layout(&self, span: f64) -> Option<(u32, f64)> {
        if !span.is_finite() || span <= 0. { return None; }
        if let Some(width) = self.dovetail_width {
            if !width.is_finite() || width <= 0. || width >= span { return None; }
            let count = (span * self.width_ratio / width).floor().max(1.);
            if count > 64. { return None; }
            Some((count as u32, width))
        } else {
            (self.dovetails > 0).then(|| (self.dovetails, span * self.width_ratio / self.dovetails as f64))
        }
    }
    pub fn valid(&self) -> bool {
        self.depth_axis < 3
            && self.width_axis < 3
            && self.depth_axis != self.width_axis
            && self.width_ratio.is_finite()
            && (0.1..=0.9).contains(&self.width_ratio)
            && self.height_ratio.is_finite()
            && (0.1..=0.9).contains(&self.height_ratio)
            && (2..=16).contains(&self.fingers)
            && (1..=16).contains(&self.dovetails)
            && self.dovetail_width.is_none_or(|width| width.is_finite() && width > 0.)
            && self.taper_degrees.is_finite()
            && (1. ..=30.).contains(&self.taper_degrees)
            && self.clearance.is_finite()
            && (0. ..=0.01).contains(&self.clearance)
    }
}

fn board(model: &Model, id: u64) -> Result<(DVec3, DVec3), &'static str> {
    let object = model
        .objects
        .get(&id)
        .ok_or("Choose two groups or components")?;
    if object
        .entities
        .iter()
        .any(|id| !model.edges.contains_key(id) && !model.faces.contains_key(id))
    {
        return Err(
            "Each part must contain only board geometry; select individual boards, not assemblies",
        );
    }
    let contents = model.object_contents(id).ok_or("Missing board")?;
    let mut points = contents.vertices.values().map(|v| v.position);
    let first = points.next().ok_or("The board has no geometry")?;
    let (lo, hi) = points.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
    if !lo.is_finite() || !hi.is_finite() || (hi - lo).min_element() <= EPS * 10. {
        return Err("Boards must have finite, positive dimensions");
    }
    // Volume alone cannot distinguish a rotated/non-box surface from a box.
    for face in contents.faces.values() {
        if !face.holes.is_empty() {
            return Err("Start with rectangular boards without existing joints or holes");
        }
        let vertices: Vec<_> = face
            .vertices
            .iter()
            .map(|id| contents.vertices[id].position)
            .collect();
        if !(0..3).any(|axis| {
            [lo[axis], hi[axis]]
                .into_iter()
                .any(|x| vertices.iter().all(|p| (p[axis] - x).abs() < EPS))
        }) {
            return Err(
                "Wood Joinery currently requires rectangular boards aligned with the project axes",
            );
        }
    }
    let volume = model
        .copy_entities(&BTreeSet::from([id]))
        .solid_volume(id)?;
    let size = hi - lo;
    let expected = size.x * size.y * size.z;
    if (volume - expected).abs() > expected * 1e-6 {
        return Err("Start with solid rectangular boards without existing joints or holes");
    }
    Ok((lo, hi))
}

/// Convex eight-corner prism, with winding determined in world coordinates.
fn prism(points: [DVec3; 8], frame: Frame) -> Result<Vec<Polygon>, &'static str> {
    let center = points.iter().copied().sum::<DVec3>() / 8.;
    [
        [0, 1, 2, 3],
        [4, 7, 6, 5],
        [0, 4, 5, 1],
        [1, 5, 6, 2],
        [2, 6, 7, 3],
        [3, 7, 4, 0],
    ]
    .into_iter()
    .map(|ring| {
        let mut p = Polygon::new(
            ring.map(|i| frame.local(points[i])).to_vec(),
            None,
            frame.epsilon,
        )
        .ok_or("Joint details are too small for the model precision")?;
        if p.normal
            .dot(frame.local(points[ring[0]]) - frame.local(center))
            < 0.
        {
            p.flip();
        }
        Ok(p)
    })
    .collect()
}
fn box_points(lo: DVec3, hi: DVec3) -> [DVec3; 8] {
    [
        DVec3::new(lo.x, lo.y, lo.z),
        DVec3::new(hi.x, lo.y, lo.z),
        DVec3::new(hi.x, hi.y, lo.z),
        DVec3::new(lo.x, hi.y, lo.z),
        DVec3::new(lo.x, lo.y, hi.z),
        DVec3::new(hi.x, lo.y, hi.z),
        DVec3::new(hi.x, hi.y, hi.z),
        DVec3::new(lo.x, hi.y, hi.z),
    ]
}
// The tail/tenon needs a shoulder on its own board. The receiver can
// continue along a different axis, as in a cabinet corner or a T joint.
fn joint_direction(a: (DVec3, DVec3), b: (DVec3, DVec3), lo: DVec3, hi: DVec3, d: usize) -> Option<bool> {
    let lower = lo[d] - a.0[d] > EPS;
    let upper = a.1[d] - hi[d] > EPS;
    let receiver_stock = (0..3).any(|axis| lo[axis] - b.0[axis] > EPS || b.1[axis] - hi[axis] > EPS);
    if (!lower && !upper) || !receiver_stock { return None; }
    Some(lower && (!upper || a.0[d] + a.1[d] < b.0[d] + b.1[d]))
}
impl Model {
    /// Complete contours of material removed from one source board. Edges on
    /// the original stock rim can disappear from the retained solid entirely,
    /// but still belong to the closed cut outline shown by the viewport.
    pub fn joinery_cut_edges(
        &self,
        evaluated: &Model,
        member: u64,
    ) -> Result<Vec<[DVec3; 2]>, &'static str> {
        let roots = BTreeSet::from([member]);
        let stock = self.copy_entities(&roots);
        let retained = evaluated.copy_entities(&roots);
        let frame = Frame::new(&[&stock, &retained])?;
        let removed = csg::apply(
            solids::import(&stock, frame)?,
            solids::import(&retained, frame)?,
            Boolean::Difference,
            frame.epsilon,
        )?;
        if removed.is_empty() {
            return Ok(Vec::new());
        }
        let mut cut = Model::default();
        solids::output(&mut cut, removed, frame, "Joint cut", false, true)?;
        Ok(cut
            .edges
            .values()
            .map(|edge| {
                let (a, b) = cut.points(edge);
                [a, b]
            })
            .collect())
    }

    /// Axis pairs that can cut the selected stock with the current profile.
    pub fn supported_joinery_axes(&self, members: [u64; 2], settings: &JointSettings) -> Vec<(usize, usize)> {
        if members[0] == members[1] { return Vec::new(); }
        let (Ok(a), Ok(b)) = (board(self, members[0]), board(self, members[1])) else {
            return Vec::new();
        };
        let (lo, hi) = (a.0.max(b.0), a.1.min(b.1));
        let size = hi - lo;
        if size.min_element() <= EPS * 10. { return Vec::new(); }
        let mut supported = Vec::new();
        for d in 0..3 {
            if joint_direction(a, b, lo, hi, d).is_none() { continue; }
            for u in 0..3 {
                if u == d { continue; }
                let v = 3 - d - u;
                let mut candidate = settings.clone();
                candidate.depth_axis = d;
                candidate.width_axis = u;
                if !candidate.valid() || settings.clearance * 2. >= size[u].min(size[v]) * 0.1 { continue; }
                if settings.kind == JointKind::Dovetail {
                    let Some((_, width)) = settings.dovetail_layout(size[u]) else { continue; };
                    if size[d] * settings.taper_degrees.to_radians().tan() * 2. >= width - EPS { continue; }
                }
                supported.push((d, u));
            }
        }
        supported
    }
    /// Apply a relation to a disposable evaluated model. Original board bounds
    /// always come from stock, allowing multiple joints to affect the same board.
    pub(crate) fn cut_joint(
        &mut self,
        members: [u64; 2],
        settings: &JointSettings,
        stock: &Model,
    ) -> Result<(), &'static str> {
        if !settings.valid() || members[0] == members[1] {
            return Err("Invalid wood joint settings or pair of boards");
        }
        let (a, b) = (board(stock, members[0])?, board(stock, members[1])?);
        self.cut_joint_with_bounds(members, settings, [a, b])
    }

    fn cut_joint_with_bounds(&mut self, members: [u64; 2], settings: &JointSettings,
        [a, b]: [(DVec3, DVec3); 2]) -> Result<(), &'static str>
    {
        let (lo, hi) = (a.0.max(b.0), a.1.min(b.1));
        let size = hi - lo;
        if size.min_element() <= EPS * 10. {
            return Err(
                "Position the boards with a shared volume: their overlap defines the joint depth",
            );
        }
        let (d, u) = (settings.depth_axis, settings.width_axis);
        let v = 3 - d - u;
        let forward = joint_direction(a, b, lo, hi, d).ok_or(
            "Choose an orientation with a shoulder on the first board and stock remaining on the receiving board",
        )?;
        if settings.clearance * 2. >= size[u].min(size[v]) * 0.1 {
            return Err("Clearance is too large for this joint");
        }
        let copies = members.map(|id| self.copy_entities(&BTreeSet::from([id])));
        let frame = Frame::new(&[&copies[0], &copies[1]])?;
        let mut solids = [
            solids::import(&copies[0], frame)?,
            solids::import(&copies[1], frame)?,
        ];
        let overlap = prism(box_points(lo, hi), frame)?;
        let mut profiles = Vec::new();
        let mut pockets = Vec::new();
        let dovetail = if settings.kind == JointKind::Dovetail {
            Some(settings.dovetail_layout(size[u]).ok_or("Choose a tail width that fits the overlap and produces at most 64 tails")?)
        } else { None };
        let count = if settings.kind == JointKind::Finger {
            settings.fingers * 2
        } else if let Some((count, _)) = dovetail {
            count
        } else {
            1
        };
        for i in 0..count {
            if settings.kind == JointKind::Finger && i % 2 == 1 {
                continue;
            }
            let (mut l, mut h) = (lo, hi);
            match settings.kind {
                JointKind::MortiseTenon | JointKind::TongueGroove | JointKind::Dovetail => {
                    let slot_width = size[u] / count as f64;
                    let tail_width = dovetail.map_or(slot_width * settings.width_ratio, |(_, width)| width);
                    let margin = (slot_width - tail_width) * 0.5;
                    l[u] = lo[u] + slot_width * i as f64 + margin;
                    h[u] = lo[u] + slot_width * (i + 1) as f64 - margin;
                    if settings.kind == JointKind::MortiseTenon {
                        let margin = size[v] * (1. - settings.height_ratio) * 0.5;
                        l[v] += margin;
                        h[v] -= margin;
                    }
                }
                JointKind::HalfLap => {
                    h[v] = lo[v] + size[v] * 0.5;
                }
                JointKind::Rabbet => {
                    h[u] = lo[u] + size[u] * settings.width_ratio;
                }
                JointKind::Finger => {
                    l[u] = lo[u] + size[u] * i as f64 / count as f64;
                    h[u] = lo[u] + size[u] * (i + 1) as f64 / count as f64;
                }
            }
            let mut points = box_points(l, h);
            if settings.kind == JointKind::Dovetail {
                let taper = size[d] * settings.taper_degrees.to_radians().tan();
                if taper * 2. >= h[u] - l[u] - EPS {
                    return Err("Dovetail angle or depth leaves no material at the neck");
                }
                // A narrow neck at the first board's shoulder, widening into B.
                for p in &mut points {
                    let neck = if forward {
                        (p[d] - lo[d]).abs() < EPS
                    } else {
                        (p[d] - hi[d]).abs() < EPS
                    };
                    if neck {
                        p[u] += if (p[u] - l[u]).abs() < EPS {
                            taper
                        } else {
                            -taper
                        };
                    }
                }
            }
            profiles.push(prism(points, frame)?);
            // Grow the receiver cut only laterally; clip it to its stock by CSG.
            let center = (l + h) * 0.5;
            for p in &mut points {
                for axis in [u, v] {
                    p[axis] += if p[axis] < center[axis] {
                        -settings.clearance
                    } else {
                        settings.clearance
                    };
                }
            }
            pockets.push(prism(points, frame)?);
        }
        let mut waste = overlap;
        for profile in profiles {
            waste = csg::apply(waste, profile, Boolean::Difference, frame.epsilon)?;
        }
        solids[0] = csg::apply(solids[0].clone(), waste, Boolean::Difference, frame.epsilon)?;
        for pocket in pockets {
            solids[1] = csg::apply(
                solids[1].clone(),
                pocket,
                Boolean::Difference,
                frame.epsilon,
            )?;
        }
        // Output and validate both members before replacing any live document data.
        let mut next = self.clone();
        for (id, polygons) in members.into_iter().zip(solids) {
            let temp = solids::output(&mut next, polygons, frame, "Joint result", false, true)?
                .ok_or("The joint would remove an entire board")?;
            next.solid_volume(temp)?;
            let contents = next
                .object_contents(temp)
                .ok_or("Cannot read joint result")?;
            next.objects.remove(&temp);
            next.replace_contents(id, &contents)?;
            let object = next.objects.get_mut(&id).unwrap();
            if matches!(object.kind, ObjectKind::Component { .. }) {
                object.kind = ObjectKind::Component {
                    definition: uuid::Uuid::new_v4(),
                };
                object.definition_entities = object.entities.iter().map(|id| (*id, *id)).collect();
            }
        }
        // Only the derived instances change. Refresh ancestor mappings without
        // propagating a cut into unconnected copies of a source component.
        let mut ancestors = BTreeSet::new();
        for member in members {
            let mut parent = next.parent(member);
            while let Some(id) = parent {
                ancestors.insert(id);
                parent = next.parent(id);
            }
        }
        for id in ancestors {
            let geometry = next
                .expand_entities(&next.objects[&id].entities)
                .into_iter()
                .filter(|id| !next.objects.contains_key(id))
                .map(|id| (id, id))
                .collect();
            let object = next.objects.get_mut(&id).unwrap();
            if matches!(object.kind, ObjectKind::Component { .. }) {
                object.kind = ObjectKind::Component {
                    definition: uuid::Uuid::new_v4(),
                };
                object.definition_entities = geometry;
            }
        }
        // Geometry replacement invalidates saved entry-face IDs. Re-evaluate the
        // existing hardware against the new surfaces without deleting relations.
        for connection in next.connections.values_mut() {
            if connection.members.iter().any(|id| members.contains(id)) {
                connection.entry_face = None;
            }
        }

        next.validate()
            .map_err(|_| "The joint cannot be represented as valid solid geometry")?;
        for id in members {
            next.copy_entities(&BTreeSet::from([id])).solid_volume(id)?;
        }
        *self = next;
        Ok(())
    }
}

/// Derived geometry is deliberately absent from saved projects and history.
/// Invalid or separated pairs retain their source boards and their relation, so
/// bringing them back into a usable position reactivates the joint automatically.
pub struct GeometryEvaluation {
    pub model: Model,
    pub errors: Vec<(u64, String)>,
}

/// Reuse each cut while its stock, incoming machined boards and allocation
/// preconditions remain unchanged. Checked deltas preserve unrelated edits and
/// naturally invalidate downstream cuts that share a changed board.
#[derive(Default)]
pub struct EvaluationCache {
    cuts: std::collections::BTreeMap<u64, CachedCut>,
    boards: std::collections::BTreeMap<u64, (BoardSnapshot, Result<(DVec3, DVec3), &'static str>)>,
    #[cfg(test)]
    evaluations: usize,
}

struct CachedCut {
    members: [u64; 2],
    settings: JointSettings,
    stock: BoardSnapshot,
    input: BoardSnapshot,
    change: crate::history::ModelChange,
    error: Option<String>,
}

/// Small source keys for machining and guide caches. Unlike copy_entities,
/// constructing a key never clones unrelated objects or their preferences.
#[derive(PartialEq)]
pub struct BoardSnapshot {
    next_id: u64,
    vertices: Vec<crate::Vertex>,
    edges: Vec<crate::Edge>,
    faces: Vec<crate::Face>,
    objects: Vec<crate::Object>,
    colors: Vec<(u64, [u8; 3])>,
    connections: Vec<(u64, crate::connections::Connection)>,
    hidden: Vec<u64>,
    locked: Vec<u64>,
    tags: Vec<(u64, u64)>,
}

impl BoardSnapshot {
    pub fn new(model: &Model, members: [u64; 2]) -> Self {
        let mut ids = model.expand_entities(&BTreeSet::from(members));
        ids.extend(model.boundary_edges(&ids));
        let edges: Vec<_> = ids.iter().filter_map(|id| model.edges.get(id).cloned()).collect();
        let vertices: BTreeSet<_> = edges.iter().flat_map(|edge| [edge.a, edge.b]).collect();
        Self {
            next_id: model.next_id,
            vertices: vertices.iter().filter_map(|id| model.vertices.get(id).cloned()).collect(),
            edges,
            faces: ids.iter().filter_map(|id| model.faces.get(id).cloned()).collect(),
            objects: ids.iter().filter_map(|id| model.objects.get(id).cloned()).collect(),
            colors: ids.iter().filter_map(|id| model.face_colors.get(id).map(|color| (*id, *color))).collect(),
            connections: model.connections.iter().filter(|(_, c)| c.members.iter().any(|id| ids.contains(id)))
                .map(|(id, c)| (*id, c.clone())).collect(),
            hidden: model.hidden.intersection(&ids).copied().collect(),
            locked: model.locked.intersection(&ids).copied().collect(),
            tags: ids.iter().filter_map(|id| model.entity_tags.get(id).map(|tag| (*id, *tag))).collect(),
        }
    }

    pub fn same_geometry(&self, other: &Self) -> bool {
        self.vertices == other.vertices && self.edges == other.edges
            && self.faces == other.faces && self.objects == other.objects
    }

    fn transformed_bounds(&self, old: &Self, bounds: (DVec3, DVec3)) -> Option<(DVec3, DVec3)> {
        if self.edges != old.edges || self.faces != old.faces || self.vertices.len() != old.vertices.len()
            || self.hidden != old.hidden || self.locked != old.locked || self.objects.len() != old.objects.len()
            || !self.objects.iter().zip(&old.objects).all(|(a, b)| {
                let (mut a, mut b) = (a.clone(), b.clone());
                a.transform = None; b.transform = None; a == b
            }) { return None; }
        let lo = self.vertices.iter().map(|v| v.position).reduce(DVec3::min)?;
        let hi = self.vertices.iter().map(|v| v.position).reduce(DVec3::max)?;
        let size = hi - lo;
        if !lo.is_finite() || !hi.is_finite() || size.min_element() <= EPS * 10. { return None; }
        let scale = size / (bounds.1 - bounds.0);
        self.vertices.iter().zip(&old.vertices).all(|(a, b)| a.id == b.id
            && (a.position - (lo + (b.position - bounds.0) * scale)).abs().max_element() < EPS * 0.01)
            .then_some((lo, hi))
    }
}

impl EvaluationCache {
    fn board_bounds(&mut self, source: &Model, id: u64) -> Result<(DVec3, DVec3), &'static str> {
        let snapshot = BoardSnapshot::new(source, [id, id]);
        if let Some((old, result)) = self.boards.get(&id) {
            if *old == snapshot { return *result; }
            if let Ok(bounds) = result {
                if let Some(bounds) = snapshot.transformed_bounds(old, *bounds) {
                    self.boards.insert(id, (snapshot, Ok(bounds)));
                    return Ok(bounds);
                }
            }
        }
        let result = board(source, id);
        self.boards.insert(id, (snapshot, result));
        result
    }

    pub fn evaluate(&mut self, source: &Model) -> GeometryEvaluation {
        use crate::connections::ConnectionOperation;
        self.cuts.retain(|id, _| source.connections.get(id)
            .is_some_and(|c| matches!(c.operation, ConnectionOperation::Joinery(_))));
        self.boards.retain(|id, _| source.objects.contains_key(id));
        // Derived IDs are not saved. Give independent cuts stable disjoint
        // allocation ranges so a changed upstream cut cannot invalidate every
        // later cached cut merely by allocating a different number of faces.
        const STRIDE: u64 = 2_000_000;
        let count = self.cuts.len().max(source.connections.len()) as u64;
        if count.checked_add(2).and_then(|n| n.checked_mul(STRIDE))
            .and_then(|n| source.next_id.checked_add(n)).is_none()
        { return source.evaluate_joinery(); }
        let mut model = source.clone();
        let context = model.edit_context.take();
        let mut errors = Vec::new();
        let mut slot = 0;
        for (&id, connection) in &source.connections {
            let ConnectionOperation::Joinery(settings) = &connection.operation else { continue };
            let start = source.next_id + slot * STRIDE;
            slot += 1;
            model.next_id = start;
            let stock = BoardSnapshot::new(source, connection.members);
            let input = BoardSnapshot::new(&model, connection.members);
            if let Some(cut) = self.cuts.get(&id) {
                if cut.members == connection.members && cut.settings == *settings
                    && cut.stock == stock && cut.input == input
                    && cut.change.apply_checked(&mut model, true)
                {
                    if let Some(error) = &cut.error { errors.push((id, error.clone())); }
                    continue;
                }
            }
            let before = model.clone();
            let result = if !settings.valid() || connection.members[0] == connection.members[1] {
                Err("Invalid wood joint settings or pair of boards")
            } else {
                self.board_bounds(source, connection.members[0]).and_then(|a|
                    self.board_bounds(source, connection.members[1]).and_then(|b|
                        model.cut_joint_with_bounds(connection.members, settings, [a, b])))
            };
            let error = result.err().map(str::to_owned);
            if model.next_id >= start + STRIDE {
                self.cuts.clear();
                return source.evaluate_joinery();
            }
            if let Some(error) = &error { errors.push((id, error.clone())); }
            #[cfg(test)] { self.evaluations += 1; }
            self.cuts.insert(id, CachedCut {
                members: connection.members, settings: settings.clone(), stock, input,
                change: crate::history::ModelChange::between(&before, &model), error,
            });
        }
        model.edit_context = context;
        model.connections.retain(|_, c| matches!(c.operation, ConnectionOperation::Fasteners(_)));
        GeometryEvaluation { model, errors }
    }
}
impl Model {
    pub fn evaluate_joinery(&self) -> GeometryEvaluation {
        use crate::connections::ConnectionOperation;
        let mut model = self.clone();
        let context = model.edit_context.take();
        let mut errors = Vec::new();
        for (id, connection) in &self.connections {
            if let ConnectionOperation::Joinery(settings) = &connection.operation {
                if let Err(error) = model.cut_joint(connection.members, settings, self) {
                    errors.push((*id, error.into()));
                }
            }
        }
        model.edit_context = context;
        // An evaluated mesh is a terminal product, not another source model.
        // Keeping only hardware prevents accidental repeated evaluation of cuts.
        model
            .connections
            .retain(|_, c| matches!(c.operation, ConnectionOperation::Fasteners(_)));
        GeometryEvaluation { model, errors }
    }
    #[cfg(test)]
    fn test_cut_joint(
        &mut self,
        members: [u64; 2],
        settings: &JointSettings,
    ) -> Result<(), &'static str> {
        if self.selection_protected(&BTreeSet::from(members))
            || members.iter().any(|id| self.is_hidden(*id))
        {
            return Err("Protected board");
        }
        let stock = self.clone();
        let context = self.edit_context.take();
        let result = self.cut_joint(members, settings, &stock);
        self.edit_context = context;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Document, History, SolidOperation,
        connections::{Connection, ConnectionOperation},
    };

    fn board_at(model: &mut Model, lo: DVec3, hi: DVec3, component: bool) -> u64 {
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
        let normal = crate::faces::plane(&body, &body.faces[&face].vertices)
            .unwrap()
            .normal;
        body.push_pull(face, (hi.z - lo.z) / normal.z).unwrap();
        let ids = model.paste_entities(&body, DVec3::ZERO);
        model.make_object(&ids, component, "Board").unwrap()
    }
    pub(super) fn pair() -> (Model, [u64; 2]) {
        let mut model = Model::default();
        let a = board_at(&mut model, DVec3::ZERO, DVec3::new(0.3, 0.1, 0.04), false);
        let b = board_at(
            &mut model,
            DVec3::new(0.25, 0., 0.),
            DVec3::new(0.55, 0.1, 0.04),
            false,
        );
        (model, [a, b])
    }
    #[test]
    fn incremental_joinery_reuses_unaffected_cuts_and_matches_fresh_evaluation() {
        let (mut model, members) = pair();
        model.add_connection(Connection { members, entry_face: None,
            operation: ConnectionOperation::Joinery(JointSettings::default()) }).unwrap();
        let unrelated = board_at(&mut model, DVec3::splat(2.), DVec3::splat(2.2), false);
        let mut cache = EvaluationCache::default();
        cache.evaluate(&model);
        assert_eq!(cache.evaluations, 1);
        for vertex in model.selection_vertices(&BTreeSet::from([unrelated])) {
            model.vertices.get_mut(&vertex).unwrap().position.x += 0.1;
        }
        let cached = cache.evaluate(&model);
        assert_eq!(cache.evaluations, 1, "unrelated movement must reuse machining");
        assert_eq!(cached.model.vertices, model.evaluate_joinery().model.vertices);
        for offset in [0.01, 1., -1., -0.01] {
            for vertex in model.selection_vertices(&BTreeSet::from([members[1]])) {
                model.vertices.get_mut(&vertex).unwrap().position.x += offset;
            }
            let cached = cache.evaluate(&model);
            let fresh = model.evaluate_joinery();
            assert_eq!(cached.errors, fresh.errors);
            assert_eq!(cached.model.vertices, fresh.model.vertices);
            assert_eq!(cached.model.edges, fresh.model.edges);
            assert_eq!(cached.model.faces, fresh.model.faces);
            cached.model.validate().unwrap();
        }
        model.connections.clear();
        assert!(cache.evaluate(&model).errors.is_empty());
        assert!(cache.cuts.is_empty());
    }

    #[test]
    fn changed_cut_allocation_does_not_invalidate_independent_joints() {
        let (mut source, first) = pair();
        let second = [
            board_at(&mut source, DVec3::Y, DVec3::new(0.3, 1.1, 0.04), false),
            board_at(&mut source, DVec3::new(0.25, 1., 0.), DVec3::new(0.55, 1.1, 0.04), false),
        ];
        for members in [first, second] {
            source.add_connection(Connection { members, entry_face: None,
                operation: ConnectionOperation::Joinery(Default::default()) }).unwrap();
        }
        let mut cache = EvaluationCache::default();
        cache.evaluate(&source);
        assert_eq!(cache.evaluations, 2);
        for id in source.selection_vertices(&BTreeSet::from([first[1]])) {
            source.vertices.get_mut(&id).unwrap().position.x += 1.;
        }
        let result = cache.evaluate(&source);
        assert_eq!(cache.evaluations, 3, "an independent joint must keep its geometry and IDs");
        let fresh = source.evaluate_joinery();
        assert_eq!(result.errors, fresh.errors);
        for id in second {
            assert!((result.model.solid_volume(id).unwrap() - fresh.model.solid_volume(id).unwrap()).abs() < 1e-9);
        }
        result.model.validate().unwrap();
    }
    #[test]
    fn all_six_profiles_remove_only_overlap_and_leave_two_valid_nonintersecting_boards() {
        for kind in JointKind::ALL {
            for reverse in [false, true] {
                let (mut model, mut members) = pair();
                if reverse {
                    members.swap(0, 1);
                }
                let settings = JointSettings {
                    kind,
                    clearance: 0.,
                    ..Default::default()
                };
                let before = model.clone();
                model
                    .test_cut_joint(members, &settings)
                    .unwrap_or_else(|e| panic!("{kind:?}, reverse={reverse}: {e}"));
                model.validate().unwrap();
                let volumes = members.map(|id| model.solid_volume(id).unwrap());
                assert!(
                    (volumes[0] + volumes[1] - 0.0022).abs() < 1e-9,
                    "{kind:?}: {volumes:?}"
                );
                for (id, volume) in members.into_iter().zip(volumes) {
                    assert!(volume > 0. && volume < before.solid_volume(id).unwrap());
                    assert_eq!(model.objects[&id].name, before.objects[&id].name);
                }
                assert!(
                    model
                        .boolean_solids(members[0], members[1], SolidOperation::Intersect)
                        .is_err(),
                    "{kind:?} still intersects"
                );
            }
        }
    }
    #[test]
    fn clearance_removes_more_receiver_stock_without_altering_first_member() {
        for kind in JointKind::ALL {
            let (mut tight, members) = pair();
            let mut loose = tight.clone();
            tight
                .test_cut_joint(
                    members,
                    &JointSettings {
                        kind,
                        clearance: 0.,
                        ..Default::default()
                    },
                )
                .unwrap();
            loose
                .test_cut_joint(
                    members,
                    &JointSettings {
                        kind,
                        ..Default::default()
                    },
                )
                .unwrap();
            assert!(
                (tight.solid_volume(members[0]).unwrap() - loose.solid_volume(members[0]).unwrap())
                    .abs()
                    < 1e-10
            );
            assert!(
                loose.solid_volume(members[1]).unwrap() < tight.solid_volume(members[1]).unwrap()
            );
        }
    }
    #[test]
    fn invalid_settings_missing_locked_touching_and_rotated_boards_are_atomic() {
        let (model, members) = pair();
        for settings in [
            JointSettings {
                depth_axis: 3,
                ..Default::default()
            },
            JointSettings {
                clearance: f64::NAN,
                ..Default::default()
            },
            JointSettings {
                width_axis: 0,
                ..Default::default()
            },
            JointSettings {
                taper_degrees: 30.,
                kind: JointKind::Dovetail,
                ..Default::default()
            },
        ] {
            let mut next = model.clone();
            assert!(next.test_cut_joint(members, &settings).is_err());
            assert_eq!(next, model);
        }
        for variant in 0..5 {
            let mut next = model.clone();
            match variant {
                0 => {
                    next.locked.insert(members[0]);
                }
                1 => {
                    next.hidden.insert(members[1]);
                }
                2 => {
                    next.objects.remove(&members[1]);
                }
                3 => {
                    let ids = next.selection_vertices(&BTreeSet::from([members[1]]));
                    for id in ids {
                        next.vertices.get_mut(&id).unwrap().position.x += 0.05;
                    }
                }
                _ => {
                    for vertex in next.vertices.values_mut() {
                        let p = vertex.position;
                        vertex.position =
                            DVec3::new(p.x * 0.8 - p.y * 0.6, p.x * 0.6 + p.y * 0.8, p.z);
                    }
                }
            }
            let before = next.clone();
            assert!(next.test_cut_joint(members, &Default::default()).is_err());
            assert_eq!(next, before);
        }
    }
    #[test]
    fn independent_components_preserve_materials_peers_and_history_roundtrip() {
        let (mut model, members) = pair();
        let definition = uuid::Uuid::new_v4();
        let object = model.objects.get_mut(&members[0]).unwrap();
        object.kind = ObjectKind::Component { definition };
        object.definition_entities = object.entities.iter().map(|id| (*id, *id)).collect();
        object.stock_material = Some(crate::StockMaterial {
            name: "Oak".into(),
            dimensions: [400, 1000, 3000],
        });
        let peer_source = model.copy_entities(&BTreeSet::from([members[0]]));
        let peer = *model
            .paste_entities(&peer_source, DVec3::Y)
            .first()
            .unwrap();
        let peer_before = model.object_contents(peer).unwrap();
        let before = model.clone();
        model.test_cut_joint(members, &Default::default()).unwrap();
        let peer_after = model.object_contents(peer).unwrap();
        assert_eq!(peer_after.vertices, peer_before.vertices);
        assert_eq!(peer_after.faces, peer_before.faces);
        assert_eq!(peer_after.objects, peer_before.objects);
        assert_ne!(model.objects[&members[0]].kind, model.objects[&peer].kind);
        assert_eq!(
            model.objects[&members[0]].stock_material,
            before.objects[&members[0]].stock_material
        );
        let after = model.clone();
        let mut history = History::default();
        history.record("Wood joint", before.clone(), &model);
        assert!(history.undo(&mut model));
        assert_eq!(model, before);
        assert!(history.redo(&mut model));
        assert_eq!(model, after);
        let doc = Document {
            model,
            history,
            ..Default::default()
        };
        let restored = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(restored.model, doc.model);
    }
    #[test]
    fn perpendicular_boards_support_corner_joinery_in_both_part_orders() {
        for reverse in [false, true] {
            for kind in JointKind::ALL {
                let mut model = Model::default();
                let horizontal = board_at(&mut model, DVec3::ZERO, DVec3::new(0.3, 0.3, 0.04), false);
                let vertical = board_at(&mut model, DVec3::new(0.26, 0., 0.), DVec3::new(0.3, 0.3, 0.3), false);
                let members = if reverse { [vertical, horizontal] } else { [horizontal, vertical] };
                let mut settings = JointSettings { kind, clearance: 0., ..Default::default() };
                let orientations = model.supported_joinery_axes(members, &settings);
                assert!(!orientations.is_empty(), "{kind:?}, reverse={reverse}");
                (settings.depth_axis, settings.width_axis) = orientations[0];
                model.test_cut_joint(members, &settings).unwrap();
                model.validate().unwrap();
                let total: f64 = members.into_iter().map(|id| model.solid_volume(id).unwrap()).sum();
                assert!((total - (0.3 * 0.3 * 0.04 + 0.04 * 0.3 * 0.3 - 0.04 * 0.3 * 0.04)).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn only_one_wood_joint_is_allowed_per_unordered_pair() {
        let (mut model, members) = pair();
        let connection = Connection {
            members, entry_face: None,
            operation: ConnectionOperation::Joinery(JointSettings::default()),
        };
        let id = model.add_connection(connection.clone()).unwrap();
        let saved = model.clone();
        assert!(model.has_joinery_between(members));
        assert!(model.has_joinery_between([members[1], members[0]]));
        for reversed in [false, true] {
            let mut duplicate = connection.clone();
            if reversed { duplicate.members.swap(0, 1); }
            assert!(model.add_connection(duplicate).unwrap_err().contains("already have a wood joint"));
            assert_eq!(model, saved);
        }
        model.connections.remove(&id);
        assert!(!model.has_joinery_between(members));
        assert!(model.add_connection(connection).is_ok());
    }

    #[test]
    fn multiple_dovetails_cut_matching_tails_and_sockets() {
        for reverse in [false, true] {
            let mut previous = f64::INFINITY;
            for dovetails in 1..=4 {
                let (mut model, mut members) = pair();
                if reverse { members.swap(0, 1); }
                let settings = JointSettings {
                    kind: JointKind::Dovetail, dovetails, taper_degrees: 1.,
                    clearance: 0., ..Default::default()
                };
                assert!(model.supported_joinery_axes(members, &settings).contains(&(0, 1)));
                model.test_cut_joint(members, &settings).unwrap();
                model.validate().unwrap();
                let volumes = members.map(|id| model.solid_volume(id).unwrap());
                assert!((volumes[0] + volumes[1] - 0.0022).abs() < 1e-9);
                assert!(volumes[0] < previous, "each additional tapered tail removes more neck material");
                previous = volumes[0];
            }
        }
    }

    #[test]
    fn dovetail_width_fits_cuts_and_remains_fixed_when_span_changes() {
        let settings = JointSettings {
            kind: JointKind::Dovetail, dovetail_width: Some(0.01),
            taper_degrees: 1., clearance: 0., ..Default::default()
        };
        assert_eq!(settings.dovetail_layout(0.1), Some((5, 0.01)));
        assert_eq!(settings.dovetail_layout(0.2), Some((10, 0.01)));
        assert_eq!(settings.dovetail_layout(0.005), None);
        let (mut model, members) = pair();
        model.test_cut_joint(members, &settings).unwrap();
        model.validate().unwrap();
        let total: f64 = members.into_iter().map(|id| model.solid_volume(id).unwrap()).sum();
        assert!((total - 0.0022).abs() < 1e-9);
        let restored: JointSettings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored.dovetail_width, Some(0.01));
        assert!(!JointSettings { dovetail_width: Some(0.), ..Default::default() }.valid());
    }

    #[test]
    fn dovetail_count_defaults_and_round_trips() {
        let old: JointSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.dovetails, 1);
        assert_eq!(old.dovetail_width, None);
        let settings = JointSettings { dovetails: 4, ..Default::default() };
        let restored: JointSettings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored, settings);
        assert!(!JointSettings { dovetails: 0, ..Default::default() }.valid());
        assert!(!JointSettings { dovetails: 17, ..Default::default() }.valid());
    }

    #[test]
    fn supported_orientations_match_stock_and_profile_limits() {
        let (model, members) = pair();
        for kind in JointKind::ALL {
            let settings = JointSettings { kind, ..Default::default() };
            let supported = model.supported_joinery_axes(members, &settings);
            assert!(!supported.is_empty());
            for (depth_axis, width_axis) in supported {
                assert_ne!(depth_axis, width_axis);
                let mut evaluated = model.clone();
                evaluated.test_cut_joint(members, &JointSettings {
                    depth_axis, width_axis, ..settings.clone()
                }).unwrap();
            }
        }
        assert!(model.supported_joinery_axes([members[0], members[0]], &JointSettings::default()).is_empty());
        let invalid = JointSettings { clearance: 0.01, ..Default::default() };
        assert!(model.supported_joinery_axes(members, &invalid).is_empty());
    }

    #[test]
    fn all_axis_pairs_and_nested_editing_contexts_work() {
        for d in 0..3 {
            for u in 0..3 {
                if d == u {
                    continue;
                }
                let (mut model, members) = pair();
                for vertex in model.vertices.values_mut() {
                    let p = vertex.position;
                    let mut q = DVec3::ZERO;
                    q[d] = p.x;
                    q[u] = p.y;
                    q[3 - d - u] = p.z;
                    vertex.position = q;
                }
                assert!(model.supported_joinery_axes(members, &JointSettings::default())
                    .contains(&(d, u)));
                let container = model
                    .make_object(&BTreeSet::from(members), true, "Assembly")
                    .unwrap();
                let members: Vec<_> = model.objects[&container].entities.iter().copied().collect();
                assert!(model.enter_object(container));
                model
                    .test_cut_joint(
                        [members[0], members[1]],
                        &JointSettings {
                            depth_axis: d,
                            width_axis: u,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                model.validate().unwrap();
            }
        }
    }
    #[test]
    fn existing_fasteners_survive_machining_and_follow_actual_pocket_surfaces() {
        let (mut model, members) = pair();
        let id = model
            .add_connection(Connection {
                members,
                entry_face: None,
                operation: ConnectionOperation::Fasteners(Default::default()),
            })
            .unwrap();
        model
            .test_cut_joint(
                members,
                &JointSettings {
                    clearance: 0.,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(model.connections.contains_key(&id));
        assert_eq!(model.connections[&id].entry_face, None);
        let result = model.connections[&id].evaluate(&model);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.placements.len(), 1);
        assert!((result.placements[0].first_depth - 0.3).abs() < EPS);
        assert!((result.placements[0].penetration - 0.125).abs() < EPS);
    }
}

#[cfg(test)]
mod dynamic_tests {
    use super::*;
    use crate::{
        Document, History,
        connections::{Connection, ConnectionOperation},
    };
    fn pair() -> (Model, [u64; 2]) {
        super::tests::pair()
    }
    fn add(model: &mut Model, members: [u64; 2], kind: JointKind) -> u64 {
        model
            .add_connection(Connection {
                members,
                entry_face: None,
                operation: ConnectionOperation::Joinery(JointSettings {
                    kind,
                    clearance: 0.,
                    ..Default::default()
                }),
            })
            .unwrap()
    }
    fn shift(model: &mut Model, id: u64, delta: DVec3) {
        for vertex in model.selection_vertices(&BTreeSet::from([id])) {
            model.vertices.get_mut(&vertex).unwrap().position += delta;
        }
    }
    #[test]
    fn moves_recompute_all_profiles_and_separation_recovers_without_baking_source() {
        for kind in JointKind::ALL {
            let (mut model, members) = pair();
            let source = model.clone();
            let id = add(&mut model, members, kind);
            assert_eq!(model.vertices, source.vertices);
            assert_eq!(model.faces, source.faces);
            assert_eq!(model.objects, source.objects);
            let first = model.evaluate_joinery();
            assert!(first.errors.is_empty(), "{kind:?}: {:?}", first.errors);
            let volume = first.model.solid_volume(members[0]).unwrap();
            shift(&mut model, members[1], DVec3::X * 0.01);
            let moved_source = model.clone();
            let moved = model.evaluate_joinery();
            assert!(moved.errors.is_empty(), "{kind:?}: {:?}", moved.errors);
            assert!((moved.model.solid_volume(members[0]).unwrap() - volume).abs() > 1e-8);
            assert_eq!(model, moved_source, "Evaluation mutated the source");
            shift(&mut model, members[1], DVec3::X);
            let separated = model.evaluate_joinery();
            assert_eq!(separated.errors.len(), 1);
            assert_eq!(separated.errors[0].0, id);
            assert_eq!(separated.model.vertices, model.vertices);
            assert!(model.connections.contains_key(&id));
            shift(&mut model, members[1], -DVec3::X);
            assert!(model.evaluate_joinery().errors.is_empty());
            model.connections.remove(&id);
            assert_eq!(model.evaluate_joinery().model.vertices, model.vertices);
        }
    }
    #[test]
    fn relation_roundtrip_undo_copy_and_grouping_keep_dynamic_members() {
        let (mut model, members) = pair();
        let before = model.clone();
        let id = add(&mut model, members, JointKind::MortiseTenon);
        let mut history = History::default();
        history.record("Dynamic joint", before.clone(), &model);
        assert!(history.undo(&mut model));
        assert_eq!(model, before);
        assert!(history.redo(&mut model));
        assert!(model.connections.contains_key(&id));
        let doc = Document {
            model: model.clone(),
            history,
            ..Default::default()
        };
        let loaded = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(loaded.model, model);
        assert!(loaded.model.evaluate_joinery().errors.is_empty());
        assert!(
            model
                .copy_entities(&BTreeSet::from([members[0]]))
                .connections
                .is_empty()
        );
        let copy = model.copy_entities(&BTreeSet::from(members));
        model.paste_entities(&copy, DVec3::Y);
        assert_eq!(model.connections.len(), 2);
        assert!(model.evaluate_joinery().errors.is_empty());
        model
            .make_object(&BTreeSet::from(members), false, "Assembly")
            .unwrap();
        assert!(model.evaluate_joinery().errors.is_empty());
    }
    #[test]
    fn settings_edits_deletion_and_invalid_positions_restore_source_without_accumulating_cuts() {
        let (mut model, members) = pair();
        let source = model.clone();
        let id = add(&mut model, members, JointKind::MortiseTenon);
        let baseline = model
            .evaluate_joinery()
            .model
            .solid_volume(members[0])
            .unwrap();
        if let ConnectionOperation::Joinery(settings) =
            &mut model.connections.get_mut(&id).unwrap().operation
        {
            settings.width_ratio = 0.7;
        }
        let edited = model.evaluate_joinery();
        assert!(edited.errors.is_empty());
        assert!(edited.model.solid_volume(members[0]).unwrap() > baseline);
        model.locked.insert(members[0]);
        assert!(
            model.evaluate_joinery().errors.is_empty(),
            "Locking must not suppress an existing joint"
        );
        model.locked.clear();
        model.connections.remove(&id);
        let restored = model.evaluate_joinery();
        assert_eq!(restored.model.vertices, source.vertices);
        assert_eq!(restored.model.faces, source.faces);
        assert_eq!(restored.model.objects, source.objects);
        let mut legacy = serde_json::to_value(Document::default()).unwrap();
        legacy["model"]
            .as_object_mut()
            .unwrap()
            .remove("connections");
        assert!(
            Document::from_json(&legacy.to_string())
                .unwrap()
                .model
                .evaluate_joinery()
                .errors
                .is_empty()
        );
    }

    #[test]
    fn multiple_relations_on_one_board_evaluate_from_original_stock() {
        let (mut model, members) = pair();
        let copy = model.copy_entities(&BTreeSet::from([members[1]]));
        let c = *model
            .paste_entities(&copy, DVec3::X * -0.5)
            .first()
            .unwrap();
        add(&mut model, members, JointKind::MortiseTenon);
        add(&mut model, [members[0], c], JointKind::Finger);
        let evaluation = model.evaluate_joinery();
        assert!(evaluation.errors.is_empty(), "{:?}", evaluation.errors);
        evaluation.model.validate().unwrap();
        assert_eq!(
            model.objects[&members[0]].entities.len(),
            model.objects[&members[1]].entities.len()
        );
    }
}
