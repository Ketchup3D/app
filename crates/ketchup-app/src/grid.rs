//! Grid rendering and snapping share the same zoom-dependent spacing.
use super::*;

pub(super) fn spacing(camera: &Camera) -> f64 {
    10.0_f64.powf((camera.distance / 15.).log10().floor())
}

pub(super) fn lines(camera: &Camera) -> impl Iterator<Item = (DVec3, DVec3, bool)> {
    let spacing = spacing(camera);
    let x = (camera.target.x / spacing).round() as i64;
    let y = (camera.target.y / spacing).round() as i64;
    // The camera chooses the visible window, but each line's position and
    // emphasis belong to the world lattice, including at negative coordinates.
    (-35..=35).flat_map(move |offset| {
        let xi = x + offset;
        let yi = y + offset;
        [
            (
                DVec3::new(xi as f64 * spacing, (y - 35) as f64 * spacing, 0.),
                DVec3::new(xi as f64 * spacing, (y + 35) as f64 * spacing, 0.),
                xi % 5 == 0,
            ),
            (
                DVec3::new((x - 35) as f64 * spacing, yi as f64 * spacing, 0.),
                DVec3::new((x + 35) as f64 * spacing, yi as f64 * spacing, 0.),
                yi % 5 == 0,
            ),
        ]
    })
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
            self.snap = self
                .snap
                .map(|point| snap(point, spacing(&self.doc.camera), plane));
        }
    }
}
