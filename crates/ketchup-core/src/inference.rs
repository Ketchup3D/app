use crate::{Camera, Model};
use glam::{DVec2, DVec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inference {
    Plane,
    Endpoint,
    Midpoint,
    Origin,
    Axis(Axis),
}
impl Inference {
    pub fn label(self) -> &'static str {
        match self {
            Self::Plane => "On plane",
            Self::Endpoint => "Endpoint",
            Self::Midpoint => "Midpoint",
            Self::Origin => "Origin",
            Self::Axis(a) => a.label(),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Snap {
    pub point: DVec3,
    pub kind: Inference,
}

pub fn infer(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    cursor: DVec2,
    start: Option<DVec3>,
    lock: Option<Axis>,
) -> Option<Snap> {
    let ray = camera.ray(cursor, size);
    let mut candidates = vec![(DVec3::ZERO, Inference::Origin)];
    candidates.extend(
        model
            .vertices
            .values()
            .map(|v| (v.position, Inference::Endpoint)),
    );
    candidates.extend(model.edges.values().map(|e| {
        let (a, b) = model.points(e);
        ((a + b) * 0.5, Inference::Midpoint)
    }));
    let nearby = candidates
        .into_iter()
        .filter(|(p, _)| {
            if let (Some(start), Some(axis)) = (start, lock) {
                let diff = *p - start;
                (diff - axis.vector() * diff.dot(axis.vector())).length() < 1e-6
            } else {
                true
            }
        })
        .filter_map(|(point, kind)| {
            let distance = camera.project(point, size)?.distance(cursor);
            (distance < 11.0).then_some((distance, Snap { point, kind }))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, snap)) = nearby {
        return Some(snap);
    }
    if let (Some(start), Some(axis)) = (start, lock) {
        return Some(Snap {
            point: ray.axis(start, axis.vector())?,
            kind: Inference::Axis(axis),
        });
    }
    if let Some(start) = start {
        let screen_start = camera.project(start, size)?;
        if screen_start.distance(cursor) > 16.0 {
            let axis = [Axis::X, Axis::Y, Axis::Z]
                .into_iter()
                .filter_map(|axis| {
                    let point = ray.axis(start, axis.vector())?;
                    let distance = camera.project(point, size)?.distance(cursor);
                    (distance < 7.0).then_some((
                        distance,
                        Snap {
                            point,
                            kind: Inference::Axis(axis),
                        },
                    ))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, snap)) = axis {
                return Some(snap);
            }
        }
    }
    Some(Snap {
        point: ray.plane_z(start.unwrap_or(DVec3::ZERO).z)?,
        kind: Inference::Plane,
    })
}
