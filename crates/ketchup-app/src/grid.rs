//! World-anchored grid with zoom-dependent spacing.
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
