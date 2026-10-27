//! Measurements and persistent, geometry-attached linear dimensions.
use crate::{Axis, DVec3, Model, document::EPS};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DimensionAnchor {
    pub point: DVec3,
    /// Barycentric attachment to a vertex, edge, or surface triangle.
    #[serde(default)]
    pub weights: Vec<(u64, f64)>,
}
impl DimensionAnchor {
    pub(crate) fn validate(&self, model: &Model) -> Result<(), &'static str> {
        if !valid_point(self.point)
            || self.weights.len() > 3
            || self
                .weights
                .iter()
                .map(|(id, _)| *id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.weights.len()
            || self.weights.iter().any(|(id, w)| {
                !model.vertices.contains_key(id) || !w.is_finite() || *w < -EPS || *w > 1. + EPS
            })
        {
            return Err("Invalid dimension attachment");
        }
        if !self.weights.is_empty()
            && (self.weights.iter().map(|(_, w)| w).sum::<f64>() - 1.).abs() > EPS
        {
            return Err("Invalid dimension attachment weights");
        }
        if !valid_point(self.position(model)) {
            return Err("Invalid dimension position");
        }
        Ok(())
    }
    pub fn position(&self, model: &Model) -> DVec3 {
        if self.weights.is_empty()
            || self
                .weights
                .iter()
                .any(|(id, _)| !model.vertices.contains_key(id))
        {
            return self.point;
        }
        self.weights
            .iter()
            .map(|(id, w)| model.vertices[id].position * *w)
            .sum()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub id: u64,
    pub start: DimensionAnchor,
    pub end: DimensionAnchor,
    pub offset: DVec3,
    /// When set, measure the separation along this world axis. A perpendicular
    /// axis keeps the full span instead of collapsing it to a zero-length line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<Axis>,
}
impl Dimension {
    pub fn points(&self, model: &Model) -> (DVec3, DVec3) {
        (self.start.position(model), self.end.position(model))
    }
    fn projected_axis(&self, a: DVec3, b: DVec3) -> Option<Axis> {
        self.axis
            .filter(|axis| (b - a).dot(axis.vector()).abs() >= EPS)
    }
    /// Direction of the dimension line, preserving the span when the requested
    /// axis is perpendicular to it. Placement can then use that axis as an offset.
    pub fn line_direction(&self, model: &Model) -> DVec3 {
        let (a, b) = self.points(model);
        self.projected_axis(a, b)
            .map(Axis::vector)
            .unwrap_or_else(|| (b - a).try_normalize().unwrap_or(DVec3::X))
    }
    pub fn length(&self, model: &Model) -> f64 {
        let (a, b) = self.points(model);
        self.projected_axis(a, b)
            .map_or_else(|| a.distance(b), |axis| (b - a).dot(axis.vector()).abs())
    }
    pub fn layout(&self, model: &Model) -> [DVec3; 4] {
        let (a, b) = self.points(model);
        let direction = self.line_direction(model);
        let offset = self.offset - direction * self.offset.dot(direction);
        if self.projected_axis(a, b).is_some() {
            let center = (a + b) * 0.5;
            let line = center + offset;
            [
                a,
                b,
                line + direction * (a - center).dot(direction),
                line + direction * (b - center).dot(direction),
            ]
        } else {
            [a, b, a + offset, b + offset]
        }
    }
}

pub fn angle_degrees(center: DVec3, baseline: DVec3, target: DVec3) -> Option<f64> {
    if !center.is_finite() || !baseline.is_finite() || !target.is_finite() {
        return None;
    }
    let a = (baseline - center).try_normalize()?;
    let b = (target - center).try_normalize()?;
    Some(a.cross(b).length().atan2(a.dot(b)).to_degrees())
}

pub fn angle_arc(center: DVec3, baseline: DVec3, target: DVec3) -> Vec<DVec3> {
    let Some(u) = (baseline - center).try_normalize() else {
        return vec![];
    };
    let Some(angle) = angle_degrees(center, baseline, target) else {
        return vec![];
    };
    let normal = u.cross(target - center).try_normalize().unwrap_or_else(|| {
        u.cross(if u.z.abs() < 0.9 { DVec3::Z } else { DVec3::X })
            .normalize()
    });
    let v = normal.cross(u);
    let radius = baseline.distance(center).min(target.distance(center)) * 0.3;
    (0..=32)
        .map(|i| {
            let t = angle.to_radians() * i as f64 / 32.;
            center + radius * (u * t.cos() + v * t.sin())
        })
        .collect()
}

impl Model {
    pub fn dimension_anchor(&self, point: DVec3) -> DimensionAnchor {
        let mut anchor = DimensionAnchor {
            point,
            weights: vec![],
        };
        for edge in self.edges.values().filter(|e| {
            !self.is_hidden(e.id)
                && (self.edit_context.is_none() || self.entity_in_context(e.id).is_some())
        }) {
            let (a, b) = self.points(edge);
            for (id, p) in [(edge.a, a), (edge.b, b)] {
                if p.distance(point) < EPS {
                    anchor.weights = vec![(id, 1.)];
                    return anchor;
                }
            }
        }
        for edge in self.edges.values().filter(|e| {
            !self.is_hidden(e.id)
                && (self.edit_context.is_none() || self.entity_in_context(e.id).is_some())
        }) {
            let (a, b) = self.points(edge);
            let t = (point - a).dot(b - a) / (b - a).length_squared();
            if (0. ..=1.).contains(&t) && (a + (b - a) * t).distance(point) < EPS {
                anchor.weights = vec![(edge.a, 1. - t), (edge.b, t)];
                return anchor;
            }
        }
        // Use the same triangulation as rendering, so holes never acquire attachments.
        for face in self.faces.values().filter(|f| {
            !self.is_hidden(f.id)
                && (self.edit_context.is_none() || self.entity_in_context(f.id).is_some())
                && self.face_contains(f, point, true)
        }) {
            for [a, b, c] in self.face_triangles(face) {
                let (u, v, w) = (b - a, c - a, point - a);
                let denominator = u.dot(u) * v.dot(v) - u.dot(v).powi(2);
                if denominator.abs() < 1e-16 {
                    continue;
                }
                let y = (v.dot(v) * w.dot(u) - u.dot(v) * w.dot(v)) / denominator;
                let z = (u.dot(u) * w.dot(v) - u.dot(v) * w.dot(u)) / denominator;
                let weights = [1. - y - z, y, z];
                if weights.iter().any(|w| *w < -EPS || *w > 1. + EPS) {
                    continue;
                }
                let ids: Option<Vec<_>> = [a, b, c]
                    .into_iter()
                    .map(|p| {
                        face.vertices
                            .iter()
                            .chain(face.holes.iter().flatten())
                            .find(|id| self.vertices[id].position.distance(p) < EPS)
                            .copied()
                    })
                    .collect();
                if let Some(ids) = ids {
                    anchor.weights = ids.into_iter().zip(weights).collect();
                    return anchor;
                }
            }
        }
        anchor
    }
    pub fn add_dimension(
        &mut self,
        start: DVec3,
        end: DVec3,
        offset: DVec3,
    ) -> Result<u64, &'static str> {
        self.add_dimension_with_axis(start, end, offset, None)
    }
    pub fn add_dimension_with_axis(
        &mut self,
        start: DVec3,
        end: DVec3,
        offset: DVec3,
        axis: Option<Axis>,
    ) -> Result<u64, &'static str> {
        if self.edit_context.is_some() {
            return self
                .edit_in_context(|model| model.add_dimension_with_axis(start, end, offset, axis));
        }
        if !valid_point(start) || !valid_point(end) || !valid_point(offset) {
            return Err("Dimension coordinates must be finite and within the model limits");
        }
        if start.distance(end) < EPS {
            return Err("Choose two different dimension endpoints");
        }
        let (start, end) = (self.dimension_anchor(start), self.dimension_anchor(end));
        let mut dimension = Dimension {
            id: 0,
            start,
            end,
            offset,
            axis,
        };
        if dimension.layout(self).iter().any(|p| !valid_point(*p)) {
            return Err("Dimension is outside the model limits");
        }
        let id = self.id();
        dimension.id = id;
        self.dimensions.insert(id, dimension);
        Ok(id)
    }
    /// Snapshot attachments before topology is removed; detached annotations retain their last positions.
    pub(crate) fn sync_dimensions(&mut self) {
        self.sync_texts();
        let updates: Vec<_> = self
            .dimensions
            .values()
            .map(|d| (d.id, d.points(self)))
            .collect();
        for (id, (a, b)) in updates {
            let d = self.dimensions.get_mut(&id).unwrap();
            d.start.point = a;
            d.end.point = b;
            for anchor in [&mut d.start, &mut d.end] {
                if anchor
                    .weights
                    .iter()
                    .any(|(id, _)| !self.vertices.contains_key(id))
                {
                    anchor.weights.clear();
                }
            }
        }
    }
    pub(crate) fn validate_dimensions(&self) -> Result<(), &'static str> {
        for (id, d) in &self.dimensions {
            if *id != d.id || !valid_point(d.offset) {
                return Err("Invalid dimension");
            }
            for anchor in [&d.start, &d.end] {
                anchor.validate(self)?;
            }
            if d.layout(self).iter().any(|p| !valid_point(*p)) {
                return Err("Dimension is outside the model limits");
            }
        }
        Ok(())
    }
}
pub(crate) fn valid_point(p: DVec3) -> bool {
    p.is_finite() && p.abs().max_element() <= 1e10
}
