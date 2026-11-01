use super::*;

fn projected_grid(camera: &Camera, size: DVec2) -> Vec<grid::GridLine> {
    grid::visible_lines(camera, &ketchup_core::DisplaySettings::default(), size)
}

#[test]
fn finite_grid_bounds_remain_centered_on_world_origin_during_navigation() {
    let size = DVec2::new(2400., 1800.);
    let display = ketchup_core::DisplaySettings {
        grid_infinite: false,
        grid_max_length: 8.,
        grid_auto_size: false,
        grid_spacing: 0.5,
        ..Default::default()
    };
    for projection in [
        ketchup_core::Projection::Perspective,
        ketchup_core::Projection::Parallel,
    ] {
        for distance in [30., 60.] {
            for yaw in [0., std::f64::consts::FRAC_PI_4, -0.9] {
                let camera = Camera {
                    distance,
                    yaw,
                    projection,
                    target: DVec3::new(1., -1., 0.),
                    ..Default::default()
                };
                let lines = grid::visible_lines(&camera, &display, size);
                assert!(!lines.is_empty());
                let mut lo = DVec2::splat(f64::INFINITY);
                let mut hi = DVec2::splat(f64::NEG_INFINITY);
                for line in lines {
                    assert_eq!(line.opacity, [1., 1.]);
                    for pixel in line.points {
                        let point = camera.ray(pixel, size).plane_z(0.).unwrap().truncate();
                        assert!(point.abs().max_element() <= 4. + 1e-6);
                        lo = lo.min(point);
                        hi = hi.max(point);
                    }
                }
                assert!(lo.distance(DVec2::splat(-4.)) < 1e-6);
                assert!(hi.distance(DVec2::splat(4.)) < 1e-6);
            }
        }
    }
}

#[test]
fn rendered_grid_fades_continuously_over_the_whole_origin_to_horizon_interval() {
    let size = DVec2::new(1200., 900.);
    for yaw in [
        0.,
        std::f64::consts::FRAC_PI_4,
        -std::f64::consts::FRAC_PI_4,
    ] {
        let camera = Camera {
            yaw,
            pitch: 0.3,
            ..Default::default()
        };
        let lines = projected_grid(&camera, size);
        assert!(lines.len() < 100000);
        let origin_y = camera.project(DVec3::ZERO, size).unwrap().y;
        let focal = size.y / (2. * (camera.fov * 0.5).tan());
        let horizon_y = size.y * 0.5 - focal * camera.pitch.tan();
        let mut previous = 1_f32;
        // Sample every percent of the visible fade, including its opaque end.
        // Only the final two percent are allowed to fall below density/contrast
        // visibility; the analytic fade itself reaches zero exactly at infinity.
        for sample in 0..=98 {
            let progress = sample as f64 / 100.;
            let y = origin_y + (horizon_y - origin_y) * progress;
            let expected = (1. - progress) as f32;
            let mut brightest = 0_f32;
            for line in &lines {
                let [a, b] = line.points;
                if (b.y - a.y).abs() < 1e-6 {
                    continue;
                }
                let t = (y - a.y) / (b.y - a.y);
                if (-1e-7..=1.0000001).contains(&t) {
                    let alpha = line.opacity[0] + (line.opacity[1] - line.opacity[0]) * t as f32;
                    assert!(alpha <= expected + 1e-5);
                    brightest = brightest.max(alpha);
                }
            }
            assert!(
                (brightest - expected).abs() < 0.005,
                "Fade disappeared early: yaw={yaw}, progress={progress}, alpha={brightest}, expected={expected}"
            );
            assert!(brightest <= previous + 1e-5);
            assert!(previous - brightest < 0.015);
            previous = brightest;
        }
        assert!(
            lines
                .iter()
                .all(|line| line.points.iter().all(|p| p.y > horizon_y))
        );
    }
}

