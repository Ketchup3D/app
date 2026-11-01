//! Grid rendering and snapping share the same zoom-dependent spacing.
use super::*;

pub(super) fn spacing(camera: &Camera) -> f64 {
    10.0_f64.powf((camera.distance / 15.).log10().floor())
}

pub(super) fn configured_spacing(camera: &Camera, display: &ketchup_core::DisplaySettings) -> f64 {
    if display.grid_auto_size {
        spacing(camera)
    } else {
        display.grid_spacing.clamp(0.000001, 1e6)
    }
}

/// Screen-space endpoints let the renderer interpolate the perceptual fade.
/// All lengths here are logical pixels, consistently on native and web.
pub(super) struct GridLine {
    pub points: [DVec2; 2],
    pub opacity: [f32; 2],
    pub major: bool,
    #[cfg(test)]
    pub level: usize,
}

fn clip_polygon(polygon: &[DVec3], distance: impl Fn(DVec3) -> f64) -> Vec<DVec3> {
    let mut result = Vec::with_capacity(polygon.len() + 1);
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        let da = distance(a);
        let db = distance(b);
        if da >= 0. {
            result.push(a);
        }
        if (da < 0.) != (db < 0.) {
            result.push(a + (b - a) * (da / (da - db)));
        }
    }
    result
}

fn opacity(separation: f64) -> f32 {
    // Below this spacing even the major lines merge into a distracting moire.
    let t = ((separation - 0.8) / 2.2).clamp(0., 1.);
    (t * t * (3. - 2. * t)) as f32
}

/// The remaining contrast must exceed an 8-bit alpha step to merit geometry.
const MIN_OPACITY: f32 = 1. / 255.;

fn distance_opacity(depth: f64, origin_depth: f64) -> f32 {
    // On a perspective ground plane, distance from a projected point to the
    // horizon is proportional to 1 / view depth. This is therefore a LINEAR
    // screen-space fade: 1 at the origin, 0 at the actual infinite horizon.
    // No finite world-space fog distance defines where the fade should end.
    (origin_depth / depth).clamp(0., 1.) as f32
}

