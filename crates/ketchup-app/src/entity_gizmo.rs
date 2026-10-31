use super::*;

const AXES: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];
const AXIS_NAMES: [&str; 3] = ["X", "Y", "Z"];
const LABEL_PADDING: Vec2 = egui::vec2(8., 5.);
const GIZMO_HEIGHT: f32 = 98.;

#[derive(Clone, Copy)]
struct GizmoAxis {
    tip: Pos2,
    label: Rect,
    depth: f64,
}

fn regions(rect: Rect) -> (Rect, Rect) {
    let content = rect.shrink(8.);
    let label_width = content.width() * 0.55;
    let labels = Rect::from_min_max(
        egui::pos2(content.right() - label_width, content.top()),
        content.max,
    );
    let diagram = Rect::from_min_max(
        content.min,
        egui::pos2(labels.left() - 12., content.bottom()),
    );
    (diagram, labels)
}

fn layout(camera: &Camera, rect: Rect, label_sizes: [Vec2; 3]) -> (Pos2, [GizmoAxis; 3]) {
    let (diagram, labels) = regions(rect);
    let radius = (diagram.width() * 0.4).min(diagram.height() * 0.32);
    let right = camera.right();
    let up = camera.up();
    let toward_eye = (camera.eye() - camera.target).normalize();
    let projected = AXES.map(|axis| {
        let direction = axis.vector();
        egui::vec2(direction.dot(right) as f32, -direction.dot(up) as f32) * radius
    });
    // Balance the positive arrows and negative stubs vertically in their own
    // region. Labels have a separate column that no axis geometry can enter.
    let (top, bottom) = projected
        .iter()
        .fold((0_f32, 0_f32), |(top, bottom), delta| {
            (
                top.min(delta.y).min(-delta.y * 0.28),
                bottom.max(delta.y).max(-delta.y * 0.28),
            )
        });
    let center = egui::pos2(
        diagram.center().x,
        (diagram.center().y - (top + bottom) * 0.5)
            .clamp(diagram.top() - top + 6., diagram.bottom() - bottom - 6.),
    );
    let height = label_sizes.iter().map(|size| size.y).sum::<f32>() + 8.;
    let mut label_top = labels.center().y - height * 0.5;
    let axes = std::array::from_fn(|index| {
        let label = Rect::from_min_size(
            egui::pos2(labels.left(), label_top),
            egui::vec2(labels.width(), label_sizes[index].y),
        );
        label_top += label_sizes[index].y + 4.;
        GizmoAxis {
            tip: center + projected[index],
            label,
            depth: AXES[index].vector().dot(toward_eye),
        }
    });
    (center, axes)
}