#[test]
fn horizon_fade_is_uniform_across_the_viewport_and_during_rotation() {
    let size = DVec2::new(1200., 900.);
    let mut references = std::collections::BTreeMap::<usize, f32>::new();
    for yaw in [
        0.,
        std::f64::consts::FRAC_PI_8,
        std::f64::consts::FRAC_PI_4,
        std::f64::consts::FRAC_PI_2,
        -std::f64::consts::FRAC_PI_4,
    ] {
        let camera = Camera {
            yaw,
            pitch: 0.3,
            ..Default::default()
        };
        let eye = camera.eye();
        let forward = (camera.target - eye).normalize();
        // A horizontal screen row in the middle of the distant fade. Rotate
        // the lattice under it; neither family may extend further at the sides.
        let depth = 100.;
        let along_ground = DVec3::new(forward.x, forward.y, 0.).normalize();
        let point = eye - DVec3::Z * eye.z
            + along_ground * ((depth - eye.z * camera.pitch.sin()) / camera.pitch.cos());
        let y = camera.project(point, size).unwrap().y;
        let mut samples = Vec::new();
        for line in projected_grid(&camera, size).iter().filter(|l| l.major) {
            let [a, b] = line.points;
            if (b.y - a.y).abs() < 1e-6 {
                continue;
            }
            let t = (y - a.y) / (b.y - a.y);
            if (0. ..=1.).contains(&t) {
                samples.push((
                    line.level,
                    line.opacity[0] + (line.opacity[1] - line.opacity[0]) * t as f32,
                ));
            }
        }
        assert!(samples.len() > 5);
        for (level, alpha) in samples {
            assert!(
                alpha > 0.05 && alpha < 0.95,
                "Expected visible fade: {alpha}"
            );
            let expected = *references.entry(level).or_insert(alpha);
            assert!(
                (alpha - expected).abs() < 0.005,
                "Direction-dependent horizon fade at yaw {yaw}: {alpha} versus {expected}"
            );
        }
    }
}

#[test]
fn grid_lattice_and_major_indices_stay_anchored_during_orbit_pan_and_zoom() {
    let size = DVec2::new(1200., 900.);
    for projection in [
        ketchup_core::Projection::Perspective,
        ketchup_core::Projection::Parallel,
    ] {
        let mut camera = Camera {
            projection,
            ..Default::default()
        };
        for delta in [DVec2::ZERO, DVec2::new(20., 10.), DVec2::new(-40., -20.)] {
            camera.orbit_around(delta, DVec3::new(1., 2., 0.));
            camera.pan(DVec2::new(10., -5.), size.y);
            let spacing = grid::spacing(&camera);
            let lines = projected_grid(&camera, size);
            for index in [-5, -1, 0, 1, 5] {
                let p = DVec3::new(index as f64 * spacing, 0.37 * spacing, 0.);
                let screen = camera.project(p, size).unwrap();
                let direction = camera.project(p + DVec3::Y * spacing, size).unwrap() - screen;
                assert!(
                    lines.iter().any(|line| {
                        let [a, b] = line.points;
                        let d = b - a;
                        line.major == (index % 5 == 0)
                            && d.perp_dot(direction).abs() < 1e-5 * d.length() * direction.length()
                            && d.perp_dot(screen - a).abs() / d.length() < 1e-5
                            && (screen - a).dot(d) >= -1e-5
                            && (screen - b).dot(d) <= 1e-5
                    }),
                    "Missing world lattice line {index}"
                );
            }
        }
        let before = grid::spacing(&camera);
        camera.zoom(800., size * 0.5, size);
        assert!(grid::spacing(&camera) < before);
    }
}

#[test]
fn grid_covers_wide_viewport_without_a_world_rectangle() {
    let mut camera = Camera::default();
    camera.set_standard_view(ketchup_core::StandardView::Top);
    let size = DVec2::new(3000., 600.);
    let lines = projected_grid(&camera, size);
    assert!(!lines.is_empty());
    // Every top-view line crosses opposite viewport edges, even beyond the
    // old 35-spacing square and after a large world-space pan.
    for target in [DVec3::ZERO, DVec3::new(10000.3, -9000.7, 0.)] {
        camera.target = target;
        for line in projected_grid(&camera, size) {
            for p in line.points {
                let edge =
                    p.x.abs()
                        .min((p.x - size.x).abs())
                        .min(p.y.abs())
                        .min((p.y - size.y).abs());
                assert!(edge < 1e-5, "Interior grid endpoint: {p:?}");
            }
        }
    }
}