pub(super) fn visible_lines(
    camera: &Camera,
    display: &ketchup_core::DisplaySettings,
    size: DVec2,
) -> Vec<GridLine> {
    if !size.is_finite() || size.min_element() <= 0. {
        return Vec::new();
    }
    let spacing = configured_spacing(camera, display);
    let perspective = camera.projection == ketchup_core::Projection::Perspective;
    let eye = camera.eye();
    let forward = (camera.target - eye).normalize();
    let world_origin_depth = (-eye).dot(forward);
    // Keep navigation useful when the world origin is behind the camera.
    let origin_depth = if world_origin_depth > 0.005 {
        world_origin_depth
    } else {
        camera.distance
    };
    let focal = size.y / (2. * (camera.fov * 0.5).tan());
    let slope = (camera.fov * 0.5).tan() * (size.x / size.y).max(1.);
    let infinite = display.grid_infinite;
    let half_length = display.grid_max_length.clamp(0.000001, 1e6) * 0.5;
    let far = if infinite {
        origin_depth / MIN_OPACITY as f64
    } else {
        // A conservative camera-space bound, never a camera-dependent world edge.
        eye.length() + half_length * 2_f64.sqrt() + 1.
    };
    let radius = if infinite {
        far * (1. + 2. * slope) + camera.distance * (1. + slope)
    } else {
        half_length
    };
    let center = if infinite {
        eye.truncate()
    } else {
        DVec2::ZERO
    };
    let fog = |depth| {
        if infinite {
            distance_opacity(depth, origin_depth)
        } else {
            1.
        }
    };
    let mut polygon: Vec<_> = [
        DVec2::new(-1., -1.),
        DVec2::new(1., -1.),
        DVec2::new(1., 1.),
        DVec2::new(-1., 1.),
    ]
    .map(|p| (center + p * radius).extend(0.))
    .to_vec();
    let matrix = camera.matrix(size);
    // Clip the plane itself, including the horizon and the near plane, before
    // enumerating lattice indices. No target-centered rectangle is drawn.
    for plane in [
        glam::DVec4::new(1., 0., 0., 1.),
        glam::DVec4::new(-1., 0., 0., 1.),
        glam::DVec4::new(0., 1., 0., 1.),
        glam::DVec4::new(0., -1., 0., 1.),
        glam::DVec4::new(0., 0., 1., 0.),
    ] {
        polygon = clip_polygon(&polygon, |p| plane.dot(matrix * p.extend(1.)));
    }
    let depth = |p: DVec3| (p - eye).dot(forward);
    polygon = clip_polygon(&polygon, |p| far - depth(p));
    if polygon.len() < 3 {
        return Vec::new();
    }
    let view = camera.view(size);
    let mut result = Vec::new();
    // Use one conservative spacing bound for BOTH lattice directions. Measuring
    // each family separately creates lobes toward its vanishing point (most
    // noticeable at 45 degrees). Ground-plane compression depends on view depth,
    // not world yaw or horizontal screen position. The perspective margin covers
    // the extra shear at the viewport edges, keeping the common cutoff alias-free.
    let separation = |depth: f64, step: f64| {
        if perspective {
            focal * step / depth * (eye.z.abs() / depth).min(1.) / (1. + slope)
        } else {
            focal * step / camera.distance * camera.pitch.sin().abs()
        }
    };
    let mut steps = vec![spacing, spacing * 5.];
    // Preserve an intentionally subpixel fixed grid when zoomed out. Only add
    // distant levels when its existing major lattice is visible at model depth.
    if perspective && separation(camera.distance, spacing * 5.) > 0.8 {
        while separation(far, *steps.last().unwrap()) < 3. && steps.len() < 32 {
            steps.push(steps.last().unwrap() * 2.);
        }
    }
    let last_step = *steps.last().unwrap();
    let mut near = polygon
        .iter()
        .map(|&p| depth(p))
        .fold(f64::INFINITY, f64::min)
        .max(0.005);
    let visible_depth = polygon.iter().map(|&p| depth(p)).fold(near, f64::max);
    while near < far {
        // Both factors decrease with depth, so all subsequent bands are invisible.
        if opacity(separation(near, last_step)) * fog(near) < MIN_OPACITY {
            break;
        }
        // Short depth bands keep screen-space alpha interpolation consistent
        // when a segment is clipped by a side of the viewport.
        let mut end = (near * 1.05).min(far);
        // Anchor the opaque end exactly, including when a band straddles it.
        if near < origin_depth && end > origin_depth {
            end = origin_depth;
        }
        let band = clip_polygon(&clip_polygon(&polygon, |p| depth(p) - near), |p| {
            end - depth(p)
        });
        if band.len() >= 3 {
            for axis in 0..2 {
                for (level, &step) in steps.iter().enumerate() {
                    let major = level > 0;
                    if separation(near, step) <= 0.8 {
                        continue;
                    }
                    let lo = band.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
                    let hi = band
                        .iter()
                        .map(|p| p[axis])
                        .fold(f64::NEG_INFINITY, f64::max);
                    let first = (lo / step).ceil() as i64;
                    let last = (hi / step).floor() as i64;
                    // Degenerate, nearly edge-on views must remain bounded.
                    if last.saturating_sub(first) > 16384 {
                        continue;
                    }
                    for index in first..=last {
                        let divisor = if level == 0 { 5 } else { 2 };
                        if level + 1 < steps.len() && index % divisor == 0 {
                            continue;
                        }
                        let coordinate = index as f64 * step;
                        let mut intersections = [DVec3::ZERO; 2];
                        let mut count = 0;
                        for i in 0..band.len() {
                            let a = band[i];
                            let b = band[(i + 1) % band.len()];
                            let da = a[axis] - coordinate;
                            let db = b[axis] - coordinate;
                            if (da <= 0. && db > 0.) || (db <= 0. && da > 0.) {
                                if count < 2 {
                                    intersections[count] = a + (b - a) * (da / (da - db));
                                }
                                count += 1;
                            }
                        }
                        if count != 2 {
                            continue;
                        }
                        let a = intersections[0];
                        let b = intersections[1];
                        let alpha =
                            [a, b].map(|p| opacity(separation(depth(p), step)) * fog(depth(p)));
                        // At most one alpha quantization step: invisible even before
                        // accounting for the palette's already subtle contrast.
                        if alpha.iter().all(|&a| a < MIN_OPACITY) {
                            continue;
                        }
                        if let Some((a, b)) = view.segment(a, b) {
                            if a.distance_squared(b) >= 0.01 {
                                result.push(GridLine {
                                    points: [a, b],
                                    opacity: alpha,
                                    major,
                                    #[cfg(test)]
                                    level,
                                });
                            }
                        }
                    }
                }
            }
        }
        near = end;
        if near >= visible_depth {
            break;
        }
    }
    result
}

