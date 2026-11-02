//! Exploded vector schematics of supported profiles, using two contrasting parts.
//! These describe joint topology; dimensions still come from the settings/model.
use super::*;

fn description(kind: JointKind) -> &'static str {
    match kind {
        JointKind::MortiseTenon => "A tenon with shoulders fits into a rectangular mortise.",
        JointKind::HalfLap => "Opposing half-depth cuts let the two parts overlap.",
        JointKind::TongueGroove => "A continuous thin tongue fits into a matching groove.",
        JointKind::Rabbet => "A stepped edge fits against the complementary rebate.",
        JointKind::Dovetail => "A flared tail fits into a matching tapered socket.",
        JointKind::Finger => "Alternating straight fingers interlock across the joint.",
    }
}

pub(super) fn selector(ui: &mut egui::Ui, current: &mut JointKind) {
    let colors = Palette::for_ui(ui);
    let spacing = ui.spacing().item_spacing.x;
    let columns = ((ui.available_width() + spacing) / (145. + spacing))
        .floor()
        .clamp(1., 3.) as usize;
    let width = ((ui.available_width() - spacing * (columns - 1) as f32) / columns as f32).max(1.);
    let mut responses = Vec::new();
    // Retain the previous control's key; selection remains in JointSettings.
    ui.push_id("wood_joint_kind", |ui| {
        for row in JointKind::ALL.chunks(columns) {
            ui.horizontal(|ui| {
                for &kind in row {
                    let mut response = ui
                        .push_id(kind as u8, |ui| {
                            ui.add_sized(
                                [width, 121.],
                                egui::Button::new("")
                                    .selected(*current == kind)
                                    .corner_radius(5),
                            )
                        })
                        .inner
                        .on_hover_text(format!("{}\n{}", kind.label(), description(kind)));
                    if response.clicked() && *current != kind {
                        *current = kind;
                        response.mark_changed();
                    }
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::Role::RadioButton,
                            ui.is_enabled(),
                            *current == kind,
                            kind.label(),
                        )
                    });
                    if ui.is_rect_visible(response.rect) {
                        let p = ui.painter().with_clip_rect(response.rect.shrink(3.));
                        let diagram = Rect::from_min_max(
                            response.rect.min + egui::vec2(7., 7.),
                            egui::pos2(response.rect.right() - 7., response.rect.top() + 82.),
                        );
                        preview(
                            &p,
                            diagram,
                            kind,
                            colors.ink,
                            ui.visuals().dark_mode,
                            ui.is_enabled(),
                        );
                        if *current == kind {
                            // Selection has a textual/accessibility state as well as a fill.
                            p.text(
                                response.rect.right_top() + egui::vec2(-9., 7.),
                                Align2::RIGHT_TOP,
                                "✓",
                                FontId::proportional(14.),
                                colors.ink,
                            );
                        }
                        let mut label = egui::text::LayoutJob::simple(
                            kind.label().to_owned(),
                            FontId::proportional(12.),
                            colors.ink,
                            width - 14.,
                        );
                        label.halign = egui::Align::Center;
                        let galley = ui.fonts_mut(|fonts| fonts.layout_job(label));
                        p.galley(
                            egui::pos2(response.rect.center().x, response.rect.top() + 86.),
                            galley,
                            colors.ink,
                        );
                        if response.has_focus() {
                            p.rect_stroke(
                                response.rect.shrink(2.),
                                4,
                                Stroke::new(1.5, colors.ink),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                    responses.push((kind, response));
                }
            });
        }
    });
    // Buttons support Tab/Space/Enter; arrow keys also navigate the radio group.
    if ui.is_enabled() {
        if let Some(index) = responses.iter().position(|(_, r)| r.has_focus()) {
            let step = ui.input(|i| {
                if i.key_pressed(Key::ArrowRight) {
                    1
                } else if i.key_pressed(Key::ArrowLeft) {
                    -1
                } else if i.key_pressed(Key::ArrowDown) {
                    columns as isize
                } else if i.key_pressed(Key::ArrowUp) {
                    -(columns as isize)
                } else {
                    0
                }
            });
            if step != 0 {
                let next = (index as isize + step).clamp(0, responses.len() as isize - 1) as usize;
                *current = responses[next].0;
                responses[next].1.request_focus();
            }
        }
    }
}

