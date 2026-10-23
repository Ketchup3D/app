use eframe::egui::{self, Color32, Rect};
use ketchup_core::{Camera, DisplaySettings};

/// A distant backdrop, painted before the depth-tested model. Ground is a
/// color, so orbiting below Z=0 never hides the user's model.
pub fn paint(painter: &egui::Painter, rect: Rect, camera: &Camera, display: &DisplaySettings) {
    painter.add(backdrop(rect, camera, display));
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
    // With no camera roll, the Z component of an unnormalized camera ray is
    // constant across each row. Its zero gives the true world-space horizon.
    (1. - camera.pitch.tan() / (Camera::FOV * 0.5).tan()) * 0.5
}

fn backdrop(rect: Rect, camera: &Camera, display: &DisplaySettings) -> egui::Mesh {
    let horizon = horizon_fraction(camera);
    let elevation_span = 2. * camera.pitch.cos() * (Camera::FOV * 0.5).tan();
    // Reach the sky color above the horizon, independently of pan and zoom.
    let blue = horizon - 0.45 / elevation_span;
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
                display
                    .sky_color
                    .map(|channel| (255. + (channel as f64 - 255.) * amount).round() as u8)
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
    ui.label("Environment");
    ui.separator();
    egui::Grid::new("environment_colors").show(ui, |ui| {
        ui.checkbox(&mut display.sky, "Sky");
        ui.add_enabled_ui(display.sky, |ui| {
            ui.color_edit_button_srgb(&mut display.sky_color);
        });
        ui.end_row();
        ui.checkbox(&mut display.ground, "Ground");
        ui.add_enabled_ui(display.ground, |ui| {
            ui.color_edit_button_srgb(&mut display.ground_color);
        });
        ui.end_row();
        ui.label("Horizon");
        ui.add_enabled_ui(display.sky || display.ground, |ui| {
            ui.color_edit_button_srgb(&mut display.horizon_color);
        });
        ui.end_row();
        ui.label("Background");
        ui.color_edit_button_srgb(&mut display.background);
        ui.end_row();
    });
    if ui.button("Reset environment colors").clicked() {
        let defaults = DisplaySettings::default();
        display.sky_color = defaults.sky_color;
        display.ground_color = defaults.ground_color;
        display.horizon_color = defaults.horizon_color;
        display.background = defaults.background;
    }
    *display != before
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
            let mesh = backdrop(rect, &camera, &DisplaySettings::default());
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
                backdrop(rect, &moved, &DisplaySettings::default()).vertices
            );
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
            let mesh = backdrop(rect, &camera, &display);
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
