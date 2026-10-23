use glam::{DMat4, DQuat, DVec2, DVec3, DVec4};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub target: DVec3,
    pub distance: f64,
    pub yaw: f64,
    pub pitch: f64,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            target: DVec3::ZERO,
            distance: 18.0,
            yaw: -0.9,
            pitch: 0.62,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: DVec3,
    pub direction: DVec3,
}
impl Ray {
    pub fn segment_point(self, a: DVec3, b: DVec3) -> DVec3 {
        let delta = b - a;
        let point = self.axis(a, delta.normalize()).unwrap_or(a);
        a + delta * ((point - a).dot(delta) / delta.length_squared()).clamp(0., 1.)
    }
    pub fn plane_z(self, z: f64) -> Option<DVec3> {
        if self.direction.z.abs() < 1e-8 {
            return None;
        }
        let t = (z - self.origin.z) / self.direction.z;
        (t > 0.0 && t < 1e7).then_some(self.origin + self.direction * t)
    }
    pub fn axis(self, start: DVec3, axis: DVec3) -> Option<DVec3> {
        let b = self.direction.dot(axis);
        let denominator = 1.0 - b * b;
        if denominator < 1e-6 {
            return None;
        }
        let w = start - self.origin;
        let s = (b * self.direction.dot(w) - axis.dot(w)) / denominator;
        Some(start + axis * s)
    }
}
impl Camera {
    pub const FOV: f64 = 45.0_f64.to_radians();
    pub fn valid(&self) -> bool {
        self.target.is_finite()
            && self.target.abs().max_element() <= 1e10
            && self.distance.is_finite()
            && (0.05..=1e6).contains(&self.distance)
            && self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.pitch.abs() <= 1.55
    }
    pub fn eye(&self) -> DVec3 {
        self.target
            + self.distance
                * DVec3::new(
                    self.pitch.cos() * self.yaw.cos(),
                    self.pitch.cos() * self.yaw.sin(),
                    self.pitch.sin(),
                )
    }
    pub fn matrix(&self, size: DVec2) -> DMat4 {
        DMat4::perspective_rh(Self::FOV, (size.x / size.y.max(1.0)).max(0.01), 0.005, 1e8)
            * DMat4::look_at_rh(self.eye(), self.target, DVec3::Z)
    }
    /// Prepare projection once for a render pass or pointer query.
    pub fn view(&self, size: DVec2) -> CameraView {
        CameraView {
            matrix: self.matrix(size),
            inverse: std::cell::OnceCell::new(),
            eye: self.eye(),
            size,
        }
    }
    pub fn project(&self, point: DVec3, size: DVec2) -> Option<DVec2> {
        self.view(size).project(point)
    }
    pub fn project_depth(&self, point: DVec3, size: DVec2) -> Option<DVec3> {
        self.view(size).project_depth(point)
    }
    pub fn segment(&self, a: DVec3, b: DVec3, size: DVec2) -> Option<(DVec2, DVec2)> {
        self.view(size).segment(a, b)
    }
    pub fn segment_depth(&self, a: DVec3, b: DVec3, size: DVec2) -> Option<(DVec3, DVec3)> {
        self.view(size).segment_depth(a, b)
    }
    pub fn triangle_depth(&self, triangle: [DVec3; 3], size: DVec2) -> Vec<[DVec3; 3]> {
        self.view(size).triangle_depth(triangle)
    }
    pub fn ray(&self, point: DVec2, size: DVec2) -> Ray {
        self.view(size).ray(point)
    }
    pub fn orbit(&mut self, delta: DVec2) {
        self.yaw -= delta.x * 0.007;
        self.pitch = (self.pitch + delta.y * 0.007).clamp(-1.50, 1.50);
    }
    pub fn orbit_around(&mut self, delta: DVec2, pivot: DVec3) {
        if delta == DVec2::ZERO {
            return;
        }
        let orientation =
            |yaw: f64, pitch: f64| DQuat::from_rotation_z(yaw) * DQuat::from_rotation_y(-pitch);
        let before = orientation(self.yaw, self.pitch);
        self.orbit(delta);
        let rotation = orientation(self.yaw, self.pitch) * before.conjugate();
        // Rotate the eye and viewing target together around the model center.
        // Keeping the target offset preserves the current framing after a pan.
        self.target = pivot + rotation * (self.target - pivot);
    }
    pub fn pan(&mut self, delta: DVec2, height: f64) {
        let forward = (self.target - self.eye()).normalize();
        let right = forward.cross(DVec3::Z).normalize();
        let up = right.cross(forward).normalize();
        let scale = 2.0 * self.distance * (Self::FOV * 0.5).tan() / height.max(1.0);
        self.target += (-right * delta.x + up * delta.y) * scale;
    }
    pub fn zoom(&mut self, scroll: f64, point: DVec2, size: DVec2) {
        let forward = (self.target - self.eye()).normalize();
        let ray = self.ray(point, size);
        let denominator = ray.direction.dot(forward);
        let anchor =
            ray.origin + ray.direction * ((self.target - ray.origin).dot(forward) / denominator);
        let old = self.distance;
        self.distance = (old * (-scroll * 0.002).exp()).clamp(0.05, 1e6);
        self.target += (anchor - self.target) * (1.0 - self.distance / old);
    }
    pub fn fit(&mut self, bounds: Option<(DVec3, DVec3)>, size: DVec2) {
        if let Some((lo, hi)) = bounds {
            self.target = (lo + hi) * 0.5;
            let angle =
                (Self::FOV * 0.5).min(((Self::FOV * 0.5).tan() * size.x / size.y.max(1.)).atan());
            self.distance = ((hi - lo).length() * 0.6 / angle.sin()).clamp(1.0, 1e6);
        } else {
            *self = Self::default();
        }
    }
}