#[test]
fn grid_projection_is_finite_bounded_and_fades_near_the_horizon() {
    let size = DVec2::new(1200., 900.);
    for projection in [
        ketchup_core::Projection::Perspective,
        ketchup_core::Projection::Parallel,
    ] {
        for pitch in [0., 0.000001, 0.05, 0.62, -0.62, std::f64::consts::FRAC_PI_2] {
            let camera = Camera {
                projection,
                pitch,
                ..Default::default()
            };
            let lines = projected_grid(&camera, size);
            assert!(lines.len() < 100000);
            for line in &lines {
                for p in line.points {
                    assert!(p.is_finite());
                    assert!(p.x >= -1e-4 && p.x <= size.x + 1e-4);
                    assert!(p.y >= -1e-4 && p.y <= size.y + 1e-4);
                }
                assert!(line.opacity.iter().all(|a| (0. ..=1.).contains(a)));
                assert_ne!(line.opacity, [0., 0.]);
            }
            if projection == ketchup_core::Projection::Perspective && pitch == 0.05 {
                assert!(
                    lines
                        .iter()
                        .any(|l| l.opacity.iter().any(|&a| a > 0. && a < 1.))
                );
            }
        }
    }
}

#[test]
fn fixed_subpixel_grid_is_culled_without_changing_snap_spacing() {
    let camera = Camera {
        distance: 1e6,
        ..Default::default()
    };
    let display = ketchup_core::DisplaySettings {
        grid_auto_size: false,
        grid_spacing: 0.000001,
        ..Default::default()
    };
    assert_eq!(grid::configured_spacing(&camera, &display), 0.000001);
    assert!(grid::visible_lines(&camera, &display, DVec2::new(1200., 900.)).is_empty());
    assert!(projected_grid(&camera, DVec2::ZERO).is_empty());
}

fn hover(h: &mut Harness, point: DVec3) {
    h.frame(
        vec![egui::Event::PointerMoved(h.at(point))],
        egui::Modifiers::NONE,
    );
}

#[test]
fn grid_snapping_menu_defaults_off_and_toggles_independently() {
    let mut h = Harness::new();
    assert!(!h.app.snap_to_grid);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, "▾"));
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let checkbox = text_position(&output, "Snap to grid");
    h.click(checkbox);
    assert!(h.app.snap_to_grid);
    assert!(h.app.doc.snap_to_geometry);
    assert!(h.app.doc.snap_to_axes);
    assert!(!h.app.project.modified);
    h.click(checkbox);
    assert!(!h.app.snap_to_grid);
}

#[test]
fn grid_snapping_line_preview_and_commit_use_the_displayed_spacing() {
    for distance in [6., 30., 300.] {
        let mut h = Harness::new();
        h.app.doc.camera.distance = distance;
        h.app.doc.snap_to_geometry = false;
        h.app.doc.snap_to_axes = false;
        h.app.set_tool(Tool::Line);
        let spacing = grid::spacing(&h.app.doc.camera);
        let target = DVec3::new(2.3, -1.7, 0.) * spacing;
        hover(&mut h, target);
        assert!(h.app.snap.unwrap().point.distance(target) < spacing * 1e-4);
        h.app.snap_to_grid = true;
        hover(&mut h, target);
        let expected = DVec3::new(2., -2., 0.) * spacing;
        assert!(h.app.snap.unwrap().point.distance(expected) < 1e-7);
        h.click(h.at(target));
        assert!(h.app.line_start.unwrap().distance(expected) < 1e-7);
        let end = DVec3::new(-2.2, 1.8, 0.) * spacing;
        hover(&mut h, end);
        let preview = h.app.line_preview_snap().unwrap().point;
        assert!(preview.distance(DVec3::new(-2., 2., 0.) * spacing) < 1e-7);
        h.click(h.at(end));
        let edge = h.app.doc.model.edges.values().next().unwrap();
        let (a, b) = h.app.doc.model.points(edge);
        assert!(a.distance(expected) < 1e-7);
        assert!(b.distance(preview) < 1e-7);
    }
}

#[test]
fn grid_snapping_preserves_object_targets_and_off_grid_axis_anchors() {
    let mut h = Harness::new();
    h.app.snap_to_grid = true;
    h.app.set_tool(Tool::Line);
    let target = DVec3::new(2.3, 1.7, 0.);
    h.app
        .doc
        .model
        .add_edge(target, target + DVec3::X * 2.)
        .unwrap();
    hover(&mut h, target);
    assert_eq!(h.app.snap.unwrap().kind, Inference::Endpoint);
    assert!(h.app.snap.unwrap().point.distance(target) < 1e-7);
    h.click(h.at(target));
    h.app.doc.snap_to_geometry = false;
    h.key(Key::ArrowRight);
    hover(&mut h, target + DVec3::X * 3.1);
    let point = h.app.snap.unwrap().point;
    assert!((point - target).cross(DVec3::X).length() < 1e-7);
    assert!((point.x - 5.).abs() < 1e-7);
}