pub(super) fn snap(mut snap: Snap, spacing: f64, plane: Option<DVec3>) -> Snap {
    let round = |value: f64| (value / spacing).round() * spacing;
    match snap.kind {
        Inference::Plane => {
            // Round in the working plane, preserving its elevation and tilt.
            // World-aligned planes use the same lattice as the visible grid.
            let normal = plane.unwrap_or(DVec3::Z);
            let u = (DVec3::X - normal * normal.x)
                .try_normalize()
                .unwrap_or(DVec3::Y);
            let v = normal.cross(u);
            let x = snap.point.dot(u);
            let y = snap.point.dot(v);
            snap.point += u * (round(x) - x) + v * (round(y) - y);
        }
        Inference::Axis(axis) | Inference::OnAxis(axis)
            if plane.is_none_or(|normal| normal.dot(axis.vector()).abs() < 1e-7) =>
        {
            // Only the moving coordinate changes, keeping off-grid axis anchors.
            let direction = axis.vector();
            let value = snap.point.dot(direction);
            snap.point += direction * (round(value) - value);
        }
        // Exact object targets and intersections of alignment guides win.
        _ => {}
    }
    snap
}

impl KetchupApp {
    pub(super) fn apply_grid_snap(&mut self) {
        if self.snap_to_grid {
            let plane = self
                .drawing
                .as_ref()
                .map(|drawing| drawing.normal)
                .or_else(|| self.measurement_plane().map(|(_, normal)| normal));
            self.snap = self.snap.map(|point| {
                snap(
                    point,
                    configured_spacing(&self.doc.camera, &self.doc.display),
                    plane,
                )
            });
        }
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn fade_progresses_continuously_from_opaque_origin_to_transparent_horizon() {
        for origin_depth in [0.05, 18., 1e6] {
            assert_eq!(distance_opacity(origin_depth * 0.5, origin_depth), 1.);
            assert_eq!(distance_opacity(origin_depth, origin_depth), 1.);
            let mut previous = 1.;
            for sample in 0..=1000 {
                let progress = sample as f64 / 1000.;
                let depth = origin_depth / (1. - progress);
                let alpha = distance_opacity(depth, origin_depth);
                assert!((alpha - (1. - progress) as f32).abs() < 1e-6);
                assert!(alpha <= previous);
                assert!((previous - alpha) <= 0.001001);
                previous = alpha;
            }
            assert_eq!(previous, 0.);
            assert_eq!(distance_opacity(f64::INFINITY, origin_depth), 0.);
        }
    }

    #[test]
    fn fixed_grid_and_snapping_share_spacing_at_every_zoom() {
        let mut camera = Camera::default();
        let display = ketchup_core::DisplaySettings {
            grid_auto_size: false,
            grid_spacing: 0.25,
            ..Default::default()
        };
        for distance in [0.1, 15., 1000.] {
            camera.distance = distance;
            let spacing = configured_spacing(&camera, &display);
            assert_eq!(spacing, 0.25);
            let point = snap(
                Snap {
                    point: DVec3::new(0.36, 0.64, 0.),
                    kind: Inference::Plane,
                },
                spacing,
                None,
            );
            assert_eq!(point.point, DVec3::new(0.25, 0.75, 0.));
        }
    }
}