/// Viewport-dependent math shared by all geometry in one pass. This is separate
/// from Camera so direct camera edits can never leave a stale cached matrix.
pub struct CameraView {
    matrix: DMat4,
    inverse: std::cell::OnceCell<DMat4>,
    eye: DVec3,
    size: DVec2,
}
impl CameraView {
    pub fn eye(&self) -> DVec3 {
        self.eye
    }
    pub fn project(&self, point: DVec3) -> Option<DVec2> {
        self.project_depth(point).map(|p| p.truncate())
    }
    pub fn project_depth(&self, point: DVec3) -> Option<DVec3> {
        let p = self.matrix * point.extend(1.0);
        if p.w < 0.005 {
            return None;
        }
        let ndc = p.truncate() / p.w;
        Some(DVec3::new(
            (ndc.x + 1.0) * self.size.x * 0.5,
            (1.0 - ndc.y) * self.size.y * 0.5,
            ndc.z,
        ))
    }
    pub fn segment(&self, a: DVec3, b: DVec3) -> Option<(DVec2, DVec2)> {
        self.segment_depth(a, b)
            .map(|(a, b)| (a.truncate(), b.truncate()))
    }
    pub fn segment_depth(&self, a: DVec3, b: DVec3) -> Option<(DVec3, DVec3)> {
        let matrix = self.matrix;
        let mut a = matrix * a.extend(1.0);
        let mut b = matrix * b.extend(1.0);
        // Clip in homogeneous space so grid lines crossing the camera remain stable.
        for plane in [
            DVec4::new(0., 0., 1., 0.),
            DVec4::new(1., 0., 0., 1.),
            DVec4::new(-1., 0., 0., 1.),
            DVec4::new(0., 1., 0., 1.),
            DVec4::new(0., -1., 0., 1.),
        ] {
            let da = plane.dot(a);
            let db = plane.dot(b);
            if da < 0.0 && db < 0.0 {
                return None;
            }
            if (da < 0.0) != (db < 0.0) {
                let p = a + (b - a) * (da / (da - db));
                if da < 0.0 {
                    a = p;
                } else {
                    b = p;
                }
            }
        }
        let screen = |p: DVec4| {
            DVec3::new(
                (p.x / p.w + 1.) * self.size.x * 0.5,
                (1. - p.y / p.w) * self.size.y * 0.5,
                p.z / p.w,
            )
        };
        Some((screen(a), screen(b)))
    }
    pub fn triangle_depth(&self, triangle: [DVec3; 3]) -> Vec<[DVec3; 3]> {
        let mut triangles = Vec::new();
        self.for_each_triangle_depth(triangle, |t| triangles.push(t));
        triangles
    }
    /// Project directly into a render mesh without allocating per triangle.
    pub fn for_each_triangle_depth(&self, triangle: [DVec3; 3], mut emit: impl FnMut([DVec3; 3])) {
        const PLANES: [DVec4; 5] = [
            DVec4::new(0., 0., 1., 0.),
            DVec4::new(1., 0., 0., 1.),
            DVec4::new(-1., 0., 0., 1.),
            DVec4::new(0., 1., 0., 1.),
            DVec4::new(0., -1., 0., 1.),
        ];
        // Clipping a convex triangle against five planes adds at most five
        // vertices. Most triangles are fully visible and skip clipping entirely.
        let mut polygon = [DVec4::ZERO; 8];
        let mut outside = 0u8;
        let mut common = 0b11111u8;
        for (p, world) in polygon.iter_mut().zip(triangle) {
            *p = self.matrix * world.extend(1.);
            let mask = u8::from(p.z < 0.)
                | (u8::from(p.x < -p.w) << 1)
                | (u8::from(p.x > p.w) << 2)
                | (u8::from(p.y < -p.w) << 3)
                | (u8::from(p.y > p.w) << 4);
            outside |= mask;
            common &= mask;
        }
        if common != 0 {
            return;
        }
        let screen = |p: DVec4| {
            DVec3::new(
                (p.x / p.w + 1.) * self.size.x * 0.5,
                (1. - p.y / p.w) * self.size.y * 0.5,
                p.z / p.w,
            )
        };
        let mut len: usize = 3;
        for (index, plane) in PLANES.into_iter().enumerate() {
            if outside & (1 << index) == 0 {
                continue;
            }
            let mut clipped = [DVec4::ZERO; 8];
            let mut count = 0;
            for i in 0..len {
                let a = polygon[i];
                let b = polygon[(i + 1) % len];
                let da = plane.dot(a);
                let db = plane.dot(b);
                if da >= 0. {
                    clipped[count] = a;
                    count += 1;
                }
                if (da < 0.) != (db < 0.) {
                    clipped[count] = a + (b - a) * (da / (da - db));
                    count += 1;
                }
            }
            if count < 3 {
                return;
            }
            polygon = clipped;
            len = count;
        }
        let first = screen(polygon[0]);
        let mut previous = screen(polygon[1]);
        for &point in &polygon[2..len] {
            let next = screen(point);
            emit([first, previous, next]);
            previous = next;
        }
    }
    pub fn ray(&self, point: DVec2) -> Ray {
        let ndc = DVec2::new(
            point.x / self.size.x * 2.0 - 1.0,
            1.0 - point.y / self.size.y * 2.0,
        );
        let world = *self.inverse.get_or_init(|| self.matrix.inverse())
            * DVec4::new(ndc.x, ndc.y, 0.5, 1.0);
        let origin = self.eye;
        Ray {
            origin,
            direction: (world.truncate() / world.w - origin).normalize(),
        }
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;

    #[test]
    fn triangle_projection_preserves_visible_geometry_and_clips_boundaries() {
        let size = DVec2::new(1000., 800.);
        let view = CameraView {
            matrix: DMat4::IDENTITY,
            inverse: std::cell::OnceCell::new(),
            eye: DVec3::ZERO,
            size,
        };
        let inside = [
            DVec3::new(-0.5, -0.5, 0.5),
            DVec3::new(0.5, -0.5, 0.5),
            DVec3::new(0., 0.5, 0.5),
        ];
        assert_eq!(
            view.triangle_depth(inside),
            vec![inside.map(|p| view.project_depth(p).unwrap())]
        );
        for outside in [
            inside.map(|p| p + DVec3::X * 3.),
            inside.map(|p| p - DVec3::X * 3.),
            inside.map(|p| p + DVec3::Y * 3.),
            inside.map(|p| p - DVec3::Y * 3.),
            inside.map(|p| p - DVec3::Z),
        ] {
            assert!(view.triangle_depth(outside).is_empty());
        }
        // Crossing any side or the near plane produces a clipped quad.
        for corner in [
            DVec3::new(2., 0.5, 0.5),
            DVec3::new(-2., 0.5, 0.5),
            DVec3::new(0., 2., 0.5),
            DVec3::new(0., -2., 0.5),
            DVec3::new(0., 0.5, -0.5),
        ] {
            let projected = view.triangle_depth([inside[0], inside[1], corner]);
            assert_eq!(projected.len(), 2);
            for p in projected.into_iter().flatten() {
                assert!(p.is_finite());
                assert!((-1e-8..=size.x + 1e-8).contains(&p.x));
                assert!((-1e-8..=size.y + 1e-8).contains(&p.y));
                assert!((0. ..=1.).contains(&p.z));
            }
        }
        // One large triangle covers the entire viewport after multi-plane clipping.
        let projected = view.triangle_depth([
            DVec3::new(-10., -10., 0.5),
            DVec3::new(10., -10., 0.5),
            DVec3::new(0., 10., 0.5),
        ]);
        let area: f64 = projected
            .iter()
            .map(|[a, b, c]| ((*b - *a).truncate().perp_dot((*c - *a).truncate())).abs() * 0.5)
            .sum();
        assert!((area - size.x * size.y).abs() < 1e-6);
    }
}
