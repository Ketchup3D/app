use crate::{Camera, Model};
use glam::DVec2;
use std::collections::BTreeSet;

pub fn distance_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let d = b - a;
    let t = if d.length_squared() < 1e-12 {
        0.0
    } else {
        ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
    };
    p.distance(a + d * t)
}
pub fn segment_distance(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> f64 {
    let ab = b - a;
    let cd = d - c;
    let denominator = ab.perp_dot(cd);
    if denominator.abs() > 1e-12 {
        let t = (c - a).perp_dot(cd) / denominator;
        let u = (c - a).perp_dot(ab) / denominator;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            return 0.0;
        }
    }
    distance_to_segment(a, c, d)
        .min(distance_to_segment(b, c, d))
        .min(distance_to_segment(c, a, b))
        .min(distance_to_segment(d, a, b))
}
pub fn pick(model: &Model, camera: &Camera, size: DVec2, point: DVec2, radius: f64) -> Option<u64> {
    model
        .edges
        .values()
        .filter_map(|e| {
            let (a, b) = model.points(e);
            let (a, b) = camera.segment(a, b, size)?;
            let d = distance_to_segment(point, a, b);
            (d <= radius).then_some((e.id, d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|v| v.0)
}
pub fn intersects_rect(a: DVec2, b: DVec2, lo: DVec2, hi: DVec2) -> bool {
    let delta = b - a;
    let mut enter: f64 = 0.0;
    let mut exit: f64 = 1.0;
    for i in 0..2 {
        if delta[i].abs() < 1e-12 {
            if a[i] < lo[i] || a[i] > hi[i] {
                return false;
            }
        } else {
            let t1 = (lo[i] - a[i]) / delta[i];
            let t2 = (hi[i] - a[i]) / delta[i];
            enter = enter.max(t1.min(t2));
            exit = exit.min(t1.max(t2));
            if enter > exit {
                return false;
            }
        }
    }
    true
}
pub fn box_select(
    model: &Model,
    camera: &Camera,
    size: DVec2,
    start: DVec2,
    end: DVec2,
) -> BTreeSet<u64> {
    let lo = start.min(end);
    let hi = start.max(end);
    let crossing = start.x > end.x;
    let inside = |p: DVec2| p.cmpge(lo).all() && p.cmple(hi).all();
    model
        .edges
        .values()
        .filter_map(|e| {
            let (a, b) = model.points(e);
            let (a, b) = if crossing {
                camera.segment(a, b, size)?
            } else {
                (camera.project(a, size)?, camera.project(b, size)?)
            };
            (if crossing {
                intersects_rect(a, b, lo, hi)
            } else {
                inside(a) && inside(b)
            })
            .then_some(e.id)
        })
        .collect()
}
