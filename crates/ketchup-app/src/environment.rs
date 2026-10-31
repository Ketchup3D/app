use crate::inspector_table::{Column, InspectorTable};
use eframe::egui::{self, Color32, Rect};
use ketchup_core::{Camera, DisplaySettings, Projection};

/// Adapt the default environment without changing saved or custom project colors.
pub fn for_theme(display: &DisplaySettings, dark: bool) -> DisplaySettings {
    let mut themed = display.clone();
    if dark {
        let defaults = DisplaySettings::default();
        for (color, light, dark) in [
            (&mut themed.sky_color, defaults.sky_color, [34, 46, 61]),
            (
                &mut themed.ground_color,
                defaults.ground_color,
                [43, 47, 41],
            ),
            (
                &mut themed.horizon_color,
                defaults.horizon_color,
                [62, 72, 76],
            ),
            (&mut themed.background, defaults.background, [28, 34, 30]),
        ] {
            if *color == light {
                *color = dark;
            }
        }
    }
    themed
}

/// A distant backdrop, painted before the depth-tested model. Ground is a
/// color, so orbiting below Z=0 never hides the user's model.
pub fn paint(
    painter: &egui::Painter,
    rect: Rect,
    camera: &Camera,
    display: &DisplaySettings,
    dark: bool,
) {
    painter.add(backdrop(rect, camera, display, dark));
    let horizon = horizon_fraction(camera);
    if (display.sky || display.ground) && (0. ..=1.).contains(&horizon) {
        let [r, g, b] = display.horizon_color;
        painter.hline(
            rect.x_range(),
            rect.top() + rect.height() * horizon as f32,
            egui::Stroke::new(1., Color32::from_rgb(r, g, b)),
        );
    }
}

fn horizon_fraction(camera: &Camera) -> f64 {
    if camera.projection == Projection::Parallel {
        return if camera.pitch < 0. {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        };
    }
    // With no camera roll, the Z component of an unnormalized camera ray is
    // constant across each row. Its zero gives the true world-space horizon.
    (1. - camera.pitch.tan() / (camera.fov * 0.5).tan()) * 0.5
}

fn backdrop(rect: Rect, camera: &Camera, display: &DisplaySettings, dark: bool) -> egui::Mesh {
    if camera.projection == Projection::Parallel {
        let sky = camera.pitch < 0.;
        let rgb = if sky && display.sky {
            display.sky_color
        } else if !sky && display.ground {
            display.ground_color
        } else {
            display.background
        };
        let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        let mut mesh = egui::Mesh::default();
        for pos in [
            rect.left_top(),
            rect.right_top(),
            rect.left_bottom(),
            rect.right_bottom(),
        ] {
            mesh.colored_vertex(pos, color);
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 3, 2);
        return mesh;
    }
    let horizon = horizon_fraction(camera);
    let elevation_span = 2. * camera.pitch.cos() * (camera.fov * 0.5).tan();
    // Reach the sky color above the horizon, independently of pan and zoom.
    let blue = horizon - 0.45 / elevation_span;
    let sky_base = if dark {
        display.horizon_color
    } else {
        [255; 3]
    };
    let mut rows = vec![0., 1.];
    for y in [horizon, blue] {
        if y > 0. && y < 1. {
            rows.push(y);
        }
    }
    rows.sort_by(f64::total_cmp);
    let mut mesh = egui::Mesh::default();
    for band in rows.windows(2) {
        let sky = (band[0] + band[1]) * 0.5 <= horizon;
        let enabled = if sky { display.sky } else { display.ground };
        for &y in band {
            let [r, g, b] = if !enabled {
                display.background
            } else if sky {
                let amount = ((horizon - y) * elevation_span / 0.45).clamp(0., 1.);
                std::array::from_fn(|i| {
                    let base = f64::from(sky_base[i]);
                    (base + (f64::from(display.sky_color[i]) - base) * amount).round() as u8
                })
            } else {
                display.ground_color
            };
            let color = Color32::from_rgb(r, g, b);
            let screen_y = rect.top() + rect.height() * y as f32;
            mesh.colored_vertex(egui::pos2(rect.left(), screen_y), color);
            mesh.colored_vertex(egui::pos2(rect.right(), screen_y), color);
        }
        let i = mesh.vertices.len() as u32 - 4;
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i + 1, i + 3, i + 2);
    }
    mesh
}

pub fn controls(ui: &mut egui::Ui, display: &mut DisplaySettings) -> bool {
    let before = display.clone();
    let mut themed = for_theme(display, ui.visuals().dark_mode);
    ui.label("Environment");
    ui.separator();
    let mut table = InspectorTable::new(ui.available_width(), [Column::flex(1.), Column::flex(1.)]);
    for (row, label) in ["Sky", "Ground", "Horizon", "Background"]
        .into_iter()
        .enumerate()
    {
        table.row(ui, ("environment_colors", row), |column, ui| {
            if column == 0 {
                match row {
                    0 => {
                        ui.checkbox(&mut display.sky, label);
                    }
                    1 => {
                        ui.checkbox(&mut display.ground, label);
                    }
                    _ => {
                        ui.label(label);
                    }
                }
            } else {
                // Color wells use exact sizing rather than layout justification.
                ui.spacing_mut().interact_size.x = ui.available_width();
                match row {
                    0 => {
                        ui.add_enabled_ui(display.sky, |ui| {
                            if ui.color_edit_button_srgb(&mut themed.sky_color).changed() {
                                display.sky_color = themed.sky_color;
                            }
                        });
                    }
                    1 => {
                        ui.add_enabled_ui(display.ground, |ui| {
                            if ui
                                .color_edit_button_srgb(&mut themed.ground_color)
                                .changed()
                            {
                                display.ground_color = themed.ground_color;
                            }
                        });
                    }
                    2 => {
                        ui.add_enabled_ui(display.sky || display.ground, |ui| {
                            if ui
                                .color_edit_button_srgb(&mut themed.horizon_color)
                                .changed()
                            {
                                display.horizon_color = themed.horizon_color;
                            }
                        });
                    }
                    _ => background_control(ui, display),
                }
            }
        });
    }
    if ui.button("Reset environment colors").clicked() {
        let defaults = DisplaySettings::default();
        display.sky_color = defaults.sky_color;
        display.ground_color = defaults.ground_color;
        display.horizon_color = defaults.horizon_color;
        display.background = defaults.background;
    }
    *display != before
}

