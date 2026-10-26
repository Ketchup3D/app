use glam::{DMat4, DQuat, DVec2, DVec3, DVec4};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Camera {
    pub target: DVec3,
    pub distance: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub projection: Projection,
    /// Vertical field of view in radians; also sets the parallel view scale.
    pub fov: f64,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            target: DVec3::ZERO,
            distance: 18.0,
            yaw: -0.9,
            pitch: 0.62,
            projection: Projection::default(),
            fov: Self::FOV,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    #[default]
    Perspective,
    Parallel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandardView {
    Top,
    Front,
    Right,
    Back,
    Left,
    Bottom,
    Iso,
}

impl StandardView {
    pub const ALL: [Self; 7] = [
        Self::Top,
        Self::Front,
        Self::Right,
        Self::Back,
        Self::Left,
        Self::Bottom,
        Self::Iso,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Top => "Top",
            Self::Front => "Front",
            Self::Right => "Right",
            Self::Back => "Back",
            Self::Left => "Left",
            Self::Bottom => "Bottom",
            Self::Iso => "Iso",
        }
    }

    pub fn angles(self) -> (f64, f64) {
        use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
        match self {
            Self::Top => (-FRAC_PI_2, FRAC_PI_2),
            Self::Front => (-FRAC_PI_2, 0.),
            Self::Right => (0., 0.),
            Self::Back => (FRAC_PI_2, 0.),
            Self::Left => (PI, 0.),
            Self::Bottom => (-FRAC_PI_2, -FRAC_PI_2),
            Self::Iso => (-FRAC_PI_4, (1. / 2_f64.sqrt()).atan()),
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
            && self.pitch.abs() <= std::f64::consts::FRAC_PI_2
            && self.fov.is_finite()
            && (15_f64.to_radians()..=100_f64.to_radians()).contains(&self.fov)
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
    /// An analytic basis stays well-defined at exact top and bottom views.
    pub fn right(&self) -> DVec3 {
        DVec3::new(-self.yaw.sin(), self.yaw.cos(), 0.)
    }
    pub fn up(&self) -> DVec3 {
        self.right().cross((self.target - self.eye()).normalize())
    }
    pub fn view_distance(&self, point: DVec3) -> f64 {
        match self.projection {
            Projection::Perspective => point.distance(self.eye()),
            Projection::Parallel => {
                (point - self.eye()).dot((self.target - self.eye()).normalize())
            }
        }
    }
    pub fn toward_eye(&self, point: DVec3) -> DVec3 {
        match self.projection {
            Projection::Perspective => self.eye() - point,
            Projection::Parallel => self.eye() - self.target,
        }
    }
    pub fn set_standard_view(&mut self, view: StandardView) {
        (self.yaw, self.pitch) = view.angles();
    }
    pub fn standard_view(&self) -> Option<StandardView> {
        StandardView::ALL.into_iter().find(|view| {
            let (yaw, pitch) = view.angles();
            let yaw_delta = self.yaw - yaw;
            yaw_delta.sin().abs() < 1e-8
                && yaw_delta.cos() > 0.
                && (self.pitch - pitch).abs() < 1e-8
        })
    }
    pub fn matrix(&self, size: DVec2) -> DMat4 {
        let aspect = (size.x / size.y.max(1.0)).max(0.01);
        let projection = match self.projection {
            Projection::Perspective => DMat4::perspective_rh(self.fov, aspect, 0.005, 1e8),
            Projection::Parallel => {
                let half_height = self.distance * (self.fov * 0.5).tan();
                let half_width = half_height * aspect;
                DMat4::orthographic_rh(
                    -half_width,
                    half_width,
                    -half_height,
                    half_height,
                    0.005,
                    1e8,
                )
            }
        };
        projection * DMat4::look_at_rh(self.eye(), self.target, self.up())
    }
    /// Prepare projection once for a render pass or pointer query.
    pub fn view(&self, size: DVec2) -> CameraView {
        CameraView {
            matrix: self.matrix(size),
            inverse: std::cell::OnceCell::new(),
            eye: self.eye(),
            parallel_direction: (self.projection == Projection::Parallel)
                .then(|| (self.target - self.eye()).normalize()),
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
        self.pitch = (self.pitch + delta.y * 0.007)
            .clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
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
        let right = self.right();
        let up = self.up();
        let scale = 2.0 * self.distance * (self.fov * 0.5).tan() / height.max(1.0);
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
                (self.fov * 0.5).min(((self.fov * 0.5).tan() * size.x / size.y.max(1.)).atan());
            self.distance = ((hi - lo).length() * 0.6 / angle.sin()).clamp(1.0, 1e6);
        } else {
            self.target = DVec3::ZERO;
            self.distance = Self::default().distance;
        }
    }
}

/// Viewport-dependent math shared by all geometry in one pass. This is separate
/// from Camera so direct camera edits can never leave a stale cached matrix.
pub struct CameraView {
    matrix: DMat4,
    inverse: std::cell::OnceCell<DMat4>,
    eye: DVec3,
    parallel_direction: Option<DVec3>,
    size: DVec2,
}
impl CameraView {
    pub fn eye(&self) -> DVec3 {
        self.eye
    }
    pub fn view_distance(&self, point: DVec3) -> f64 {
        self.parallel_direction.map_or_else(
            || point.distance(self.eye),
            |forward| (point - self.eye).dot(forward),
        )
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
        if self.parallel_direction.is_some() && ndc.z < 0. {
            return None;
        }
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
            * DVec4::new(
                ndc.x,
                ndc.y,
                if self.parallel_direction.is_some() {
                    0.
                } else {
                    0.5
                },
                1.0,
            );
        let world = world.truncate() / world.w;
        if let Some(direction) = self.parallel_direction {
            Ray {
                origin: world,
                direction,
            }
        } else {
            Ray {
                origin: self.eye,
                direction: (world - self.eye).normalize(),
            }
        }
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;

    #[test]
    fn older_cameras_default_to_perspective_and_new_options_round_trip() {
        let old: Camera = serde_json::from_str(
            r#"{"target":[1.0,2.0,3.0],"distance":20.0,"yaw":0.2,"pitch":0.4}"#,
        )
        .unwrap();
        assert_eq!(old.projection, Projection::Perspective);
        assert_eq!(old.fov, Camera::FOV);
        assert!(old.valid());
        let camera = Camera {
            projection: Projection::Parallel,
            fov: 60_f64.to_radians(),
            ..old
        };
        assert_eq!(
            serde_json::from_str::<Camera>(&serde_json::to_string(&camera).unwrap()).unwrap(),
            camera
        );
        assert!(
            !Camera {
                fov: f64::NAN,
                ..camera.clone()
            }
            .valid()
        );
        assert!(!Camera { fov: 0., ..camera }.valid());
    }

    #[test]
    fn every_standard_view_has_a_valid_basis_and_project_ray_round_trip() {
        let size = DVec2::new(1000., 800.);
        for projection in [Projection::Perspective, Projection::Parallel] {
            for standard in StandardView::ALL {
                let mut camera = Camera {
                    projection,
                    ..Camera::default()
                };
                camera.set_standard_view(standard);
                assert!(camera.valid());
                assert_eq!(camera.standard_view(), Some(standard));
                assert!((camera.up().length() - 1.).abs() < 1e-12);
                let point = camera.target + camera.right() * 2. + camera.up();
                let pixel = camera.project(point, size).unwrap();
                let ray = camera.ray(pixel, size);
                assert!((point - ray.origin).cross(ray.direction).length() < 1e-6);
                assert!(camera.matrix(size).is_finite());
                camera.pan(DVec2::new(30., 20.), size.y);
                assert!(camera.valid());
                assert!(camera.matrix(size).is_finite());
            }
        }
        let mut camera = Camera::default();
        camera.set_standard_view(StandardView::Iso);
        let direction = (camera.eye() - camera.target).normalize().abs();
        assert!((direction.x - direction.y).abs() < 1e-12);
        assert!((direction.y - direction.z).abs() < 1e-12);
        camera.yaw += std::f64::consts::TAU;
        assert_eq!(camera.standard_view(), Some(StandardView::Iso));
        camera.orbit(DVec2::new(10., 0.));
        assert_eq!(camera.standard_view(), None);
    }

    #[test]
    fn parallel_projection_keeps_scale_and_pointer_zoom_anchor_at_all_depths() {
        let size = DVec2::new(1000., 800.);
        let mut camera = Camera {
            projection: Projection::Parallel,
            ..Camera::default()
        };
        camera.set_standard_view(StandardView::Top);
        let forward = (camera.target - camera.eye()).normalize();
        let a = DVec3::new(-2., 1., 0.);
        let b = DVec3::new(2., 1., 0.);
        let near = camera
            .project(a, size)
            .unwrap()
            .distance(camera.project(b, size).unwrap());
        let far = camera
            .project(a + forward * 10., size)
            .unwrap()
            .distance(camera.project(b + forward * 10., size).unwrap());
        assert!((near - far).abs() < 1e-8);
        let cursor = DVec2::new(730., 520.);
        let anchor = camera.ray(cursor, size).plane_z(0.).unwrap();
        let center_ray = camera.ray(size * 0.5, size);
        let cursor_ray = camera.ray(cursor, size);
        assert!(center_ray.direction.distance(cursor_ray.direction) < 1e-12);
        assert!(center_ray.origin.distance(cursor_ray.origin) > 1.);
        camera.zoom(200., cursor, size);
        assert!(camera.project(anchor, size).unwrap().distance(cursor) < 1e-6);
        assert!(camera.project(camera.eye() - forward, size).is_none());
        camera.fit(None, size);
        assert_eq!(camera.projection, Projection::Parallel);
        assert_eq!(camera.standard_view(), Some(StandardView::Top));
    }

    #[test]
    fn parallel_visibility_uses_each_pixels_ray_origin() {
        let mut camera = Camera {
            projection: Projection::Parallel,
            distance: 10.,
            ..Camera::default()
        };
        camera.set_standard_view(StandardView::Top);
        let mut model = crate::Model::default();
        let corners = [
            DVec3::new(-5., -5., 0.),
            DVec3::new(5., -5., 0.),
            DVec3::new(5., 5., 0.),
            DVec3::new(-5., 5., 0.),
        ];
        for i in 0..4 {
            model.add_edge(corners[i], corners[(i + 1) % 4]).unwrap();
        }
        assert_eq!(model.faces.len(), 1);
        let size = DVec2::new(1000., 800.);
        // An off-center point on or above the surface must not be hidden
        // merely because its radial distance to the eye exceeds its ray depth.
        assert!(crate::selection::visible(
            &model,
            &camera,
            size,
            DVec3::new(4., 0., 0.)
        ));
        assert!(crate::selection::visible(
            &model,
            &camera,
            size,
            DVec3::new(4., 0., 0.2)
        ));
        assert!(!crate::selection::visible(
            &model,
            &camera,
            size,
            DVec3::new(4., 0., -0.2)
        ));
    }

    #[test]
    fn triangle_projection_preserves_visible_geometry_and_clips_boundaries() {
        let size = DVec2::new(1000., 800.);
        let view = CameraView {
            matrix: DMat4::IDENTITY,
            inverse: std::cell::OnceCell::new(),
            eye: DVec3::ZERO,
            parallel_direction: None,
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
