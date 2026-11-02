//! Non-destructive object relations. Geometry is evaluated from current vertices,
//! never baked into either member. Members must be watertight planar solids.
use crate::{DVec3, Model};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
const TOL: f64 = 1e-7;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FastenerKind {
    #[default]
    Screw,
    Nail,
    InclinedScrew,
}
impl FastenerKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Screw => "Screw",
            Self::Nail => "Nail",
            Self::InclinedScrew => "Angled screw",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pattern {
    #[default]
    Single,
    Row,
    DoubleRow,
    Grid,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FastenerSettings {
    pub kind: FastenerKind,
    pub pattern: Pattern,
    pub count: u32,
    pub rows: u32,
    /// Metres. Zero spacing distributes evenly over the common projected area.
    pub spacing: f64,
    pub row_spacing: f64,
    pub diameter: f64,
    /// Zero chooses a geometric preview length, not a product specification.
    pub length: f64,
    /// Geometric clearance only; never a manufacturer's edge-distance rule.
    pub clearance: f64,
    pub angle_degrees: f64,
}
impl Default for FastenerSettings {
    fn default() -> Self {
        Self {
            kind: FastenerKind::Screw,
            pattern: Pattern::Single,
            count: 3,
            rows: 3,
            spacing: 0.,
            row_spacing: 0.,
            diameter: 0.004,
            length: 0.,
            clearance: 0.01,
            angle_degrees: 15.,
        }
    }
}
impl FastenerSettings {
    pub fn valid(&self) -> bool {
        (1..=100).contains(&self.count)
            && (1..=100).contains(&self.rows)
            && [self.spacing, self.row_spacing, self.length, self.clearance]
                .iter()
                .all(|x| x.is_finite() && *x >= 0. && *x <= 100.)
            && self.diameter.is_finite()
            && self.diameter > 0.
            && self.diameter <= 1.
            && self.angle_degrees.is_finite()
            && (0. ..=60.).contains(&self.angle_degrees)
    }
}
/// Persistent relations evaluated from the current source geometry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    pub members: [u64; 2],
    /// Stable entry face on the first member. None selects it automatically.
    pub entry_face: Option<u64>,
    pub operation: ConnectionOperation,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ConnectionOperation {
    Fasteners(FastenerSettings),
    Joinery(crate::joinery::JointSettings),
}
#[derive(Clone, Debug)]
pub struct FastenerPlacement {
    pub head: DVec3,
    pub tip: DVec3,
    pub interface: DVec3,
    pub first_depth: f64,
    pub penetration: f64,
    pub warnings: Vec<String>,
}
#[derive(Default, Debug)]
pub struct Evaluation {
    pub placements: Vec<FastenerPlacement>,
    pub errors: Vec<String>,
    pub entry_face: Option<u64>,
}
#[derive(Clone)]
struct Plane {
    id: u64,
    origin: DVec3,
    normal: DVec3,
    points: Vec<DVec3>,
}
struct Member {
    geometry: Model,
    convex: bool,
    planes: Vec<Plane>,
    center: DVec3,
    points: Vec<DVec3>,
}
impl Member {
    fn read(model: &Model, id: u64) -> Result<Self, String> {
        if !model.objects.contains_key(&id) {
            return Err("Part is missing or has been deleted".into());
        }
        model
            .copy_entities(&BTreeSet::from([id]))
            .solid_volume(id)?;
        let geometry = model.object_contents(id).ok_or("Part is missing")?;
        let entities = model.expand_entities(&BTreeSet::from([id]));
        let mut edges = BTreeMap::new();
        let mut planes = Vec::new();
        let mut points = Vec::new();
        for face in entities.iter().filter_map(|id| model.faces.get(id)) {
            if face.vertices.len() < 3 {
                return Err("Degenerate face".into());
            }
            let p: Vec<_> = face
                .vertices
                .iter()
                .filter_map(|id| model.vertices.get(id).map(|v| v.position))
                .collect();
            if p.len() != face.vertices.len() || p.iter().any(|v| !v.is_finite()) {
                return Err("Invalid vertices".into());
            }
            let n = (1..p.len() - 1)
                .map(|i| (p[i] - p[0]).cross(p[i + 1] - p[0]))
                .find(|n| n.length() > TOL)
                .ok_or("Degenerate face")?
                .normalize();
            for ring in std::iter::once(&face.vertices).chain(&face.holes) {
                for i in 0..ring.len() {
                    let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                    *edges.entry((a.min(b), a.max(b))).or_insert(0u32) += 1;
                }
            }
            points.extend(&p);
            planes.push(Plane {
                id: face.id,
                origin: p[0],
                normal: n,
                points: p,
            });
        }
        if planes.len() < 4 || edges.values().any(|n| *n != 2) {
            return Err("The part must be a closed solid".into());
        }
        let center = points.iter().copied().sum::<DVec3>() / points.len() as f64;
        // Convex parts retain their half-space clipping. Machined parts use
        // actual bounded face crossings, so pockets and shoulders are respected.
        let mut convex = geometry.faces.values().all(|f| f.holes.is_empty());
        for plane in &mut planes {
            if plane.normal.dot(plane.origin - center) < 0. {
                plane.normal = -plane.normal;
            }
            if plane
                .points
                .iter()
                .any(|p| plane.normal.dot(*p - plane.origin).abs() > TOL)
            {
                return Err("Only planar faces are supported".into());
            }
            if points
                .iter()
                .any(|p| plane.normal.dot(*p - plane.origin) > TOL)
            {
                convex = false;
            }
        }
        Ok(Self {
            geometry,
            convex,
            planes,
            center,
            points,
        })
    }
    fn interval(&self, origin: DVec3, direction: DVec3) -> Option<(f64, f64)> {
        if !self.convex {
            // Sort all line/surface crossings, including faces behind the origin.
            // Midpoints between crossings are classified by a second generic ray
            // to avoid treating a tangent edge as a material boundary.
            let crossings = |origin: DVec3, direction: DVec3| {
                let mut hits: Vec<f64> = self
                    .geometry
                    .faces
                    .values()
                    .filter_map(|face| {
                        let plane = crate::faces::plane(&self.geometry, &face.vertices)?;
                        let den = plane.normal.dot(direction);
                        if den.abs() < TOL {
                            return None;
                        }
                        let t = plane
                            .normal
                            .dot(self.geometry.vertices[&face.vertices[0]].position - origin)
                            / den;
                        self.geometry
                            .face_contains(face, origin + direction * t, true)
                            .then_some(t)
                    })
                    .collect();
                hits.sort_by(f64::total_cmp);
                hits.dedup_by(|a, b| (*a - *b).abs() < TOL);
                hits
            };
            let hits = crossings(origin, direction);
            let mut spans: Vec<(f64, f64)> = Vec::new();
            for pair in hits.windows(2) {
                if pair[1] - pair[0] <= TOL {
                    continue;
                }
                let midpoint = origin + direction * ((pair[0] + pair[1]) * 0.5);
                let probe = DVec3::new(0.376321, 0.583719, 0.719371).normalize();
                let inside = crossings(midpoint, probe)
                    .iter()
                    .filter(|t| **t > TOL)
                    .count()
                    % 2
                    == 1;
                if inside {
                    if let Some(last) = spans
                        .last_mut()
                        .filter(|last| (last.1 - pair[0]).abs() < TOL)
                    {
                        last.1 = pair[1];
                    } else {
                        spans.push((pair[0], pair[1]));
                    }
                }
            }
            return spans.into_iter().find(|(_, hi)| *hi > TOL);
        }
        let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
        for p in &self.planes {
            let distance = p.normal.dot(origin - p.origin);
            let den = p.normal.dot(direction);
            if den.abs() < TOL {
                if distance > TOL {
                    return None;
                }
            } else if den < 0. {
                lo = lo.max(-distance / den);
            } else {
                hi = hi.min(-distance / den);
            }
        }
        (lo <= hi + TOL).then_some((lo, hi))
    }
}
impl Model {
    pub fn has_joinery_between(&self, members: [u64; 2]) -> bool {
        self.connections.values().any(|connection|
            matches!(connection.operation, ConnectionOperation::Joinery(_))
                && (connection.members == members || connection.members == [members[1], members[0]]))
    }

    pub fn add_connection(&mut self, mut connection: Connection) -> Result<u64, String> {
        if matches!(connection.operation, ConnectionOperation::Joinery(_)) {
            if self.has_joinery_between(connection.members) {
                return Err("These pieces already have a wood joint. Edit or delete the existing joint first.".into());
            }
            let pair = BTreeSet::from(connection.members);
            if pair.len() != 2
                || self.selection_protected(&pair)
                || connection
                    .members
                    .iter()
                    .any(|id| self.is_hidden(*id) || self.entity_in_context(*id) != Some(*id))
            {
                return Err("Select two visible, editable boards in the current context".into());
            }
        }
        let evaluation = connection.evaluate(self);
        if !evaluation.errors.is_empty()
            || (matches!(connection.operation, ConnectionOperation::Fasteners(_))
                && evaluation.placements.is_empty())
        {
            return Err(evaluation.errors.join(" · "));
        }
        connection.entry_face = evaluation.entry_face;
        let id = self.id();
        self.connections.insert(id, connection);
        Ok(id)
    }
}
impl Connection {
    pub fn entry_faces(&self, model: &Model) -> Vec<u64> {
        Member::read(model, self.members[0])
            .map(|m| m.planes.iter().map(|p| p.id).collect())
            .unwrap_or_default()
    }
    pub fn evaluate(&self, model: &Model) -> Evaluation {
        if matches!(self.operation, ConnectionOperation::Fasteners(_))
            && model
                .connections
                .values()
                .any(|c| matches!(c.operation, ConnectionOperation::Joinery(_)))
        {
            let geometry = model.evaluate_joinery();
            let mut connection = self.clone();
            if connection
                .entry_face
                .is_some_and(|face| !geometry.model.faces.contains_key(&face))
            {
                connection.entry_face = None;
            }
            return connection.evaluate(&geometry.model);
        }
        let mut result = Evaluation::default();
        let ConnectionOperation::Fasteners(settings) = &self.operation else {
            if let ConnectionOperation::Joinery(settings) = &self.operation {
                let mut geometry = model.clone();
                geometry.edit_context = None;
                if let Err(error) = geometry.cut_joint(self.members, settings, model) {
                    result.errors.push(error.into());
                }
            }
            return result;
        };
        if !settings.valid() || self.members[0] == self.members[1] {
            result
                .errors
                .push("Invalid settings or pair of parts".into());
            return result;
        }
        let (a, b) = match (
            Member::read(model, self.members[0]),
            Member::read(model, self.members[1]),
        ) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => {
                result.errors.push(e);
                return result;
            }
        };
        if self.entry_face.is_none() {
            // Try actual contact rays on every entry face. Center direction alone
            // is unreliable for offset T joints or unequal board lengths.
            let mut candidates: Vec<_> = a
                .planes
                .iter()
                .map(|face| {
                    let mut candidate = self.clone();
                    candidate.entry_face = Some(face.id);
                    (
                        candidate.evaluate(model),
                        face.normal.dot(b.center - a.center),
                    )
                })
                .collect();
            candidates.sort_by(|(a, da), (b, db)| {
                a.errors
                    .len()
                    .cmp(&b.errors.len())
                    .then_with(|| b.placements.len().cmp(&a.placements.len()))
                    .then_with(|| da.total_cmp(db))
            });
            return candidates.remove(0).0;
        }
        let face = a.planes.iter().find(|p| Some(p.id) == self.entry_face);
        let Some(face) = face else {
            result
                .errors
                .push("The entry face is missing or does not belong to the first part".into());
            return result;
        };
        result.entry_face = Some(face.id);
        let Some(u) = face
            .points
            .iter()
            .map(|p| *p - face.origin)
            .find(|d| d.length() > TOL)
            .map(|d| d.normalize())
        else {
            result.errors.push("Degenerate face".into());
            return result;
        };
        let v = face.normal.cross(u).normalize();
        let direction = if settings.kind == FastenerKind::InclinedScrew {
            (-face.normal + u * settings.angle_degrees.to_radians().tan()).normalize()
        } else {
            -face.normal
        };
        let range = |points: &[DVec3], axis: DVec3| {
            points
                .iter()
                .map(|p| p.dot(axis))
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                    (lo.min(x), hi.max(x))
                })
        };
        // Project receiver backwards onto the entry plane along the insertion axis.
        let projected: Vec<_> = b
            .points
            .iter()
            .map(|p| {
                *p - direction * (face.normal.dot(*p - face.origin) / face.normal.dot(direction))
            })
            .collect();
        let overlap = |axis| {
            let x = range(&face.points, axis);
            let y = range(&projected, axis);
            (x.0.max(y.0), x.1.min(y.1))
        };
        let (ur, vr) = (overlap(u), overlap(v));
        if ur.1 - ur.0 <= TOL || vr.1 - vr.0 <= TOL {
            result.errors.push("No shared area on this face".into());
            return result;
        }
        let (columns, rows) = match settings.pattern {
            Pattern::Single => (1, 1),
            Pattern::Row => (settings.count, 1),
            Pattern::DoubleRow => (settings.count, 2),
            Pattern::Grid => (settings.count, settings.rows),
        };
        if columns * rows > 1000 {
            result
                .errors
                .push("Maximum of 1,000 fasteners per connection".into());
            return result;
        }
        for (range, n, spacing) in [
            (ur, columns, settings.spacing),
            (vr, rows, settings.row_spacing),
        ] {
            let available = range.1 - range.0 - 2. * (settings.clearance + settings.diameter * 0.5);
            let step = if spacing > 0. {
                spacing
            } else {
                available / (n.max(2) - 1) as f64
            };
            if n > 1 && step < settings.diameter - TOL {
                result
                    .errors
                    .push("Fasteners overlap or there is not enough space for this pattern".into());
                return result;
            }
        }
        let coordinate = |range: (f64, f64), i: u32, n: u32, spacing: f64| {
            let step = if spacing > 0. {
                spacing
            } else {
                (range.1 - range.0 - 2. * (settings.clearance + settings.diameter * 0.5)).max(0.)
                    / n.max(2).saturating_sub(1) as f64
            };
            (range.0 + range.1) * 0.5 + (i as f64 - (n - 1) as f64 * 0.5) * step
        };
        for row in 0..rows {
            for col in 0..columns {
                let x = coordinate(ur, col, columns, settings.spacing);
                let y = coordinate(vr, row, rows, settings.row_spacing);
                let head =
                    face.origin + u * (x - face.origin.dot(u)) + v * (y - face.origin.dot(v));
                let Some((entry, exit)) = a.interval(head, direction) else {
                    result.errors.push(format!(
                        "Fastener {} is outside the entry face",
                        result.placements.len() + 1
                    ));
                    continue;
                };
                let Some((start, end)) = b.interval(head, direction) else {
                    result
                        .errors
                        .push("Fastener is outside the second part".into());
                    continue;
                };
                if entry.abs() > TOL || exit <= TOL || start > exit + TOL || end <= exit + TOL {
                    result
                        .errors
                        .push("The parts have no usable contact in this direction".into());
                    continue;
                }
                let length = if settings.length > 0. {
                    settings.length
                } else {
                    exit + (end - exit) * 0.5
                };
                let mut warnings = Vec::new();
                if length <= exit + TOL {
                    warnings.push("No penetration into the second part".into());
                }
                if length > end + TOL {
                    warnings.push("The tip protrudes beyond the second part".into());
                }
                let interface = head + direction * exit;
                let tip = head + direction * length;
                let radius = settings.diameter * 0.5;
                for (member, ends) in [(&a, [head, interface]), (&b, [interface, tip])] {
                    for plane in &member.planes {
                        // Entry/exit planes do not constrain lateral edge clearance.
                        let (near, far) = member.interval(head, direction).unwrap();
                        let crosses = plane.normal.dot(direction).abs() > TOL;
                        if crosses
                            && [near, far].iter().any(|t| {
                                plane.normal.dot(head + direction * *t - plane.origin).abs() < TOL
                            })
                        {
                            continue;
                        }
                        if ends.iter().any(|p| {
                            -plane.normal.dot(*p - plane.origin) < settings.clearance + radius - TOL
                        }) {
                            warnings.push("Close to an edge: check the geometric clearance".into());
                            break;
                        }
                    }
                }
                warnings.sort();
                warnings.dedup();
                result.placements.push(FastenerPlacement {
                    head,
                    tip,
                    interface,
                    first_depth: exit,
                    penetration: (length.min(end) - exit).max(0.),
                    warnings,
                });
            }
        }
        result.errors.sort();
        result.errors.dedup();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, History};
    fn box_at(model: &mut Model, lo: DVec3, hi: DVec3) -> u64 {
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
        model.make_object(&ids, false, "Board").unwrap()
    }
    fn pair() -> (Model, Connection) {
        let mut model = Model::default();
        let a = box_at(&mut model, DVec3::ZERO, DVec3::new(0.3, 0.2, 0.02));
        let b = box_at(
            &mut model,
            DVec3::new(0., 0., 0.02),
            DVec3::new(0.3, 0.2, 0.04),
        );
        (
            model,
            Connection {
                members: [a, b],
                entry_face: None,
                operation: ConnectionOperation::Fasteners(Default::default()),
            },
        )
    }
    #[test]
    fn patterns_and_actual_depths() {
        let (model, mut c) = pair();
        for (pattern, quantity) in [
            (Pattern::Single, 1),
            (Pattern::Row, 3),
            (Pattern::DoubleRow, 6),
            (Pattern::Grid, 9),
        ] {
            let ConnectionOperation::Fasteners(s) = &mut c.operation else {
                panic!("Expected fasteners")
            };
            s.pattern = pattern;
            let e = c.evaluate(&model);
            assert!(e.errors.is_empty(), "{:?}", e.errors);
            assert_eq!(e.placements.len(), quantity);
            for p in e.placements {
                assert!((p.first_depth - 0.02).abs() < TOL);
                assert!((p.penetration - 0.01).abs() < TOL);
            }
        }
    }
    #[test]
    fn relations_roundtrip_undo_redo_and_legacy_default() {
        let (mut model, c) = pair();
        let before = model.clone();
        let id = model.add_connection(c).unwrap();
        let mut history = History::default();
        history.record("Fasteners", before, &model);
        assert!(history.undo(&mut model));
        assert!(model.connections.is_empty());
        assert!(history.redo(&mut model));
        assert!(model.connections.contains_key(&id));
        let doc = Document {
            model,
            history,
            ..Default::default()
        };
        let json = serde_json::to_string(&doc).unwrap();
        let restored = Document::from_json(&json).unwrap();
        assert_eq!(restored.model.connections, doc.model.connections);
        let mut value = serde_json::to_value(Document::default()).unwrap();
        value["model"]
            .as_object_mut()
            .unwrap()
            .remove("connections");
        assert!(
            Document::from_json(&value.to_string())
                .unwrap()
                .model
                .connections
                .is_empty()
        );
    }
    #[test]
    fn movement_separation_and_deleted_faces_are_diagnosed() {
        let (mut model, mut c) = pair();
        let before = c.evaluate(&model).placements[0].head;
        for vertex in model.vertices.values_mut() {
            vertex.position.x += 1.;
        }
        assert!((c.evaluate(&model).placements[0].head.x - before.x - 1.).abs() < TOL);
        c.entry_face = Some(u64::MAX);
        assert!(!c.evaluate(&model).errors.is_empty());
        c.entry_face = None;
        let ids = model.expand_entities(&BTreeSet::from([c.members[1]]));
        for id in ids {
            if let Some(v) = model.vertices.get_mut(&id) {
                v.position.z += 0.1;
            }
        }
        assert!(!c.evaluate(&model).errors.is_empty());
    }
    #[test]
    fn protrusion_spacing_and_invalid_parameters() {
        let (model, mut c) = pair();
        let ConnectionOperation::Fasteners(s) = &mut c.operation else {
            panic!("Expected fasteners")
        };
        s.length = 0.05;
        assert!(
            c.evaluate(&model).placements[0]
                .warnings
                .iter()
                .any(|s| s.contains("protrudes"))
        );
        let ConnectionOperation::Fasteners(s) = &mut c.operation else {
            panic!("Expected fasteners")
        };
        s.pattern = Pattern::Row;
        s.spacing = 1.;
        assert!(!c.evaluate(&model).errors.is_empty());
        let ConnectionOperation::Fasteners(s) = &mut c.operation else {
            panic!("Expected fasteners")
        };
        s.length = f64::NAN;
        assert!(!c.evaluate(&model).errors.is_empty());
    }
    #[test]
    fn grouping_members_preserves_relation_identity() {
        let (mut model, c) = pair();
        let members = c.members;
        let id = model.add_connection(c).unwrap();
        model
            .make_object(&BTreeSet::from(members), false, "Assembly")
            .unwrap();
        assert_eq!(model.connections.len(), 1);
        assert!(model.connections[&id].evaluate(&model).errors.is_empty());
        model.validate().unwrap();
    }
    #[test]
    fn reverse_and_thickness_changes_recompute_depths() {
        let (mut model, mut c) = pair();
        c.members.swap(0, 1);
        let evaluation = c.evaluate(&model);
        assert!(evaluation.errors.is_empty());
        assert!(evaluation.placements[0].tip.z < evaluation.placements[0].head.z);
        c.members.swap(0, 1);
        let entities = model.expand_entities(&BTreeSet::from([c.members[1]]));
        for id in entities {
            if let Some(v) = model.vertices.get_mut(&id) {
                if v.position.z > 0.039 {
                    v.position.z = 0.06;
                }
            }
        }
        assert!((c.evaluate(&model).placements[0].penetration - 0.02).abs() < TOL);
        c.members[1] = u64::MAX;
        assert!(!c.evaluate(&model).errors.is_empty());
    }
    #[test]
    fn rotated_members_use_real_faces_and_overlapping_pairs_work() {
        let (mut model, c) = pair();
        let before = c.evaluate(&model).placements[0].clone();
        let rotate = |p: DVec3| DVec3::new(p.x * 0.6 - p.z * 0.8, p.y, p.x * 0.8 + p.z * 0.6);
        for v in model.vertices.values_mut() {
            v.position = rotate(v.position);
        }
        let evaluation = c.evaluate(&model);
        assert!(evaluation.errors.is_empty(), "{:?}", evaluation.errors);
        let after = &evaluation.placements[0];
        assert!(after.head.distance(rotate(before.head)) < TOL);
        assert!((after.penetration - before.penetration).abs() < TOL);
        let (mut model, c) = pair();
        let ids = model.expand_entities(&BTreeSet::from([c.members[1]]));
        for id in ids {
            if let Some(v) = model.vertices.get_mut(&id) {
                v.position.z -= 0.005;
            }
        }
        assert!(c.evaluate(&model).errors.is_empty());
    }
    #[test]
    fn display_defaults_accept_old_projects() {
        let display: crate::DisplaySettings = serde_json::from_str("{}").unwrap();
        assert!(display.fasteners);
        assert!(!display.fastener_preview);
        assert!(!display.fastener_face_handles);
    }
    #[test]
    fn copied_relations_only_follow_both_members_with_new_ids() {
        let (mut model, c) = pair();
        let members = c.members;
        model.add_connection(c).unwrap();
        assert!(
            model
                .copy_entities(&BTreeSet::from([members[0]]))
                .connections
                .is_empty()
        );
        let clipboard = model.copy_entities(&BTreeSet::from(members));
        model.paste_entities(&clipboard, DVec3::new(1., 0., 0.));
        assert_eq!(model.connections.len(), 2);
        let copied = model
            .connections
            .values()
            .find(|c| c.members != members)
            .unwrap();
        assert!(copied.evaluate(&model).errors.is_empty());
    }
}