pub fn background_control(ui: &mut egui::Ui, display: &mut DisplaySettings) {
    let mut background = for_theme(display, ui.visuals().dark_mode).background;
    if ui.color_edit_button_srgb(&mut background).changed() {
        display.background = background;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ketchup_core::{DVec2, DVec3, Document};

    #[test]
    fn horizon_matches_camera_rays_and_stays_fixed_when_panning_or_zooming() {
        let rect = Rect::from_min_size(egui::pos2(20., 50.), egui::vec2(900., 600.));
        for pitch in [-1.5, -0.2, 0., 0.2, 0.62, 1.5] {
            let camera = Camera {
                pitch,
                ..Default::default()
            };
            let mesh = backdrop(rect, &camera, &DisplaySettings::default(), false);
            assert!(mesh.is_valid());
            assert_eq!(mesh.vertices.first().unwrap().pos, rect.left_top());
            assert_eq!(mesh.vertices.last().unwrap().pos, rect.right_bottom());
            for band in mesh.vertices.chunks_exact(4) {
                for row in band.chunks_exact(2) {
                    assert_eq!(row[0].pos.y, row[1].pos.y);
                    assert_eq!(row[0].color, row[1].color);
                }
                let y = (band[0].pos.y + band[2].pos.y) * 0.5 - rect.top();
                let ray = camera.ray(DVec2::new(450., y as f64), DVec2::new(900., 600.));
                if ray.direction.z < 0. {
                    assert!(band.iter().all(|v| v.color == band[0].color));
                }
            }
            for bands in mesh.vertices.chunks_exact(4).collect::<Vec<_>>().windows(2) {
                assert_eq!(bands[0][2].pos, bands[1][0].pos);
                if bands[0][2].color != bands[1][0].color {
                    assert_eq!(bands[0][2].color, Color32::WHITE);
                    let y = (bands[0][2].pos.y - rect.top()) as f64;
                    let ray = camera.ray(DVec2::new(450., y), DVec2::new(900., 600.));
                    assert!(ray.direction.z.abs() < 1e-6);
                }
            }
            let moved = Camera {
                target: DVec3::new(100., -300., 50.),
                distance: 200.,
                yaw: 1.7,
                ..camera
            };
            assert_eq!(
                mesh.vertices,
                backdrop(rect, &moved, &DisplaySettings::default(), false).vertices
            );
        }
    }

    #[test]
    fn parallel_backdrop_is_uniform_and_field_of_view_matches_perspective_horizon() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800., 600.));
        let display = DisplaySettings::default();
        for standard in ketchup_core::StandardView::ALL {
            let mut camera = Camera {
                projection: Projection::Parallel,
                ..Camera::default()
            };
            camera.set_standard_view(standard);
            let mesh = backdrop(rect, &camera, &display, false);
            assert!(mesh.is_valid());
            assert!(
                mesh.vertices
                    .iter()
                    .all(|v| v.pos.is_finite() && v.color == mesh.vertices[0].color)
            );
            assert!(!horizon_fraction(&camera).is_finite());
        }
        for degrees in [15_f64, 45., 100.] {
            let camera = Camera {
                pitch: 0.1,
                fov: degrees.to_radians(),
                ..Camera::default()
            };
            let horizon = horizon_fraction(&camera);
            let ray = camera.ray(DVec2::new(400., horizon * 600.), DVec2::new(800., 600.));
            assert!(ray.direction.z.abs() < 1e-6);
        }
    }

    #[test]
    fn disabled_hemispheres_use_the_flat_background_throughout() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800., 600.));
        let camera = Camera {
            pitch: 0.,
            ..Default::default()
        };
        for (sky, ground) in [(false, true), (true, false), (false, false)] {
            let display = DisplaySettings {
                sky,
                ground,
                ..Default::default()
            };
            let [r, g, b] = display.background;
            let background = Color32::from_rgb(r, g, b);
            let mesh = backdrop(rect, &camera, &display, false);
            for band in mesh.vertices.chunks_exact(4) {
                let upper = (band[0].pos.y + band[2].pos.y) * 0.5 < rect.center().y;
                if (upper && !sky) || (!upper && !ground) {
                    assert!(band.iter().all(|v| v.color == background));
                }
            }
        }
    }

    #[test]
    fn environment_settings_round_trip_and_older_projects_get_defaults() {
        let mut doc = Document::default();
        doc.display.ground_color = [100, 120, 80];
        doc.display.horizon_color = [240, 230, 210];
        doc.display.sky = false;
        let json = serde_json::to_string(&doc).unwrap();
        assert_eq!(Document::from_json(&json).unwrap().display, doc.display);
        let mut old = serde_json::to_value(&doc).unwrap();
        for field in [
            "ground",
            "sky",
            "ground_color",
            "sky_color",
            "horizon_color",
        ] {
            old["display"].as_object_mut().unwrap().remove(field);
        }
        let restored = Document::from_json(&old.to_string()).unwrap();
        assert!(restored.display.sky && restored.display.ground);
        assert_eq!(
            restored.display.ground_color,
            DisplaySettings::default().ground_color
        );
    }
}