fn preview(
    p: &egui::Painter,
    rect: Rect,
    kind: JointKind,
    outline: Color32,
    _dark: bool,
    enabled: bool,
) {
    let first = Color32::from_rgb(102, 171, 205);
    let second = Color32::from_rgb(207, 165, 107);
    let mut solids: Vec<Solid> = Vec::new();
    macro_rules! prism {
        ($xy:expr, $bottom:expr, $top:expr, $color:expr $(,)?) => {
            solids.push(($xy.to_vec(), $bottom, $top, $color))
        };
    }
    macro_rules! block {
        ($x0:expr, $x1:expr, $y0:expr, $y1:expr, $z0:expr, $z1:expr, $color:expr) => {
            prism!(
                [[$x0, $y0], [$x1, $y0], [$x1, $y1], [$x0, $y1]],
                $z0,
                $z1,
                $color
            )
        };
    }
    // First member and its retained profile are exploded from the receiver.
    block!(2.15, 3., 0., 1., 0., 0.55, second);
    block!(0., 0.85, 0., 1., 0., 0.55, first);
    match kind {
        JointKind::MortiseTenon => {
            block!(0.85, 1.35, 0.3, 0.7, 0.18, 0.37, first);
            block!(1.65, 2.15, 0., 0.3, 0., 0.55, second);
            block!(1.65, 2.15, 0.7, 1., 0., 0.55, second);
            block!(1.65, 2.15, 0.3, 0.7, 0., 0.18, second);
            block!(1.65, 2.15, 0.3, 0.7, 0.37, 0.55, second);
        }
        JointKind::HalfLap => {
            block!(0.85, 1.35, 0., 1., 0., 0.275, first);
            block!(1.65, 2.15, 0., 1., 0.275, 0.55, second);
        }
        JointKind::TongueGroove => {
            block!(0.85, 1.35, 0., 1., 0.2, 0.35, first);
            block!(1.65, 2.15, 0., 1., 0., 0.2, second);
            block!(1.65, 2.15, 0., 1., 0.35, 0.55, second);
        }
        JointKind::Rabbet => {
            block!(0.85, 1.35, 0., 0.5, 0., 0.55, first);
            block!(1.65, 2.15, 0.5, 1., 0., 0.55, second);
        }
        JointKind::Dovetail => {
            prism!(
                [[0.85, 0.35], [1.35, 0.2], [1.35, 0.8], [0.85, 0.65]],
                0.,
                0.55,
                first,
            );
            prism!(
                [[1.65, 0.], [2.15, 0.], [2.15, 0.35], [1.65, 0.2]],
                0.,
                0.55,
                second,
            );
            prism!(
                [[1.65, 0.8], [2.15, 0.65], [2.15, 1.], [1.65, 1.]],
                0.,
                0.55,
                second,
            );
        }
        JointKind::Finger => {
            for i in 0..6 {
                let lo = i as f32 / 6.;
                let hi = (i + 1) as f32 / 6.;
                if i % 2 == 0 {
                    block!(0.85, 1.35, lo, hi, 0., 0.55, first);
                } else {
                    block!(1.65, 2.15, lo, hi, 0., 0.55, second);
                }
            }
        }
    }
    paint_solids(p, rect, &solids, kind, outline, enabled);
}

type Solid = (Vec<[f32; 2]>, f32, f32, Color32);

