//! Trim a chosen end at the other board's outer face, without cutting the joint.
use super::{EPS, JointSettings, KeepEnd, box_points, prism};
use crate::{
    DVec3, Model,
    csg::{self, Boolean, Polygon},
    solids::Frame,
};

pub(super) fn trim_excess(
    source: &Model,
    members: [u64; 2],
    index: usize,
    settings: &JointSettings,
    other: &[Polygon],
    overlap: &[Polygon],
    solid: Vec<Polygon>,
    frame: Frame,
) -> Result<Vec<Polygon>, &'static str> {
    let axes = source.joinery_frame(members[index])?;
    let (lo, hi) = axes
        .bounds(source, members[index])
        .ok_or("Missing board geometry")?;
    let size = hi - lo;
    let length = settings
        .trim_axis
        .unwrap_or_else(|| (0..3).max_by(|&a, &b| size[a].total_cmp(&size[b])).unwrap());
    let mut direction = DVec3::ZERO;
    direction[length] = if settings.keep_end == KeepEnd::Lower {
        1.
    } else {
        -1.
    };
    let direction = axes.world(direction);
    // Prefer a real supporting face, giving a flush angled trim. Concave stock
    // without a supporting face uses the far boundary of the actual overlap.
    let points: Vec<_> = overlap
        .iter()
        .flat_map(|polygon| polygon.vertices.iter().copied())
        .collect();
    let plane = other
        .iter()
        .filter(|polygon| {
            polygon.normal.dot(direction) > 1e-9
                && points
                    .iter()
                    .all(|p| polygon.normal.dot(*p) <= polygon.w + frame.epsilon * 16.)
                && points
                    .iter()
                    .any(|p| (polygon.normal.dot(*p) - polygon.w).abs() < frame.epsilon * 16.)
        })
        .max_by(|a, b| a.normal.dot(direction).total_cmp(&b.normal.dot(direction)));
    let (normal, w) = plane.map_or_else(
        || {
            (
                direction,
                points
                    .iter()
                    .map(|p| direction.dot(*p))
                    .fold(f64::NEG_INFINITY, f64::max),
            )
        },
        |polygon| (polygon.normal, polygon.w),
    );
    if solid
        .iter()
        .flat_map(|polygon| &polygon.vertices)
        .all(|p| normal.dot(*p) <= w + frame.epsilon)
    {
        return Ok(solid);
    }
    let tangent = normal
        .cross(if normal.x.abs() < 0.9 {
            DVec3::X
        } else {
            DVec3::Y
        })
        .normalize();
    let bitangent = normal.cross(tangent);
    let center = normal * w;
    let mut projected = solid.iter().flat_map(|polygon| &polygon.vertices).map(|p| {
        let p = *p - center;
        DVec3::new(p.dot(tangent), p.dot(bitangent), p.dot(normal))
    });
    let first = projected
        .next()
        .ok_or("The joint would remove an entire board")?;
    let (lo, hi) = projected.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
    let padding = (hi - lo).max_element().max(EPS) * 0.1;
    let (lo, hi) = (
        lo - DVec3::splat(padding),
        DVec3::new(hi.x + padding, hi.y + padding, 0.),
    );
    if lo.z >= -frame.epsilon {
        return Err("Trimming would remove an entire board; keep the other end");
    }
    let cutter = prism(
        box_points(lo, hi)
            .map(|p| frame.world(center + tangent * p.x + bitangent * p.y + normal * p.z)),
        frame,
    )?;
    csg::apply(solid, cutter, Boolean::Intersect, frame.epsilon)
}