#[test]
fn grid_snapping_shapes_stay_on_each_locked_plane() {
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        let mut h = Harness::new();
        h.app.doc.snap_to_geometry = false;
        h.app.doc.snap_to_axes = false;
        h.app.snap_to_grid = true;
        h.app.set_tool(Tool::Rectangle);
        h.click(h.at(DVec3::ZERO));
        h.app.set_axis_lock(Some(axis), &h.ctx);
        let drawing = h.app.drawing.as_ref().unwrap();
        let (origin, u, v) = (drawing.origin, drawing.u, drawing.v);
        hover(&mut h, origin + u * 2.3 + v * 1.7);
        let point = h.app.snap.unwrap().point;
        assert!(point.distance(origin + u * 2. + v * 2.) < 1e-7);
        assert!((point - origin).dot(axis.vector()).abs() < 1e-7);
    }
}

#[test]
fn grid_snapping_move_and_measurement_use_grid_points() {
    let mut h = Harness::new();
    h.app.doc.snap_to_geometry = false;
    h.app.doc.snap_to_axes = false;
    h.app.snap_to_grid = true;
    let edge = h.app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    h.app.selected.insert(edge);
    h.app.set_tool(Tool::Move);
    h.app.begin_modeling(h.at(DVec3::ZERO), &h.ctx);
    hover(&mut h, DVec3::new(2.3, -1.7, 0.));
    let edit = h.app.modeling.as_ref().unwrap();
    assert!(edit.direction.distance(DVec3::new(2., -2., 0.)) < 1e-7);
    h.key(Key::Escape);
    h.app.set_tool(Tool::TapeMeasure);
    hover(&mut h, DVec3::new(2.3, -1.7, 0.));
    assert!(h.app.snap.unwrap().point.distance(DVec3::new(2., -2., 0.)) < 1e-7);
}

#[test]
fn edge_and_face_snapping_menu_toggles_are_independent_preferences() {
    let mut h = Harness::new();
    assert!(h.app.snap_to_edge);
    assert!(h.app.snap_to_face);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, "▾"));
    for label in ["Snap to edge", "Snap to face"] {
        let output = h.frame(vec![], egui::Modifiers::NONE);
        let checkbox = text_position(&output, label);
        h.click(checkbox);
        assert_eq!(h.app.snap_to_edge, label != "Snap to edge");
        assert_eq!(h.app.snap_to_face, label != "Snap to face");
        assert!(h.app.doc.snap_to_geometry);
        assert!(h.app.doc.snap_to_axes);
        assert!(!h.app.project.modified);
        h.click(checkbox);
        assert!(h.app.snap_to_edge);
        assert!(h.app.snap_to_face);
    }
}

#[test]
fn drawing_uses_edge_and_face_preferences_with_cached_picking() {
    let mut h = Harness::new();
    h.app.set_tool(Tool::Line);
    h.app.doc.snap_to_axes = false;
    h.app
        .doc
        .model
        .add_edge(DVec3::new(0., 0., 2.), DVec3::new(6., 0., 2.))
        .unwrap();
    let edge_target = DVec3::new(1.5, 0., 2.);
    hover(&mut h, edge_target);
    assert_eq!(h.app.snap.unwrap().kind, Inference::OnEdge);
    h.app.snap_to_edge = false;
    hover(&mut h, edge_target);
    assert_eq!(h.app.snap.unwrap().kind, Inference::Plane);
    for (a, b) in [
        (DVec3::new(-3., -3., 4.), DVec3::new(3., -3., 4.)),
        (DVec3::new(3., -3., 4.), DVec3::new(3., 3., 4.)),
        (DVec3::new(3., 3., 4.), DVec3::new(-3., 3., 4.)),
        (DVec3::new(-3., 3., 4.), DVec3::new(-3., -3., 4.)),
    ] {
        h.app.doc.model.add_edge(a, b).unwrap();
    }
    let face_target = DVec3::new(0.5, 0.5, 4.);
    hover(&mut h, face_target);
    assert_eq!(h.app.snap.unwrap().kind, Inference::OnFace);
    h.app.snap_to_face = false;
    hover(&mut h, face_target);
    assert_eq!(h.app.snap.unwrap().kind, Inference::Plane);
}
