use super::*;

fn grid_line_styles(camera: &Camera) -> std::collections::BTreeMap<(bool, i64), bool> {
    let spacing = grid::spacing(camera);
    grid::lines(camera)
        .map(|(a, b, major)| {
            assert_eq!(a.z, 0.);
            assert_eq!(b.z, 0.);
            let vertical = a.x == b.x;
            let coordinate = if vertical { a.x } else { a.y };
            ((vertical, (coordinate / spacing).round() as i64), major)
        })
        .collect()
}

#[test]
fn grid_lines_stay_anchored_while_orbiting_and_panning() {
    for projection in [
        ketchup_core::Projection::Perspective,
        ketchup_core::Projection::Parallel,
    ] {
        for distance in [6., 30., 300.] {
            let mut camera = Camera {
                distance,
                projection,
                ..Default::default()
            };
            let spacing = grid::spacing(&camera);
            camera.target = DVec3::new(2.3, -1.7, 4.) * spacing;
            let before = grid_line_styles(&camera);
            // The two axes must use their own absolute line indices.
            for vertical in [false, true] {
                for index in [-5, 0, 5] {
                    assert_eq!(before.get(&(vertical, index)), Some(&true));
                }
                for index in [-4, -1, 1, 4] {
                    assert_eq!(before.get(&(vertical, index)), Some(&false));
                }
            }
            let target = camera.target;
            let pivot = DVec3::new(-3., 2., 1.) * spacing;
            for delta in [DVec2::new(100., 50.), DVec2::new(-200., -100.)] {
                camera.orbit_around(delta, pivot);
                assert!(camera.target.distance(target) > spacing);
                assert_eq!(grid::spacing(&camera), spacing);
                let after = grid_line_styles(&camera);
                let mut shared = 0;
                for (line, major) in &before {
                    if let Some(after_major) = after.get(line) {
                        assert_eq!(after_major, major, "Grid line {line:?} moved during orbit");
                        shared += 1;
                    }
                }
                assert!(shared > 100);
            }
            camera.pan(DVec2::new(80., -50.), 900.);
            let after = grid_line_styles(&camera);
            for (line, major) in &before {
                if let Some(after_major) = after.get(line) {
                    assert_eq!(after_major, major, "Grid line {line:?} moved during pan");
                }
            }
        }
    }
}

#[test]
fn grid_spacing_can_readjust_when_zooming_in() {
    let mut camera = Camera {
        distance: 30.,
        target: DVec3::new(2.3, -1.7, 4.),
        ..Default::default()
    };
    let before = grid::spacing(&camera);
    let size = DVec2::new(1200., 900.);
    camera.zoom(800., size * 0.5, size);
    let after = grid::spacing(&camera);
    assert!(after < before);
    for (a, b, _) in grid::lines(&camera) {
        for point in [a, b] {
            for coordinate in [point.x, point.y] {
                let index = coordinate / after;
                assert!((index - index.round()).abs() < 1e-7);
            }
        }
    }
}
