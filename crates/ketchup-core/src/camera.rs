use glam::{DMat4, DVec2, DVec3, DVec4};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    pub fn project(&self, point: DVec3, size: DVec2) -> Option<DVec2> {
        let p = self.matrix(size) * point.extend(1.0);
        if p.w < 0.005 {
            return None;
        }
        let ndc = p.truncate() / p.w;
        Some(DVec2::new(
            (ndc.x + 1.0) * size.x * 0.5,
            (1.0 - ndc.y) * size.y * 0.5,
        ))
    }
    pub fn segment(&self, a: DVec3, b: DVec3, size: DVec2) -> Option<(DVec2, DVec2)> {
        let matrix = self.matrix(size);
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
            DVec2::new(
                (p.x / p.w + 1.) * size.x * 0.5,
                (1. - p.y / p.w) * size.y * 0.5,
            )
        };
        Some((screen(a), screen(b)))
    }
    pub fn ray(&self, point: DVec2, size: DVec2) -> Ray {
        let ndc = DVec2::new(point.x / size.x * 2.0 - 1.0, 1.0 - point.y / size.y * 2.0);
        let world = self.matrix(size).inverse() * DVec4::new(ndc.x, ndc.y, 0.5, 1.0);
        let origin = self.eye();
        Ray {
            origin,
            direction: (world.truncate() / world.w - origin).normalize(),
        }
    }
    pub fn orbit(&mut self, delta: DVec2) {
        self.yaw -= delta.x * 0.007;
        self.pitch = (self.pitch + delta.y * 0.007).clamp(-1.50, 1.50);
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
