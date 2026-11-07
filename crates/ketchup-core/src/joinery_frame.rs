//! Orthonormal machining axes derived from the first board, at any orientation.
use crate::{DVec3, Model};
use glam::DMat3;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JoineryFrame {
    basis: DMat3,
}

impl Default for JoineryFrame {
    fn default() -> Self {
        Self {
            basis: DMat3::IDENTITY,
        }
    }
}

impl JoineryFrame {
    pub fn local(self, point: DVec3) -> DVec3 {
        self.basis.transpose() * point
    }

    pub fn world(self, point: DVec3) -> DVec3 {
        self.basis * point
    }

    pub fn is_project_aligned(self) -> bool {
        self.basis == DMat3::IDENTITY
    }

    pub fn bounds(self, model: &Model, member: u64) -> Option<(DVec3, DVec3)> {
        let ids = model.selection_vertices(&BTreeSet::from([member]));
        let mut points = ids.iter().map(|id| self.local(model.vertices[id].position));
        let first = points.next()?;
        Some(points.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }
}

impl Model {
    /// Keep project axes for existing aligned stock. Rotated boards use retained
    /// instance axes or, for imported stock, the closest face directions.
    pub fn joinery_frame(&self, member: u64) -> Result<JoineryFrame, &'static str> {
        let object = self
            .objects
            .get(&member)
            .ok_or("Choose two groups or components")?;
        if object
            .entities
            .iter()
            .any(|id| !self.edges.contains_key(id) && !self.faces.contains_key(id))
        {
            return Err(
                "Each part must contain only board geometry; select individual boards, not assemblies",
            );
        }
        let normals: Vec<_> = object
            .entities
            .iter()
            .filter_map(|id| self.faces.get(id))
            .filter_map(|face| crate::faces::plane(self, &face.vertices).map(|plane| plane.normal))
            .collect();
        let fits = |basis: DMat3| {
            basis.is_finite()
                && (basis.transpose() * basis - DMat3::IDENTITY)
                    .to_cols_array()
                    .iter()
                    .all(|x| x.abs() < 1e-9)
                && !normals.is_empty()
                && normals
                    .iter()
                    .all(|normal| (basis.transpose() * *normal).abs().max_element() > 1. - 1e-12)
        };
        if fits(DMat3::IDENTITY) {
            return Ok(JoineryFrame {
                basis: DMat3::IDENTITY,
            });
        }
        if let Some(transform) = object.transform {
            let basis = DMat3::from_cols(
                transform.basis.x_axis.normalize_or_zero(),
                transform.basis.y_axis.normalize_or_zero(),
                transform.basis.z_axis.normalize_or_zero(),
            );
            if basis.is_finite()
                && (basis.transpose() * basis - DMat3::IDENTITY)
                    .to_cols_array()
                    .iter()
                    .all(|x| x.abs() < 1e-9)
            {
                return Ok(JoineryFrame { basis });
            }
        }
        let x = *normals.first().ok_or("The board has no geometry")?;
        let y = normals
            .iter()
            .find(|normal| normal.dot(x).abs() < 1e-9)
            .copied()
            .or_else(|| {
                object
                    .entities
                    .iter()
                    .filter_map(|id| self.faces.get(id))
                    .find_map(|face| crate::faces::plane(self, &face.vertices).map(|plane| plane.u))
            })
            .ok_or("The board has no planar faces")?;
        let y = (y - x * x.dot(y)).normalize();
        let axes = [x, y, x.cross(y).normalize_or_zero()];
        let permutations = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        let order = permutations
            .into_iter()
            .max_by(|a, b| {
                let score =
                    |order: &[usize; 3]| (0..3).map(|i| axes[order[i]][i].abs()).sum::<f64>();
                score(a).total_cmp(&score(b))
            })
            .unwrap();
        let aligned = std::array::from_fn::<_, 3, _>(|i| {
            let axis = axes[order[i]];
            if axis[i] < 0. { -axis } else { axis }
        });
        let basis = DMat3::from_cols(aligned[0], aligned[1], aligned[2]);
        Ok(JoineryFrame { basis })
    }
}