impl KetchupApp {
    pub(super) fn entity_axis_gizmo(&self, ui: &mut egui::Ui, size: DVec3) {
        let colors = Palette::for_ui(ui);
        ui.label(RichText::new("Object lenghts").size(12.));
        ui.add_space(4.);
        let lengths =
            [size.x, size.y, size.z].map(|value| self.doc.display.units.format_length(value));
        let labels: [String; 3] =
            std::array::from_fn(|index| format!("{}  {}", AXIS_NAMES[index], lengths[index]));
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), GIZMO_HEIGHT),
            Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8, colors.canvas);
        painter.rect_stroke(
            rect,
            8,
            Stroke::new(1., colors.border),
            egui::StrokeKind::Inside,
        );
        let galleys: [_; 3] = std::array::from_fn(|index| {
            let color = self.axis_color(AXES[index]);
            let mut galley =
                painter.layout_no_wrap(labels[index].clone(), FontId::monospace(12.), color);
            let available = (regions(rect).1.width() - 2. * LABEL_PADDING.x).max(1.);
            if galley.size().x > available {
                galley = painter.layout_no_wrap(
                    labels[index].clone(),
                    FontId::monospace(12. * available / galley.size().x),
                    color,
                );
            }
            galley
        });
        let (center, axes) = layout(
            &self.doc.camera,
            rect,
            std::array::from_fn(|index| galleys[index].size() + 2. * LABEL_PADDING),
        );
        let mut order = [0, 1, 2];
        order.sort_by(|&a, &b| axes[a].depth.total_cmp(&axes[b].depth));
        for index in order {
            let axis = axes[index];
            let color = self.axis_color(AXES[index]);
            let delta = axis.tip - center;
            painter.line_segment(
                [center - delta * 0.28, center],
                Stroke::new(1., color.gamma_multiply(0.25)),
            );
            if delta.length() > 4. {
                painter.arrow(center, delta, Stroke::new(2., color));
            }
        }
        painter.circle_filled(center, 3., colors.muted);
        for index in order {
            let axis = axes[index];
            let color = self.axis_color(AXES[index]);
            if axis.tip.distance(center) <= 4. {
                painter.circle(axis.tip, 5., colors.canvas, Stroke::new(1.5, color));
                if axis.depth >= 0. {
                    painter.circle_filled(axis.tip, 2., color);
                } else {
                    for sign in [-1., 1.] {
                        let offset = egui::vec2(2.5, 2.5 * sign);
                        painter.line_segment(
                            [axis.tip - offset, axis.tip + offset],
                            Stroke::new(1., color),
                        );
                    }
                }
            }
            painter.rect_filled(axis.label, 5, colors.paper);
            painter.rect_stroke(
                axis.label,
                5,
                Stroke::new(1., color.gamma_multiply(0.35)),
                egui::StrokeKind::Inside,
            );
            painter.galley(
                axis.label.center() - galleys[index].size() * 0.5,
                galleys[index].clone(),
                color,
            );
        }
        let summary = format!("Axis lengths: {}. {}. {}.", labels[0], labels[1], labels[2]);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), &summary)
        });
        response.on_hover_ui(|ui| {
            ui.label("Solid lengths along world axes. Orientation follows the active view.");
            for label in &labels {
                ui.monospace(label);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ketchup_core::{Projection, StandardView};

    #[test]
    fn compact_gizmo_shifts_origin_to_fit_the_view() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(224., GIZMO_HEIGHT));
        for view in StandardView::ALL {
            let mut camera = Camera::default();
            camera.set_standard_view(view);
            let (center, axes) = layout(&camera, rect, [egui::vec2(100., 24.); 3]);
            assert!(rect.shrink(8.).contains(center));
            for axis in axes {
                assert!(rect.shrink(7.9).contains(axis.tip));
            }
            match view {
                StandardView::Front => assert!(center.y > rect.center().y + 5.),
                StandardView::Bottom => assert!(center.y < rect.center().y - 5.),
                _ => {}
            }
        }
    }

    #[test]
    fn gizmo_matches_viewport_axis_directions_including_top_and_bottom_views() {
        let rect = Rect::from_min_size(egui::pos2(20., 30.), egui::vec2(240., GIZMO_HEIGHT));
        for view in StandardView::ALL {
            let mut camera = Camera {
                projection: Projection::Parallel,
                ..Camera::default()
            };
            camera.set_standard_view(view);
            let (center, arms) = layout(&camera, rect, [egui::vec2(100., 24.); 3]);
            let viewport = DVec2::new(800., 600.);
            let origin = camera.project(camera.target, viewport).unwrap();
            for (axis, arm) in AXES.into_iter().zip(arms) {
                let expected = camera
                    .project(camera.target + axis.vector(), viewport)
                    .unwrap()
                    - origin;
                let actual = dv(arm.tip - center);
                if expected.length() < 1e-6 {
                    assert!(actual.length() < 1e-4, "{view:?} {axis:?} must be end-on");
                } else {
                    assert!(
                        expected.normalize().distance(actual.normalize()) < 1e-5,
                        "{view:?} {axis:?} must point the same way as the viewport"
                    );
                }
            }
            // Orientation is independent of pan, zoom, FOV, and projection mode.
            camera.target = DVec3::new(12., 23., 34.);
            camera.distance = 100.;
            camera.fov = 1.2;
            camera.projection = Projection::Perspective;
            let (_, moved) = layout(&camera, rect, [egui::vec2(100., 24.); 3]);
            for (a, b) in arms.into_iter().zip(moved) {
                assert!(a.tip.distance(b.tip) < 1e-4);
            }
        }
    }

    #[test]
    fn labels_stay_clear_of_all_axis_lines_and_arrowheads_during_orbit() {
        for width in [180., 224., 340.] {
            let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(width, GIZMO_HEIGHT));
            for yaw in 0..24 {
                for pitch in -6..=6 {
                    let camera = Camera {
                        yaw: f64::from(yaw) * std::f64::consts::PI / 12.,
                        pitch: f64::from(pitch) * std::f64::consts::PI / 12.,
                        ..Camera::default()
                    };
                    let (center, arms) = layout(&camera, rect, [egui::vec2(width - 16., 24.); 3]);
                    let (diagram, label_region) = regions(rect);
                    for (index, arm) in arms.iter().enumerate() {
                        assert!(arm.tip.is_finite());
                        assert!(label_region.expand(0.1).contains_rect(arm.label));
                        assert!(!diagram.expand(4.).intersects(arm.label));
                        for other in arms {
                            let delta = other.tip - center;
                            let negative = center - delta * 0.28;
                            assert!(diagram.contains(negative));
                            // Include both arrowhead wings, matching egui's arrow geometry.
                            let rotation =
                                egui::emath::Rot2::from_angle(std::f32::consts::TAU / 10.);
                            for end in [
                                other.tip,
                                other.tip - 0.25 * (rotation * delta),
                                other.tip - 0.25 * (rotation.inverse() * delta),
                            ] {
                                assert!(diagram.contains(end));
                                assert!(
                                    !arm.label
                                        .expand(4.)
                                        .intersects(Rect::from_two_pos(center, end).expand(2.)),
                                    "label overlaps an axis at yaw {yaw}, pitch {pitch}, width {width}"
                                );
                            }
                        }
                        for other in &arms[..index] {
                            assert!(
                                !arm.label.expand(1.).intersects(other.label),
                                "labels overlap at yaw {yaw}, pitch {pitch}, width {width}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn orbit_keeps_measurements_stable_in_the_reserved_label_column() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.doc.display.units.length = ketchup_core::LengthUnit::Millimeters;
        app.doc.display.units.precision.length = Some(1);
        let size = DVec3::new(1., 2., 0.);
        let render = |app: &KetchupApp| {
            ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_max_width(224.);
                app.entity_axis_gizmo(ui, size);
            })
        };
        let position = |output: &egui::FullOutput, label: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == label => {
                        assert_eq!(text.galley.rows.len(), 1, "units must never wrap");
                        Some(text.pos)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing measurement: {label}"))
        };
        app.doc.camera.set_standard_view(StandardView::Front);
        let before = render(&app);
        app.doc.camera.set_standard_view(StandardView::Right);
        let after = render(&app);
        for label in ["X  1000.0 mm", "Y  2000.0 mm", "Z  0.0 mm"] {
            let a = position(&before, label);
            let b = position(&after, label);
            assert!(
                a.distance(b) < 1e-4,
                "{label} must stay in its clear label column"
            );
        }
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());
    }
}