/// Draw only exposed surfaces. Split at every profile boundary so adjoining
/// blocks never contribute internal faces or outlines through the other member.
fn paint_solids(
    p: &egui::Painter,
    rect: Rect,
    solids: &[Solid],
    kind: JointKind,
    outline: Color32,
    enabled: bool,
) {
    // Flat profiles read from above; depth cuts need their vertical walls.
    let (tilt, across, vertical) = match kind {
        JointKind::Dovetail => (0.08, 0.72, 0.48),
        JointKind::Finger => (0.08, 0.76, 0.44),
        JointKind::Rabbet => (0.10, 0.62, 0.60),
        JointKind::HalfLap => (0.16, 0.40, 0.85),
        JointKind::MortiseTenon => (0.16, 0.28, 0.98),
        JointKind::TongueGroove => (0.16, 0.24, 1.02),
    };
    // Derive the camera direction from the projection so face visibility and
    // depth ordering stay consistent for every joint's viewing angle.
    let camera_x = 0.42 * vertical;
    let camera_y = 0.82 * vertical;
    let camera_z = 0.82 * across - 0.42 * tilt;
    let scale = (rect.width() / 3.65).min(rect.height() / 1.45);
    // Open the exploded pair toward the viewer: both mating ends are visible.
    // Preserve the common insertion axis and matching profile dimensions.
    let project = |v: [f32; 3], left: bool| {
        let direction = if left { -1. } else { 1. };
        let center = if left { 0.675 } else { 2.325 };
        rect.center()
            + egui::vec2(
                (0.82 * (v[0] - 1.5) + direction * (0.42 * (v[1] - 0.5) + 0.15)) * scale,
                (direction * tilt * (v[0] - center) + across * (v[1] - 0.5)
                    - vertical * (v[2] - 0.275))
                    * scale,
            )
    };
    let is_left = |color: Color32| color.b() > color.r();
    // Clip each convex face against the other solids in its plane. Keeping the
    // exterior fragments also supports the tapered dovetail without voxel steps.
    let mut faces: Vec<(Vec<[f32; 3]>, Color32, f32)> = Vec::new();
    for (poly, bottom, top, color) in solids {
        let mut candidates = vec![(
            poly.iter().map(|v| [v[0], v[1], *top]).collect::<Vec<_>>(),
            1.0,
        )];
        for (a, b) in poly.iter().zip(poly.iter().cycle().skip(1)) {
            let normal = [b[1] - a[1], a[0] - b[0]];
            if normal[0] * (if is_left(*color) { camera_x } else { -camera_x })
                + normal[1] * camera_y
                <= 0.
            {
                continue;
            }
            candidates.push((
                vec![
                    [a[0], a[1], *bottom],
                    [b[0], b[1], *bottom],
                    [b[0], b[1], *top],
                    [a[0], a[1], *top],
                ],
                if normal[0].abs() > normal[1].abs() {
                    0.72
                } else {
                    0.86
                },
            ));
        }
        for (vertices, shade) in candidates {
            let a = vertices[0];
            let b = vertices[1];
            let c = vertices[2];
            let u = std::array::from_fn::<_, 3, _>(|i| b[i] - a[i]);
            let v = std::array::from_fn::<_, 3, _>(|i| c[i] - a[i]);
            let outward = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            // Other solids can cover a portion of a face. Subtract their convex
            // volumes, preserving convex fragments for egui's tessellator.
            let mut fragments = vec![vertices];
            for (other, lo, hi, _) in solids {
                if std::ptr::eq(poly, other) {
                    continue;
                }
                let mut planes = vec![([0., 0., 1.], *hi), ([0., 0., -1.], -*lo)];
                for (a, b) in other.iter().zip(other.iter().cycle().skip(1)) {
                    let n = [b[1] - a[1], a[0] - b[0], 0.];
                    planes.push((n, n[0] * a[0] + n[1] * a[1]));
                }
                let mut remaining = Vec::new();
                for fragment in fragments {
                    // Touching volumes do not overlap this surface. In particular,
                    // clipping a face down to a shared edge must not turn that
                    // zero-area edge into another occluding polygon.
                    if planes.iter().any(|(n, d)| {
                        let distance = |v: &[f32; 3]| n[0] * v[0] + n[1] * v[1] + n[2] * v[2] - d;
                        fragment.iter().all(|v| distance(v) >= -0.000001)
                            && (fragment.iter().any(|v| distance(v) > 0.000001)
                                || n.iter().zip(outward).map(|(a, b)| a * b).sum::<f32>() >= 0.)
                    }) {
                        remaining.push(fragment);
                        continue;
                    }
                    let mut covered = fragment;
                    for (n, d) in &planes {
                        let (inner, outer) = split_face(&covered, *n, *d);
                        if face_area(&outer) > 0.000001 {
                            remaining.push(outer);
                        }
                        covered = inner;
                        if face_area(&covered) <= 0.000001 {
                            break;
                        }
                    }
                }
                fragments = remaining;
            }
            for face in fragments {
                faces.push((face, *color, shade));
            }
        }
    }
    let depth = |face: &[[f32; 3]], color: Color32| {
        let x = if is_left(color) { camera_x } else { -camera_x };
        face.iter()
            .map(|v| x * v[0] + camera_y * v[1] + camera_z * v[2])
            .sum::<f32>()
            / face.len() as f32
    };
    faces.sort_by(|a, b| depth(&a.0, a.1).total_cmp(&depth(&b.0, b.1)));
    // Split edges at neighboring vertices, then suppress edges shared by
    // coplanar faces. Outlines describe the finished profile, not its blocks.
    let vertices: Vec<_> = faces
        .iter()
        .flat_map(|(face, _, _)| face.iter().copied())
        .collect();
    let distance2 = |a: [f32; 3], b: [f32; 3]| (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>();
    let mut edges = Vec::new();
    for (index, (face, color, shade)) in faces.iter().enumerate() {
        for (a, b) in face.iter().zip(face.iter().cycle().skip(1)) {
            let delta: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
            let length2 = distance2(*a, *b);
            if length2 < 0.00000001 {
                continue;
            }
            let mut cuts = vec![0., 1.];
            for v in &vertices {
                let t = (0..3).map(|i| (v[i] - a[i]) * delta[i]).sum::<f32>() / length2;
                let on = std::array::from_fn(|i| a[i] + t * delta[i]);
                if t > 0.00001 && t < 0.99999 && distance2(*v, on) < 0.00000001 {
                    cuts.push(t);
                }
            }
            cuts.sort_by(f32::total_cmp);
            cuts.dedup_by(|a, b| (*a - *b).abs() < 0.00001);
            for pair in cuts.windows(2) {
                edges.push((
                    index,
                    *color,
                    *shade,
                    std::array::from_fn::<_, 3, _>(|i| a[i] + pair[0] * delta[i]),
                    std::array::from_fn::<_, 3, _>(|i| a[i] + pair[1] * delta[i]),
                ));
            }
        }
    }
    let edge_stroke = Stroke::new(
        0.85,
        outline.gamma_multiply(if enabled { 0.7 } else { 0.3 }),
    );
    for (index, (face, color, shade)) in faces.iter().enumerate() {
        let left = is_left(*color);
        let fill = color.gamma_multiply(shade * if enabled { 1. } else { 0.45 });
        p.add(egui::Shape::convex_polygon(
            face.iter().map(|v| project(*v, left)).collect(),
            fill,
            Stroke::NONE,
        ));
        for (_, _, _, a, b) in edges.iter().filter(|e| e.0 == index) {
            let seam = edges.iter().any(|(other, c, s, x, y)| {
                *other != index
                    && c == color
                    && s == shade
                    && ((distance2(*a, *x) < 0.00000001 && distance2(*b, *y) < 0.00000001)
                        || (distance2(*a, *y) < 0.00000001 && distance2(*b, *x) < 0.00000001))
            });
            if !seam {
                p.line_segment([project(*a, left), project(*b, left)], edge_stroke);
            }
        }
    }
}

fn split_face(face: &[[f32; 3]], n: [f32; 3], d: f32) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    for (a, b) in face
        .iter()
        .zip(face.iter().cycle().skip(1))
        .take(face.len())
    {
        let da = n[0] * a[0] + n[1] * a[1] + n[2] * a[2] - d;
        let db = n[0] * b[0] + n[1] * b[1] + n[2] * b[2] - d;
        if da <= 0.000001 {
            inside.push(*a);
        }
        if da >= -0.000001 {
            outside.push(*a);
        }
        if (da < -0.000001 && db > 0.000001) || (da > 0.000001 && db < -0.000001) {
            let t = da / (da - db);
            let v = std::array::from_fn(|i| a[i] + t * (b[i] - a[i]));
            inside.push(v);
            outside.push(v);
        }
    }
    (inside, outside)
}

// Area in 3D is independent of camera projection and rejects collinear clipping
// fragments before they reach egui's convex polygon tessellator.
fn face_area(face: &[[f32; 3]]) -> f32 {
    let mut normal = [0.; 3];
    for (a, b) in face
        .iter()
        .zip(face.iter().cycle().skip(1))
        .take(face.len())
    {
        normal[0] += (a[1] - b[1]) * (a[2] + b[2]);
        normal[1] += (a[2] - b[2]) * (a[0] + b[0]);
        normal[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    normal.iter().map(|v| v * v).sum::<f32>().sqrt() * 0.5
}
